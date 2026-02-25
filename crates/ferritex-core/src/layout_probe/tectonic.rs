use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::SystemTime,
};

use anyhow::{Result, anyhow};
use tectonic::{
    TexEngine,
    config::PersistentConfig,
    driver::{OutputFormat, ProcessingSessionBuilder},
    latex_to_pdf,
    status::NoopStatusBackend,
    unstable_opts::UnstableOptions,
};

use crate::model::LayoutProbeOutput;

use super::pt_to_twips_rounded;

const PROBE_MARKER_PREFIX: &str = "FERRITEX_PROBE:";
const PROBE_TEX_INPUT_NAME: &str = "ferritex_probe.tex";
const PROBE_LOG_NAME: &str = "ferritex_probe.log";
const PROBE_TEX_ENGINE_HALT_ON_ERROR: bool = false;
const PROBE_TEX_ENGINE_SHELL_ESCAPE_ENABLED: bool = false;
const PROBE_TEX_ENGINE_BUILD_DATE: SystemTime = SystemTime::UNIX_EPOCH;
const PDF_PAGE_COUNT_KEYWORD: &[u8] = b"/Count";

#[derive(Debug)]
struct CurrentDirGuard {
    previous_dir: PathBuf,
}

impl CurrentDirGuard {
    fn change_to(target_dir: &Path) -> Result<Self> {
        let previous_dir = std::env::current_dir()
            .map_err(|error| anyhow!("failed to read current working directory: {error}"))?;
        std::env::set_current_dir(target_dir).map_err(|error| {
            anyhow!(
                "failed to switch current working directory to {}: {error}",
                target_dir.display()
            )
        })?;
        Ok(Self { previous_dir })
    }
}

impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        if let Err(error) = std::env::set_current_dir(&self.previous_dir) {
            log::warn!(
                "failed to restore working directory to {}: {error}",
                self.previous_dir.display()
            );
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldConfidence {
    Trusted,
    Degraded,
}

#[derive(Debug, Clone, Copy)]
struct ProbeConfidenceModel {
    font_size_body_hp: FieldConfidence,
    body_line_spacing_twips: FieldConfidence,
}

impl Default for ProbeConfidenceModel {
    fn default() -> Self {
        Self {
            font_size_body_hp: FieldConfidence::Trusted,
            body_line_spacing_twips: FieldConfidence::Trusted,
        }
    }
}

impl ProbeConfidenceModel {
    fn downgraded_fields(self) -> Vec<&'static str> {
        let mut fields = Vec::new();
        if self.font_size_body_hp == FieldConfidence::Degraded {
            fields.push("font_size_body_hp");
        }
        if self.body_line_spacing_twips == FieldConfidence::Degraded {
            fields.push("body_line_spacing_twips");
        }
        fields
    }
}

fn build_probe_tex_engine() -> TexEngine {
    let mut tex_engine = TexEngine::default();
    tex_engine
        .halt_on_error_mode(PROBE_TEX_ENGINE_HALT_ON_ERROR)
        .shell_escape(PROBE_TEX_ENGINE_SHELL_ESCAPE_ENABLED)
        .build_date(PROBE_TEX_ENGINE_BUILD_DATE);
    tex_engine
}

fn with_temporary_working_dir<T, F>(target_dir: &Path, action: F) -> Result<T>
where
    F: FnOnce() -> Result<T>,
{
    let _guard = CurrentDirGuard::change_to(target_dir)?;
    action()
}

