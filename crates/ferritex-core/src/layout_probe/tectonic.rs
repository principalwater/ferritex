use std::{collections::HashMap, path::Path};

use anyhow::{Result, anyhow};
use tectonic::{
    config::PersistentConfig,
    driver::{OutputFormat, ProcessingSessionBuilder},
    status::NoopStatusBackend,
};

use crate::model::LayoutProbeOutput;

use super::pt_to_twips_rounded;

const PROBE_MARKER_PREFIX: &str = "FERRITEX_PROBE:";
const PROBE_TEX_INPUT_NAME: &str = "ferritex_probe.tex";
const PROBE_LOG_NAME: &str = "ferritex_probe.log";

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
        .do_not_write_output_files();

    let mut session = builder
        .create(&mut status)
        .map_err(|error| anyhow!("failed to create tectonic processing session: {error}"))?;
    session
        .run(&mut status)
        .map_err(|error| anyhow!("tectonic layout probe run failed: {error}"))?;

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

    Ok(parse_probe_log(&log_text))
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
        .trim()
        .split_whitespace()
        .next()?;
    token.parse::<f64>().ok()
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
}