pub(crate) fn probe_layout_with_tectonic(
    input_path: &Path,
    expanded_source: &str,
) -> Result<LayoutProbeOutput> {
    let preamble = expanded_source
        .split_once("\\begin{document}")
        .map(|(head, _)| head)
        .unwrap_or(expanded_source);
    let probe_source = build_probe_source(preamble);

    let root_dir = input_path.parent().unwrap_or_else(|| Path::new("."));
    let tex_engine_profile = build_probe_tex_engine();
    log::debug!("tectonic probe engine profile: {:?}", tex_engine_profile);

    let mut status = NoopStatusBackend::default();
    let config = PersistentConfig::open(false)
        .map_err(|error| anyhow!("failed to open default tectonic config: {error}"))?;

    let bundle = config
        .default_bundle(false, &mut status)
        .map_err(|error| anyhow!("failed to load tectonic default bundle: {error}"))?;
    let format_cache_path = config
        .format_cache_path()
        .map_err(|error| anyhow!("failed to resolve tectonic format cache path: {error}"))?;

    let mut builder = ProcessingSessionBuilder::default();
    builder
        .bundle(bundle)
        .primary_input_buffer(probe_source.as_bytes())
        .tex_input_name(PROBE_TEX_INPUT_NAME)
        .filesystem_root(root_dir)
        .format_name("latex")
        .format_cache_path(format_cache_path)
        .output_format(OutputFormat::Aux)
        .keep_logs(true)
        .keep_intermediates(false)
        .print_stdout(false)
        .do_not_write_output_files()
        .build_date(PROBE_TEX_ENGINE_BUILD_DATE)
        .shell_escape_disabled()
        // Probe should keep partial extraction signal even if TeX reports
        // recoverable errors in template-specific preamble logic.
        .unstables(UnstableOptions {
            continue_on_errors: !PROBE_TEX_ENGINE_HALT_ON_ERROR,
            ..Default::default()
        });

    let mut session = builder
        .create(&mut status)
        .map_err(|error| anyhow!("failed to create tectonic processing session: {error}"))?;
    let run_result = session.run(&mut status);

    let mut files = session.into_file_data();
    let log_text = if let Some(file) = files.remove(PROBE_LOG_NAME) {
        String::from_utf8_lossy(&file.data).into_owned()
    } else {
        let mut fallback = None;
        for (name, file) in files {
            if name.ends_with(".log") {
                fallback = Some(file);
                break;
            }
        }

        let fallback =
            fallback.ok_or_else(|| anyhow!("tectonic probe did not produce a .log artifact"))?;
        String::from_utf8_lossy(&fallback.data).into_owned()
    };

    let mut probe_output = parse_probe_log(&log_text);
    let has_log_errors = probe_log_has_tex_errors(&log_text);
    let has_run_error = run_result.is_err();

    if let Err(error) = run_result {
        let populated_before_filter = probe_output.populated_field_names();
        if populated_before_filter.is_empty() {
            return Err(anyhow!(
                "tectonic layout probe run failed: {error}. log excerpt: {}",
                probe_log_excerpt(&log_text)
            ));
        }
    }

    let confidence_model = build_probe_confidence_model(&log_text, has_run_error, has_log_errors);
    let downgraded_fields = confidence_model.downgraded_fields();
    if !downgraded_fields.is_empty() {
        apply_probe_confidence_model(&mut probe_output, confidence_model);

        let populated_fields = probe_output.populated_field_names();
        log::warn!(
            "tectonic layout probe degraded confidence for field(s): {}; retained {} field(s) after safety filter: {}",
            downgraded_fields.join(", "),
            populated_fields.len(),
            populated_fields.join(", ")
        );
    }

    Ok(probe_output)
}

pub(crate) fn infer_total_pages_with_tectonic_runtime(
    input_path: &Path,
    expanded_source: &str,
) -> Option<u32> {
    let root_dir = input_path.parent().unwrap_or_else(|| Path::new("."));
    let tex_engine_profile = build_probe_tex_engine();
    log::debug!(
        "TotPages runtime inference via tectonic APIs; engine profile: {:?}",
        tex_engine_profile
    );

    let pdf_data = match with_temporary_working_dir(root_dir, || {
        latex_to_pdf(expanded_source).map_err(|error| {
            anyhow!(
                "tectonic::latex_to_pdf failed while inferring TotPages for {}: {error}",
                input_path.display()
            )
        })
    }) {
        Ok(data) => data,
        Err(error) => {
            log::debug!("{error}");
            return None;
        }
    };

    infer_total_pages_from_pdf_bytes(&pdf_data)
}

fn infer_total_pages_from_pdf_bytes(pdf_data: &[u8]) -> Option<u32> {
    let mut cursor = 0usize;
    let mut max_pages = 0u32;

    while let Some(offset) = find_subslice(&pdf_data[cursor..], PDF_PAGE_COUNT_KEYWORD) {
        let mut index = cursor + offset + PDF_PAGE_COUNT_KEYWORD.len();
        while index < pdf_data.len() && pdf_data[index].is_ascii_whitespace() {
            index += 1;
        }

        let value_start = index;
        while index < pdf_data.len() && pdf_data[index].is_ascii_digit() {
            index += 1;
        }

        if value_start < index
            && let Ok(raw) = std::str::from_utf8(&pdf_data[value_start..index])
            && let Ok(pages) = raw.parse::<u32>()
        {
            max_pages = max_pages.max(pages);
        }

        cursor += offset + PDF_PAGE_COUNT_KEYWORD.len();
    }

    if max_pages > 0 { Some(max_pages) } else { None }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }

    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn build_probe_source(preamble: &str) -> String {
    let mut source = String::with_capacity(preamble.len() + PROBE_BODY.len() + 32);
    source.push_str(preamble);
    if !source.ends_with('\n') {
        source.push('\n');
    }
    source.push_str(PROBE_BODY);
    source
}

fn parse_probe_log(log: &str) -> LayoutProbeOutput {
    let markers = collect_probe_markers(log);

    let list_label_sep_twips = read_length_twips(&markers, "list_label_sep");
    let list_label_width_twips = read_length_twips(&markers, "list_label_width");
    let list_hanging_indent_twips = match (list_label_sep_twips, list_label_width_twips) {
        (Some(sep), Some(width)) => Some(sep.saturating_add(width)),
        _ => None,
    };

    LayoutProbeOutput {
        page_margin_top_twips: read_length_twips(&markers, "page_margin_top"),
        page_margin_bottom_twips: read_length_twips(&markers, "page_margin_bottom"),
        page_margin_left_twips: read_length_twips(&markers, "page_margin_left"),
        page_margin_right_twips: read_length_twips(&markers, "page_margin_right"),
        page_margin_header_twips: read_length_twips(&markers, "page_margin_header"),
        page_margin_footer_twips: read_length_twips(&markers, "page_margin_footer"),
        page_gutter_twips: read_length_twips(&markers, "page_gutter"),
        page_width_twips: read_length_twips(&markers, "page_width")
            .and_then(|twips| u32::try_from(twips).ok()),
        page_height_twips: read_length_twips(&markers, "page_height")
            .and_then(|twips| u32::try_from(twips).ok()),
        font_family_body: read_font_family(&markers),
        font_size_body_hp: read_font_size_half_points(&markers, "body_font_size_pt"),
        body_first_line_indent_twips: read_length_twips(&markers, "body_parindent"),
        body_line_spacing_twips: read_length_twips(&markers, "body_line_spacing"),
        list_left_indent_twips: read_length_twips(&markers, "list_left_indent"),
        list_hanging_indent_twips,
        list_item_indent_twips: read_length_twips(&markers, "list_item_indent")
            .or_else(|| read_length_twips(&markers, "list_listpar_indent")),
        list_label_sep_twips,
        list_label_width_twips,
    }
}

fn collect_probe_markers(log: &str) -> HashMap<String, String> {
    let mut markers = HashMap::new();

    for raw_line in log.lines() {
        let line = raw_line.trim();
        let Some(rest) = line.strip_prefix(PROBE_MARKER_PREFIX) else {
            continue;
        };
        let Some((key, value)) = rest.split_once('=') else {
            continue;
        };
        markers.insert(key.trim().to_string(), value.trim().to_string());
    }

    markers
}

fn read_length_twips(markers: &HashMap<String, String>, key: &str) -> Option<i32> {
    let value = markers.get(key)?;
    let points = parse_points(value)?;
    pt_to_twips_rounded(points)
}

fn read_font_size_half_points(markers: &HashMap<String, String>, key: &str) -> Option<usize> {
    let value = markers.get(key)?;
    let points = parse_points(value)?;
    if points <= 0.0 {
        return None;
    }

    let half_points = (points * 2.0).round();
    if !half_points.is_finite() || half_points <= 0.0 || half_points > usize::MAX as f64 {
        return None;
    }

    Some(half_points as usize)
}

fn read_font_family(markers: &HashMap<String, String>) -> Option<String> {
    if let Some(name) = markers
        .get("body_font_name")
        .and_then(|raw| normalize_font_name(raw))
    {
        return Some(name);
    }

    let family_code = markers
        .get("body_font_family")
        .map(String::as_str)
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();

    match family_code.as_str() {
        "ptm" => Some("Times New Roman".to_string()),
        "phv" => Some("Arial".to_string()),
        "pcr" => Some("Courier New".to_string()),
        _ => None,
    }
}

fn normalize_font_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut candidate = trimmed
        .trim_matches(|c| c == '[' || c == ']' || c == '"' || c == '\'')
        .split(':')
        .next()
        .unwrap_or(trimmed)
        .split('/')
        .next()
        .unwrap_or(trimmed)
        .replace('_', " ");

    candidate = candidate.trim().to_string();
    if candidate.is_empty() {
        return None;
    }

    // Avoid overriding parser values with low-signal NFSS/internal aliases.
    let lower = candidate.to_ascii_lowercase();
    let looks_internal = lower.starts_with("cm")
        || lower.starts_with("lm")
        || lower.contains("font")
        || lower
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-');
    if looks_internal {
        return None;
    }

    Some(candidate)
}

fn parse_points(raw: &str) -> Option<f64> {
    let token = raw
        .trim()
        .trim_end_matches("pt")
        .split_whitespace()
        .next()?;
    token.parse::<f64>().ok()
}

fn probe_log_excerpt(log_text: &str) -> String {
    let mut selected = Vec::new();
    for raw_line in log_text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('!')
            || line.contains("Error")
            || line.contains("Undefined control sequence")
            || line.contains("Emergency stop")
        {
            selected.push(line.to_string());
            if selected.len() == 3 {
                break;
            }
        }
    }

    if selected.is_empty() {
        selected.extend(
            log_text
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .take(3)
                .map(ToOwned::to_owned),
        );
    }

    selected.join(" | ")
}

fn probe_log_has_tex_errors(log_text: &str) -> bool {
    log_text.lines().any(|raw_line| {
        let line = raw_line.trim();
        line.starts_with('!')
            || line.contains("LaTeX Error")
            || line.contains("Undefined control sequence")
            || line.contains("Emergency stop")
            || line.contains("Fatal error")
    })
}

fn build_probe_confidence_model(
    log_text: &str,
    has_run_error: bool,
    has_log_errors: bool,
) -> ProbeConfidenceModel {
    if !has_run_error && !has_log_errors {
        return ProbeConfidenceModel::default();
    }

    let mut model = ProbeConfidenceModel::default();
    let lower = log_text.to_ascii_lowercase();

    let has_general_failure = has_run_error
        || lower.contains("undefined control sequence")
        || lower.contains("emergency stop")
        || lower.contains("fatal error")
        || log_text
            .lines()
            .any(|line| line.trim_start().starts_with('!'));

    if has_general_failure {
        model.font_size_body_hp = FieldConfidence::Degraded;
        model.body_line_spacing_twips = FieldConfidence::Degraded;
        return model;
    }

    let has_font_metric_risk = lower.contains("fontspec")
        || lower.contains("font warning")
        || lower.contains("font shape")
        || lower.contains("font not found")
        || lower.contains("missing character");
    if has_font_metric_risk {
        model.font_size_body_hp = FieldConfidence::Degraded;
    }

    let has_spacing_metric_risk = lower.contains("illegal unit of measure")
        || lower.contains("missing number")
        || lower.contains("bad register code")
        || lower.contains("dimension too large");
    if has_spacing_metric_risk {
        model.body_line_spacing_twips = FieldConfidence::Degraded;
    }

    // Conservative fallback: when TeX reports errors but risk class is unknown,
    // degrade typography-sensitive fields rather than emitting potentially
    // inflated Word spacing/size mappings.
    if model.downgraded_fields().is_empty() {
        model.font_size_body_hp = FieldConfidence::Degraded;
        model.body_line_spacing_twips = FieldConfidence::Degraded;
    }

    model
}

fn apply_probe_confidence_model(
    probe_output: &mut LayoutProbeOutput,
    confidence_model: ProbeConfidenceModel,
) {
    if confidence_model.font_size_body_hp == FieldConfidence::Degraded {
        probe_output.font_size_body_hp = None;
    }
    if confidence_model.body_line_spacing_twips == FieldConfidence::Degraded {
        probe_output.body_line_spacing_twips = None;
    }
}

const PROBE_BODY: &str = r#"
\makeatletter
\newcommand{\fxtprobeemit}[2]{\typeout{FERRITEX_PROBE:#1=#2}}
\newcommand{\fxtprobeemitlen}[2]{%
  \begingroup
    \dimen0=#2\relax
    \edef\fxtprobedim{\strip@pt\dimen0}%
    \typeout{FERRITEX_PROBE:#1=\fxtprobedim pt}%
  \endgroup
}
\newcommand{\fxtprobeemitmargins}{%
  \@ifundefined{Gm@lmargin}{%
    \fxtprobeemitlen{page_margin_left}{\dimexpr 1in+\oddsidemargin\relax}%
  }{%
    \fxtprobeemitlen{page_margin_left}{\Gm@lmargin}%
  }%
  \@ifundefined{Gm@rmargin}{%
    \fxtprobeemitlen{page_margin_right}{\dimexpr \paperwidth-\textwidth-(1in+\oddsidemargin)\relax}%
  }{%
    \fxtprobeemitlen{page_margin_right}{\Gm@rmargin}%
  }%
  \@ifundefined{Gm@tmargin}{%
    \fxtprobeemitlen{page_margin_top}{\dimexpr 1in+\topmargin+\headheight+\headsep\relax}%
  }{%
    \fxtprobeemitlen{page_margin_top}{\Gm@tmargin}%
  }%
  \@ifundefined{Gm@bmargin}{%
    \fxtprobeemitlen{page_margin_bottom}{\dimexpr \paperheight-\textheight-(1in+\topmargin+\headheight+\headsep)\relax}%
  }{%
    \fxtprobeemitlen{page_margin_bottom}{\Gm@bmargin}%
  }%
  \@ifundefined{Gm@bindingoffset}{%
    \fxtprobeemitlen{page_gutter}{0pt}%
  }{%
    \fxtprobeemitlen{page_gutter}{\Gm@bindingoffset}%
  }%
}
\newcommand{\fxtprobeemitfontinfo}{%
  \fxtprobeemit{body_font_size_pt}{\f@size}%
  \fxtprobeemit{body_font_family}{\f@family}%
}
\makeatother

\begin{document}
\normalsize
\fxtprobeemitlen{page_width}{\paperwidth}
\fxtprobeemitlen{page_height}{\paperheight}
\fxtprobeemitmargins
\fxtprobeemitlen{page_margin_header}{\headsep}
\fxtprobeemitlen{page_margin_footer}{\footskip}
\fxtprobeemitfontinfo
\fxtprobeemit{body_font_name}{\fontname\font}
\fxtprobeemitlen{body_parindent}{\parindent}
\fxtprobeemitlen{body_line_spacing}{\baselineskip}

\begin{itemize}
\fxtprobeemitlen{list_left_indent}{\leftmargin}
\fxtprobeemitlen{list_label_sep}{\labelsep}
\fxtprobeemitlen{list_label_width}{\labelwidth}
\fxtprobeemitlen{list_item_indent}{\itemindent}
\fxtprobeemitlen{list_listpar_indent}{\listparindent}
\item ferritex probe item
\end{itemize}
\end{document}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_probe_markers_maps_lengths_and_font_size() {
        let log = "\
FERRITEX_PROBE:page_margin_left=72 pt
FERRITEX_PROBE:page_width=595 pt
FERRITEX_PROBE:body_font_size_pt=14
FERRITEX_PROBE:list_label_sep=6 pt
FERRITEX_PROBE:list_label_width=8 pt
";

        let parsed = parse_probe_log(log);
        assert_eq!(parsed.page_margin_left_twips, Some(1440));
        assert_eq!(parsed.page_width_twips, Some(11_900));
        assert_eq!(parsed.font_size_body_hp, Some(28));
        assert_eq!(parsed.list_hanging_indent_twips, Some(280));
    }

    #[test]
    fn parse_points_accepts_pt_suffix_and_plain_numbers() {
        assert_eq!(parse_points("12.5pt"), Some(12.5));
        assert_eq!(parse_points("12.5 pt"), Some(12.5));
        assert_eq!(parse_points("12.5"), Some(12.5));
    }

    #[test]
    fn probe_log_error_detection_detects_tex_error_markers() {
        let clean_log = "This is XeTeX\nFERRITEX_PROBE:page_width=595 pt\nOutput written";
        assert!(!probe_log_has_tex_errors(clean_log));

        let error_log = "This is XeTeX\n! Undefined control sequence.\nl.42 \\badmacro";
        assert!(probe_log_has_tex_errors(error_log));
    }

    #[test]
    fn probe_confidence_model_general_failure_degrades_both_typography_fields() {
        let model = build_probe_confidence_model("! Undefined control sequence", false, true);
        assert_eq!(
            model.downgraded_fields(),
            vec!["font_size_body_hp", "body_line_spacing_twips"]
        );
    }

    #[test]
    fn probe_confidence_model_font_error_degrades_font_only() {
        let model = build_probe_confidence_model(
            "LaTeX Error: fontspec error: The font \"Foo\" cannot be found.",
            false,
            true,
        );
        assert_eq!(model.downgraded_fields(), vec!["font_size_body_hp"]);
    }

    #[test]
    fn probe_confidence_model_spacing_error_degrades_spacing_only() {
        let model = build_probe_confidence_model(
            "LaTeX Error: Illegal unit of measure (pt inserted).",
            false,
            true,
        );
        assert_eq!(model.downgraded_fields(), vec!["body_line_spacing_twips"]);
    }

    #[test]
    fn probe_confidence_model_application_clears_only_degraded_fields() {
        let mut probe = LayoutProbeOutput {
            page_margin_left_twips: Some(1_200),
            font_size_body_hp: Some(29),
            body_line_spacing_twips: Some(485),
            list_label_sep_twips: Some(120),
            ..LayoutProbeOutput::default()
        };

        let model = ProbeConfidenceModel {
            font_size_body_hp: FieldConfidence::Trusted,
            body_line_spacing_twips: FieldConfidence::Degraded,
        };
        apply_probe_confidence_model(&mut probe, model);

        assert_eq!(probe.font_size_body_hp, Some(29));
        assert_eq!(probe.body_line_spacing_twips, None);
        assert_eq!(probe.page_margin_left_twips, Some(1_200));
        assert_eq!(probe.list_label_sep_twips, Some(120));
    }

    #[test]
    fn infer_total_pages_from_pdf_bytes_reads_max_count_marker() {
        let sample = br#"%PDF-1.7
1 0 obj << /Type /Pages /Count 3 >>
2 0 obj << /Type /Pages /Count 17 >>
trailer << /Root 1 0 R >>
"#;
        assert_eq!(infer_total_pages_from_pdf_bytes(sample), Some(17));
    }

    #[test]
    fn infer_total_pages_from_pdf_bytes_returns_none_without_count_marker() {
        let sample = br#"%PDF-1.7
1 0 obj << /Type /Catalog >>
trailer << /Root 1 0 R >>
"#;
        assert_eq!(infer_total_pages_from_pdf_bytes(sample), None);
    }

    #[test]
    fn build_probe_tex_engine_uses_runtime_profile() {
        let profile = format!("{:?}", build_probe_tex_engine());
        assert!(profile.contains("halt_on_error: false"));
        assert!(profile.contains("shell_escape_enabled: false"));
    }
}
