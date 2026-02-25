use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use crate::layout_probe::{active_probe_backend, merge_probe_and_parser_layout, probe_layout};
use crate::model::{
    Block, Document, DocumentLayout, Figure, Inline, LayoutProbeOutput, List, PageOrientation,
    ParagraphStyle, Table, TableCell, TableRow, TocEntry,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutociteMode {
    InlinePlaceholder,
    FootnotePlaceholder,
}

#[derive(Debug, Clone, Default)]
struct BibEntry {
    fields: HashMap<String, String>,
}

#[derive(Debug, Clone, Default)]
struct ParseMetadata {
    counters: HashMap<String, i64>,
    text_counters: HashMap<String, String>,
    bibliography: HashMap<String, BibEntry>,
    /// LaTeX document class name (e.g. `"memoir"`, `"disser"`, `"article"`).
    /// `None` when no `\documentclass` is found (e.g. in test snippets).
    document_class: Option<String>,
}

const DEFAULT_PAGE_WIDTH_TWIPS: i32 = 11_906;
const DEFAULT_PAGE_MARGIN_LEFT_TWIPS: i32 = 1_138;
const DEFAULT_PAGE_MARGIN_RIGHT_TWIPS: i32 = 288;
const DEFAULT_BODY_FONT_SIZE_HP: usize = 28;
const DEFAULT_BODY_LINE_SPACING_TWIPS: i32 = 360;
/// DOCX auto line-spacing unit: `240` represents single spacing.
const DOCX_AUTO_LINE_SPACING_UNIT_TWIPS: f64 = 240.0;
/// DOCX conversion constant: `1pt = 20 twips`.
const DOCX_TWIPS_PER_POINT_F64: f64 = 20.0;
/// TeX pica: `1pc = 12pt`.
const TWIPS_PER_PICA_F64: f64 = 12.0 * DOCX_TWIPS_PER_POINT_F64;
/// TeX big point: `1bp = 1/72in = 20 twips`.
const TWIPS_PER_BIG_POINT_F64: f64 = 20.0;
/// TeX scaled point: `65536sp = 1pt`.
const TWIPS_PER_SCALED_POINT_F64: f64 = DOCX_TWIPS_PER_POINT_F64 / 65_536.0;
/// TeX didot point: `1157dd = 1238pt`.
const TWIPS_PER_DIDOT_POINT_F64: f64 = DOCX_TWIPS_PER_POINT_F64 * (1238.0 / 1157.0);
/// TeX cicero: `1cc = 12dd`.
const TWIPS_PER_CICERO_F64: f64 = TWIPS_PER_DIDOT_POINT_F64 * 12.0;
/// Approximate x-height used for `ex` conversion when font metrics are unavailable.
const EX_TO_EM_RATIO_F64: f64 = 0.430_556;
/// Approximate average glyph width for serif text, in `em`.
///
/// Used only for mapping `flushright + tabular{l}` blocks to a fixed-width
/// right-aligned text block in DOCX without corpus-specific constants.
const ESTIMATED_AVERAGE_CHAR_WIDTH_EM: f64 = 0.49;

/// Parse a LaTeX source string into a [`Document`] AST.
///
/// Supported constructs:
/// - `\chapter{…}`, `\section{…}`, `\subsection{…}`, `\subsubsection{…}`
/// - Plain paragraphs (blank-line separated)
/// - `\textbf{…}`, `\textit{…}`, `{\bf …}`, `{\it …}`
/// - `\label{…}`, `\ref{…}`, `\cite{…}` — emitted as placeholder text
/// - `\autocite{…}` — style-aware placeholder:
///   - inline (`[key]`) by default
///   - footnote placeholder if the project style config enables footnote autocites
/// - `\begin{table}…\end{table}` with `\begin{tabular}` / `\begin{tblr}` / `\begin{longtblr}`
/// - `\begin{figure}…\end{figure}` with `\includegraphics` and `\caption`
/// - Display math blocks: `\begin{equation}…\end{equation}` / `equation*` / `\[…\]`
/// - `\footnote{…}` inline notes
/// - `\tablesource{…}` / `\figuresource{…}` — stored as source attribution
/// - Preamble directives (`\documentclass`, `\usepackage`, `\begin{document}`,
///   `\end{document}`) are silently skipped.
pub fn parse_latex(source: &str) -> Document {
    let autocite_mode = detect_autocite_mode(source);
    parse_latex_with_mode(
        source,
        autocite_mode,
        &ParseMetadata::default(),
        false,
        false,
    )
}

/// Parse an entry `.tex` file with recursive `\input{...}` / `\include{...}` expansion.
///
/// Missing input files are kept as-is in the expanded source (best-effort mode).
pub fn parse_latex_file(input_path: &Path) -> anyhow::Result<Document> {
    let root_dir = input_path.parent().unwrap_or_else(|| Path::new("."));
    let mut stack = Vec::new();
    let expanded = expand_inputs_recursive(input_path, root_dir, &mut stack)?;
    let probe_layout_output = match probe_layout(input_path, &expanded) {
        Ok(output) => output,
        Err(error) => {
            log::warn!(
                "LayoutProbe backend '{}' failed for {}: {error}; falling back to parser-only extraction.",
                active_probe_backend(),
                input_path.display()
            );
            LayoutProbeOutput::default()
        }
    };
    let probe_fields = probe_layout_output.populated_field_names();
    if probe_fields.is_empty() {
        log::debug!(
            "LayoutProbe backend '{}' produced no extracted fields for {}.",
            active_probe_backend(),
            input_path.display()
        );
    } else {
        log::info!(
            "LayoutProbe backend '{}' extracted {} field(s): {}",
            active_probe_backend(),
            probe_fields.len(),
            probe_fields.join(", ")
        );
    }
    let autocite_mode = detect_autocite_mode(&expanded);
    let mut metadata = collect_parse_metadata(&expanded, input_path, root_dir);
    let mut document = parse_latex_with_mode(&expanded, autocite_mode, &metadata, true, true);
    document.layout = merge_probe_and_parser_layout(&probe_layout_output, document.layout);
    enrich_structural_counters(&mut metadata, &document, &expanded);
    resolve_dynamic_placeholders(&mut document.blocks, &metadata);
    resolve_footnote_citation_placeholders(&mut document.blocks, &metadata.bibliography);
    resolve_citation_placeholders(&mut document.blocks, &metadata.bibliography);
    inject_bibliography_entries(&mut document.blocks, &metadata.bibliography);
    document.toc_entries =
        parse_toc_entries_from_sidecar(input_path, autocite_mode, &metadata, &expanded);
    Ok(document)
}

fn parse_toc_entries_from_sidecar(
    input_path: &Path,
    autocite_mode: AutociteMode,
    metadata: &ParseMetadata,
    source: &str,
) -> Vec<TocEntry> {
    let toc_path = input_path.with_extension("toc");
    let Ok(raw) = std::fs::read_to_string(toc_path) else {
        return Vec::new();
    };

    raw.lines()
        .filter_map(|line| parse_toc_line(line, autocite_mode, metadata, source))
        .collect()
}

fn parse_toc_line(
    line: &str,
    autocite_mode: AutociteMode,
    metadata: &ParseMetadata,
    source: &str,
) -> Option<TocEntry> {
    parse_toc_contents_line(line, autocite_mode, metadata)
        .or_else(|| parse_toc_helper_line(line, autocite_mode, metadata, source))
}

fn parse_toc_contents_line(
    line: &str,
    autocite_mode: AutociteMode,
    metadata: &ParseMetadata,
) -> Option<TocEntry> {
    let marker = "\\contentsline";
    let pos = line.find(marker)?;
    let mut rest = &line[pos + marker.len()..];

    let (kind, next) = consume_braced_argument(rest)?;
    rest = next;
    let (payload, next) = consume_braced_argument(rest)?;
    rest = next;
    let (page_raw, _next) = consume_braced_argument(rest)?;

    let kind = kind.trim();
    let level = match kind {
        "chapter" => 1,
        "section" => 2,
        "subsection" => 3,
        "subsubsection" => 4,
        _ => return None,
    };

    let (number, title_latex) = extract_toc_number_and_title(payload.trim(), level);
    let title_inlines = parse_inlines(&title_latex, autocite_mode, metadata, true);
    let title = normalize_whitespace(&plain_text_from_inlines(&title_inlines));
    if title.is_empty() {
        return None;
    }

    let page_inlines = parse_inlines(page_raw.trim(), autocite_mode, metadata, true);
    let page = normalize_whitespace(&plain_text_from_inlines(&page_inlines));
    let page = if page.is_empty() { None } else { Some(page) };

    Some(TocEntry {
        level,
        number,
        title,
        page,
    })
}

fn parse_toc_helper_line(
    line: &str,
    autocite_mode: AutociteMode,
    metadata: &ParseMetadata,
    source: &str,
) -> Option<TocEntry> {
    let mut trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed = trimmed.trim_end_matches('%').trim_end();
    if let Some(rest) = trimmed.strip_prefix("\\protect") {
        trimmed = rest.trim_start();
    }

    let (command_name, consumed) = parse_control_word(trimmed)?;
    if command_name == "contentsline" {
        return None;
    }
    if !trimmed[consumed..].trim().is_empty() {
        return None;
    }

    let title = extract_toc_helper_text(command_name, source, autocite_mode, metadata)?;
    Some(TocEntry {
        level: 0,
        number: None,
        title,
        page: None,
    })
}

fn extract_toc_helper_text(
    command_name: &str,
    source: &str,
    autocite_mode: AutociteMode,
    metadata: &ParseMetadata,
) -> Option<String> {
    let body = extract_renewcommand_value(source, command_name)?;
    let inlines = parse_inlines(&body, autocite_mode, metadata, false);
    let title = normalize_whitespace(&plain_text_from_inlines(&inlines))
        .trim()
        .to_string();
    if title.is_empty() { None } else { Some(title) }
}

fn consume_braced_argument(src: &str) -> Option<(String, &str)> {
    let trimmed = src.trim_start();
    let len = braced_len(trimmed)?;
    let payload = trimmed[1..len - 1].to_string();
    Some((payload, &trimmed[len..]))
}

fn extract_toc_number_and_title(payload: &str, level: u8) -> (Option<String>, String) {
    let payload = unwrap_makeuppercase_macro(payload).unwrap_or_else(|| payload.to_string());

    if let Some((number, title)) = extract_numberline_payload(&payload, "\\chapternumberline") {
        let number = if level == 1 {
            Some(format!("{}.", number.trim().trim_end_matches('.')))
        } else {
            Some(number.trim().to_string())
        };
        return (number, title.trim().to_string());
    }
    if let Some((number, title)) = extract_numberline_payload(&payload, "\\numberline") {
        return (
            Some(number.trim().trim_end_matches('.').to_string()),
            title.trim().to_string(),
        );
    }

    (None, payload.trim().to_string())
}

fn unwrap_makeuppercase_macro(payload: &str) -> Option<String> {
    let mut rest = payload.trim_start();
    rest = rest.strip_prefix("\\MakeUppercase")?.trim_start();
    if rest.starts_with('[') {
        let opt_len = bracketed_len(rest)?;
        rest = rest[opt_len..].trim_start();
    }
    let len = braced_len(rest)?;
    Some(rest[1..len - 1].to_string())
}

fn extract_numberline_payload(payload: &str, cmd: &str) -> Option<(String, String)> {
    let mut rest = payload.trim_start();
    rest = rest.strip_prefix(cmd)?.trim_start();
    let len = braced_len(rest)?;
    let number = rest[1..len - 1].to_string();
    let title = rest[len..].to_string();
    Some((number, title))
}

fn plain_text_from_inlines(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for inline in inlines {
        match inline {
            Inline::Text(text) => out.push_str(text),
            Inline::LineBreak => out.push(' '),
            Inline::Bold(children) | Inline::Italic(children) | Inline::Footnote(children) => {
                out.push_str(&plain_text_from_inlines(children))
            }
            Inline::InlineMath(src) | Inline::Reference(src) => out.push_str(src),
        }
    }
    out
}

fn parse_latex_with_mode(
    source: &str,
    autocite_mode: AutociteMode,
    metadata: &ParseMetadata,
    preserve_dynamic_markers: bool,
    preserve_reference_nodes: bool,
) -> Document {
    let source = strip_comments(source);
    let layout = extract_layout_settings(&source);
    let source = expand_simple_macros(&source);
    let declared_labels = collect_declared_labels(&source);
    let body = extract_document_body(&source);
    let filtered = filter_skippable_lines(body);
    // Segment the source into typed spans before paragraph-splitting,
    // so that multi-line environments are kept intact.
    let segments = segment(&filtered);
    let mut blocks = Vec::new();
    let mut text_flow_state = TextFlowState::default();

    for seg in segments {
        match seg {
            Segment::Float(content) => {
                if let Some(block) =
                    parse_float(&content, autocite_mode, metadata, preserve_dynamic_markers)
                {
                    blocks.push(block);
                }
            }
            Segment::Text(content) => {
                for raw_chunk in split_paragraphs(&content) {
                    let Some(prepared) =
                        prepare_text_chunk(&raw_chunk, &mut text_flow_state, &layout)
                    else {
                        continue;
                    };
                    let (leading_structural_blocks, remaining_text) =
                        consume_leading_structural_blocks(prepared.text.as_str());
                    blocks.extend(leading_structural_blocks);
                    if remaining_text.trim().is_empty() {
                        continue;
                    }

                    if let Some(label) = extract_standalone_label(remaining_text) {
                        attach_standalone_label(&mut blocks, label);
                        continue;
                    }

                    let heading_candidate = strip_heading_prefix_noise(remaining_text);
                    if let Some(block) = try_parse_section(
                        heading_candidate,
                        autocite_mode,
                        metadata,
                        preserve_dynamic_markers,
                    ) {
                        blocks.push(block);
                    } else if let Some(block) = try_parse_plain_heading(
                        heading_candidate,
                        autocite_mode,
                        metadata,
                        layout.document_language.as_deref(),
                        preserve_dynamic_markers,
                    ) {
                        blocks.push(block);
                    } else if let Some(block) = try_parse_structural_heading_command(remaining_text)
                    {
                        blocks.push(block);
                    } else if let Some(block) = try_parse_bibliography_command(
                        remaining_text,
                        layout.document_language.as_deref(),
                    ) {
                        blocks.push(block);
                    } else {
                        let cleaned_chunk = trim_spaces_around_manual_linebreaks(remaining_text);
                        let inlines = parse_inlines(
                            cleaned_chunk.as_str(),
                            autocite_mode,
                            metadata,
                            preserve_dynamic_markers,
                        );
                        if !inlines.is_empty()
                            && !is_single_brace_paragraph(&inlines)
                            && !is_only_linebreaks(&inlines)
                        {
                            if let Some(mut style) = prepared.style {
                                if let Some(left_indent) =
                                    estimate_flushright_tabular_left_indent_twips(
                                        remaining_text,
                                        &inlines,
                                        &layout,
                                        style.font_size_hp,
                                    )
                                {
                                    style.alignment = Some("left".to_string());
                                    style.left_indent_twips = Some(left_indent);
                                    style.first_line_indent_twips = Some(0);
                                }
                                blocks.push(Block::StyledParagraph { inlines, style });
                            } else {
                                blocks.push(Block::Paragraph(inlines));
                            }
                        }
                    }
                }
            }
        }
    }

    assign_section_numbers(&mut blocks);
    resolve_references(
        &mut blocks,
        &declared_labels,
        &layout,
        preserve_reference_nodes,
    );
    Document {
        blocks,
        layout,
        toc_entries: Vec::new(),
    }
}

fn strip_heading_prefix_noise(mut chunk: &str) -> &str {
    loop {
        let trimmed = chunk.trim_start();
        if trimmed.is_empty() {
            return trimmed;
        }
        if let Some(rest) = trimmed.strip_prefix('{') {
            chunk = rest;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('}') {
            chunk = rest;
            continue;
        }
        if trimmed.starts_with('\\') {
            if starts_with_section_command(trimmed) {
                return trimmed;
            }
            if let Some(consumed) = consume_leading_control_command(trimmed) {
                chunk = &trimmed[consumed..];
                continue;
            }
        }
        return trimmed;
    }
}

fn starts_with_section_command(src: &str) -> bool {
    src.starts_with("\\chapter")
        || src.starts_with("\\section")
        || src.starts_with("\\subsection")
        || src.starts_with("\\subsubsection")
}

fn consume_leading_control_command(src: &str) -> Option<usize> {
    if let Some(_rest) = src.strip_prefix("\\\\") {
        let mut consumed = 2usize;
        if let Some(arg_len) = bracketed_len(&src[consumed..]) {
            consumed += arg_len;
        }
        return Some(consumed);
    }
    if !src.starts_with('\\') {
        return None;
    }

    let mut consumed = 1usize;
    while consumed < src.len() && src.as_bytes()[consumed].is_ascii_alphabetic() {
        consumed += 1;
    }
    if consumed == 1 {
        return Some(1);
    }
    if src[consumed..].starts_with('*') {
        consumed += 1;
    }

    loop {
        while consumed < src.len() && src.as_bytes()[consumed].is_ascii_whitespace() {
            consumed += 1;
        }
        if consumed >= src.len() {
            break;
        }

        if src[consumed..].starts_with('{')
            && let Some(arg_len) = braced_len(&src[consumed..])
        {
            consumed += arg_len;
            continue;
        }
        if src[consumed..].starts_with('[')
            && let Some(arg_len) = bracketed_len(&src[consumed..])
        {
            consumed += arg_len;
            continue;
        }
        break;
    }

    Some(consumed)
}

fn detect_autocite_mode(source: &str) -> AutociteMode {
    let source = strip_comments(source);
    let mut mode = AutociteMode::InlinePlaceholder;

    for raw_line in source.lines() {
        let line = raw_line.trim();
        if line.contains("\\setcounter{usefootcite}{1}") {
            mode = AutociteMode::FootnotePlaceholder;
        } else if line.contains("\\setcounter{usefootcite}{0}") {
            mode = AutociteMode::InlinePlaceholder;
        }

        if let Some(value) = extract_latex_option_value(line, "autocite") {
            if value.eq_ignore_ascii_case("footnote") {
                mode = AutociteMode::FootnotePlaceholder;
            } else {
                mode = AutociteMode::InlinePlaceholder;
            }
        }

        if let Some(value) = extract_latex_option_value(line, "citestyle")
            && value.to_ascii_lowercase().contains("footnote")
        {
            mode = AutociteMode::FootnotePlaceholder;
        }
    }

    mode
}

fn extract_latex_option_value(line: &str, key: &str) -> Option<String> {
    let idx = line.find(key)?;
    let after_key = &line[idx + key.len()..];
    let after_key = after_key.trim_start();
    let after_eq = after_key.strip_prefix('=')?.trim_start();
    let end = after_eq
        .find(|c: char| c == ',' || c == ']' || c == '}' || c.is_whitespace())
        .unwrap_or(after_eq.len());
    let value = after_eq[..end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn extract_layout_settings(source: &str) -> DocumentLayout {
    let mut layout = DocumentLayout::default();
    let preamble = source
        .split_once("\\begin{document}")
        .map(|(head, _)| head)
        .unwrap_or(source);
    layout.font_size_body_hp = extract_documentclass_fontsize_hp(source);
    let documentclass_name = extract_documentclass_name(source);

    if let Some(options) = extract_last_macro_braced_argument(source, "\\geometry") {
        layout.page_margin_top_twips =
            extract_latex_option_value(&options, "top").and_then(|value| {
                parse_latex_length_to_twips_with_body_font(&value, layout.font_size_body_hp)
            });
        layout.page_margin_bottom_twips =
            extract_latex_option_value(&options, "bottom").and_then(|value| {
                parse_latex_length_to_twips_with_body_font(&value, layout.font_size_body_hp)
            });
        layout.page_margin_left_twips =
            extract_latex_option_value(&options, "left").and_then(|value| {
                parse_latex_length_to_twips_with_body_font(&value, layout.font_size_body_hp)
            });
        layout.page_margin_right_twips =
            extract_latex_option_value(&options, "right").and_then(|value| {
                parse_latex_length_to_twips_with_body_font(&value, layout.font_size_body_hp)
            });
        layout.page_gutter_twips =
            extract_latex_option_value(&options, "bindingoffset").and_then(|value| {
                parse_latex_length_to_twips_with_body_font(&value, layout.font_size_body_hp)
            });
    }

    if let Some(factor) = extract_last_spacing_factor(preamble, layout.font_size_body_hp) {
        layout.body_line_spacing_twips = spacing_factor_to_twips(factor);
    }

    if let Some(header_twips) =
        extract_setlength_value_twips_with_body_font(source, "headsep", layout.font_size_body_hp)
    {
        layout.page_margin_header_twips = Some(header_twips);
    }
    if let Some(footer_twips) =
        extract_setlength_value_twips_with_body_font(source, "footskip", layout.font_size_body_hp)
    {
        layout.page_margin_footer_twips = Some(footer_twips);
    }

    // Extract float counter scoping from \setcounter{contnumfig/tab/eq}{0|1}.
    // Convention (shared by many LaTeX dissertation/thesis style packages):
    //   contnumfig=1  → global figure counter  → figure_counter_within_chapter = false
    //   contnumfig=0  → per-chapter counter     → figure_counter_within_chapter = true
    // Fallback when absent: renderer decides (typically per-chapter, matching LaTeX default).
    if let Some(val) = extract_last_setcounter_value(source, "contnumfig") {
        layout.figure_counter_within_chapter = Some(val == 0);
    }
    if let Some(val) = extract_last_setcounter_value(source, "contnumtab") {
        layout.table_counter_within_chapter = Some(val == 0);
    }
    if let Some(val) = extract_last_setcounter_value(source, "contnumeq") {
        layout.equation_counter_within_chapter = Some(val == 0);
    }

    // ── Font family ────────────────────────────────────────────────────
    // XeLaTeX / LuaLaTeX: \setmainfont{Times New Roman}
    // The font may be selected conditionally via \ifnumequal{\value{fontfamily}}{N}.
    // Try conditional extraction first; fall back to last unconditional occurrence.
    let fontfamily_val = extract_last_setcounter_value(source, "fontfamily");
    if let Some(font) = extract_setmainfont_conditional(source, fontfamily_val) {
        layout.font_family_body = Some(font);
    } else if let Some(font) = extract_last_macro_braced_argument(source, "\\setmainfont") {
        let font = font.trim().to_string();
        if !font.is_empty() {
            layout.font_family_body = Some(font);
        }
    }

    // ── Table body font size ─────────────────────────────────────────
    // \SetTblrInner{font=\footnotesize} → 12pt at 14pt base
    layout.font_size_table_hp = extract_tblr_inner_font_size_hp(source, &layout);

    // ── Caption font size ────────────────────────────────────────────
    // \captionsetup[table]{font={normalsize,bf}} or \captionsetup[figure]{font={normalsize,bf}}
    layout.font_size_caption_hp = extract_captionsetup_font_size_hp(source, &layout);

    // ── Footnote font size ───────────────────────────────────────────
    // Derive from body size: 14pt → 10pt, 12pt → 8pt (LaTeX convention: body − 4pt).
    if layout.font_size_footnote_hp.is_none()
        && let Some(body_hp) = layout.font_size_body_hp
    {
        // Standard LaTeX \footnotesize is body − 4pt (in half-points: − 8).
        let footnote_hp = body_hp.saturating_sub(8);
        if footnote_hp >= 12 {
            // Minimum 6pt
            layout.font_size_footnote_hp = Some(footnote_hp);
        }
    }

    // ── Paragraph indent ───────────────────────────────────────────────
    if let Some(indent_twips) =
        extract_setlength_value_twips_with_body_font(source, "parindent", layout.font_size_body_hp)
    {
        layout.body_first_line_indent_twips = Some(indent_twips);
    }

    // ── Caption labels ─────────────────────────────────────────────────
    // \renewcommand{\figurename}{Рисунок}
    if let Some(name) = extract_renewcommand_value(source, "figurename") {
        layout.caption_label_figure = Some(name);
    }
    // \renewcommand{\tablename}{Таблица}
    if let Some(name) = extract_renewcommand_value(source, "tablename") {
        layout.caption_label_table = Some(name);
    }
    layout.caption_label_separator_figure = extract_captionsetup_label_separator(source, "figure");
    layout.caption_label_separator_table = extract_captionsetup_label_separator(source, "table");
    layout.caption_label_bold_figure = extract_captionsetup_labelfont_bold(source, "figure");
    layout.caption_label_bold_table = extract_captionsetup_labelfont_bold(source, "table");
    layout.caption_skip_twips_figure =
        extract_captionsetup_skip_twips_with_body_font(source, "figure", layout.font_size_body_hp);
    layout.caption_skip_twips_table =
        extract_captionsetup_skip_twips_with_body_font(source, "table", layout.font_size_body_hp);
    layout.caption_position_figure = extract_captionsetup_position(source, "figure");
    layout.caption_position_table = extract_captionsetup_position(source, "table");
    layout.caption_singlelinecheck_figure = extract_captionsetup_singlelinecheck(source, "figure");
    layout.caption_singlelinecheck_table = extract_captionsetup_singlelinecheck(source, "table");
    layout.caption_indent_twips_figure = extract_captionsetup_indent_twips_with_body_font(
        source,
        "figure",
        layout.font_size_body_hp,
    );
    layout.caption_indent_twips_table =
        extract_captionsetup_indent_twips_with_body_font(source, "table", layout.font_size_body_hp);

    // ── Chapter name prefix ────────────────────────────────────────────
    // \renewcommand{\chaptername}{Глава}
    if let Some(name) = extract_renewcommand_value(source, "chaptername") {
        layout.chapter_name = Some(name);
    }

    // ── Heading uppercase detection ────────────────────────────────────
    // Detect \MakeUppercase in \printchaptertitle or chapter format definitions.
    if source.contains("\\MakeUppercase") {
        // Only flag uppercase if it appears in heading-format context, not just anywhere.
        if source.contains("printchaptertitle") || source.contains("chapnamefont") {
            layout.heading_uppercase = Some(true);
        }
    }

    // ── Page size ─────────────────────────────────────────────────────
    // \geometry{paperwidth=210mm, paperheight=297mm}
    if let Some(options) = extract_last_macro_braced_argument(source, "\\geometry") {
        if let Some(w) = extract_latex_option_value(&options, "paperwidth")
            .and_then(|v| parse_latex_length_to_twips_with_body_font(&v, layout.font_size_body_hp))
        {
            layout.page_width_twips = Some(w as u32);
        }
        if let Some(h) = extract_latex_option_value(&options, "paperheight")
            .and_then(|v| parse_latex_length_to_twips_with_body_font(&v, layout.font_size_body_hp))
        {
            layout.page_height_twips = Some(h as u32);
        }
    }
    // \documentclass[a4paper]{...} or \documentclass[letterpaper]{...}
    if layout.page_width_twips.is_none()
        && let Some(size) = extract_documentclass_paper_size(source)
    {
        layout.page_width_twips = Some(size.0);
        layout.page_height_twips = Some(size.1);
    }

    // ── Mono font family ─────────────────────────────────────────────
    // \setmonofont{Courier New}
    if let Some(font) = extract_setmainfont_conditional_for(source, fontfamily_val, "\\setmonofont")
    {
        layout.font_family_mono = Some(font);
    } else if let Some(font) = extract_last_macro_braced_argument(source, "\\setmonofont") {
        let font = font.trim().to_string();
        if !font.is_empty() {
            layout.font_family_mono = Some(font);
        }
    }

    // ── Heading alignment ────────────────────────────────────────────
    // \titleformat{\chapter}[display]{\centering\bfseries}{...}
    // or memoir-style heading format commands
    layout.heading_alignment = extract_heading_alignment(source);

    // ── Heading number delimiter ─────────────────────────────────────
    // \renewcommand{\thechapter}{\arabic{chapter}.} or similar
    layout.heading_number_delimiter = extract_heading_number_delimiter(source);
    layout.heading_number_delimiter_section = extract_heading_number_delimiter_for_level(source, 2);
    layout.heading_number_delimiter_subsection =
        extract_heading_number_delimiter_for_level(source, 3);
    layout.heading_number_delimiter_subsubsection =
        extract_heading_number_delimiter_for_level(source, 4);
    layout.heading_indent_section_twips = extract_heading_indent_twips_with_body_font(
        source,
        "\\setsecindent",
        layout.body_first_line_indent_twips,
        layout.font_size_body_hp,
    );
    layout.heading_indent_subsection_twips = extract_heading_indent_twips_with_body_font(
        source,
        "\\setsubsecindent",
        layout.body_first_line_indent_twips,
        layout.font_size_body_hp,
    );
    layout.heading_indent_subsubsection_twips = extract_heading_indent_twips_with_body_font(
        source,
        "\\setsubsubsecindent",
        layout.body_first_line_indent_twips,
        layout.font_size_body_hp,
    );
    layout.heading_space_before_chapter_twips = extract_setlength_heading_skip_twips(
        source,
        "beforechapskip",
        layout.body_line_spacing_twips,
        layout.font_size_body_hp,
    );
    layout.heading_space_after_chapter_twips = extract_setlength_heading_skip_twips(
        source,
        "afterchapskip",
        layout.body_line_spacing_twips,
        layout.font_size_body_hp,
    );
    layout.heading_space_before_section_twips = extract_heading_skip_macro_twips(
        source,
        "\\setbeforesecskip",
        layout.body_line_spacing_twips,
        layout.font_size_body_hp,
    );
    layout.heading_space_after_section_twips = extract_heading_skip_macro_twips(
        source,
        "\\setaftersecskip",
        layout.body_line_spacing_twips,
        layout.font_size_body_hp,
    );
    layout.heading_space_before_subsection_twips = extract_heading_skip_macro_twips(
        source,
        "\\setbeforesubsecskip",
        layout.body_line_spacing_twips,
        layout.font_size_body_hp,
    );
    layout.heading_space_after_subsection_twips = extract_heading_skip_macro_twips(
        source,
        "\\setaftersubsecskip",
        layout.body_line_spacing_twips,
        layout.font_size_body_hp,
    );
    layout.heading_space_before_subsubsection_twips = extract_heading_skip_macro_twips(
        source,
        "\\setbeforesubsubsecskip",
        layout.body_line_spacing_twips,
        layout.font_size_body_hp,
    );
    layout.heading_space_after_subsubsection_twips = extract_heading_skip_macro_twips(
        source,
        "\\setaftersubsubsecskip",
        layout.body_line_spacing_twips,
        layout.font_size_body_hp,
    );
    layout.toc_right_margin_twips =
        extract_toc_right_margin_twips_with_body_font(source, layout.font_size_body_hp);
    layout.toc_depth = extract_toc_depth(source);
    layout.toc_use_dot_leader = extract_toc_dot_leader(source);

    // ── List formatting ──────────────────────────────────────────────
    let (list_label_sep, list_label_width, list_item_indent, list_left_margin, list_bullet) =
        extract_list_settings_with_body_font(
            source,
            layout.body_first_line_indent_twips,
            layout.font_size_body_hp,
        );
    layout.list_label_sep_twips = list_label_sep;
    layout.list_label_width_twips = list_label_width;
    layout.list_item_indent_twips = list_item_indent;
    layout.list_left_indent_twips = list_left_margin;
    layout.list_bullet_char = list_bullet;
    // Left indent for list items = \parindent (body first-line indent).
    if layout.list_left_indent_twips.is_none() {
        layout.list_left_indent_twips = layout.body_first_line_indent_twips;
    }

    // ── Source attribution spacing ────────────────────────────────────
    layout.source_vspace_table_twips =
        extract_source_vspace_twips_with_body_font(source, "tablesource", layout.font_size_body_hp);
    layout.source_vspace_figure_twips = extract_source_vspace_twips_with_body_font(
        source,
        "figuresource",
        layout.font_size_body_hp,
    );

    // ── Title page page number suppression ────────────────────────────
    layout.title_page_suppress_number = extract_title_page_suppress_number(source);

    // ── Caption alignment ────────────────────────────────────────────
    // \captionsetup{justification=centering}
    layout.caption_alignment = extract_captionsetup_justification(source);
    layout.body_text_alignment = extract_body_text_alignment(preamble);
    layout.page_number_alignment = extract_page_number_alignment(source);
    layout.hyperlink_text_color = extract_hypersetup_link_color(source);
    layout.hyperlink_underline = extract_hypersetup_link_underline(source);

    // ── Graphics search paths ────────────────────────────────────────
    // \graphicspath{{./figures/}{./img/}}
    layout.graphics_search_paths = extract_graphicspath(source);

    // ── Document language ──────────────────────────────────────────────
    layout.document_language = extract_document_language(source);

    // ── Chapter name fallback from chapter-style counters ───────────────
    // Some memoir templates enable chapter-name rendering via counters
    // (e.g. chapstyle=1 + \@chapapp) without explicit \renewcommand{\chaptername}{...}.
    // Derive the visible chapter prefix from language defaults in that case.
    if layout.chapter_name.is_none() {
        layout.chapter_name =
            extract_chapter_name_from_chapstyle(source, layout.document_language.as_deref());
    }
    layout.toc_chapter_name_prefix =
        extract_toc_chapter_name_prefix(source, layout.chapter_name.as_deref());
    apply_chapstyle_toc_prefix_override(source, &mut layout);
    layout.toc_chapter_entry_bold = extract_toc_chapter_entry_bold(source);
    layout.toc_chapter_page_bold = extract_toc_chapter_page_bold(source);
    layout.toc_aftersnum_chapter = extract_toc_aftersnum(source, "chapter");
    layout.toc_aftersnum_section = extract_toc_aftersnum(source, "section");
    layout.toc_aftersnum_subsection = extract_toc_aftersnum(source, "subsection");
    layout.toc_aftersnum_subsubsection = extract_toc_aftersnum(source, "subsubsection");
    apply_headingdelim_toc_aftersnum_overrides(source, &mut layout);
    layout.toc_appendix_name = extract_toc_appendix_name(source);
    let (toc_chapter_indent, toc_chapter_numwidth) =
        extract_toc_indent_numwidth_twips_with_body_font(
            source,
            "chapter",
            layout.font_size_body_hp,
        );
    layout.toc_indent_chapter_twips = toc_chapter_indent;
    layout.toc_numwidth_chapter_twips = toc_chapter_numwidth;
    layout.toc_chapter_space_before_twips = extract_toc_chapter_before_skip_twips(
        source,
        layout.font_size_body_hp,
        documentclass_name.as_deref(),
    );
    layout.toc_section_space_before_twips =
        extract_toc_before_skip_twips(source, "cftbeforesectionskip", layout.font_size_body_hp);
    layout.toc_subsection_space_before_twips =
        extract_toc_before_skip_twips(source, "cftbeforesubsectionskip", layout.font_size_body_hp);
    layout.toc_subsubsection_space_before_twips = extract_toc_before_skip_twips(
        source,
        "cftbeforesubsubsectionskip",
        layout.font_size_body_hp,
    );
    let (toc_section_indent, toc_section_numwidth) =
        extract_toc_indent_numwidth_twips_with_body_font(
            source,
            "section",
            layout.font_size_body_hp,
        );
    layout.toc_indent_section_twips = toc_section_indent;
    layout.toc_numwidth_section_twips = toc_section_numwidth;
    let (toc_subsection_indent, toc_subsection_numwidth) =
        extract_toc_indent_numwidth_twips_with_body_font(
            source,
            "subsection",
            layout.font_size_body_hp,
        );
    layout.toc_indent_subsection_twips = toc_subsection_indent;
    layout.toc_numwidth_subsection_twips = toc_subsection_numwidth;
    let (toc_subsubsection_indent, toc_subsubsection_numwidth) =
        extract_toc_indent_numwidth_twips_with_body_font(
            source,
            "subsubsection",
            layout.font_size_body_hp,
        );
    layout.toc_indent_subsubsection_twips = toc_subsubsection_indent;
    layout.toc_numwidth_subsubsection_twips = toc_subsubsection_numwidth;
    apply_memoir_toc_indent_numwidth_defaults(&mut layout, documentclass_name.as_deref());

    layout
}

fn em_twips_for_body_font(body_font_size_hp: Option<usize>, em: f64) -> i32 {
    let body_pt = body_font_size_hp.unwrap_or(28) as f64 / 2.0;
    let twips = (body_pt * 20.0 * em).round();
    if twips.is_finite() { twips as i32 } else { 0 }
}

fn apply_memoir_toc_indent_numwidth_defaults(
    layout: &mut DocumentLayout,
    documentclass_name: Option<&str>,
) {
    if !documentclass_name.is_some_and(|name| name.eq_ignore_ascii_case("memoir")) {
        return;
    }

    if layout.toc_indent_chapter_twips.is_none() {
        layout.toc_indent_chapter_twips = Some(0);
    }
    if layout.toc_numwidth_chapter_twips.is_none() {
        layout.toc_numwidth_chapter_twips =
            Some(em_twips_for_body_font(layout.font_size_body_hp, 1.5));
    }
    if layout.toc_indent_section_twips.is_none() {
        layout.toc_indent_section_twips =
            Some(em_twips_for_body_font(layout.font_size_body_hp, 1.5));
    }
    if layout.toc_numwidth_section_twips.is_none() {
        layout.toc_numwidth_section_twips =
            Some(em_twips_for_body_font(layout.font_size_body_hp, 2.3));
    }
    if layout.toc_indent_subsection_twips.is_none() {
        layout.toc_indent_subsection_twips =
            Some(em_twips_for_body_font(layout.font_size_body_hp, 3.8));
    }
    if layout.toc_numwidth_subsection_twips.is_none() {
        layout.toc_numwidth_subsection_twips =
            Some(em_twips_for_body_font(layout.font_size_body_hp, 3.2));
    }
    if layout.toc_indent_subsubsection_twips.is_none() {
        layout.toc_indent_subsubsection_twips =
            Some(em_twips_for_body_font(layout.font_size_body_hp, 7.0));
    }
    if layout.toc_numwidth_subsubsection_twips.is_none() {
        layout.toc_numwidth_subsubsection_twips =
            Some(em_twips_for_body_font(layout.font_size_body_hp, 4.1));
    }
}

/// Extract the last value assigned to a LaTeX command via `\renewcommand{\name}{value}`
/// or `\newcommand{\name}{value}`.
fn extract_renewcommand_value(source: &str, cmd_name: &str) -> Option<String> {
    let mut last = None;
    let target = format!("\\{cmd_name}");
    for needle in ["\\renewcommand", "\\newcommand", "\\providecommand"] {
        let mut pos = 0usize;
        while let Some(rel) = source[pos..].find(needle) {
            let start = pos + rel + needle.len();
            let mut cur = start;
            // Skip optional * after the command.
            if cur < source.len() && source.as_bytes()[cur] == b'*' {
                cur += 1;
            }
            while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
                cur += 1;
            }
            // Accept both `\renewcommand{\cmdname}` and `\renewcommand\cmdname` forms.
            let name;
            if cur < source.len() && source.as_bytes()[cur] == b'{' {
                // Braced form: {\cmdname}
                let Some(name_len) = braced_len(&source[cur..]) else {
                    pos = start;
                    continue;
                };
                name = source[cur + 1..cur + name_len - 1].trim().to_string();
                cur += name_len;
            } else if cur < source.len() && source.as_bytes()[cur] == b'\\' {
                // Unbraced form: \cmdname
                cur += 1; // skip leading backslash
                let cmd_end = cur
                    + source[cur..]
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '@')
                        .map(char::len_utf8)
                        .sum::<usize>();
                name = format!("\\{}", &source[cur..cmd_end]);
                cur = cmd_end;
            } else {
                pos = start;
                continue;
            };
            // Skip optional [nargs]
            while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
                cur += 1;
            }
            if cur < source.len()
                && source.as_bytes()[cur] == b'['
                && let Some(close) = source[cur..].find(']')
            {
                cur += close + 1;
            }
            while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
                cur += 1;
            }
            let Some(value_len) = braced_len(&source[cur..]) else {
                pos = cur;
                continue;
            };
            let value = source[cur + 1..cur + value_len - 1].trim();
            if name == target {
                last = Some(value.to_string());
            }
            pos = cur + value_len;
        }
    }
    last
}

/// Extract the body font size from `\documentclass[...,Npt,...]{...}` options.
/// Returns the size in half-points (e.g. 28 for 14pt).
fn extract_documentclass_fontsize_hp(source: &str) -> Option<usize> {
    let pos = source.find("\\documentclass")?;
    let rest = &source[pos + "\\documentclass".len()..];
    // Options are in optional [...]
    let rest = rest.trim_start();
    if !rest.starts_with('[') {
        return None;
    }
    let close = rest.find(']')?;
    let options = &rest[1..close];
    // Look for a token like "14pt", "12pt", "11pt", "10pt"
    for opt in options.split(',') {
        let opt = opt.trim();
        if let Some(num_str) = opt.strip_suffix("pt")
            && let Ok(pt) = num_str.parse::<usize>()
        {
            return Some(pt * 2); // half-points
        }
    }
    None
}

/// Extract document class name from `\documentclass[...]{<class>}`.
fn extract_documentclass_name(source: &str) -> Option<String> {
    let pos = source.find("\\documentclass")?;
    let mut rest = &source[pos + "\\documentclass".len()..];
    rest = rest.trim_start();
    if rest.starts_with('[') {
        let close = rest.find(']')?;
        rest = &rest[close + 1..];
        rest = rest.trim_start();
    }
    if !rest.starts_with('{') {
        return None;
    }
    let close = braced_len(rest)?;
    let class_name = rest[1..close - 1].trim();
    if class_name.is_empty() {
        None
    } else {
        Some(class_name.to_string())
    }
}

/// Map a LaTeX font-size command name to half-points relative to body size.
///
/// Standard LaTeX size commands at 14pt base:
/// `\tiny`=10pt, `\scriptsize`=14pt, `\footnotesize`=12pt,
/// `\small`=13pt, `\normalsize`=14pt, `\large`=17pt.
///
/// We express sizes in absolute half-points using the body size as anchor.
fn latex_fontsize_cmd_to_hp(cmd: &str, body_hp: usize) -> Option<usize> {
    // Offsets in half-points relative to body size.
    let hp = match cmd {
        "tiny" => body_hp.saturating_sub(8).max(10), // body − 4pt
        "scriptsize" => body_hp.saturating_sub(4).max(12), // body − 2pt
        "footnotesize" => body_hp.saturating_sub(4).max(12), // body − 2pt (at 14pt base → 12pt)
        "small" => body_hp.saturating_sub(2).max(14), // body − 1pt
        "normalsize" => body_hp,
        "large" => body_hp + 6,  // body + 3pt
        "Large" => body_hp + 10, // body + 5pt
        _ => return None,
    };
    Some(hp)
}

/// Extract table body font size from `\SetTblrInner{font=\footnotesize}`.
/// Returns size in half-points.
fn extract_tblr_inner_font_size_hp(
    source: &str,
    layout: &crate::model::DocumentLayout,
) -> Option<usize> {
    let body_hp = layout.font_size_body_hp.unwrap_or(28);
    // Look for \SetTblrInner{font=\footnotesize} or similar.
    let needle = "\\SetTblrInner";
    let pos = source.find(needle)?;
    let rest = &source[pos + needle.len()..];
    let rest = rest.trim_start();
    if !rest.starts_with('{') {
        return None;
    }
    let close = rest.find('}')?;
    let inner = &rest[1..close];
    // Parse font=\cmd
    for kv in inner.split(',') {
        let kv = kv.trim();
        if let Some(val) = kv.strip_prefix("font=") {
            let val = val.trim().trim_start_matches('\\');
            return latex_fontsize_cmd_to_hp(val, body_hp);
        }
    }
    None
}

/// Extract caption font size from `\captionsetup[table]{font={normalsize,bf}}`.
/// Looks at both `[table]` and `[figure]` variants; returns the first match.
/// Returns size in half-points.
fn extract_captionsetup_font_size_hp(
    source: &str,
    layout: &crate::model::DocumentLayout,
) -> Option<usize> {
    let body_hp = layout.font_size_body_hp.unwrap_or(28);
    let needle = "\\captionsetup";
    let mut pos = 0usize;
    while let Some(rel) = source[pos..].find(needle) {
        let start = pos + rel + needle.len();
        let mut cur = start;
        let rest = &source[cur..];
        let rest = rest.trim_start();
        cur = source.len() - rest.len();
        // Skip optional [table] or [figure]
        if rest.starts_with('[') {
            if let Some(cb) = rest.find(']') {
                cur += cb + 1;
            } else {
                pos = start;
                continue;
            }
        }
        let rest = source[cur..].trim_start();
        cur = source.len() - rest.len();
        if !rest.starts_with('{') {
            pos = start;
            continue;
        }
        // Find matching }
        if let Some(body_len) = braced_len(&source[cur..]) {
            let body = &source[cur + 1..cur + body_len - 1];
            // Look for font={...} or font=size
            for kv in body.split(',') {
                let kv = kv.trim();
                if let Some(val) = kv.strip_prefix("font=") {
                    let val = val.trim();
                    // font={normalsize,bf} or font=normalsize
                    let val = val.trim_start_matches('{').trim_end_matches('}');
                    // Extract the size command (ignore bf, it, etc.)
                    for part in val.split(',') {
                        let part = part.trim().trim_start_matches('\\');
                        if let Some(hp) = latex_fontsize_cmd_to_hp(part, body_hp) {
                            return Some(hp);
                        }
                    }
                }
            }
            pos = cur + body_len;
        } else {
            pos = start;
        }
    }
    None
}

/// Detect the document's main language from babel/polyglossia settings.
/// Returns a BCP-47 language tag.
fn extract_document_language(source: &str) -> Option<String> {
    // polyglossia: \setmainlanguage{russian}
    if let Some(lang) = extract_last_macro_braced_argument(source, "\\setmainlanguage") {
        return latex_language_to_bcp47(lang.trim());
    }
    // polyglossia variant: \setmainlanguage[...]{russian}
    if let Some(lang) = extract_last_macro_braced_argument(source, "\\setdefaultlanguage") {
        return latex_language_to_bcp47(lang.trim());
    }
    // babel: \usepackage[english, russian]{babel} — last language is the main one
    // We look for {babel} in package loading and pick the last language option.
    if let Some(babel_pos) = source.find("{babel}") {
        // Look backwards for [options]
        let before = &source[..babel_pos];
        if let Some(bracket_close) = before.rfind(']')
            && let Some(bracket_open) = before[..=bracket_close].rfind('[')
        {
            let opts = &before[bracket_open + 1..bracket_close];
            // The last listed language is the main one (babel convention).
            if let Some(last) = opts.split(',').next_back() {
                return latex_language_to_bcp47(last.trim());
            }
        }
    }
    None
}

/// Map a LaTeX language name to a BCP-47 tag.
fn latex_language_to_bcp47(lang: &str) -> Option<String> {
    let tag = match lang.to_lowercase().as_str() {
        "russian" => "ru-RU",
        "english" | "american" | "usenglish" => "en-US",
        "british" | "ukenglish" => "en-GB",
        "german" | "ngerman" => "de-DE",
        "french" | "francais" => "fr-FR",
        "spanish" | "espanol" => "es-ES",
        "italian" | "italiano" => "it-IT",
        "portuguese" | "brazilian" => "pt-BR",
        "ukrainian" => "uk-UA",
        "belarusian" => "be-BY",
        "kazakh" => "kk-KZ",
        "polish" => "pl-PL",
        "czech" => "cs-CZ",
        "turkish" => "tr-TR",
        "chinese" => "zh-CN",
        "japanese" => "ja-JP",
        "korean" => "ko-KR",
        _ => return None,
    };
    Some(tag.to_string())
}

/// Extract the last integer value assigned to `counter_name` via `\setcounter{counter_name}{N}`.
///
/// Occurrences inside runtime-only conditionals (`\IfFontExistsTF`, `\IfFileExists`) are
/// skipped because their execution depends on the host system state which the parser cannot
/// evaluate.
fn extract_last_setcounter_value(source: &str, counter_name: &str) -> Option<i64> {
    let mut pos = 0usize;
    let mut last = None;
    let needle = "\\setcounter";
    while let Some(rel) = source[pos..].find(needle) {
        let match_start = pos + rel;
        let start = match_start + needle.len();
        let mut cur = start;
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }
        let Some(name_len) = braced_len(&source[cur..]) else {
            pos = start;
            continue;
        };
        let name = source[cur + 1..cur + name_len - 1].trim();
        cur += name_len;
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }
        let Some(value_len) = braced_len(&source[cur..]) else {
            pos = cur;
            continue;
        };
        let value_src = source[cur + 1..cur + value_len - 1].trim();
        // Skip \setcounter inside runtime conditionals like \IfFontExistsTF or \IfFileExists.
        let mut lookback_start = match_start.saturating_sub(120);
        // Align to a UTF-8 char boundary.
        while lookback_start > 0 && !source.is_char_boundary(lookback_start) {
            lookback_start -= 1;
        }
        let lookback = &source[lookback_start..match_start];
        let inside_runtime_conditional =
            lookback.contains("\\IfFontExistsTF") || lookback.contains("\\IfFileExists");
        if name == counter_name
            && !inside_runtime_conditional
            && let Ok(v) = value_src.parse::<i64>()
        {
            last = Some(v);
        }
        pos = cur + value_len;
    }
    last
}

/// Extract `\setmainfont{...}` from the `\ifnumequal{\value{fontfamily}}{N}{...}` branch
/// whose `N` matches the effective `fontfamily` counter value.
///
/// If no conditional block matches (or if `fontfamily_val` is `None`), returns `None`
/// so the caller can fall back to a simpler extraction.
fn extract_setmainfont_conditional(source: &str, fontfamily_val: Option<i64>) -> Option<String> {
    extract_setmainfont_conditional_for(source, fontfamily_val, "\\setmainfont")
}

/// Extract paper size from `\documentclass[...paper]`.
///
/// Returns page `(width_twips, height_twips)` for known paper tokens:
/// - `a4paper`     → `(11906, 16838)`
/// - `letterpaper` → `(12240, 15840)`
/// - `a5paper`     → `(8391, 11906)`
fn extract_documentclass_paper_size(source: &str) -> Option<(u32, u32)> {
    const A4_PAPER_TWIPS: (u32, u32) = (11_906, 16_838);
    const LETTER_PAPER_TWIPS: (u32, u32) = (12_240, 15_840);
    const A5_PAPER_TWIPS: (u32, u32) = (8_391, 11_906);

    let mut pos = 0usize;
    let mut last = None;
    let needle = "\\documentclass";

    while let Some(rel) = source[pos..].find(needle) {
        let start = pos + rel;
        let cmd_end = start + needle.len();
        if source[cmd_end..]
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic())
        {
            pos = cmd_end;
            continue;
        }

        let mut cur = cmd_end;
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }

        if cur < source.len() && source.as_bytes()[cur] == b'[' {
            let Some(opt_len) = bracketed_len(&source[cur..]) else {
                pos = cmd_end;
                continue;
            };
            let options = &source[cur + 1..cur + opt_len - 1];
            for token in options.split(',') {
                let token = token.trim().to_ascii_lowercase();
                let size = match token.as_str() {
                    "a4paper" => Some(A4_PAPER_TWIPS),
                    "letterpaper" => Some(LETTER_PAPER_TWIPS),
                    "a5paper" => Some(A5_PAPER_TWIPS),
                    _ => None,
                };
                if size.is_some() {
                    last = size;
                }
            }
            pos = cur + opt_len;
            continue;
        }

        pos = cmd_end;
    }

    last
}

/// Extract `cmd{...}` from `\ifnumequal{\value{fontfamily}}{N}{...}` branch
/// whose `N` matches the effective `fontfamily` counter value.
///
/// `cmd` must be a LaTeX macro name like `\setmainfont` or `\setmonofont`.
fn extract_setmainfont_conditional_for(
    source: &str,
    fontfamily_val: Option<i64>,
    cmd: &str,
) -> Option<String> {
    let target = fontfamily_val?;
    if cmd.trim().is_empty() {
        return None;
    }

    // Search for \ifnumequal{\value{fontfamily}}{N}{ ... }
    let needle = "\\ifnumequal{\\value{fontfamily}}";
    let mut pos = 0usize;
    while let Some(rel) = source[pos..].find(needle) {
        let start = pos + rel + needle.len();
        let mut cur = start;
        // Skip whitespace before {N}
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }
        // Read {N}
        let Some(val_len) = braced_len(&source[cur..]) else {
            pos = start;
            continue;
        };
        let val_str = source[cur + 1..cur + val_len - 1].trim();
        let Ok(n) = val_str.parse::<i64>() else {
            pos = cur + val_len;
            continue;
        };
        cur += val_len;
        // Skip whitespace before {body}
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }
        // Read {body} — the "then" branch
        let Some(body_len) = braced_len(&source[cur..]) else {
            pos = cur;
            continue;
        };
        if n == target {
            let body = &source[cur + 1..cur + body_len - 1];
            if let Some(font) = extract_last_macro_braced_argument(body, cmd) {
                let font = font.trim().to_string();
                if !font.is_empty() {
                    return Some(font);
                }
            }
        }
        pos = cur + body_len;
    }
    None
}

/// Extract heading alignment for chapter-like headings from titlesec/memoir formatting commands.
///
/// Canonical return values: `"left"`, `"center"`, `"right"`, `"both"`.
fn extract_heading_alignment(source: &str) -> Option<String> {
    let mut last = None;

    // titlesec: \titleformat{\chapter}[...]{...}{...}{...}{...}
    // Also supports selector form: \titleformat{name=\chapter}[...]{...}
    for args in extract_titleformat_chapter_arguments(source) {
        // Prefer explicit format/before/after code arguments where alignment macros appear.
        for idx in [0usize, 3usize, 4usize] {
            if let Some(arg) = args.get(idx)
                && let Some(alignment) = detect_alignment_directive(arg)
            {
                last = Some(alignment.to_string());
            }
        }
    }

    // memoir/custom chapter format definitions.
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_ascii_lowercase();
        let mentions_heading = lower.contains("chapter")
            || lower.contains("chapnamefont")
            || lower.contains("chaptitlefont")
            || lower.contains("printchaptertitle");
        if mentions_heading && let Some(alignment) = detect_alignment_directive(trimmed) {
            last = Some(alignment.to_string());
        }
    }

    // dissertation/memoir template convention:
    // \setcounter{headingalign}{0} => centered headings,
    // other values => left-aligned headings.
    if last.is_none()
        && source.contains("headingalign")
        && let Some(value) = extract_last_setcounter_value(source, "headingalign")
    {
        last = Some(if value == 0 { "center" } else { "left" }.to_string());
    }

    if sethangfrom_uses_noindent_label(source)
        && !matches!(last.as_deref(), Some("center" | "right"))
    {
        last = Some("both".to_string());
    }

    last
}

fn sethangfrom_uses_noindent_label(source: &str) -> bool {
    let Some(value) = extract_last_macro_braced_argument(source, "\\sethangfrom") else {
        return false;
    };
    let compact: String = value.chars().filter(|ch| !ch.is_whitespace()).collect();
    compact.contains("\\noindent#1")
}

/// Extract heading number delimiter from chapter numbering macros or titlesec labels.
///
/// Canonical examples:
/// - `\renewcommand{\thechapter}{\arabic{chapter}.}` → `"."`
/// - `\renewcommand{\thechapter}{\arabic{chapter}}` → `""`
fn extract_heading_number_delimiter(source: &str) -> Option<String> {
    // dissertation/memoir template convention:
    // headingdelim > 0 => dot delimiter, headingdelim == 0 => no delimiter.
    if source.contains("headingdelim")
        && let Some(value) = extract_last_setcounter_value(source, "headingdelim")
    {
        return Some(if value > 0 {
            ".".to_string()
        } else {
            String::new()
        });
    }

    if let Some(chapter_fmt) = extract_renewcommand_value(source, "thechapter")
        && let Some(delim) = extract_heading_number_delimiter_from_expr(&chapter_fmt)
    {
        return Some(delim);
    }

    if let Some(after_num) = extract_renewcommand_value(source, "afterchapternum") {
        return Some(normalize_delimiter_suffix(&after_num));
    }

    if let Some(label_fmt) = extract_last_macro_braced_argument(source, "\\titlelabel") {
        if let Some(delim) = extract_heading_number_delimiter_from_expr(&label_fmt) {
            return Some(delim);
        }
        return Some(normalize_delimiter_suffix(&label_fmt));
    }

    let mut last = None;
    for args in extract_titleformat_chapter_arguments(source) {
        // titlesec label argument (2nd braced argument after format) carries number template.
        if let Some(label) = args.get(1)
            && let Some(delim) = extract_heading_number_delimiter_from_expr(label)
        {
            last = Some(delim);
        }
    }

    last
}

fn extract_heading_number_delimiter_for_level(source: &str, level: u8) -> Option<String> {
    if level <= 1 {
        return extract_heading_number_delimiter(source);
    }

    // dissertation/memoir template convention:
    // headingdelim:
    //   0 => no dot for chapter/section/subsection;
    //   1 => chapter has dot, section/subsection do not;
    //   2 => chapter and section/subsection have dot.
    if source.contains("headingdelim")
        && let Some(value) = extract_last_setcounter_value(source, "headingdelim")
    {
        return Some(if value > 1 {
            ".".to_string()
        } else {
            String::new()
        });
    }

    extract_section_number_delimiter(source)
}

fn extract_section_number_delimiter(source: &str) -> Option<String> {
    let raw = extract_last_macro_braced_argument(source, "\\setsecnumformat")?;
    extract_section_number_delimiter_from_expr(&raw)
}

/// Extract caption alignment from `\captionsetup{justification=...}`.
///
/// Canonical return values: `"left"`, `"center"`, `"right"`, `"both"`.
fn extract_captionsetup_justification(source: &str) -> Option<String> {
    let value = extract_captionsetup_option(source, None, "justification")?;
    normalize_caption_justification(&value).map(|v| v.to_string())
}

/// Extract a caption label separator for `target` (`"figure"` or `"table"`).
///
/// Handles direct values (`labelsep=colon`) and custom declarations via
/// `\DeclareCaptionLabelSeparator{name}{...}`.
fn extract_captionsetup_label_separator(source: &str, target: &str) -> Option<String> {
    let declarations = extract_caption_label_separator_declarations(source);
    let raw = extract_captionsetup_option(source, Some(target), "labelsep")?;
    resolve_caption_label_separator(&raw, &declarations, source)
}

fn extract_captionsetup_skip_twips_with_body_font(
    source: &str,
    target: &str,
    body_font_size_hp: Option<usize>,
) -> Option<i32> {
    let raw = extract_captionsetup_option(source, Some(target), "skip")?;
    let resolved = resolve_captionsetup_option_value(&raw, source)?;
    parse_latex_length_to_twips_or_zero_with_body_font(&resolved, body_font_size_hp)
}

/// Extract caption position for `target` (`"figure"` or `"table"`).
///
/// Canonical return values: `"top"` or `"bottom"`.
fn extract_captionsetup_position(source: &str, target: &str) -> Option<String> {
    let raw = extract_captionsetup_option(source, Some(target), "position")?;
    let resolved = resolve_captionsetup_option_value(&raw, source)?;
    normalize_caption_position(&resolved).map(|value| value.to_string())
}

/// Extract caption `singlelinecheck` for `target` (`"figure"` or `"table"`).
fn extract_captionsetup_singlelinecheck(source: &str, target: &str) -> Option<bool> {
    let raw = extract_captionsetup_option(source, Some(target), "singlelinecheck")?;
    let resolved = resolve_captionsetup_option_value(&raw, source)?;
    parse_caption_bool(&resolved)
}

fn extract_captionsetup_indent_twips_with_body_font(
    source: &str,
    target: &str,
    body_font_size_hp: Option<usize>,
) -> Option<i32> {
    let raw = extract_captionsetup_option(source, Some(target), "indent")?;
    let resolved = resolve_captionsetup_option_value(&raw, source)?;
    parse_latex_length_to_twips_or_zero_with_body_font(&resolved, body_font_size_hp)
}

fn extract_captionsetup_labelfont_bold(source: &str, target: &str) -> Option<bool> {
    let raw = extract_captionsetup_option(source, Some(target), "labelfont")?;
    let resolved = resolve_captionsetup_option_value(&raw, source)?;
    let normalized = resolved
        .trim()
        .trim_matches(['{', '}'])
        .to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    if normalized.contains("bf") || normalized.contains("bold") {
        return Some(true);
    }
    if normalized.contains("normalfont") || normalized.contains("mdseries") {
        return Some(false);
    }
    None
}

/// Resolve `\captionsetup{...}` option value if it references a macro.
///
/// Example:
/// - `singlelinecheck=\tabsinglecenter` with
///   `\newcommand{\tabsinglecenter}{false}` resolves to `"false"`.
fn resolve_captionsetup_option_value(raw: &str, source: &str) -> Option<String> {
    resolve_captionsetup_option_value_inner(raw, source, 0)
}

fn resolve_captionsetup_option_value_inner(
    raw: &str,
    source: &str,
    depth: usize,
) -> Option<String> {
    if depth > 8 {
        return None;
    }

    let trimmed = raw.trim().trim_matches(['{', '}']).trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(cmd_name) = trimmed.strip_prefix('\\') {
        if let Some(value) = extract_renewcommand_value(source, cmd_name) {
            return resolve_captionsetup_option_value_inner(&value, source, depth + 1);
        }
        return Some(cmd_name.to_string());
    }

    Some(trimmed.to_string())
}

fn parse_caption_bool(raw: &str) -> Option<bool> {
    match raw
        .trim()
        .trim_matches(['{', '}'])
        .to_ascii_lowercase()
        .as_str()
    {
        "true" | "on" | "yes" | "1" => Some(true),
        "false" | "off" | "no" | "0" => Some(false),
        _ => None,
    }
}

fn normalize_caption_position(raw: &str) -> Option<&'static str> {
    let normalized = raw.trim().trim_matches(['{', '}']).to_ascii_lowercase();
    match normalized.as_str() {
        "top" | "above" => Some("top"),
        "bottom" | "below" => Some("bottom"),
        _ => None,
    }
}

fn parse_latex_length_to_twips_or_zero_with_body_font(
    raw: &str,
    body_font_size_hp: Option<usize>,
) -> Option<i32> {
    let trimmed = raw.trim().trim_matches(['{', '}']).trim();
    if trimmed == "0" {
        return Some(0);
    }
    parse_latex_length_to_twips_with_body_font(trimmed, body_font_size_hp)
}

fn extract_heading_indent_twips_with_body_font(
    source: &str,
    macro_name: &str,
    parindent_fallback: Option<i32>,
    body_font_size_hp: Option<usize>,
) -> Option<i32> {
    let raw = extract_last_macro_braced_argument(source, macro_name)?;
    let trimmed = raw.trim().trim_matches(['{', '}']).trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed == "\\parindent" || trimmed.contains("\\parindent") {
        return parindent_fallback;
    }
    parse_latex_length_to_twips_with_body_font(trimmed, body_font_size_hp)
}

fn extract_toc_right_margin_twips_with_body_font(
    source: &str,
    body_font_size_hp: Option<usize>,
) -> Option<i32> {
    let raw = extract_last_macro_braced_argument(source, "\\setrmarg")?;
    parse_latex_length_prefix_to_twips_with_body_font(&raw, body_font_size_hp)
}

fn extract_setlength_heading_skip_twips(
    source: &str,
    name: &str,
    body_line_spacing_twips: Option<i32>,
    body_font_size_hp: Option<usize>,
) -> Option<i32> {
    let raw = extract_setlength_value_raw(source, name)?;
    parse_heading_skip_to_twips(&raw, body_line_spacing_twips, body_font_size_hp)
}

fn extract_heading_skip_macro_twips(
    source: &str,
    macro_name: &str,
    body_line_spacing_twips: Option<i32>,
    body_font_size_hp: Option<usize>,
) -> Option<i32> {
    let raw = extract_last_macro_braced_argument(source, macro_name)?;
    parse_heading_skip_to_twips(&raw, body_line_spacing_twips, body_font_size_hp)
}

fn parse_heading_skip_to_twips(
    raw: &str,
    body_line_spacing_twips: Option<i32>,
    body_font_size_hp: Option<usize>,
) -> Option<i32> {
    let trimmed = raw.trim().trim_matches(['{', '}']).trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed == "0" || trimmed == "0pt" {
        return Some(0);
    }

    let compact: String = trimmed.chars().filter(|ch| !ch.is_whitespace()).collect();
    for marker in ["\\onelineskip", "\\baselineskip"] {
        if let Some(idx) = compact.find(marker) {
            let factor_raw = compact[..idx].trim();
            let factor = if factor_raw.is_empty() {
                1.0
            } else {
                parse_spacing_factor(factor_raw)?
            };
            let base = body_line_spacing_twips.unwrap_or(DEFAULT_BODY_LINE_SPACING_TWIPS) as f64;
            let twips = (base * factor).round();
            if twips.is_finite() && twips >= 0.0 {
                return Some(twips as i32);
            }
            return None;
        }
    }

    parse_latex_length_to_twips_with_body_font(trimmed, body_font_size_hp)
        .or_else(|| parse_latex_length_prefix_to_twips_with_body_font(trimmed, body_font_size_hp))
}

fn extract_toc_depth(source: &str) -> Option<i32> {
    extract_last_setcounter_value(source, "tocdepth").map(|value| value as i32)
}

/// Detect whether TOC page numbers should use dot leaders.
///
/// Returns:
/// - `Some(true)` when explicit `\cft...leader`/`cftdotfill` configuration
///   requests dotted leaders.
/// - `Some(false)` when explicit leader configuration is present but not dotted.
/// - `None` when LaTeX source does not express a leader preference.
fn extract_toc_dot_leader(source: &str) -> Option<bool> {
    for cmd in [
        "cftchapterleader",
        "cftsectionleader",
        "cftsubsectionleader",
        "cftsubsubsectionleader",
    ] {
        if let Some(value) = extract_renewcommand_value(source, cmd) {
            let value_lc = value.to_ascii_lowercase();
            return Some(value_lc.contains("cftdotfill") || value_lc.contains("dotfill"));
        }
    }

    if source.contains("\\cftdotfill") || source.contains("\\dotfill") {
        return Some(true);
    }
    None
}

/// Extract chapter-name prefix for numbered TOC chapter entries.
///
/// The value is parsed from `\renewcommand*{\cftchaptername}{...}` and normalized
/// to plain text. When the definition references `\chaptername` or `\@chapapp`,
/// the already-extracted chapter name is substituted.
fn extract_toc_chapter_name_prefix(source: &str, chapter_name: Option<&str>) -> Option<String> {
    let raw = extract_renewcommand_value(source, "cftchaptername")?;
    if raw.trim().is_empty() {
        return Some(String::new());
    }

    let mut value = raw.replace("\\space", " ").replace('~', " ");
    if let Some(name) = chapter_name {
        value = value.replace("\\chaptername", name);
        value = value.replace("\\@chapapp", name);
    }

    let mut plain = String::new();
    let mut i = 0usize;
    while i < value.len() {
        let rest = &value[i..];
        if let Some(after_slash) = rest.strip_prefix('\\') {
            let cmd_len: usize = after_slash
                .chars()
                .take_while(|c| c.is_ascii_alphabetic() || *c == '@')
                .map(char::len_utf8)
                .sum();

            if cmd_len == 0 {
                i += 1;
                if let Some(symbol) = rest[1..].chars().next() {
                    if symbol.is_ascii_whitespace() || symbol == '~' {
                        plain.push(' ');
                    }
                    i += symbol.len_utf8();
                }
                continue;
            }

            let cmd = &after_slash[..cmd_len];
            if cmd == "space" {
                plain.push(' ');
            }
            i += 1 + cmd_len;
            continue;
        }

        let ch = rest.chars().next()?;
        i += ch.len_utf8();
        if ch == '{' || ch == '}' {
            continue;
        }
        plain.push(ch);
    }

    Some(normalize_whitespace(&plain).trim().to_string())
}

/// Detect whether chapter TOC entry text should be bold.
///
/// Returns `Some(false)` when `\renewcommand{\cftchapterfont}{\normalfont}` (or `\mdseries`)
/// is present, indicating the project explicitly requests non-bold chapter TOC entries.
/// Returns `Some(true)` when `\bfseries` or `\bf` is seen.
/// Returns `None` when no explicit font override is found.
fn extract_toc_chapter_entry_bold(source: &str) -> Option<bool> {
    let value = extract_renewcommand_value(source, "cftchapterfont")?;
    let v = value.trim().to_ascii_lowercase();
    if v.contains("normalfont") || v.contains("mdseries") {
        Some(false)
    } else if v.contains("bfseries") || v.contains("\\bf") {
        Some(true)
    } else {
        // Any other override: treat as non-bold (explicit override detected).
        Some(false)
    }
}

/// Detect whether chapter TOC page-numbers should be bold.
///
/// Returns `Some(false)` when `\renewcommand{\cftchapterpagefont}{\normalfont}` is present.
/// Returns `Some(true)` when `\bfseries` is seen.
/// Returns `None` when no explicit page-font override is found.
fn extract_toc_chapter_page_bold(source: &str) -> Option<bool> {
    let value = extract_renewcommand_value(source, "cftchapterpagefont")?;
    let v = value.trim().to_ascii_lowercase();
    if v.contains("normalfont") || v.contains("mdseries") {
        Some(false)
    } else if v.contains("bfseries") || v.contains("\\bf") {
        Some(true)
    } else {
        Some(false)
    }
}

/// Strip LaTeX control sequences from a string, substituting known shorthands.
///
/// - `\space`, `~` → space
/// - `\quad` → four spaces
/// - `\,` → thin space (`\u{202F}`)
/// - Other `\cmd` sequences → removed
/// - Braces `{` `}` → removed
fn strip_latex_controls(raw: &str) -> String {
    let expanded = raw
        .replace("\\space", " ")
        .replace("\\quad", "    ")
        .replace("\\,", "\u{202F}")
        .replace('~', "\u{00A0}");
    let mut out = String::new();
    let mut chars = expanded.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            // Skip over alphabetic command name.
            while chars.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
                chars.next();
            }
            continue;
        }
        if ch == '{' || ch == '}' {
            continue;
        }
        out.push(ch);
    }
    out
}

/// Extract the `\cft<level>aftersnum{...}` separator for a given TOC level.
///
/// `level` should be one of `"chapter"`, `"section"`, `"subsection"`, `"subsubsection"`.
/// The content is normalized via [`strip_latex_controls`].
fn extract_toc_aftersnum(source: &str, level: &str) -> Option<String> {
    let cmd = format!("cft{level}aftersnum");
    let raw = extract_renewcommand_value(source, &cmd)?;
    if raw.trim().is_empty() {
        return Some(String::new());
    }
    let plain = strip_latex_controls(&raw);
    // Do not trim trailing whitespace: separators like ". " must preserve the trailing space.
    Some(normalize_whitespace(&plain))
}

fn apply_headingdelim_toc_aftersnum_overrides(source: &str, layout: &mut DocumentLayout) {
    // dissertation/memoir template convention:
    // headingdelim:
    //   0 => no separator after chapter/section numbers in TOC
    //   1 => chapter ". ", section/subsection empty
    //   2 => chapter/section/subsection ". "
    if !source.contains("headingdelim") {
        return;
    }
    let Some(value) = extract_last_setcounter_value(source, "headingdelim") else {
        return;
    };
    layout.toc_aftersnum_chapter = Some(if value > 0 {
        ". ".to_string()
    } else {
        String::new()
    });
    let section_delim = if value > 1 {
        ". ".to_string()
    } else {
        String::new()
    };
    layout.toc_aftersnum_section = Some(section_delim.clone());
    layout.toc_aftersnum_subsection = Some(section_delim.clone());
    layout.toc_aftersnum_subsubsection = Some(section_delim);
}

fn apply_chapstyle_toc_prefix_override(source: &str, layout: &mut DocumentLayout) {
    // dissertation/memoir template convention:
    // chapstyle:
    //   0 => chapter titles without chapter-name prefix in TOC
    //   1 => chapter-name prefix enabled (e.g. "ГЛАВА 1")
    if !source.contains("chapstyle") {
        return;
    }
    let Some(value) = extract_last_setcounter_value(source, "chapstyle") else {
        return;
    };
    if value == 0 {
        layout.toc_chapter_name_prefix = Some(String::new());
        return;
    }
    if value > 0
        && layout.toc_chapter_name_prefix.is_none()
        && let Some(chapter_name) = layout.chapter_name.as_deref()
    {
        layout.toc_chapter_name_prefix = Some(chapter_name.trim().to_string());
    }
}

/// Extract the TOC appendix prefix from `\renewcommand{\cftappendixname}{...}`.
///
/// Any `\appendixname` reference is substituted with the value extracted from
/// `\renewcommand{\appendixname}{...}`, or `"Appendix"` as a final fallback.
fn extract_toc_appendix_name(source: &str) -> Option<String> {
    let raw = extract_renewcommand_value(source, "cftappendixname")?;
    if raw.trim().is_empty() {
        return Some(String::new());
    }
    let appendix_name = extract_renewcommand_value(source, "appendixname")
        .unwrap_or_else(|| "Appendix".to_string());
    let substituted = raw.replace("\\appendixname", &appendix_name);
    let plain = strip_latex_controls(&substituted);
    Some(normalize_whitespace(&plain).trim().to_string())
}

/// Extract vertical spacing before TOC chapter entries in twips.
///
/// Sources:
/// - `\setlength{\cftbeforechapterskip}{...}` (preferred)
/// - `\renewcommand*{\cftbeforechapterskip}{...}`
/// - memoir default (`1.0em plus 1pt`) when class is `memoir`.
fn extract_toc_chapter_before_skip_twips(
    source: &str,
    body_font_size_hp: Option<usize>,
    documentclass_name: Option<&str>,
) -> Option<i32> {
    if let Some(twips) =
        extract_toc_before_skip_twips(source, "cftbeforechapterskip", body_font_size_hp)
    {
        return Some(twips);
    }

    if documentclass_name.is_some_and(|name| name.eq_ignore_ascii_case("memoir")) {
        let em_twips = em_twips_for_body_font(body_font_size_hp, 1.0);
        // memoir default: `1.0em plus 1pt`; map glue to a deterministic nominal value.
        return Some(em_twips.saturating_add(20));
    }

    None
}

/// Extract vertical spacing before TOC entries for the given `\cftbefore...skip` command.
///
/// Supported forms:
/// - `\setlength{\<command>}{...}`
/// - `\renewcommand*{\<command>}{...}`
fn extract_toc_before_skip_twips(
    source: &str,
    command: &str,
    body_font_size_hp: Option<usize>,
) -> Option<i32> {
    if let Some((twips, _)) =
        extract_last_setlength_value_twips_with_pos(source, command, body_font_size_hp)
    {
        return Some(twips);
    }

    if let Some(raw) = extract_renewcommand_value(source, command)
        && let Some(twips) =
            parse_latex_length_prefix_to_twips_with_body_font(&raw, body_font_size_hp)
    {
        return Some(twips);
    }

    None
}

// ── List settings extraction ─────────────────────────────────────────────────

/// Extract enumitem list settings from `\setlist{...}` and `\renewcommand{\labelitemi}{...}`.
///
/// Returns `(label_sep_twips, label_width_twips, item_indent_twips, left_margin_twips, bullet_char)`.
/// - `label_sep_twips`: from `labelsep=<dim>` in `\setlist{...}`.
/// - `label_width_twips`: `None` when `labelwidth=!` (auto), otherwise from `labelwidth=<dim>`.
/// - `item_indent_twips`: from `itemindent=<dim>` (including simple `\dimexpr` forms).
///   Falls back to `listparindent=<dim>` when `itemindent` is absent.
/// - `left_margin_twips`: from `leftmargin=<dim>` (including simple `\dimexpr` forms).
/// - `bullet_char`: from `\renewcommand{\labelitemi}{...}`, stripped of formatting commands.
type ListSettings = (
    Option<i32>,
    Option<i32>,
    Option<i32>,
    Option<i32>,
    Option<String>,
);

fn extract_list_settings_with_body_font(
    source: &str,
    body_parindent_twips: Option<i32>,
    body_font_size_hp: Option<usize>,
) -> ListSettings {
    let sep =
        extract_setlist_param_twips(source, &["labelsep"], None, None, None, body_font_size_hp);
    let width = extract_setlist_labelwidth_twips_with_body_font(source, body_font_size_hp);
    let listparindent = extract_setlist_param_twips(
        source,
        &["listparindent"],
        sep,
        width,
        body_parindent_twips,
        body_font_size_hp,
    );
    let itemindent = extract_setlist_param_twips(
        source,
        &["itemindent"],
        sep,
        width,
        body_parindent_twips,
        body_font_size_hp,
    )
    .or(listparindent);
    let leftmargin = extract_setlist_param_twips(
        source,
        &["leftmargin"],
        sep,
        width,
        body_parindent_twips,
        body_font_size_hp,
    );
    let bullet = extract_labelitemi_char(source);
    (sep, width, itemindent, leftmargin, bullet)
}

/// Extract a dimension parameter from the last matching `\setlist{..., name=<dim>, ...}` block.
///
/// Supports plain lengths (`0.5em`, `1.25cm`) and simple `\dimexpr` arithmetic
/// that references `\labelwidth`, `\labelsep`, and `\parindent`.
fn extract_setlist_param_twips(
    source: &str,
    params: &[&str],
    label_sep_twips: Option<i32>,
    label_width_twips: Option<i32>,
    body_parindent_twips: Option<i32>,
    body_font_size_hp: Option<usize>,
) -> Option<i32> {
    let raw = extract_setlist_param_raw(source, params)?;
    if raw.is_empty() {
        return None;
    }
    let raw_trimmed = raw.trim();
    if raw_trimmed.eq_ignore_ascii_case("\\parindent") {
        return body_parindent_twips;
    }
    if raw_trimmed.eq_ignore_ascii_case("\\labelsep") {
        return label_sep_twips;
    }
    if raw_trimmed.eq_ignore_ascii_case("\\labelwidth") {
        return label_width_twips.or(label_sep_twips);
    }
    if let Some(twips) = parse_latex_length_to_twips_with_body_font(raw.as_str(), body_font_size_hp)
    {
        return Some(twips);
    }
    if let Some(twips) =
        parse_latex_length_prefix_to_twips_with_body_font(raw.as_str(), body_font_size_hp)
    {
        return Some(twips);
    }
    evaluate_setlist_dimexpr_twips(
        raw.as_str(),
        label_sep_twips,
        label_width_twips,
        body_parindent_twips,
        body_font_size_hp,
    )
}

fn extract_setlist_param_raw(source: &str, params: &[&str]) -> Option<String> {
    let needle = "\\setlist";
    let mut pos = 0usize;
    let mut last_match: Option<String> = None;
    while let Some(rel) = source[pos..].find(needle) {
        let start = pos + rel + needle.len();
        // Skip optional [list-type] argument.
        let after_cmd = source[start..].trim_start_matches([' ', '\t', '\n', '\r']);
        let after_cmd_pos = start + (source[start..].len() - after_cmd.len());
        let content_start = if after_cmd.starts_with('[') {
            // Skip past the optional argument.
            if let Some(close) = source[after_cmd_pos..].find(']') {
                after_cmd_pos + close + 1
            } else {
                pos = start;
                continue;
            }
        } else {
            after_cmd_pos
        };
        // Now find the mandatory {…} argument.
        let trimmed = source[content_start..].trim_start_matches([' ', '\t', '\n', '\r']);
        let trimmed_pos = content_start + (source[content_start..].len() - trimmed.len());
        if !trimmed.starts_with('{') {
            pos = start;
            continue;
        }
        if let Some(body) = extract_braced(&source[trimmed_pos..]) {
            last_match = extract_setlist_param_from_body(body, params).or(last_match);
        }
        pos = start;
    }
    last_match
}

fn extract_setlist_param_from_body(body: &str, params: &[&str]) -> Option<String> {
    let mut last_match: Option<String> = None;
    for entry in split_top_level_csv(body) {
        let token = entry.trim();
        let Some((raw_key, raw_value)) = token.split_once('=') else {
            continue;
        };
        let key = normalize_setlist_key(raw_key);
        if params
            .iter()
            .any(|param| normalize_setlist_key(param) == key)
        {
            last_match = Some(raw_value.trim().to_string());
        }
    }
    last_match
}

fn split_top_level_csv(input: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut brace_depth = 0i32;
    let mut bracket_depth = 0i32;
    let mut paren_depth = 0i32;

    for (idx, ch) in input.char_indices() {
        match ch {
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            ',' if brace_depth == 0 && bracket_depth == 0 && paren_depth == 0 => {
                parts.push(&input[start..idx]);
                start = idx + 1;
            }
            _ => {}
        }
    }
    parts.push(&input[start..]);
    parts
}

fn normalize_setlist_key(raw: &str) -> String {
    raw.trim().trim_end_matches('*').to_ascii_lowercase()
}

fn evaluate_setlist_dimexpr_twips(
    raw: &str,
    label_sep_twips: Option<i32>,
    label_width_twips: Option<i32>,
    body_parindent_twips: Option<i32>,
    body_font_size_hp: Option<usize>,
) -> Option<i32> {
    let mut expr = raw.trim().to_string();
    if !expr.starts_with("\\dimexpr") {
        return None;
    }
    expr = expr["\\dimexpr".len()..].trim().to_string();
    expr = expr.replace("\\relax", "");

    let sep = label_sep_twips?;
    let width = label_width_twips.unwrap_or(sep);
    let parindent = body_parindent_twips?;

    expr = expr.replace("\\labelsep", &sep.to_string());
    expr = expr.replace("\\labelwidth", &width.to_string());
    expr = expr.replace("\\parindent", &parindent.to_string());
    expr = expr.replace([' ', '\t'], "");

    let mut total: i32 = 0;
    let mut sign: i32 = 1;
    let mut token = String::new();

    for ch in expr.chars() {
        if ch == '+' || ch == '-' {
            if !token.is_empty() {
                let value = parse_setlist_dimexpr_term_twips(&token, body_font_size_hp)?;
                total = total.saturating_add(sign.saturating_mul(value));
                token.clear();
            }
            sign = if ch == '+' { 1 } else { -1 };
        } else {
            token.push(ch);
        }
    }
    if !token.is_empty() {
        let value = parse_setlist_dimexpr_term_twips(&token, body_font_size_hp)?;
        total = total.saturating_add(sign.saturating_mul(value));
    }
    Some(total)
}

fn parse_setlist_dimexpr_term_twips(term: &str, body_font_size_hp: Option<usize>) -> Option<i32> {
    let trimmed = term.trim().trim_matches(['{', '}']);
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(value) = trimmed.parse::<i32>() {
        return Some(value);
    }
    parse_latex_length_prefix_to_twips_with_body_font(trimmed, body_font_size_hp)
}

fn extract_setlist_labelwidth_twips_with_body_font(
    source: &str,
    body_font_size_hp: Option<usize>,
) -> Option<i32> {
    let raw = extract_setlist_param_raw(source, &["labelwidth"])?;
    let raw = raw.trim();
    if raw.starts_with('!') || raw.starts_with('*') {
        return None;
    }
    parse_latex_length_to_twips_with_body_font(raw, body_font_size_hp)
        .or_else(|| parse_latex_length_prefix_to_twips_with_body_font(raw, body_font_size_hp))
}

/// Extract the bullet character from `\renewcommand{\labelitemi}{...}`.
///
/// Strips formatting control sequences (`\normalfont`, `\bfseries`, etc.) and returns
/// the visible character(s). Common mappings: `{--}` → `"–"`, `{\textendash}` → `"–"`.
fn extract_labelitemi_char(source: &str) -> Option<String> {
    let raw = extract_renewcommand_value(source, "labelitemi")?;
    if raw.trim().is_empty() {
        return None;
    }
    // Map known LaTeX commands to Unicode.
    let substituted = raw
        .replace("\\textendash", "–")
        .replace("\\textemdash", "—")
        .replace("\\textbullet", "•")
        .replace("\\textasteriskcentered", "*")
        .replace("---", "—")
        .replace("--", "–");
    // Strip remaining control sequences and formatting.
    let plain = strip_latex_controls(&substituted);
    let result = plain.trim().to_string();
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

// ── Source vspace extraction ─────────────────────────────────────────────────

fn extract_source_vspace_twips_with_body_font(
    source: &str,
    macro_name: &str,
    body_font_size_hp: Option<usize>,
) -> Option<i32> {
    let def_marker_new = format!("\\newcommand{{\\{macro_name}}}");
    let def_marker_renew = format!("\\renewcommand{{\\{macro_name}}}");
    let def_pos = source
        .find(&def_marker_new)
        .or_else(|| source.find(&def_marker_renew))?;
    // Scan a window after the definition marker for the first \vspace{dim}.
    let window = &source[def_pos..source.len().min(def_pos + 400)];
    let vspace_rel = window.find("\\vspace{")?;
    let after_v = &window[vspace_rel + "\\vspace{".len()..];
    let close = after_v.find('}')?;
    let dim = after_v[..close].trim();
    parse_latex_length_to_twips_with_body_font(dim, body_font_size_hp)
}

// ── Title page page number suppression ──────────────────────────────────────

/// Detect `\thispagestyle{empty}` inside `\begin{titlingpage}` / `\begin{titlepage}`.
///
/// Returns `Some(true)` when the title page suppresses the page number.
fn extract_title_page_suppress_number(source: &str) -> Option<bool> {
    // Look for titlingpage or titlepage environment containing \thispagestyle{empty}.
    for env in ["titlingpage", "titlepage"] {
        let begin = format!("\\begin{{{env}}}");
        let end = format!("\\end{{{env}}}");
        let mut pos = 0usize;
        while let Some(rel) = source[pos..].find(&begin) {
            let start = pos + rel + begin.len();
            let body = if let Some(end_rel) = source[start..].find(&end) {
                &source[start..start + end_rel]
            } else {
                &source[start..]
            };
            if body.contains("\\thispagestyle{empty}") {
                return Some(true);
            }
            pos = start;
        }
    }
    // Also check for a standalone \thispagestyle{empty} very early in the document
    // (within the first 3000 bytes), which is a common pattern for title pages.
    let early = &source[..source.len().min(3000)];
    if early.contains("\\thispagestyle{empty}") {
        return Some(true);
    }
    None
}

// ── Bibliography block parsing ───────────────────────────────────────────────

/// Extract the title from a `\printbibliography[title=...]` optional argument.
///
/// Returns `None` if no `title=` key is found.
fn extract_printbibliography_title(args: &str) -> Option<String> {
    // args is the content inside [...] of \printbibliography[...]
    let key = "title=";
    let pos = args.find(key)?;
    let after = &args[pos + key.len()..];
    // Title value may be bare (title=WORD) or braced (title={text}).
    if after.starts_with('{') {
        extract_braced(after).map(str::to_string)
    } else {
        let val: String = after
            .chars()
            .take_while(|c| *c != ',' && *c != ']')
            .collect();
        let val = val.trim().to_string();
        if val.is_empty() { None } else { Some(val) }
    }
}

fn extract_toc_indent_numwidth_twips_with_body_font(
    source: &str,
    kind: &str,
    body_font_size_hp: Option<usize>,
) -> (Option<i32>, Option<i32>) {
    let indent_name = format!("cft{kind}indent");
    let numwidth_name = format!("cft{kind}numwidth");

    let mut indent =
        extract_last_setlength_value_twips_with_pos(source, &indent_name, body_font_size_hp);
    let mut numwidth =
        extract_last_setlength_value_twips_with_pos(source, &numwidth_name, body_font_size_hp);

    if let Some((cft_indent, cft_numwidth, cft_pos)) =
        extract_last_cftsetindents_twips_with_pos(source, kind, body_font_size_hp)
    {
        if let Some(v) = cft_indent
            && indent.is_none_or(|(_, set_pos)| cft_pos >= set_pos)
        {
            indent = Some((v, cft_pos));
        }
        if let Some(v) = cft_numwidth
            && numwidth.is_none_or(|(_, set_pos)| cft_pos >= set_pos)
        {
            numwidth = Some((v, cft_pos));
        }
    }

    (indent.map(|(v, _)| v), numwidth.map(|(v, _)| v))
}

fn extract_last_setlength_value_twips_with_pos(
    source: &str,
    name: &str,
    body_font_size_hp: Option<usize>,
) -> Option<(i32, usize)> {
    let mut pos = 0usize;
    let mut last = None;

    while let Some(rel) = source[pos..].find("\\setlength") {
        let cmd_start = pos + rel;
        let mut cur = cmd_start + "\\setlength".len();
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }
        let Some(name_len) = braced_len(&source[cur..]) else {
            pos = cur;
            continue;
        };
        let raw_name = source[cur + 1..cur + name_len - 1]
            .trim()
            .trim_start_matches('\\');
        cur += name_len;
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }
        let Some(value_len) = braced_len(&source[cur..]) else {
            pos = cur;
            continue;
        };
        let raw_value = source[cur + 1..cur + value_len - 1].trim();
        let end_pos = cur + value_len;
        if raw_name == name
            && let Some(twips) =
                parse_latex_length_prefix_to_twips_with_body_font(raw_value, body_font_size_hp)
        {
            last = Some((twips, end_pos));
        }
        pos = end_pos;
    }

    last
}

fn extract_last_cftsetindents_twips_with_pos(
    source: &str,
    kind: &str,
    body_font_size_hp: Option<usize>,
) -> Option<(Option<i32>, Option<i32>, usize)> {
    let mut pos = 0usize;
    let mut last = None;

    while let Some(rel) = source[pos..].find("\\cftsetindents") {
        let cmd_start = pos + rel;
        let mut cur = cmd_start + "\\cftsetindents".len();
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }

        let Some(kind_len) = braced_len(&source[cur..]) else {
            pos = cur;
            continue;
        };
        let raw_kind = source[cur + 1..cur + kind_len - 1]
            .trim()
            .trim_start_matches('\\');
        cur += kind_len;
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }

        let Some(indent_len) = braced_len(&source[cur..]) else {
            pos = cur;
            continue;
        };
        let raw_indent = source[cur + 1..cur + indent_len - 1].trim();
        cur += indent_len;
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }

        let Some(numwidth_len) = braced_len(&source[cur..]) else {
            pos = cur;
            continue;
        };
        let raw_numwidth = source[cur + 1..cur + numwidth_len - 1].trim();
        let end_pos = cur + numwidth_len;

        if raw_kind == kind {
            last = Some((
                parse_latex_length_prefix_to_twips_with_body_font(raw_indent, body_font_size_hp),
                parse_latex_length_prefix_to_twips_with_body_font(raw_numwidth, body_font_size_hp),
                end_pos,
            ));
        }

        pos = end_pos;
    }

    last
}

fn extract_caption_label_separator_declarations(source: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut pos = 0usize;
    let needle = "\\DeclareCaptionLabelSeparator";

    while let Some(rel) = source[pos..].find(needle) {
        let start = pos + rel + needle.len();
        let mut cur = start;
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }
        let Some(name_len) = braced_len(&source[cur..]) else {
            pos = start;
            continue;
        };
        let name = source[cur + 1..cur + name_len - 1].trim();
        cur += name_len;
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }
        let Some(value_len) = braced_len(&source[cur..]) else {
            pos = cur;
            continue;
        };
        let value = source[cur + 1..cur + value_len - 1].trim();
        if !name.is_empty() && !value.is_empty() {
            map.insert(name.to_ascii_lowercase(), value.to_string());
        }
        pos = cur + value_len;
    }

    map
}

fn extract_captionsetup_option(source: &str, target: Option<&str>, key: &str) -> Option<String> {
    let mut pos = 0usize;
    let mut last_global = None;
    let mut last_targeted = None;
    let needle = "\\captionsetup";

    while let Some(rel) = source[pos..].find(needle) {
        let start = pos + rel + needle.len();
        let mut cur = start;
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }

        let mut scope = None;
        if cur < source.len() && source.as_bytes()[cur] == b'[' {
            if let Some(scope_len) = bracketed_len(&source[cur..]) {
                scope = Some(source[cur + 1..cur + scope_len - 1].to_string());
                cur += scope_len;
            } else {
                pos = start;
                continue;
            }
        }
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }

        let Some(body_len) = braced_len(&source[cur..]) else {
            pos = start;
            continue;
        };
        let body = &source[cur + 1..cur + body_len - 1];
        if let Some(value) = extract_top_level_kv_option_value(body, key) {
            if let Some(target_name) = target {
                match scope.as_deref() {
                    Some(scope_expr) if captionsetup_scope_matches(scope_expr, target_name) => {
                        last_targeted = Some(value);
                    }
                    None => {
                        last_global = Some(value);
                    }
                    _ => {}
                }
            } else {
                last_global = Some(value);
            }
        }
        pos = cur + body_len;
    }

    if target.is_some() {
        last_targeted.or(last_global)
    } else {
        last_global
    }
}

fn captionsetup_scope_matches(scope_expr: &str, target: &str) -> bool {
    let target = target.to_ascii_lowercase();
    scope_expr
        .split(',')
        .map(|token| token.trim().to_ascii_lowercase())
        .any(|token| token == target)
}

fn extract_top_level_kv_option_value(options: &str, key: &str) -> Option<String> {
    let key = key.to_ascii_lowercase();
    for segment in split_top_level_by_comma(options) {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        let Some(eq_pos) = find_top_level_char(segment, '=') else {
            continue;
        };
        let raw_key = segment[..eq_pos].trim().to_ascii_lowercase();
        if raw_key != key {
            continue;
        }
        let raw_value = segment[eq_pos + 1..].trim();
        if raw_value.is_empty() {
            return None;
        }
        return Some(raw_value.to_string());
    }
    None
}

fn split_top_level_by_comma(src: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut paren_depth = 0usize;

    for (idx, ch) in src.char_indices() {
        match ch {
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            ',' if brace_depth == 0 && bracket_depth == 0 && paren_depth == 0 => {
                out.push(&src[start..idx]);
                start = idx + 1;
            }
            _ => {}
        }
    }
    out.push(&src[start..]);
    out
}

fn find_top_level_char(src: &str, needle: char) -> Option<usize> {
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut paren_depth = 0usize;

    for (idx, ch) in src.char_indices() {
        match ch {
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            _ => {}
        }
        if ch == needle && brace_depth == 0 && bracket_depth == 0 && paren_depth == 0 {
            return Some(idx);
        }
    }

    None
}

fn extract_hypersetup_option(source: &str, key: &str) -> Option<String> {
    let needle = "\\hypersetup";
    let mut pos = 0usize;
    let mut last = None;
    let key = key.to_ascii_lowercase();

    while let Some(rel) = source[pos..].find(needle) {
        let start = pos + rel + needle.len();
        let mut cur = start;
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }
        let Some(body_len) = braced_len(&source[cur..]) else {
            pos = start;
            continue;
        };
        let body = &source[cur + 1..cur + body_len - 1];
        if let Some(value) = extract_top_level_kv_option_value(body, &key) {
            last = Some(value);
        }
        pos = cur + body_len;
    }

    last
}

fn hypersetup_has_flag(source: &str, flag: &str) -> bool {
    let needle = "\\hypersetup";
    let mut pos = 0usize;
    let target = flag.to_ascii_lowercase();

    while let Some(rel) = source[pos..].find(needle) {
        let start = pos + rel + needle.len();
        let mut cur = start;
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }
        let Some(body_len) = braced_len(&source[cur..]) else {
            pos = start;
            continue;
        };
        let body = &source[cur + 1..cur + body_len - 1];
        if split_top_level_by_comma(body)
            .iter()
            .map(|entry| entry.trim().to_ascii_lowercase())
            .any(|entry| entry == target)
        {
            return true;
        }
        pos = cur + body_len;
    }

    false
}

fn extract_hypersetup_link_color(source: &str) -> Option<String> {
    let raw = extract_hypersetup_option(source, "linkcolor")
        .or_else(|| extract_hypersetup_option(source, "allcolors"))?;
    normalize_hypersetup_color(&raw)
}

fn normalize_hypersetup_color(raw: &str) -> Option<String> {
    let value = raw
        .trim()
        .trim_matches(['{', '}'])
        .trim()
        .trim_matches('"')
        .trim();
    if value.is_empty() {
        None
    } else {
        Some(value.trim_start_matches('\\').to_string())
    }
}

fn extract_hypersetup_link_underline(source: &str) -> Option<bool> {
    if hypersetup_has_flag(source, "hidelinks") {
        return Some(false);
    }

    if let Some(value) = extract_hypersetup_option(source, "colorlinks")
        && let Some(colorlinks) = parse_caption_bool(&value)
    {
        // hyperref `colorlinks=true` renders colored text without boxes.
        // Use no underline in this mode and underline when links are not color-only.
        return Some(!colorlinks);
    }

    if hypersetup_has_flag(source, "colorlinks") {
        return Some(false);
    }

    None
}

fn extract_body_text_alignment(preamble: &str) -> Option<String> {
    let mut last = None;

    // Global body alignment is often wrapped in \AtBeginDocument{...}.
    let mut pos = 0usize;
    while let Some(rel) = preamble[pos..].find("\\AtBeginDocument") {
        let start = pos + rel + "\\AtBeginDocument".len();
        let mut cur = start;
        while cur < preamble.len() && preamble.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }
        let Some(arg_len) = braced_len(&preamble[cur..]) else {
            pos = start;
            continue;
        };
        let body = &preamble[cur + 1..cur + arg_len - 1];
        if let Some(value) = detect_alignment_directive_top_level(body) {
            last = Some(value.to_string());
        }
        pos = cur + arg_len;
    }

    if let Some(value) = detect_alignment_directive_top_level(preamble) {
        last = Some(value.to_string());
    }

    last
}

fn detect_alignment_directive_top_level(src: &str) -> Option<&'static str> {
    let mut last = None;
    let mut depth = 0usize;
    let bytes = src.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' && (i == 0 || bytes[i - 1] != b'\\') {
            // Skip comments till end of line.
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b == b'{' {
            depth = depth.saturating_add(1);
            i += 1;
            continue;
        }
        if b == b'}' {
            depth = depth.saturating_sub(1);
            i += 1;
            continue;
        }
        if b == b'\\' {
            let mut j = i + 1;
            while j < bytes.len() {
                let ch = bytes[j];
                if ch.is_ascii_alphabetic() || ch == b'@' {
                    j += 1;
                } else {
                    break;
                }
            }
            if depth == 0 {
                let cmd = src[i + 1..j].to_ascii_lowercase();
                match cmd.as_str() {
                    "centering" | "filcenter" => last = Some("center"),
                    "raggedright" | "flushleft" => last = Some("left"),
                    "raggedleft" | "flushright" => last = Some("right"),
                    "justifying" | "justified" => last = Some("both"),
                    _ => {}
                }
            }
            i = if j > i + 1 { j } else { i + 1 };
            continue;
        }
        i += 1;
    }

    last
}

fn extract_page_number_alignment(source: &str) -> Option<String> {
    let mut last: Option<(usize, String)> = None;

    capture_footer_alignment_by_command(source, "\\lfoot", "left", &mut last);
    capture_footer_alignment_by_command(source, "\\cfoot", "center", &mut last);
    capture_footer_alignment_by_command(source, "\\rfoot", "right", &mut last);
    capture_fancyfoot_alignment(source, &mut last);
    capture_memoir_foot_alignment(source, "\\makeoddfoot", &mut last);
    capture_memoir_foot_alignment(source, "\\makeevenfoot", &mut last);
    capture_memoir_foot_alignment(source, "\\makeoddhead", &mut last);
    capture_memoir_foot_alignment(source, "\\makeevenhead", &mut last);

    last.map(|(_, value)| value)
}

fn capture_footer_alignment_by_command(
    source: &str,
    command: &str,
    alignment: &str,
    last: &mut Option<(usize, String)>,
) {
    let mut pos = 0usize;
    while let Some(rel) = source[pos..].find(command) {
        let start = pos + rel;
        let mut cur = start + command.len();
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }
        if cur < source.len()
            && source.as_bytes()[cur] == b'['
            && let Some(scope_len) = bracketed_len(&source[cur..])
        {
            cur += scope_len;
            while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
                cur += 1;
            }
        }
        if cur < source.len()
            && source.as_bytes()[cur] == b'{'
            && let Some(arg_len) = braced_len(&source[cur..])
        {
            let body = &source[cur + 1..cur + arg_len - 1];
            if body.contains("\\thepage") {
                let end_pos = cur + arg_len;
                if last.as_ref().is_none_or(|(prev, _)| end_pos >= *prev) {
                    *last = Some((end_pos, alignment.to_string()));
                }
            }
            pos = cur + arg_len;
            continue;
        }
        pos = cur;
    }
}

fn capture_fancyfoot_alignment(source: &str, last: &mut Option<(usize, String)>) {
    let command = "\\fancyfoot";
    let mut pos = 0usize;
    while let Some(rel) = source[pos..].find(command) {
        let start = pos + rel;
        let mut cur = start + command.len();
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }
        let mut scope: Option<String> = None;
        if cur < source.len()
            && source.as_bytes()[cur] == b'['
            && let Some(scope_len) = bracketed_len(&source[cur..])
        {
            scope = Some(source[cur + 1..cur + scope_len - 1].to_ascii_uppercase());
            cur += scope_len;
            while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
                cur += 1;
            }
        }
        if cur < source.len()
            && source.as_bytes()[cur] == b'{'
            && let Some(arg_len) = braced_len(&source[cur..])
        {
            let body = &source[cur + 1..cur + arg_len - 1];
            if body.contains("\\thepage") {
                let alignment = scope
                    .as_deref()
                    .and_then(|value| {
                        if value.contains('C') {
                            Some("center")
                        } else if value.contains('R') {
                            Some("right")
                        } else if value.contains('L') {
                            Some("left")
                        } else {
                            None
                        }
                    })
                    .unwrap_or("center");
                let end_pos = cur + arg_len;
                if last.as_ref().is_none_or(|(prev, _)| end_pos >= *prev) {
                    *last = Some((end_pos, alignment.to_string()));
                }
            }
            pos = cur + arg_len;
            continue;
        }
        pos = cur;
    }
}

fn capture_memoir_foot_alignment(source: &str, command: &str, last: &mut Option<(usize, String)>) {
    let mut pos = 0usize;
    while let Some(rel) = source[pos..].find(command) {
        let start = pos + rel;
        let mut cur = start + command.len();
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }

        let Some(style_len) = braced_len(&source[cur..]) else {
            pos = cur;
            continue;
        };
        cur += style_len;
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }
        let Some(left_len) = braced_len(&source[cur..]) else {
            pos = cur;
            continue;
        };
        let left = source[cur + 1..cur + left_len - 1].to_string();
        cur += left_len;
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }
        let Some(center_len) = braced_len(&source[cur..]) else {
            pos = cur;
            continue;
        };
        let center = source[cur + 1..cur + center_len - 1].to_string();
        cur += center_len;
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }
        let Some(right_len) = braced_len(&source[cur..]) else {
            pos = cur;
            continue;
        };
        let right = source[cur + 1..cur + right_len - 1].to_string();
        let end_pos = cur + right_len;

        let alignment = if center.contains("\\thepage") {
            Some("center")
        } else if right.contains("\\thepage") {
            Some("right")
        } else if left.contains("\\thepage") {
            Some("left")
        } else {
            None
        };

        if let Some(alignment) = alignment
            && last.as_ref().is_none_or(|(prev, _)| end_pos >= *prev)
        {
            *last = Some((end_pos, alignment.to_string()));
        }

        pos = end_pos;
    }
}

fn resolve_caption_label_separator(
    raw: &str,
    declarations: &HashMap<String, String>,
    source: &str,
) -> Option<String> {
    let mut visited = HashSet::new();
    resolve_caption_label_separator_inner(raw, declarations, source, &mut visited, 0)
}

fn resolve_caption_label_separator_inner(
    raw: &str,
    declarations: &HashMap<String, String>,
    source: &str,
    visited: &mut HashSet<String>,
    depth: usize,
) -> Option<String> {
    if depth > 8 {
        return None;
    }

    let trimmed = raw.trim().trim_matches(['{', '}']).trim();
    if trimmed.is_empty() {
        return None;
    }

    let lowered = trimmed.to_ascii_lowercase();
    if let Some(mapped) = map_known_caption_separator_keyword(&lowered) {
        return Some(mapped.to_string());
    }
    if !visited.insert(lowered.clone()) {
        return None;
    }

    if let Some(value) = declarations.get(&lowered) {
        return resolve_caption_label_separator_inner(
            value,
            declarations,
            source,
            visited,
            depth + 1,
        );
    }

    if let Some(cmd_name) = trimmed.strip_prefix('\\') {
        let cmd_key = cmd_name.to_ascii_lowercase();
        if let Some(value) = declarations.get(&cmd_key) {
            return resolve_caption_label_separator_inner(
                value,
                declarations,
                source,
                visited,
                depth + 1,
            );
        }
        if let Some(value) = extract_renewcommand_value(source, cmd_name) {
            return resolve_caption_label_separator_inner(
                &value,
                declarations,
                source,
                visited,
                depth + 1,
            );
        }
    }

    let normalized = normalize_caption_separator_literal(trimmed);
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn map_known_caption_separator_keyword(value: &str) -> Option<&'static str> {
    match value {
        "colon" => Some(": "),
        "period" => Some(". "),
        "space" => Some(" "),
        "quad" => Some("    "),
        "qquad" => Some("        "),
        "endash" => Some(" – "),
        "emdash" => Some(" — "),
        "dash" => Some(" - "),
        "newline" => Some(" "),
        "none" | "empty" => Some(""),
        _ => None,
    }
}

fn normalize_caption_separator_literal(raw: &str) -> String {
    let mut out = String::new();
    let mut chars = raw.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '{' | '}' => {}
            '~' => out.push(' '),
            '\\' => {
                if chars.peek().is_some_and(|next| next.is_ascii_alphabetic()) {
                    let mut command = String::new();
                    while let Some(next) = chars.peek() {
                        if next.is_ascii_alphabetic() {
                            command.push(*next);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    if let Some(fragment) = map_known_caption_separator_keyword(&command) {
                        out.push_str(fragment);
                    } else if let Some(fragment) = map_known_caption_separator_command(&command) {
                        out.push_str(fragment);
                    }
                } else if let Some(next) = chars.next() {
                    match next {
                        ' ' | '~' => out.push(' '),
                        '-' | ':' | ';' | '.' | ',' | '!' | '?' => out.push(next),
                        _ => {}
                    }
                }
            }
            '\n' | '\r' | '\t' => out.push(' '),
            _ => out.push(ch),
        }
    }

    let normalized = if out.chars().all(|ch| ch.is_whitespace()) && !out.is_empty() {
        " ".to_string()
    } else {
        out
    };
    let normalized = normalized.replace("---", "—").replace("--", "–");
    ensure_caption_separator_spacing(normalized)
}

fn ensure_caption_separator_spacing(mut separator: String) -> String {
    if separator.is_empty() || separator.chars().last().is_some_and(char::is_whitespace) {
        return separator;
    }

    if separator
        .chars()
        .last()
        .is_some_and(|ch| matches!(ch, '.' | ':' | '-' | '—' | '–'))
    {
        separator.push(' ');
    }

    separator
}

fn map_known_caption_separator_command(command: &str) -> Option<&'static str> {
    match command {
        "space" | "enspace" | "thinspace" => Some(" "),
        "quad" => Some("    "),
        "qquad" => Some("        "),
        "textemdash" | "emdash" => Some("—"),
        "textendash" | "endash" => Some("–"),
        _ => None,
    }
}

/// Extract graphics search paths from `\graphicspath{{...}{...}}`.
///
/// Returns cleaned paths from the *last* `\graphicspath` command.
fn extract_graphicspath(source: &str) -> Vec<String> {
    let mut pos = 0usize;
    let mut last_paths = Vec::new();
    let needle = "\\graphicspath";

    while let Some(rel) = source[pos..].find(needle) {
        let start = pos + rel;
        let cmd_end = start + needle.len();
        if source[cmd_end..]
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic())
        {
            pos = cmd_end;
            continue;
        }

        let mut cur = cmd_end;
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }

        let Some(arg_len) = braced_len(&source[cur..]) else {
            pos = cmd_end;
            continue;
        };
        let body = &source[cur + 1..cur + arg_len - 1];
        let mut extracted = Vec::new();
        let mut inner_pos = 0usize;

        while inner_pos < body.len() {
            while inner_pos < body.len() && body.as_bytes()[inner_pos].is_ascii_whitespace() {
                inner_pos += 1;
            }
            if inner_pos >= body.len() {
                break;
            }
            if body[inner_pos..].starts_with('{')
                && let Some(path_len) = braced_len(&body[inner_pos..])
            {
                let raw = &body[inner_pos + 1..inner_pos + path_len - 1];
                if let Some(clean) = normalize_graphics_path_entry(raw)
                    && !extracted.contains(&clean)
                {
                    extracted.push(clean);
                }
                inner_pos += path_len;
                continue;
            }
            let consumed = body[inner_pos..]
                .chars()
                .next()
                .map(|ch| ch.len_utf8())
                .unwrap_or(1);
            inner_pos += consumed;
        }

        last_paths = extracted;
        pos = cur + arg_len;
    }

    last_paths
}

fn detect_alignment_directive(src: &str) -> Option<&'static str> {
    let lower = src.to_ascii_lowercase();
    if lower.contains("\\centering") || lower.contains("\\filcenter") {
        Some("center")
    } else if lower.contains("\\raggedright") || lower.contains("\\flushleft") {
        Some("left")
    } else if lower.contains("\\raggedleft") || lower.contains("\\flushright") {
        Some("right")
    } else if lower.contains("\\justifying") || lower.contains("\\justified") {
        Some("both")
    } else {
        None
    }
}

fn extract_heading_number_delimiter_from_expr(expr: &str) -> Option<String> {
    let tokens = [
        "\\thetitle",
        "\\thechapter",
        "\\arabic{chapter}",
        "\\Roman{chapter}",
        "\\roman{chapter}",
        "\\Alph{chapter}",
        "\\alph{chapter}",
    ];
    for token in tokens {
        if let Some(idx) = expr.rfind(token) {
            let suffix = &expr[idx + token.len()..];
            return Some(normalize_delimiter_suffix(suffix));
        }
    }
    None
}

fn extract_section_number_delimiter_from_expr(expr: &str) -> Option<String> {
    let tokens = ["\\csname the#1\\endcsname", "\\the#1"];
    for token in tokens {
        if let Some(idx) = expr.rfind(token) {
            let suffix = &expr[idx + token.len()..];
            return Some(normalize_delimiter_suffix(suffix));
        }
    }
    None
}

fn normalize_delimiter_suffix(raw_suffix: &str) -> String {
    let trimmed = raw_suffix.trim_start();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut delim = String::new();
    for ch in trimmed.chars() {
        if ch == '\\' || ch == '{' || ch.is_ascii_alphanumeric() {
            break;
        }
        delim.push(ch);
    }
    delim.trim().to_string()
}

fn normalize_caption_justification(raw: &str) -> Option<&'static str> {
    let normalized = raw.trim().trim_matches(['{', '}']).to_ascii_lowercase();
    match normalized.as_str() {
        "centering" | "center" => Some("center"),
        "raggedright" | "left" | "flushleft" => Some("left"),
        "raggedleft" | "right" | "flushright" => Some("right"),
        "justified" | "justify" | "both" => Some("both"),
        _ => None,
    }
}

fn normalize_graphics_path_entry(raw: &str) -> Option<String> {
    let mut path = raw.trim().replace('\\', "/");
    while let Some(stripped) = path
        .strip_prefix(".//")
        .or_else(|| path.strip_prefix("./"))
        .or_else(|| path.strip_prefix(".\\"))
    {
        path = stripped.to_string();
    }
    while path.contains("//") {
        path = path.replace("//", "/");
    }
    if path.is_empty() || path == "." {
        return None;
    }
    Some(path)
}

fn extract_chapter_name_from_chapstyle(source: &str, language: Option<&str>) -> Option<String> {
    let chapstyle = extract_last_setcounter_value(source, "chapstyle")?;
    if chapstyle <= 0 {
        return None;
    }

    let uses_chapter_name_macro = source.contains("\\@chapapp")
        || source.contains("\\chaptername\\space")
        || source.contains("\\printchaptername");
    if !uses_chapter_name_macro {
        return None;
    }

    default_chapter_name_for_language(language).map(|name| name.to_string())
}

fn default_chapter_name_for_language(language: Option<&str>) -> Option<&'static str> {
    match language {
        Some("ru-RU") => Some("Глава"),
        Some("uk-UA") => Some("Розділ"),
        Some("be-BY") => Some("Раздзел"),
        Some("kk-KZ") => Some("Тарау"),
        Some("en-US") | Some("en-GB") => Some("Chapter"),
        Some("de-DE") => Some("Kapitel"),
        Some("fr-FR") => Some("Chapitre"),
        Some("es-ES") => Some("Capítulo"),
        Some("it-IT") => Some("Capitolo"),
        Some("pt-BR") => Some("Capítulo"),
        Some("tr-TR") => Some("Bölüm"),
        _ => None,
    }
}

/// Returns the default bibliography section title for the given BCP-47 language tag.
///
/// Used as fallback when `\printbibliography` has no `title=` argument and when
/// `\insertbibliofullsorted` or similar commands appear without an explicit title.
///
/// Follows the same pattern as [`default_chapter_name_for_language`].
/// Falls back to `"REFERENCES"` for unknown or absent language tags.
fn default_bibliography_title_for_language(language: Option<&str>) -> &'static str {
    match language {
        Some("ru-RU") => "СПИСОК ЛИТЕРАТУРЫ",
        Some("uk-UA") => "СПИСОК ЛІТЕРАТУРИ",
        Some("be-BY") => "СПІС ЛІТАРАТУРЫ",
        Some("kk-KZ") => "ӘДЕБИЕТТЕР ТІЗІМІ",
        Some("en-US") | Some("en-GB") => "REFERENCES",
        Some("de-DE") => "LITERATURVERZEICHNIS",
        Some("fr-FR") => "BIBLIOGRAPHIE",
        Some("es-ES") => "BIBLIOGRAFÍA",
        Some("it-IT") => "BIBLIOGRAFIA",
        Some("pt-BR") => "REFERÊNCIAS",
        Some("tr-TR") => "KAYNAKLAR",
        _ => "REFERENCES",
    }
}

/// Extract braced argument lists for chapter-related `\titleformat` commands.
///
/// Supports:
/// - `\titleformat{\chapter}[shape]{format}{label}{sep}{before}[after]`
/// - `\titleformat*{\chapter}{format}`
/// - `\titleformat{name=\chapter}[shape]{format}{label}{sep}{before}[after]`
fn extract_titleformat_chapter_arguments(source: &str) -> Vec<Vec<String>> {
    let mut all_args = Vec::new();
    let needle = "\\titleformat";
    let mut pos = 0usize;

    while let Some(rel) = source[pos..].find(needle) {
        let start = pos + rel + needle.len();
        let mut cur = start;
        if cur < source.len() && source.as_bytes()[cur] == b'*' {
            cur += 1;
        }
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }

        let mut is_chapter = false;
        if cur < source.len() && source.as_bytes()[cur] == b'{' {
            let Some(target_len) = braced_len(&source[cur..]) else {
                pos = start;
                continue;
            };
            let target = source[cur + 1..cur + target_len - 1].trim();
            is_chapter = titleformat_target_mentions_chapter(target);
            cur += target_len;
        } else if cur < source.len() && source.as_bytes()[cur] == b'[' {
            let Some(selector_len) = bracketed_len(&source[cur..]) else {
                pos = start;
                continue;
            };
            let selector = source[cur + 1..cur + selector_len - 1].trim();
            is_chapter = titleformat_target_mentions_chapter(selector);
            cur += selector_len;
        }

        if !is_chapter {
            pos = cur.max(start);
            continue;
        }

        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }
        if cur < source.len() && source.as_bytes()[cur] == b'[' {
            if let Some(shape_len) = bracketed_len(&source[cur..]) {
                cur += shape_len;
            } else {
                pos = start;
                continue;
            }
        }

        let mut args = Vec::new();
        loop {
            while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
                cur += 1;
            }
            if cur >= source.len() || source.as_bytes()[cur] != b'{' {
                break;
            }
            let Some(arg_len) = braced_len(&source[cur..]) else {
                break;
            };
            args.push(source[cur + 1..cur + arg_len - 1].to_string());
            cur += arg_len;
        }

        if !args.is_empty() {
            all_args.push(args);
        }
        pos = cur.max(start);
    }

    all_args
}

fn titleformat_target_mentions_chapter(target: &str) -> bool {
    target.contains("\\chapter")
}

fn extract_last_macro_braced_argument(source: &str, macro_name: &str) -> Option<String> {
    let mut pos = 0usize;
    let mut last = None;

    while let Some(rel) = source[pos..].find(macro_name) {
        let start = pos + rel;
        let cmd_end = start + macro_name.len();
        if source[cmd_end..]
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic())
        {
            pos = cmd_end;
            continue;
        }

        let mut arg_pos = cmd_end;
        while arg_pos < source.len() && source.as_bytes()[arg_pos].is_ascii_whitespace() {
            arg_pos += 1;
        }
        if arg_pos < source.len() && source.as_bytes()[arg_pos] == b'[' {
            if let Some(opt_len) = bracketed_len(&source[arg_pos..]) {
                arg_pos += opt_len;
            } else {
                pos = cmd_end;
                continue;
            }
            while arg_pos < source.len() && source.as_bytes()[arg_pos].is_ascii_whitespace() {
                arg_pos += 1;
            }
        }

        if arg_pos < source.len()
            && source.as_bytes()[arg_pos] == b'{'
            && let Some(arg_len) = braced_len(&source[arg_pos..])
        {
            let payload = source[arg_pos + 1..arg_pos + arg_len - 1]
                .trim()
                .to_string();
            last = Some(payload);
            pos = arg_pos + arg_len;
            continue;
        }

        pos = cmd_end;
    }

    last
}

fn extract_last_spacing_factor(source: &str, body_font_size_hp: Option<usize>) -> Option<f64> {
    let mut pos = 0usize;
    let mut last = None;

    while pos < source.len() {
        let mut next_cmd: Option<(&str, usize)> = None;
        for command in [
            "\\setSpacing",
            "\\setstretch",
            "\\linespread",
            "\\SingleSpacing",
            "\\OnehalfSpacing",
            "\\DoubleSpacing",
            "\\singlespacing",
            "\\onehalfspacing",
            "\\doublespacing",
        ] {
            if let Some(rel) = source[pos..].find(command) {
                let abs = pos + rel;
                match next_cmd {
                    Some((_, best_abs)) if abs >= best_abs => {}
                    _ => next_cmd = Some((command, abs)),
                }
            }
        }
        let Some((command, cmd_start)) = next_cmd else {
            break;
        };

        let cmd_end = cmd_start + command.len();
        if source[cmd_end..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
        {
            pos = cmd_end;
            continue;
        }

        match command {
            "\\setSpacing" | "\\setstretch" | "\\linespread" => {
                let mut arg_pos = cmd_end;
                while arg_pos < source.len() && source.as_bytes()[arg_pos].is_ascii_whitespace() {
                    arg_pos += 1;
                }
                if arg_pos < source.len()
                    && source.as_bytes()[arg_pos] == b'{'
                    && let Some(arg_len) = braced_len(&source[arg_pos..])
                {
                    let value = &source[arg_pos + 1..arg_pos + arg_len - 1];
                    if let Some(factor) = parse_spacing_factor(value) {
                        last = Some(factor);
                    }
                    pos = arg_pos + arg_len;
                    continue;
                }
            }
            "\\SingleSpacing" => {
                last = Some(1.0);
            }
            "\\OnehalfSpacing" => {
                last = Some(memoir_onehalf_spacing_factor(body_font_size_hp));
            }
            "\\DoubleSpacing" => {
                last = Some(memoir_double_spacing_factor(body_font_size_hp));
            }
            "\\singlespacing" => {
                last = Some(1.0);
            }
            "\\onehalfspacing" => {
                last = Some(setspace_onehalf_spacing_factor(body_font_size_hp));
            }
            "\\doublespacing" => {
                last = Some(setspace_double_spacing_factor(body_font_size_hp));
            }
            _ => {}
        }

        pos = cmd_end;
        if source[pos..].starts_with('*') {
            pos += 1;
        }
    }

    last
}

fn spacing_factor_to_twips(factor: f64) -> Option<i32> {
    let twips = (DOCX_AUTO_LINE_SPACING_UNIT_TWIPS * factor).round();
    if twips.is_finite() && twips > 0.0 {
        Some(twips as i32)
    } else {
        None
    }
}

fn memoir_onehalf_spacing_factor(body_font_size_hp: Option<usize>) -> f64 {
    match body_font_size_hp.unwrap_or(20) {
        20 => 1.25,  // 10pt
        22 => 1.213, // 11pt
        24 => 1.241, // 12pt
        // `memoir` 14pt one-half spacing is visually tighter in DOCX than in LaTeX;
        // apply a calibrated Word-equivalent factor.
        28 => 1.30, // 14pt
        34 => 1.16, // 17pt
        18 => 1.35, // 9pt
        _ => 1.16,  // memoir extended-size fallback
    }
}

fn memoir_double_spacing_factor(body_font_size_hp: Option<usize>) -> f64 {
    match body_font_size_hp.unwrap_or(20) {
        20 => 1.667, // 10pt
        22 => 1.618, // 11pt
        24 => 1.655, // 12pt
        28 => 1.733, // 14pt (paired with one-half calibration above)
        34 => 1.545, // 17pt
        18 => 1.8,   // 9pt
        _ => 1.5,    // memoir extended-size fallback
    }
}

fn setspace_onehalf_spacing_factor(body_font_size_hp: Option<usize>) -> f64 {
    match body_font_size_hp.unwrap_or(20) {
        20 => 1.25,  // 10pt
        22 => 1.213, // 11pt
        24 => 1.241, // 12pt
        _ => 1.25,   // setspace fallback
    }
}

fn setspace_double_spacing_factor(body_font_size_hp: Option<usize>) -> f64 {
    match body_font_size_hp.unwrap_or(20) {
        20 => 1.667, // 10pt
        22 => 1.618, // 11pt
        24 => 1.655, // 12pt
        _ => 1.667,  // setspace fallback
    }
}

fn parse_spacing_factor(raw: &str) -> Option<f64> {
    raw.trim()
        .replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|factor| factor.is_finite() && *factor > 0.0)
}

fn extract_setlength_value_twips_with_body_font(
    source: &str,
    name: &str,
    body_font_size_hp: Option<usize>,
) -> Option<i32> {
    let mut pos = 0usize;
    let mut last = None;

    while let Some(rel) = source[pos..].find("\\setlength") {
        let cmd_start = pos + rel;
        let mut cur = cmd_start + "\\setlength".len();
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }
        let Some(name_len) = braced_len(&source[cur..]) else {
            pos = cur;
            continue;
        };
        let raw_name = source[cur + 1..cur + name_len - 1]
            .trim()
            .trim_start_matches('\\');
        cur += name_len;
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }
        let Some(value_len) = braced_len(&source[cur..]) else {
            pos = cur;
            continue;
        };
        let raw_value = source[cur + 1..cur + value_len - 1].trim();
        if raw_name == name {
            last = parse_latex_length_to_twips_with_body_font(raw_value, body_font_size_hp);
        }
        pos = cur + value_len;
    }

    last
}

fn extract_setlength_value_raw(source: &str, name: &str) -> Option<String> {
    let mut pos = 0usize;
    let mut last = None;

    while let Some(rel) = source[pos..].find("\\setlength") {
        let cmd_start = pos + rel;
        let mut cur = cmd_start + "\\setlength".len();
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }
        let Some(name_len) = braced_len(&source[cur..]) else {
            pos = cur;
            continue;
        };
        let raw_name = source[cur + 1..cur + name_len - 1]
            .trim()
            .trim_start_matches('\\');
        cur += name_len;
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }
        let Some(value_len) = braced_len(&source[cur..]) else {
            pos = cur;
            continue;
        };
        if raw_name == name {
            last = Some(source[cur + 1..cur + value_len - 1].trim().to_string());
        }
        pos = cur + value_len;
    }

    last
}

fn parse_latex_length_to_twips_with_body_font(
    raw: &str,
    body_font_size_hp: Option<usize>,
) -> Option<i32> {
    let normalized = raw
        .trim()
        .trim_matches(['{', '}'])
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    if normalized.is_empty() {
        return None;
    }

    parse_single_latex_length_to_twips(&normalized, body_font_size_hp)
        .or_else(|| parse_additive_latex_length_to_twips(&normalized, body_font_size_hp))
}

fn parse_single_latex_length_to_twips(
    normalized: &str,
    body_font_size_hp: Option<usize>,
) -> Option<i32> {
    // `em` is relative — 1 em equals the body font size.
    let em_twips =
        em_twips_for_body_font(body_font_size_hp.or(Some(DEFAULT_BODY_FONT_SIZE_HP)), 1.0) as f64;
    let ex_twips = em_twips * EX_TO_EM_RATIO_F64;
    for (unit, twips_per_unit) in [
        ("mm", 56.692_913_f64),
        ("cm", 566.929_133_f64),
        ("in", 1440.0_f64),
        ("em", em_twips),
        ("ex", ex_twips),
        ("pt", DOCX_TWIPS_PER_POINT_F64),
        ("pc", TWIPS_PER_PICA_F64),
        ("bp", TWIPS_PER_BIG_POINT_F64),
        ("dd", TWIPS_PER_DIDOT_POINT_F64),
        ("cc", TWIPS_PER_CICERO_F64),
        ("sp", TWIPS_PER_SCALED_POINT_F64),
    ] {
        if let Some(number) = normalized.strip_suffix(unit) {
            let value = number.replace(',', ".").parse::<f64>().ok()?;
            if !value.is_finite() {
                return None;
            }
            let twips = (value * twips_per_unit).round();
            if twips < i32::MIN as f64 || twips > i32::MAX as f64 {
                return None;
            }
            return Some(twips as i32);
        }
    }
    None
}

fn parse_additive_latex_length_to_twips(
    normalized: &str,
    body_font_size_hp: Option<usize>,
) -> Option<i32> {
    let canonical = normalized.replace("plus", "+").replace("minus", "-");
    let has_plus = canonical.contains('+');
    let has_minus_after_first = canonical.char_indices().skip(1).any(|(_, ch)| ch == '-');
    let has_operator = has_plus || has_minus_after_first;
    if !has_operator {
        return None;
    }

    let mut total: i64 = 0;
    let mut sign: i64 = 1;
    let mut token_start = 0usize;
    let chars: Vec<(usize, char)> = canonical.char_indices().collect();

    if let Some((_, first)) = chars.first()
        && (*first == '+' || *first == '-')
    {
        sign = if *first == '+' { 1 } else { -1 };
        token_start = 1;
    }

    for (idx, ch) in chars {
        if idx < token_start {
            continue;
        }
        if ch != '+' && ch != '-' {
            continue;
        }

        let token = canonical[token_start..idx].trim();
        if token.is_empty() {
            return None;
        }
        let value = parse_single_latex_length_to_twips(token, body_font_size_hp)? as i64;
        total = total.saturating_add(sign.saturating_mul(value));
        sign = if ch == '+' { 1 } else { -1 };
        token_start = idx + 1;
    }

    let token = canonical[token_start..].trim();
    if token.is_empty() {
        return None;
    }
    let value = parse_single_latex_length_to_twips(token, body_font_size_hp)? as i64;
    total = total.saturating_add(sign.saturating_mul(value));

    i32::try_from(total).ok()
}

fn parse_latex_length_prefix_to_twips_with_body_font(
    raw: &str,
    body_font_size_hp: Option<usize>,
) -> Option<i32> {
    let compact = raw
        .trim()
        .trim_matches(['{', '}'])
        .replace(',', ".")
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    if compact.is_empty() {
        return None;
    }

    let mut end = 0usize;
    let mut seen_digit = false;
    for (idx, ch) in compact.char_indices() {
        if ch.is_ascii_digit() {
            seen_digit = true;
            end = idx + ch.len_utf8();
            continue;
        }
        if ch == '.' && seen_digit {
            end = idx + ch.len_utf8();
            continue;
        }
        break;
    }
    if !seen_digit {
        return None;
    }

    let unit_tail = compact[end..].to_ascii_lowercase();
    for unit in [
        "mm", "cm", "in", "em", "ex", "pt", "pc", "bp", "dd", "cc", "sp",
    ] {
        if unit_tail.starts_with(unit) {
            let candidate = format!("{}{}", &compact[..end], unit);
            return parse_latex_length_to_twips_with_body_font(&candidate, body_font_size_hp);
        }
    }
    None
}

fn collect_parse_metadata(source: &str, input_path: &Path, root_dir: &Path) -> ParseMetadata {
    let mut metadata = ParseMetadata {
        counters: collect_setcounter_values(source),
        document_class: extract_documentclass_name(source),
        ..ParseMetadata::default()
    };

    if !metadata.counters.contains_key("year")
        && let Some(year) = current_utc_year()
    {
        metadata.counters.insert("year".to_string(), year);
    }

    metadata.counters.insert(
        "citenum".to_string(),
        count_unique_citation_keys(source) as i64,
    );

    let mut appendix_labels = HashSet::new();
    for label in extract_labels(source) {
        if let Some(rest) = label.strip_prefix("app:") {
            appendix_labels.insert(rest.to_string());
        }
    }
    metadata
        .counters
        .insert("totalappendix".to_string(), appendix_labels.len() as i64);

    if let Some(pages) = infer_total_pages_from_aux(input_path) {
        metadata
            .counters
            .insert("TotPages".to_string(), pages as i64);
    }

    let bib_paths = collect_bib_resource_paths(source, root_dir);
    metadata.bibliography = load_bibliography_map(&bib_paths);
    collect_author_publication_counters(&mut metadata, &bib_paths);

    metadata
}

fn enrich_structural_counters(metadata: &mut ParseMetadata, document: &Document, source: &str) {
    let chapter_count = document
        .blocks
        .iter()
        .filter(|block| {
            matches!(
                block,
                Block::Section {
                    level: 1,
                    number: Some(_),
                    ..
                }
            )
        })
        .count() as i64;
    metadata
        .counters
        .entry("totalchapter".to_string())
        .or_insert(chapter_count);

    let figure_count = document
        .blocks
        .iter()
        .filter(|block| matches!(block, Block::Figure(_)))
        .count() as i64;
    metadata
        .counters
        .insert("totalcount@figure".to_string(), figure_count);

    let table_count = document
        .blocks
        .iter()
        .filter(|block| matches!(block, Block::Table(_)))
        .count() as i64;
    metadata
        .counters
        .insert("totalcount@table".to_string(), table_count);

    if let Some(text) = metadata.text_counters.get("citeauthorpl").cloned() {
        metadata.text_counters.insert("formatpl".to_string(), text);
    }

    if !metadata.counters.contains_key("TotPages") {
        let approx_pages = source.matches("\\newpage").count() as i64;
        if approx_pages > 0 {
            metadata
                .counters
                .insert("TotPages".to_string(), approx_pages);
        }
    }
}

fn collect_setcounter_values(source: &str) -> HashMap<String, i64> {
    let mut counters = HashMap::new();
    let mut pos = 0usize;
    while let Some(rel) = source[pos..].find("\\setcounter") {
        let start = pos + rel + "\\setcounter".len();
        let mut cur = start;
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }
        let Some(name_len) = braced_len(&source[cur..]) else {
            pos = start;
            continue;
        };
        let name = source[cur + 1..cur + name_len - 1].trim().to_string();
        cur += name_len;
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }
        let Some(value_len) = braced_len(&source[cur..]) else {
            pos = cur;
            continue;
        };
        let value_src = source[cur + 1..cur + value_len - 1].trim();
        if let Ok(value) = value_src.parse::<i64>() {
            counters.insert(name, value);
        }
        pos = cur + value_len;
    }
    counters
}

fn current_utc_year() -> Option<i64> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    let days_since_unix_epoch = (now.as_secs() / 86_400) as i64;
    Some(civil_year_from_days_since_unix_epoch(days_since_unix_epoch))
}

fn civil_year_from_days_since_unix_epoch(days_since_unix_epoch: i64) -> i64 {
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let month = month_part + if month_part < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    year
}

fn collect_bib_resource_paths(source: &str, root_dir: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    let mut pos = 0usize;
    while let Some(rel) = source[pos..].find("\\addbibresource") {
        let start = pos + rel + "\\addbibresource".len();
        let mut cur = start;
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }
        if cur < source.len()
            && source.as_bytes()[cur] == b'['
            && let Some(opt_len) = bracketed_len(&source[cur..])
        {
            cur += opt_len;
        }
        while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }
        let Some(arg_len) = braced_len(&source[cur..]) else {
            pos = start;
            continue;
        };
        let raw = source[cur + 1..cur + arg_len - 1].trim();
        if raw.is_empty() {
            pos = cur + arg_len;
            continue;
        }
        let mut path = root_dir.join(raw);
        if path.extension().is_none() {
            path.set_extension("bib");
        }
        let canonical = std::fs::canonicalize(&path).unwrap_or(path.clone());
        if canonical.is_file() && seen.insert(canonical.clone()) {
            paths.push(canonical);
        }
        pos = cur + arg_len;
    }
    paths
}

fn load_bibliography_map(paths: &[PathBuf]) -> HashMap<String, BibEntry> {
    let mut map = HashMap::new();
    for path in paths {
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        for (key, entry) in parse_bib_entries(&content) {
            map.entry(key).or_insert(entry);
        }
    }
    map
}

fn parse_bib_entries(content: &str) -> Vec<(String, BibEntry)> {
    let mut entries = Vec::new();
    let bytes = content.as_bytes();
    let mut pos = 0usize;

    while pos < content.len() {
        let Some(rel) = content[pos..].find('@') else {
            break;
        };
        pos += rel + 1;

        while pos < content.len() && content.as_bytes()[pos].is_ascii_alphabetic() {
            pos += 1;
        }
        while pos < content.len() && content.as_bytes()[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= content.len() {
            break;
        }

        let open = bytes[pos];
        let close = match open {
            b'{' => b'}',
            b'(' => b')',
            _ => continue,
        };
        pos += 1;

        let key_start = pos;
        while pos < content.len() && bytes[pos] != b',' {
            pos += 1;
        }
        if pos >= content.len() {
            break;
        }
        let key = content[key_start..pos].trim().to_string();
        pos += 1;

        let body_start = pos;
        let mut depth = 1usize;
        while pos < content.len() {
            let b = bytes[pos];
            if b == open {
                depth += 1;
            } else if b == close {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            pos += 1;
        }
        if pos > content.len() {
            break;
        }
        let body = &content[body_start..pos];
        if !key.is_empty() {
            entries.push((
                key,
                BibEntry {
                    fields: parse_bib_fields(body),
                },
            ));
        }
        pos = pos.saturating_add(1);
    }

    entries
}

fn parse_bib_fields(body: &str) -> HashMap<String, String> {
    let bytes = body.as_bytes();
    let mut fields = HashMap::new();
    let mut pos = 0usize;

    while pos < body.len() {
        while pos < body.len() && (bytes[pos].is_ascii_whitespace() || bytes[pos] == b',') {
            pos += 1;
        }
        if pos >= body.len() {
            break;
        }

        let name_start = pos;
        while pos < body.len() && (bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'_') {
            pos += 1;
        }
        if pos == name_start {
            pos += 1;
            continue;
        }
        let name = body[name_start..pos].trim().to_ascii_lowercase();
        while pos < body.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= body.len() || bytes[pos] != b'=' {
            continue;
        }
        pos += 1;
        while pos < body.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= body.len() {
            break;
        }

        let value = if bytes[pos] == b'{' {
            let start = pos;
            let mut depth = 0usize;
            while pos < body.len() {
                if bytes[pos] == b'{' {
                    depth += 1;
                } else if bytes[pos] == b'}' {
                    depth -= 1;
                    if depth == 0 {
                        pos += 1;
                        break;
                    }
                }
                pos += 1;
            }
            body[start..pos].to_string()
        } else if bytes[pos] == b'"' {
            let start = pos + 1;
            pos += 1;
            while pos < body.len() {
                if bytes[pos] == b'"' && bytes[pos.saturating_sub(1)] != b'\\' {
                    break;
                }
                pos += 1;
            }
            let end = pos.min(body.len());
            pos = pos.saturating_add(1);
            body[start..end].to_string()
        } else {
            let start = pos;
            while pos < body.len() && bytes[pos] != b',' && bytes[pos] != b'\n' {
                pos += 1;
            }
            body[start..pos].to_string()
        };

        let normalized = normalize_bib_value(&value);
        if !normalized.is_empty() {
            fields.insert(name, normalized);
        }
    }

    fields
}

fn normalize_bib_value(raw: &str) -> String {
    let mut value = raw.trim().to_string();
    loop {
        let trimmed = value.trim();
        if trimmed.starts_with('{') && trimmed.ends_with('}') && trimmed.len() > 1 {
            value = trimmed[1..trimmed.len() - 1].trim().to_string();
            continue;
        }
        break;
    }
    value = value.replace("~", " ");
    value = value.replace("\\&", "&");
    value = value.replace("\\_", "_");
    value = value.replace("\\%", "%");
    value = value.replace("\\\"", "\"");
    value = value.replace("\\'", "'");
    value = value.replace('{', "");
    value = value.replace('}', "");
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn collect_author_publication_counters(metadata: &mut ParseMetadata, paths: &[PathBuf]) {
    let Some(author_path) = paths
        .iter()
        .find(|path| path.file_name().is_some_and(|name| name == "author.bib"))
    else {
        return;
    };
    let Ok(content) = std::fs::read_to_string(author_path) else {
        return;
    };

    let entries = parse_bib_entries(&content);
    if entries.is_empty() {
        return;
    }

    let mut citeauthor = 0i64;
    let mut citeauthorvak = 0i64;
    let mut citeauthorscopus = 0i64;
    let mut citeauthorwos = 0i64;
    let mut citeauthorconf = 0i64;
    let mut citeauthorother = 0i64;
    let mut citeauthorpatent = 0i64;
    let mut citeauthorprogram = 0i64;
    let mut citeauthorvakscopuswos = 0i64;
    let mut citeauthorscopuswos = 0i64;
    let mut citeauthorpl = 0f64;

    for (_, entry) in entries {
        let has = |field: &str| {
            entry
                .fields
                .get(field)
                .map(|v| v.eq_ignore_ascii_case("true"))
                .unwrap_or(false)
        };
        citeauthor += 1;
        if has("authorvak") {
            citeauthorvak += 1;
        }
        if has("authorscopus") {
            citeauthorscopus += 1;
        }
        if has("authorwos") {
            citeauthorwos += 1;
        }
        if has("authorconf") {
            citeauthorconf += 1;
        }
        if has("authorother") {
            citeauthorother += 1;
        }
        if has("authorpatent") {
            citeauthorpatent += 1;
        }
        if has("authorprogram") {
            citeauthorprogram += 1;
        }
        if has("authorvak") || has("authorscopus") || has("authorwos") {
            citeauthorvakscopuswos += 1;
        }
        if has("authorscopus") || has("authorwos") {
            citeauthorscopuswos += 1;
        }

        if let Some(addendum) = entry.fields.get("addendum")
            && let Some(pl) = extract_publication_volume(addendum)
        {
            citeauthorpl += pl;
        }
    }

    metadata
        .counters
        .insert("citeauthor".to_string(), citeauthor.max(0));
    metadata
        .counters
        .insert("citeauthorvak".to_string(), citeauthorvak.max(0));
    metadata
        .counters
        .insert("citeauthorscopus".to_string(), citeauthorscopus.max(0));
    metadata
        .counters
        .insert("citeauthorwos".to_string(), citeauthorwos.max(0));
    metadata
        .counters
        .insert("citeauthorconf".to_string(), citeauthorconf.max(0));
    metadata
        .counters
        .insert("citeauthorother".to_string(), citeauthorother.max(0));
    metadata
        .counters
        .insert("citeauthorpatent".to_string(), citeauthorpatent.max(0));
    metadata
        .counters
        .insert("citeauthorprogram".to_string(), citeauthorprogram.max(0));
    metadata.counters.insert(
        "citeauthorvakscopuswos".to_string(),
        citeauthorvakscopuswos.max(0),
    );
    metadata.counters.insert(
        "citeauthorscopuswos".to_string(),
        citeauthorscopuswos.max(0),
    );
    metadata.counters.insert(
        "citeregistered".to_string(),
        citeauthorpatent + citeauthorprogram,
    );

    if citeauthorpl > 0.0 {
        let formatted = format!("{citeauthorpl:.2}").replace('.', ",");
        metadata
            .text_counters
            .insert("citeauthorpl".to_string(), formatted);
    }
}

fn extract_publication_volume(text: &str) -> Option<f64> {
    let mut value = String::new();
    let mut seen_digit = false;
    for ch in text.chars() {
        if ch.is_ascii_digit() || ch == ',' || ch == '.' {
            value.push(ch);
            seen_digit = true;
        } else if seen_digit {
            break;
        }
    }
    if value.is_empty() {
        None
    } else {
        value.replace(',', ".").parse::<f64>().ok()
    }
}

fn count_unique_citation_keys(source: &str) -> usize {
    let mut keys = HashSet::new();
    for cmd in ["\\autocite", "\\cite"] {
        let mut pos = 0usize;
        while let Some(rel) = source[pos..].find(cmd) {
            let start = pos + rel + cmd.len();
            let mut cur = start;
            while cur < source.len() && source.as_bytes()[cur].is_ascii_whitespace() {
                cur += 1;
            }
            let Some(arg_len) = braced_len(&source[cur..]) else {
                pos = start;
                continue;
            };
            let payload = &source[cur + 1..cur + arg_len - 1];
            for key in payload.split(',').map(str::trim).filter(|k| !k.is_empty()) {
                keys.insert(key.to_string());
            }
            pos = cur + arg_len;
        }
    }
    keys.len()
}

fn infer_total_pages_from_aux(input_path: &Path) -> Option<u32> {
    let aux_path = input_path.with_extension("aux");
    let raw = std::fs::read_to_string(aux_path).ok()?;
    infer_total_pages_from_aux_text(&raw)
}

fn infer_total_pages_from_aux_text(aux: &str) -> Option<u32> {
    infer_total_pages_from_abs_page_last(aux)
        .or_else(|| infer_total_pages_from_last_page_label(aux))
}

fn infer_total_pages_from_abs_page_last(aux: &str) -> Option<u32> {
    for line in aux.lines() {
        let Some(marker_pos) = line.find("\\@abspage@last") else {
            continue;
        };
        let tail = &line[marker_pos + "\\@abspage@last".len()..];
        let Some(open_pos) = tail.find('{') else {
            continue;
        };
        let grouped = &tail[open_pos..];
        let Some(group_len) = braced_len(grouped) else {
            continue;
        };
        let payload = grouped[1..group_len - 1].trim();
        if let Ok(pages) = payload.parse::<u32>() {
            return Some(pages);
        }
    }

    None
}

fn infer_total_pages_from_last_page_label(aux: &str) -> Option<u32> {
    for line in aux.lines() {
        let marker = if line.contains("\\newlabel{LastPage}") {
            "\\newlabel{LastPage}"
        } else if line.contains("\\newlabel{lastpage}") {
            "\\newlabel{lastpage}"
        } else if line.contains("\\newlabel{LastPages}") {
            "\\newlabel{LastPages}"
        } else {
            continue;
        };

        let Some(marker_pos) = line.find(marker) else {
            continue;
        };
        let mut tail = line[marker_pos + marker.len()..].trim_start();
        if !tail.starts_with('{') {
            let Some(open_pos) = tail.find('{') else {
                continue;
            };
            tail = &tail[open_pos..];
        }

        let Some(payload_len) = braced_len(tail) else {
            continue;
        };
        let payload = &tail[1..payload_len - 1];

        let mut rest = payload.trim_start();
        let Some(first_group_len) = braced_len(rest) else {
            continue;
        };
        rest = rest[first_group_len..].trim_start();

        let Some(second_group_len) = braced_len(rest) else {
            continue;
        };
        let page_value = rest[1..second_group_len - 1].trim();
        if let Ok(pages) = page_value.parse::<u32>() {
            return Some(pages);
        }
    }

    None
}

fn resolve_dynamic_placeholders(blocks: &mut [Block], metadata: &ParseMetadata) {
    for block in blocks {
        match block {
            Block::Paragraph(inlines)
            | Block::StyledParagraph { inlines, .. }
            | Block::Section { title: inlines, .. }
            | Block::Figure(Figure {
                caption: inlines,
                source: _,
                ..
            }) => {
                resolve_inline_placeholders(inlines, metadata);
            }
            Block::Table(table) => {
                resolve_inline_placeholders(&mut table.caption, metadata);
                resolve_inline_placeholders(&mut table.source, metadata);
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        resolve_inline_placeholders(&mut cell.content, metadata);
                    }
                }
            }
            Block::List(list) => {
                for item in &mut list.items {
                    resolve_inline_placeholders(item, metadata);
                }
            }
            Block::DisplayMath(_)
            | Block::PageBreak
            | Block::PageOrientationSwitch { .. }
            | Block::BibliographyHeading { .. }
            | Block::TableOfContents => {}
        }

        if let Block::Figure(figure) = block {
            resolve_inline_placeholders(&mut figure.source, metadata);
        }
    }
}

fn resolve_inline_placeholders(inlines: &mut [Inline], metadata: &ParseMetadata) {
    for inline in inlines {
        match inline {
            Inline::Text(text) => {
                let with_markers = replace_counter_markers(text, metadata);
                *text = apply_known_counter_fallbacks(&with_markers, metadata);
            }
            Inline::Bold(children) | Inline::Italic(children) | Inline::Footnote(children) => {
                resolve_inline_placeholders(children, metadata);
            }
            Inline::InlineMath(_) | Inline::Reference(_) | Inline::LineBreak => {}
        }
    }
}

#[derive(Debug, Default)]
struct FootnoteCitationTracker {
    last_key: Option<String>,
    seen_keys: HashSet<String>,
}

fn resolve_footnote_citation_placeholders(
    blocks: &mut [Block],
    bibliography: &HashMap<String, BibEntry>,
) {
    let mut tracker = FootnoteCitationTracker::default();
    for block in blocks {
        match block {
            Block::Paragraph(inlines)
            | Block::StyledParagraph { inlines, .. }
            | Block::Section { title: inlines, .. }
            | Block::Figure(Figure {
                caption: inlines,
                source: _,
                ..
            }) => {
                resolve_footnote_citations_inlines(inlines, bibliography, &mut tracker, false);
            }
            Block::Table(table) => {
                resolve_footnote_citations_inlines(
                    &mut table.caption,
                    bibliography,
                    &mut tracker,
                    false,
                );
                resolve_footnote_citations_inlines(
                    &mut table.source,
                    bibliography,
                    &mut tracker,
                    false,
                );
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        resolve_footnote_citations_inlines(
                            &mut cell.content,
                            bibliography,
                            &mut tracker,
                            false,
                        );
                    }
                }
            }
            Block::List(list) => {
                for item in &mut list.items {
                    resolve_footnote_citations_inlines(item, bibliography, &mut tracker, false);
                }
            }
            Block::DisplayMath(_)
            | Block::PageBreak
            | Block::PageOrientationSwitch { .. }
            | Block::BibliographyHeading { .. }
            | Block::TableOfContents => {}
        }

        if let Block::Figure(figure) = block {
            resolve_footnote_citations_inlines(
                &mut figure.source,
                bibliography,
                &mut tracker,
                false,
            );
        }
    }
}

fn resolve_footnote_citations_inlines(
    inlines: &mut [Inline],
    bibliography: &HashMap<String, BibEntry>,
    tracker: &mut FootnoteCitationTracker,
    in_footnote: bool,
) {
    for inline in inlines {
        match inline {
            Inline::Text(text) => {
                if in_footnote {
                    *text = replace_footnote_citation_brackets(text, bibliography, tracker);
                }
            }
            Inline::Bold(children) | Inline::Italic(children) => {
                resolve_footnote_citations_inlines(children, bibliography, tracker, in_footnote);
            }
            Inline::Footnote(children) => {
                resolve_footnote_citations_inlines(children, bibliography, tracker, true);
            }
            Inline::InlineMath(_) | Inline::Reference(_) | Inline::LineBreak => {}
        }
    }
}

fn replace_footnote_citation_brackets(
    text: &str,
    bibliography: &HashMap<String, BibEntry>,
    tracker: &mut FootnoteCitationTracker,
) -> String {
    if bibliography.is_empty() || !text.contains('[') {
        return text.to_string();
    }

    let mut out = String::with_capacity(text.len());
    let mut pos = 0usize;
    while let Some(open_rel) = text[pos..].find('[') {
        let open = pos + open_rel;
        out.push_str(&text[pos..open]);
        let Some(close_rel) = text[open + 1..].find(']') else {
            out.push_str(&text[open..]);
            return out;
        };
        let close = open + 1 + close_rel;
        let payload = text[open + 1..close].trim();
        let keys: Vec<&str> = payload.split(',').map(str::trim).collect();
        let mut rendered = Vec::new();
        let mut all_known = !keys.is_empty();
        for key in keys {
            if key.is_empty() {
                continue;
            }
            let Some(entry) = bibliography.get(key) else {
                all_known = false;
                break;
            };
            let rendered_entry = if tracker.last_key.as_deref() == Some(key) {
                "Там же.".to_string()
            } else if tracker.seen_keys.contains(key) {
                format_bibliography_opcit(entry)
            } else {
                format_bibliography_entry(entry)
            };
            tracker.last_key = Some(key.to_string());
            tracker.seen_keys.insert(key.to_string());
            rendered.push(rendered_entry);
        }

        if all_known && !rendered.is_empty() {
            out.push_str(&rendered.join("; "));
        } else {
            out.push_str(&text[open..=close]);
        }
        pos = close + 1;
    }
    out.push_str(&text[pos..]);
    out
}

fn resolve_citation_placeholders(blocks: &mut [Block], bibliography: &HashMap<String, BibEntry>) {
    for block in blocks {
        match block {
            Block::Paragraph(inlines)
            | Block::StyledParagraph { inlines, .. }
            | Block::Section { title: inlines, .. }
            | Block::Figure(Figure {
                caption: inlines,
                source: _,
                ..
            }) => {
                resolve_inline_citations(inlines, bibliography);
            }
            Block::Table(table) => {
                resolve_inline_citations(&mut table.caption, bibliography);
                resolve_inline_citations(&mut table.source, bibliography);
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        resolve_inline_citations(&mut cell.content, bibliography);
                    }
                }
            }
            Block::List(list) => {
                for item in &mut list.items {
                    resolve_inline_citations(item, bibliography);
                }
            }
            Block::DisplayMath(_)
            | Block::PageBreak
            | Block::PageOrientationSwitch { .. }
            | Block::BibliographyHeading { .. }
            | Block::TableOfContents => {}
        }
        if let Block::Figure(figure) = block {
            resolve_inline_citations(&mut figure.source, bibliography);
        }
    }
}

fn resolve_inline_citations(inlines: &mut [Inline], bibliography: &HashMap<String, BibEntry>) {
    for inline in inlines {
        match inline {
            Inline::Text(text) => {
                *text = replace_citation_brackets(text, bibliography);
            }
            Inline::Bold(children) | Inline::Italic(children) | Inline::Footnote(children) => {
                resolve_inline_citations(children, bibliography);
            }
            Inline::InlineMath(_) | Inline::Reference(_) | Inline::LineBreak => {}
        }
    }
}

fn replace_citation_brackets(text: &str, bibliography: &HashMap<String, BibEntry>) -> String {
    if bibliography.is_empty() || !text.contains('[') {
        return text.to_string();
    }

    let mut out = String::with_capacity(text.len());
    let mut pos = 0usize;
    while let Some(open_rel) = text[pos..].find('[') {
        let open = pos + open_rel;
        out.push_str(&text[pos..open]);
        let Some(close_rel) = text[open + 1..].find(']') else {
            out.push_str(&text[open..]);
            return out;
        };
        let close = open + 1 + close_rel;
        let payload = text[open + 1..close].trim();
        let keys: Vec<&str> = payload.split(',').map(str::trim).collect();
        let mut rendered = Vec::new();
        let mut all_known = !keys.is_empty();
        for key in keys {
            if key.is_empty() {
                continue;
            }
            if let Some(entry) = bibliography.get(key) {
                rendered.push(format_bibliography_entry(entry));
            } else {
                all_known = false;
                break;
            }
        }
        if all_known && !rendered.is_empty() {
            out.push_str(&rendered.join("; "));
        } else {
            out.push_str(&text[open..=close]);
        }
        pos = close + 1;
    }
    out.push_str(&text[pos..]);
    out
}

fn format_bibliography_entry(entry: &BibEntry) -> String {
    let author = ["author", "organization", "institution", "publisher"]
        .iter()
        .find_map(|k| entry.fields.get(*k))
        .map(|value| value.trim().trim_matches('"').to_string())
        .unwrap_or_default();
    let title = entry
        .fields
        .get("title")
        .map(|value| value.trim().trim_matches('"').to_string())
        .unwrap_or_default();
    let year = entry
        .fields
        .get("year")
        .cloned()
        .or_else(|| {
            entry
                .fields
                .get("date")
                .and_then(|date| date.chars().take(4).collect::<String>().parse::<i32>().ok())
                .map(|y| y.to_string())
        })
        .unwrap_or_default();
    let url = entry.fields.get("url").cloned().unwrap_or_default();

    let mut parts = Vec::new();
    if !author.is_empty() {
        parts.push(author);
    }
    if !title.is_empty() {
        parts.push(title);
    }
    if !year.is_empty() {
        parts.push(year);
    }
    if !url.is_empty() {
        parts.push(url);
    }
    parts.join(". ")
}

fn format_bibliography_opcit(entry: &BibEntry) -> String {
    let lead = [
        "author",
        "organization",
        "institution",
        "publisher",
        "title",
    ]
    .iter()
    .find_map(|k| entry.fields.get(*k))
    .map(|value| value.trim().trim_matches('"').to_string())
    .unwrap_or_default();
    if lead.is_empty() {
        "Указ. соч.".to_string()
    } else {
        format!("{}. Указ. соч.", lead.trim_end_matches('.'))
    }
}

fn inject_bibliography_entries(blocks: &mut Vec<Block>, bibliography: &HashMap<String, BibEntry>) {
    if bibliography.is_empty() {
        return;
    }

    let mut sorted_keys: Vec<&str> = bibliography.keys().map(String::as_str).collect();
    sorted_keys.sort_unstable();

    let mut out = Vec::with_capacity(blocks.len() + sorted_keys.len());
    let mut inserted = false;
    for block in blocks.drain(..) {
        let is_bib_heading = matches!(block, Block::BibliographyHeading { .. });
        out.push(block);
        if is_bib_heading && !inserted {
            for (index, key) in sorted_keys.iter().enumerate() {
                if let Some(entry) = bibliography.get(*key) {
                    let rendered = format_bibliography_entry(entry);
                    if !rendered.is_empty() {
                        out.push(Block::Paragraph(vec![Inline::Text(format!(
                            "{}. {}",
                            index + 1,
                            rendered
                        ))]));
                    }
                }
            }
            inserted = true;
        }
    }
    *blocks = out;
}

fn replace_counter_markers(text: &str, metadata: &ParseMetadata) -> String {
    let mut out = text.to_string();
    loop {
        let Some(start) = out.find("[[COUNTER:") else {
            break;
        };
        let Some(end_rel) = out[start..].find("]]") else {
            break;
        };
        let end = start + end_rel + 2;
        let marker = &out[start..end];
        let key = marker
            .trim_start_matches("[[COUNTER:")
            .trim_end_matches("]]")
            .trim();
        let replacement = counter_text(metadata, key).unwrap_or_default();
        out.replace_range(start..end, &replacement);
    }
    loop {
        let Some(start) = out.find("[[FORMBYTOTAL:") else {
            break;
        };
        let Some(end_rel) = out[start..].find("]]") else {
            break;
        };
        let end = start + end_rel + 2;
        let marker = &out[start..end];
        let payload = marker
            .trim_start_matches("[[FORMBYTOTAL:")
            .trim_end_matches("]]");
        let parts: Vec<&str> = payload.split('|').collect();
        let replacement = if parts.len() == 5 {
            if let Some(total) = metadata.counters.get(parts[0]) {
                render_formbytotal(*total, parts[1], parts[2], parts[3], parts[4])
            } else {
                let stem = parts[1];
                let suffix = [parts[4], parts[3], parts[2]]
                    .iter()
                    .map(|v| v.trim())
                    .find(|v| !v.is_empty())
                    .unwrap_or("");
                format!("{stem}{suffix}")
            }
        } else {
            String::new()
        };
        out.replace_range(start..end, &replacement);
    }
    out
}

fn apply_known_counter_fallbacks(text: &str, metadata: &ParseMetadata) -> String {
    // DISSERTATION-SPECIFIC: The patterns below match placeholder strings from
    // Russian dissertation style templates (e.g. the `disser` document class).
    // Gated on document class to avoid false positives for unrelated documents.
    // When document_class is None (e.g. test snippets without \documentclass),
    // we skip all replacements — safe because the template strings are not
    // realistic content for non-dissertation documents.
    let is_dissertation_class = metadata.document_class.as_deref().is_some_and(|c| {
        c.eq_ignore_ascii_case("disser") || c.to_ascii_lowercase().contains("dissert")
    });
    if !is_dissertation_class {
        return text.to_string();
    }

    let mut out = text.to_string();

    if out.contains("XX печатных изданиях") {
        if let Some(total) = metadata.counters.get("citeauthor") {
            out = out.replacen("XX", &total.to_string(), 1);
        }
        if let Some(vak) = metadata.counters.get("citeauthorvak") {
            out = out.replacen(" X ", &format!(" {} ", vak), 1);
        }
        if let Some(conf) = metadata.counters.get("citeauthorconf") {
            out = out.replacen(" X ", &format!(" {} ", conf), 1);
        }
    }

    if out.contains("Диссертация состоит из введения, главы, заключения и приложений.")
    {
        let chapters = metadata.counters.get("totalchapter").copied().unwrap_or(0);
        let appendices = metadata.counters.get("totalappendix").copied().unwrap_or(0);
        let chapter_word = russian_cardinal_form(chapters, "глава", "главы", "глав");
        let appendix_word =
            russian_cardinal_form(appendices, "приложение", "приложения", "приложений");
        out = format!(
            "Диссертация состоит из введения, {} {}, заключения и {} {}.",
            chapters, chapter_word, appendices, appendix_word
        );
    }

    if out.contains("Полный объём диссертации составляет страницы, включая рисунков и таблицы.")
        || out.contains("Список литературы содержит наименований.")
    {
        let pages = metadata.counters.get("TotPages").copied().unwrap_or(0);
        let figures = metadata
            .counters
            .get("totalcount@figure")
            .copied()
            .unwrap_or(0);
        let tables = metadata
            .counters
            .get("totalcount@table")
            .copied()
            .unwrap_or(0);
        let refs = metadata.counters.get("citenum").copied().unwrap_or(0);

        let page_word = russian_cardinal_form(pages, "страница", "страницы", "страниц");
        let figure_word = russian_cardinal_form(figures, "рисунок", "рисунка", "рисунков");
        let table_word = russian_cardinal_form(tables, "таблица", "таблицы", "таблиц");
        let ref_word = russian_cardinal_form(refs, "наименование", "наименования", "наименований");

        out = format!(
            "Полный объём диссертации составляет {} {}, включая {} {} и {} {}. Список литературы содержит {} {}.",
            pages, page_word, figures, figure_word, tables, table_word, refs, ref_word
        );
    }

    out
}

fn russian_cardinal_form<'a>(count: i64, one: &'a str, few: &'a str, many: &'a str) -> &'a str {
    let abs = count.abs();
    let last_two = abs % 100;
    let last = abs % 10;
    if (11..=19).contains(&last_two) {
        many
    } else if last == 1 {
        one
    } else if (2..=4).contains(&last) {
        few
    } else {
        many
    }
}

fn expand_inputs_recursive(
    path: &Path,
    root_dir: &Path,
    stack: &mut Vec<PathBuf>,
) -> anyhow::Result<String> {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if stack.contains(&canonical) {
        return Ok(String::new());
    }

    stack.push(canonical);
    let source = std::fs::read_to_string(path)?;
    let source = strip_comments(&source);
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut out = String::with_capacity(source.len());
    let mut pos = 0usize;

    while pos < source.len() {
        // Expand \input{...} and \include{...} recursively as best effort.
        let include_cmd_len = if source[pos..].starts_with("\\input") {
            Some("\\input".len())
        } else if source[pos..].starts_with("\\include") {
            Some("\\include".len())
        } else {
            None
        };

        if let Some(cmd_len) = include_cmd_len {
            let cmd_end = pos + cmd_len;
            let next = source[cmd_end..].chars().next();
            if !next.is_some_and(|c| c.is_ascii_alphabetic()) {
                let mut arg_pos = cmd_end;
                while arg_pos < source.len() && source.as_bytes()[arg_pos].is_ascii_whitespace() {
                    arg_pos += 1;
                }

                if arg_pos < source.len()
                    && source.as_bytes()[arg_pos] == b'{'
                    && let Some(arg_len) = braced_len(&source[arg_pos..])
                {
                    let arg_src = &source[arg_pos + 1..arg_pos + arg_len - 1];
                    let included = arg_src.trim();
                    if !included.is_empty() {
                        if let Some(include_path) = resolve_input_path(base_dir, root_dir, included)
                        {
                            let expanded = expand_inputs_recursive(&include_path, root_dir, stack)?;
                            if !out.is_empty() && !out.ends_with('\n') {
                                out.push('\n');
                            }
                            out.push_str(&expanded);
                            if !expanded.ends_with('\n') {
                                out.push('\n');
                            }
                        } else {
                            // Missing include: keep original command for best-effort parsing.
                            out.push_str(&source[pos..arg_pos + arg_len]);
                        }
                        pos = arg_pos + arg_len;
                        continue;
                    }
                }
            }
        }

        if let Some(ch) = source[pos..].chars().next() {
            out.push(ch);
            pos += ch.len_utf8();
        } else {
            break;
        }
    }

    stack.pop();
    Ok(out)
}

fn resolve_input_path(base_dir: &Path, root_dir: &Path, include_arg: &str) -> Option<PathBuf> {
    if include_arg.trim().is_empty() {
        return None;
    }

    let raw = Path::new(include_arg);
    let mut candidates = Vec::new();

    if raw.is_absolute() {
        candidates.push(raw.to_path_buf());
    } else {
        candidates.push(base_dir.join(raw));
        if root_dir != base_dir {
            candidates.push(root_dir.join(raw));
        }
    }

    for candidate in &candidates {
        if candidate.is_file() {
            return Some(candidate.clone());
        }
    }

    for candidate in candidates {
        if candidate.extension().is_none() {
            let with_tex = candidate.with_extension("tex");
            if with_tex.is_file() {
                return Some(with_tex);
            }
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Segmenter — splits source into float environments vs plain text
// ---------------------------------------------------------------------------

enum Segment {
    /// A block-level environment or display-math block kept intact as one span.
    Float(String),
    /// Everything else (paragraphs, sections, etc.).
    Text(String),
}

enum SegmentStart {
    Env(String),
    DisplayMathBrackets,
}

/// Block-level environments extracted before paragraph splitting.
const BLOCK_ENVS: &[&str] = &[
    "table",
    "figure",
    "table*",
    "figure*",
    "tblr",
    "longtblr",
    "refsection",
    "itemize",
    "enumerate",
    "equation",
    "equation*",
];

fn segment(src: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut pos = 0;
    let len = src.len();

    while pos < len {
        let next_env = find_begin_float(src, pos)
            .map(|(env_name, begin_pos)| (begin_pos, SegmentStart::Env(env_name)));
        let next_bracket_math = find_begin_display_math(src, pos)
            .map(|begin_pos| (begin_pos, SegmentStart::DisplayMathBrackets));

        let next = match (next_env, next_bracket_math) {
            (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };

        match next {
            Some((begin_pos, SegmentStart::Env(env_name))) => {
                // Flush text before this block.
                if begin_pos > pos {
                    segments.push(Segment::Text(src[pos..begin_pos].to_string()));
                }

                // Find the matching \end{<env_name>}.
                let end_tag = format!("\\end{{{}}}", env_name);
                if let Some(end_offset) = src[begin_pos..].find(&end_tag) {
                    let end_pos = begin_pos + end_offset + end_tag.len();
                    // Also consume an optional \tablesource / \figuresource after the float.
                    let after = src[end_pos..].trim_start_matches([' ', '\t', '\n', '\r']);
                    let source_suffix = consume_source_macro(after);
                    let total_end = if source_suffix.is_empty() {
                        end_pos
                    } else {
                        end_pos + (src[end_pos..].len() - after.len()) + source_suffix.len()
                    };
                    let float_text = format!("{}{}", &src[begin_pos..end_pos], source_suffix);
                    segments.push(Segment::Float(float_text));
                    pos = total_end;
                } else {
                    // Unmatched \begin{env} — skip past the tag and continue.
                    let skip_end = begin_pos + 8 + env_name.len(); // \begin{} = 8 + env
                    if begin_pos > pos {
                        segments.push(Segment::Text(src[pos..skip_end].to_string()));
                    } else {
                        segments.push(Segment::Text(src[begin_pos..skip_end].to_string()));
                    }
                    pos = skip_end;
                }
            }
            Some((begin_pos, SegmentStart::DisplayMathBrackets)) => {
                // Flush text before this block.
                if begin_pos > pos {
                    segments.push(Segment::Text(src[pos..begin_pos].to_string()));
                }

                // Find matching \].
                if let Some(end_pos) = find_end_display_math(src, begin_pos + 2) {
                    let block_end = end_pos + 2; // include closing \]
                    segments.push(Segment::Float(src[begin_pos..block_end].to_string()));
                    pos = block_end;
                } else {
                    // Unmatched \[ — not display math (e.g. macro definition).
                    // Include the \[ as plain text and continue segmenting.
                    if begin_pos > pos {
                        segments.push(Segment::Text(src[pos..begin_pos + 2].to_string()));
                    } else {
                        segments.push(Segment::Text(src[begin_pos..begin_pos + 2].to_string()));
                    }
                    pos = begin_pos + 2;
                }
            }
            None => {
                // No more block starts — rest is text.
                segments.push(Segment::Text(src[pos..].to_string()));
                pos = len;
            }
        }
    }

    segments
}

/// Find the next `\begin{<float_env>}` at or after `from`.
/// Returns `(env_name, byte_offset_of_begin)`.
fn find_begin_float(src: &str, from: usize) -> Option<(String, usize)> {
    let mut search_pos = from;
    while search_pos < src.len() {
        if let Some(rel) = src[search_pos..].find("\\begin{") {
            let abs = search_pos + rel;
            let after_begin = abs + 7; // len("\\begin{") == 7
            if let Some(close) = src[after_begin..].find('}') {
                let env = &src[after_begin..after_begin + close];
                if BLOCK_ENVS.contains(&env) {
                    return Some((env.to_string(), abs));
                }
                // Not a float env — skip past this \begin and keep searching.
                search_pos = after_begin + close + 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    None
}

/// Find the next `\[` (display-math start) at or after `from`.
/// Ignores escaped variants like `\\[`.
fn find_begin_display_math(src: &str, from: usize) -> Option<usize> {
    let mut search_pos = from;
    while search_pos < src.len() {
        if let Some(rel) = src[search_pos..].find("\\[") {
            let abs = search_pos + rel;
            if abs > 0 && src.as_bytes()[abs - 1] == b'\\' {
                search_pos = abs + 2;
                continue;
            }
            return Some(abs);
        }
        break;
    }
    None
}

/// Find the next `\]` (display-math end) at or after `from`.
/// Ignores escaped variants like `\\]`.
fn find_end_display_math(src: &str, from: usize) -> Option<usize> {
    let mut search_pos = from;
    while search_pos < src.len() {
        if let Some(rel) = src[search_pos..].find("\\]") {
            let abs = search_pos + rel;
            if abs > 0 && src.as_bytes()[abs - 1] == b'\\' {
                search_pos = abs + 2;
                continue;
            }
            return Some(abs);
        }
        break;
    }
    None
}

/// If `src` starts with `\tablesource{…}` or `\figuresource{…}`, return that
/// entire call (including braces). Otherwise return empty string.
fn consume_source_macro(src: &str) -> String {
    let mut consumed = 0usize;
    loop {
        let tail = &src[consumed..];
        let ws_len = tail.len() - tail.trim_start_matches([' ', '\t', '\n', '\r']).len();
        let start = consumed + ws_len;
        let tail = &src[start..];

        let mut matched_len = None;
        for prefix in &["\\tablesource", "\\figuresource"] {
            if let Some(rest) = tail.strip_prefix(prefix) {
                let rest_trim = rest.trim_start_matches([' ', '\t']);
                let skip = rest.len() - rest_trim.len();
                if let Some(len) = braced_len(rest_trim) {
                    matched_len = Some(prefix.len() + skip + len);
                    break;
                }
            }
        }

        if let Some(len) = matched_len {
            consumed = start + len;
        } else {
            break;
        }
    }

    if consumed > 0 {
        src[..consumed].to_string()
    } else {
        String::new()
    }
}

// ---------------------------------------------------------------------------
// Float parser
// ---------------------------------------------------------------------------

/// Parse a block environment segment into a [`Block`].
fn parse_float(
    src: &str,
    autocite_mode: AutociteMode,
    metadata: &ParseMetadata,
    preserve_dynamic_markers: bool,
) -> Option<Block> {
    let src = src.trim();
    if src.starts_with("\\[") || src.starts_with("\\begin{equation") {
        Some(Block::DisplayMath(extract_display_math_body(src)))
    } else if src.starts_with("\\begin{refsection") {
        None
    } else if src.starts_with("\\begin{figure") {
        Some(Block::Figure(parse_figure(
            src,
            autocite_mode,
            metadata,
            preserve_dynamic_markers,
        )))
    } else if src.starts_with("\\begin{itemize") {
        Some(Block::List(parse_list(
            src,
            false,
            autocite_mode,
            metadata,
            preserve_dynamic_markers,
        )))
    } else if src.starts_with("\\begin{enumerate") {
        Some(Block::List(parse_list(
            src,
            true,
            autocite_mode,
            metadata,
            preserve_dynamic_markers,
        )))
    } else if src.starts_with("\\begin{table")
        || src.starts_with("\\begin{tblr")
        || src.starts_with("\\begin{longtblr")
    {
        Some(Block::Table(parse_table(
            src,
            autocite_mode,
            metadata,
            preserve_dynamic_markers,
        )))
    } else {
        None
    }
}

/// Extract the raw body from display-math forms:
/// - `\begin{equation}…\end{equation}` / `equation*`
/// - `\[…\]`
fn extract_display_math_body(src: &str) -> String {
    if let Some(body) = src.strip_prefix("\\[") {
        let body = if let Some(end) = body.find("\\]") {
            &body[..end]
        } else {
            body
        };
        return body.trim().to_string();
    }

    // Find the opening `}` of `\begin{equation...}`.
    let body_start = src.find('}').map(|i| i + 1).unwrap_or(src.len());
    let body = &src[body_start..];
    // Trim up to `\end{equation`.
    let body = if let Some(end) = body.find("\\end{equation") {
        &body[..end]
    } else {
        body
    };
    body.trim().to_string()
}

/// Parse a `\begin{table}…\end{table}` segment into a [`Table`].
fn parse_table(
    src: &str,
    autocite_mode: AutociteMode,
    metadata: &ParseMetadata,
    preserve_dynamic_markers: bool,
) -> Table {
    let mut caption = extract_caption(src, autocite_mode, metadata, preserve_dynamic_markers);
    if caption.is_empty()
        && let Some(value) = extract_option_value(src, "caption")
    {
        caption = parse_inlines(
            value.as_str(),
            autocite_mode,
            metadata,
            preserve_dynamic_markers,
        );
    }
    let label = extract_label_macro(src).or_else(|| extract_option_label(src));
    let source = extract_source_macro(src, autocite_mode, metadata, preserve_dynamic_markers);
    let alignment = extract_float_alignment(src);

    // Find the inner tabular/tblr/longtblr environment.
    let rows = extract_table_rows(src, autocite_mode, metadata, preserve_dynamic_markers);

    Table {
        caption,
        label,
        source,
        alignment,
        rows,
    }
}

/// Parse a `\begin{figure}…\end{figure}` segment into a [`Figure`].
fn parse_figure(
    src: &str,
    autocite_mode: AutociteMode,
    metadata: &ParseMetadata,
    preserve_dynamic_markers: bool,
) -> Figure {
    let caption = extract_caption(src, autocite_mode, metadata, preserve_dynamic_markers);
    let label = extract_label_macro(src).or_else(|| extract_option_label(src));
    let source = extract_source_macro(src, autocite_mode, metadata, preserve_dynamic_markers);
    let alignment = extract_float_alignment(src);
    let image_path = extract_includegraphics_path(src);
    let width_permille = extract_includegraphics_width_permille(src);
    Figure {
        image_path,
        width_permille,
        caption,
        label,
        source,
        alignment,
    }
}

fn extract_float_alignment(src: &str) -> Option<String> {
    let mut last: Option<(usize, &str)> = None;
    for (needle, value) in [
        ("\\centering", "center"),
        ("\\begin{center}", "center"),
        ("\\raggedright", "left"),
        ("\\flushleft", "left"),
        ("\\raggedleft", "right"),
        ("\\flushright", "right"),
    ] {
        if let Some(pos) = src.rfind(needle)
            && last.is_none_or(|(prev, _)| pos > prev)
        {
            last = Some((pos, value));
        }
    }
    last.map(|(_, value)| value.to_string())
}

/// Parse a `\begin{itemize}…\end{itemize}` or `\begin{enumerate}…\end{enumerate}` segment.
fn parse_list(
    src: &str,
    ordered: bool,
    autocite_mode: AutociteMode,
    metadata: &ParseMetadata,
    preserve_dynamic_markers: bool,
) -> List {
    let env = if ordered { "enumerate" } else { "itemize" };
    // Strip the outer \begin{env}…\end{env} wrapper.
    let begin_tag = format!("\\begin{{{}}}", env);
    let end_tag = format!("\\end{{{}}}", env);
    let inner = src
        .find(&begin_tag)
        .map(|p| &src[p + begin_tag.len()..])
        .unwrap_or(src);
    let inner = inner.find(&end_tag).map(|p| &inner[..p]).unwrap_or(inner);

    // Split on \item, discard everything before the first \item.
    let items: Vec<Vec<Inline>> = inner
        .split("\\item")
        .skip(1) // first chunk is before the first \item
        .map(|chunk| {
            // Each chunk may start with an optional [label] for \item[custom].
            let chunk = chunk.trim_start();
            let chunk = if chunk.starts_with('[') {
                chunk
                    .find(']')
                    .map_or(chunk, |i| chunk[i + 1..].trim_start())
            } else {
                chunk
            };
            // Collapse internal newlines to spaces (same as paragraph text).
            let text: String = chunk
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            parse_inlines(&text, autocite_mode, metadata, preserve_dynamic_markers)
        })
        .filter(|inlines| !inlines.is_empty())
        .collect();

    List { ordered, items }
}

/// Extract `\caption{…}` content from a float.
fn extract_caption(
    src: &str,
    autocite_mode: AutociteMode,
    metadata: &ParseMetadata,
    preserve_dynamic_markers: bool,
) -> Vec<Inline> {
    if let Some(pos) = src.find("\\caption") {
        let after = src[pos + 8..].trim_start_matches([' ', '\t']);
        // Skip optional `[short]` argument.
        let after = if after.starts_with('[') {
            if let Some(close) = after.find(']') {
                after[close + 1..].trim_start_matches([' ', '\t'])
            } else {
                after
            }
        } else {
            after
        };
        if let Some(content) = extract_braced(after) {
            return parse_inlines(content, autocite_mode, metadata, preserve_dynamic_markers);
        }
    }
    Vec::new()
}

/// Extract `\tablesource{…}` or `\figuresource{…}` content.
fn extract_source_macro(
    src: &str,
    autocite_mode: AutociteMode,
    metadata: &ParseMetadata,
    preserve_dynamic_markers: bool,
) -> Vec<Inline> {
    let mut out = Vec::new();
    let mut pos = 0usize;

    loop {
        let next_table = src[pos..].find("\\tablesource").map(|rel| pos + rel);
        let next_figure = src[pos..].find("\\figuresource").map(|rel| pos + rel);
        let next = match (next_table, next_figure) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };

        let Some(macro_pos) = next else {
            break;
        };
        let macro_name = if src[macro_pos..].starts_with("\\tablesource") {
            "\\tablesource"
        } else {
            "\\figuresource"
        };

        let after = src[macro_pos + macro_name.len()..].trim_start_matches([' ', '\t']);
        if let Some(content) = extract_braced(after) {
            let parsed = parse_inlines(content, autocite_mode, metadata, preserve_dynamic_markers);
            if !parsed.is_empty() {
                if !out.is_empty() {
                    out.push(Inline::Text(" ".to_string()));
                }
                out.extend(parsed);
            }
        }

        pos = macro_pos + macro_name.len();
    }

    out
}

/// Extract the first `\label{...}` payload from `src`.
fn extract_label_macro(src: &str) -> Option<String> {
    let pos = src.find("\\label")?;
    let after = src[pos + "\\label".len()..].trim_start_matches([' ', '\t']);
    let content = extract_braced(after)?;
    let label = content.trim();
    if label.is_empty() {
        None
    } else {
        Some(label.to_string())
    }
}

/// Extract a standalone `\label{...}` command from a chunk.
///
/// Returns `Some(label)` only when the chunk contains exactly one label command
/// and no additional visible content.
fn extract_standalone_label(chunk: &str) -> Option<String> {
    let trimmed = chunk.trim();
    if !trimmed.starts_with("\\label") {
        return None;
    }
    let after = trimmed["\\label".len()..].trim_start_matches([' ', '\t']);
    let content = extract_braced(after)?;
    let consumed = {
        let len = braced_len(after)?;
        "\\label".len() + (trimmed["\\label".len()..].len() - after.len()) + len
    };
    if !trimmed[consumed..].trim().is_empty() {
        return None;
    }
    let label = content.trim();
    if label.is_empty() {
        None
    } else {
        Some(label.to_string())
    }
}

/// Attach a standalone `\label{...}` to the most recent relevant block.
fn attach_standalone_label(blocks: &mut [Block], label: String) {
    if let Some(last) = blocks.last_mut() {
        match last {
            Block::Section {
                label: target_label,
                ..
            } => {
                if target_label.is_none() {
                    *target_label = Some(label);
                }
            }
            Block::Table(table) => {
                if table.label.is_none() {
                    table.label = Some(label);
                }
            }
            Block::Figure(figure) => {
                if figure.label.is_none() {
                    figure.label = Some(label);
                }
            }
            Block::DisplayMath(body) => {
                if !body.contains("\\label{") {
                    if !body.trim().is_empty() {
                        body.push(' ');
                    }
                    body.push_str(&format!("\\label{{{label}}}"));
                }
            }
            Block::Paragraph(_)
            | Block::StyledParagraph { .. }
            | Block::List(_)
            | Block::PageBreak
            | Block::PageOrientationSwitch { .. }
            | Block::BibliographyHeading { .. }
            | Block::TableOfContents => {}
        }
    }
}

/// Extract tabularray-style label option, e.g. `label = {tab:foo}`.
fn extract_option_label(src: &str) -> Option<String> {
    extract_option_value(src, "label")
}

fn extract_option_value(src: &str, key: &str) -> Option<String> {
    let mut search_pos = 0usize;
    while search_pos < src.len() {
        let Some(rel) = src[search_pos..].find(key) else {
            break;
        };
        let pos = search_pos + rel;
        let after_kw = pos + key.len();
        let mut tail = &src[after_kw..];
        tail = tail.trim_start_matches([' ', '\t']);
        if !tail.starts_with('=') {
            search_pos = after_kw;
            continue;
        }
        tail = tail[1..].trim_start_matches([' ', '\t']);

        if tail.starts_with('{') {
            if let Some(len) = braced_len(tail) {
                let candidate = tail[1..len - 1].trim();
                if !candidate.is_empty() && !candidate.eq_ignore_ascii_case("none") {
                    return Some(candidate.to_string());
                }
            }
        } else {
            let end = tail.find([',', ']', '\n', '\r']).unwrap_or(tail.len());
            let candidate = tail[..end].trim();
            if !candidate.is_empty() && !candidate.eq_ignore_ascii_case("none") {
                return Some(candidate.to_string());
            }
        }
        search_pos = after_kw;
    }
    None
}

/// Extract the path from `\includegraphics[…]{path}`.
fn extract_includegraphics_path(src: &str) -> Option<String> {
    let pos = src.find("\\includegraphics")?;
    let after = src[pos + 16..].trim_start_matches([' ', '\t']);
    // Skip optional `[width=…]` argument.
    let after = if after.starts_with('[') {
        let close = after.find(']')?;
        after[close + 1..].trim_start_matches([' ', '\t'])
    } else {
        after
    };
    let content = extract_braced(after)?;
    Some(content.trim().to_string())
}

fn extract_includegraphics_width_permille(src: &str) -> Option<u16> {
    let pos = src.find("\\includegraphics")?;
    let after = src[pos + 16..].trim_start_matches([' ', '\t']);
    if !after.starts_with('[') {
        return None;
    }

    let close = after.find(']')?;
    let options = &after[1..close];
    parse_width_permille_from_options(options)
}

fn parse_width_permille_from_options(options: &str) -> Option<u16> {
    for option in options.split(',') {
        let (key, value) = option.split_once('=')?;
        if key.trim() != "width" {
            continue;
        }

        let value = value.trim().trim_matches(['{', '}']);
        for unit in ["\\textwidth", "\\linewidth"] {
            if let Some(unit_pos) = value.find(unit) {
                let factor_src = value[..unit_pos].trim();
                let factor = if factor_src.is_empty() {
                    1.0
                } else {
                    factor_src.parse::<f64>().ok()?
                };
                if !factor.is_finite() || factor <= 0.0 {
                    return None;
                }

                let permille_f64 = (factor * 1000.0).round();
                let clamped = permille_f64.clamp(1.0, u16::MAX as f64);
                return Some(clamped as u16);
            }
        }
    }
    None
}

/// Parse rows from within a `tabular`, `tblr`, or `longtblr` environment.
///
/// Strategy:
/// 1. Locate the innermost table environment body (after the column spec).
/// 2. Split on `\\` (row separator).
/// 3. Split each row on `&` (cell separator).
/// 4. Parse each cell as inlines.
fn extract_table_rows(
    src: &str,
    autocite_mode: AutociteMode,
    metadata: &ParseMetadata,
    preserve_dynamic_markers: bool,
) -> Vec<TableRow> {
    // Find the body of the innermost tabular/tblr/longtblr.
    let body = find_tabular_body(src);
    if body.is_empty() {
        return Vec::new();
    }

    let mut rows = Vec::new();
    // Split on `\\` but not on `\` followed by other chars.
    for raw_row in split_table_rows(body) {
        // A single "row chunk" between `\\` terminators can contain horizontal
        // rule lines (`\hline`, `\midrule`, …) followed by actual cell data.
        // Strip those rule lines and keep only the cell content lines.
        let cell_line: String = raw_row
            .lines()
            .filter(|l| {
                let t = l.trim();
                !t.is_empty()
                    && !t.starts_with("\\hline")
                    && !t.starts_with("\\toprule")
                    && !t.starts_with("\\midrule")
                    && !t.starts_with("\\bottomrule")
                    && !t.starts_with("\\cline")
            })
            .collect::<Vec<_>>()
            .join(" ");
        let cell_line = cell_line.trim();
        if cell_line.is_empty() {
            continue;
        }
        let cells: Vec<TableCell> = cell_line
            .split('&')
            .map(|cell| TableCell {
                content: parse_inlines(
                    cell.trim(),
                    autocite_mode,
                    metadata,
                    preserve_dynamic_markers,
                ),
            })
            .collect();
        if cells.iter().any(|c| !c.content.is_empty()) {
            rows.push(TableRow { cells });
        }
    }
    rows
}

/// Locate the content between `{col_spec}` (or `{…}`) and the matching
/// `\end{tabular}` / `\end{tblr}` / `\end{longtblr}`.
fn find_tabular_body(src: &str) -> &str {
    // Find innermost tabular-family begin.
    let inner_envs = ["tabular", "tblr", "longtblr"];
    let mut best: Option<(usize, &str)> = None;
    for env in inner_envs {
        let tag = format!("\\begin{{{}}}", env);
        if let Some(pos) = src.rfind(&tag)
            && best.is_none_or(|(p, _)| pos > p)
        {
            best = Some((pos, env));
        }
    }
    let (begin_pos, env_name) = match best {
        Some(v) => v,
        None => return "",
    };

    let tag_len = 8 + env_name.len(); // \begin{} = 8 chars + env name
    let after_begin = &src[begin_pos + tag_len..];

    // Skip the column spec argument `{…}` or `[…]{…}`.
    let body_start = skip_tabular_preamble(after_begin);

    let end_tag = format!("\\end{{{}}}", env_name);
    if let Some(end_rel) = body_start.find(&end_tag) {
        &body_start[..end_rel]
    } else {
        body_start
    }
}

/// Skip optional `[…]` and mandatory `{col_spec}` after `\begin{tabular}`.
/// Returns a slice pointing at the actual cell content.
fn skip_tabular_preamble(src: &str) -> &str {
    let src = src.trim_start();
    // tblr uses `{ key=val, ... }` preamble — skip it.
    if src.starts_with('{') {
        if let Some(len) = braced_len(src) {
            src[len..].trim_start()
        } else {
            src
        }
    } else if src.starts_with('[') {
        // longtblr `[caption=…]`
        if let Some(close) = src.find(']') {
            let after = src[close + 1..].trim_start();
            // Then the `{col_spec}` block.
            if let Some(len) = braced_len(after) {
                after[len..].trim_start()
            } else {
                after
            }
        } else {
            src
        }
    } else {
        src
    }
}

/// Split table body on `\\` row terminators, respecting brace nesting.
fn split_table_rows(src: &str) -> Vec<&str> {
    let mut rows = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    let bytes = src.as_bytes();
    let len = src.len();
    let mut i = 0;

    while i < len {
        match bytes[i] {
            b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            b'\\' if depth == 0 && i + 1 < len && bytes[i + 1] == b'\\' => {
                rows.push(&src[start..i]);
                i += 2;
                start = i;
            }
            _ => {
                i += 1;
            }
        }
    }
    let last = src[start..].trim();
    if !last.is_empty() {
        rows.push(last);
    }
    rows
}

/// Expand user-defined zero-argument macros declared via
/// `\newcommand`, `\renewcommand`, or `\providecommand`.
///
/// This keeps structural parser logic unchanged while restoring missing
/// text fragments that are emitted through project-level aliases such as
/// `\actuality` → `\textbf{\actualityTXT}`.
fn expand_simple_macros(src: &str) -> String {
    let macros = collect_zero_arg_macros(src);
    if macros.is_empty() {
        return src.to_string();
    }

    let mut expanded = src.to_string();
    // Resolve small macro chains in a bounded number of passes.
    for _ in 0..8 {
        let (next, changed) = replace_macro_occurrences(&expanded, &macros);
        expanded = next;
        if !changed {
            break;
        }
    }
    expanded
}

fn collect_zero_arg_macros(src: &str) -> HashMap<String, String> {
    let mut macros = HashMap::new();
    let mut pos = 0usize;

    while pos < src.len() {
        let tail = &src[pos..];
        let command = if is_macro_definition_start(tail, "\\newcommand") {
            Some("\\newcommand")
        } else if is_macro_definition_start(tail, "\\renewcommand") {
            Some("\\renewcommand")
        } else if is_macro_definition_start(tail, "\\providecommand") {
            Some("\\providecommand")
        } else {
            None
        };

        if let Some(command) = command
            && let Some((entry, consumed)) = parse_newcommand_definition(src, pos, command)
        {
            if let Some((name, body)) = entry {
                macros.insert(name, body);
            }
            pos += consumed;
            continue;
        }

        let Some(ch) = tail.chars().next() else {
            break;
        };
        pos += ch.len_utf8();
    }

    macros
}

fn is_macro_definition_start(src: &str, command: &str) -> bool {
    if !src.starts_with(command) {
        return false;
    }
    src[command.len()..]
        .chars()
        .next()
        .is_none_or(|ch| !ch.is_ascii_alphabetic())
}

fn parse_newcommand_definition(
    src: &str,
    command_pos: usize,
    command: &str,
) -> Option<(Option<(String, String)>, usize)> {
    let mut cur = command_pos + command.len();
    if cur < src.len() && src.as_bytes()[cur] == b'*' {
        cur += 1;
    }
    while cur < src.len() && src.as_bytes()[cur].is_ascii_whitespace() {
        cur += 1;
    }

    let (name, name_len) = parse_macro_name_argument(&src[cur..])?;
    cur += name_len;
    while cur < src.len() && src.as_bytes()[cur].is_ascii_whitespace() {
        cur += 1;
    }

    let mut has_arguments = false;
    if cur < src.len() && src.as_bytes()[cur] == b'[' {
        let arg_spec_len = bracketed_len(&src[cur..])?;
        let arg_spec = src[cur + 1..cur + arg_spec_len - 1].trim();
        if !arg_spec.is_empty() && arg_spec != "0" {
            has_arguments = true;
        }
        cur += arg_spec_len;
        while cur < src.len() && src.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }

        // `\newcommand{\foo}[1][default]{...}` always has arguments.
        if cur < src.len() && src.as_bytes()[cur] == b'[' {
            let default_len = bracketed_len(&src[cur..])?;
            cur += default_len;
            has_arguments = true;
            while cur < src.len() && src.as_bytes()[cur].is_ascii_whitespace() {
                cur += 1;
            }
        }
    }

    if cur >= src.len() || src.as_bytes()[cur] != b'{' {
        return None;
    }
    let body_len = braced_len(&src[cur..])?;
    let body = src[cur + 1..cur + body_len - 1].trim().to_string();
    let consumed = cur + body_len - command_pos;

    if has_arguments || !is_expandable_macro_name(&name) {
        return Some((None, consumed));
    }
    Some((Some((name, body)), consumed))
}

fn parse_macro_name_argument(src: &str) -> Option<(String, usize)> {
    if src.starts_with('{') {
        let name_len = braced_len(src)?;
        let payload = src[1..name_len - 1].trim();
        let (name, consumed) = parse_control_word(payload)?;
        if consumed != payload.len() {
            return None;
        }
        return Some((name.to_string(), name_len));
    }

    let (name, consumed) = parse_control_word(src)?;
    Some((name.to_string(), consumed))
}

fn parse_control_word(src: &str) -> Option<(&str, usize)> {
    if !src.starts_with('\\') {
        return None;
    }

    let mut end = 1usize;
    while end < src.len() {
        let byte = src.as_bytes()[end];
        if byte.is_ascii_alphabetic() || byte == b'@' {
            end += 1;
        } else {
            break;
        }
    }
    if end == 1 {
        return None;
    }
    Some((&src[1..end], end))
}

fn is_expandable_macro_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower.contains("toc") || lower.starts_with("cft") || name.contains('@') {
        return false;
    }

    !matches!(
        name,
        "begin"
            | "end"
            | "input"
            | "include"
            | "chapter"
            | "section"
            | "subsection"
            | "subsubsection"
            | "caption"
            | "label"
            | "ref"
            | "autoref"
            | "cref"
            | "Cref"
            | "eqref"
            | "cite"
            | "autocite"
            | "textbf"
            | "textit"
            | "emph"
            | "footnote"
            | "tableofcontents"
            | "listoftables"
            | "listoffigures"
            | "appendix"
            | "newcommand"
            | "renewcommand"
            | "providecommand"
            | "setcounter"
            | "counterwithin"
            | "counterwithout"
            | "setlength"
            | "newpage"
            | "clearpage"
            | "cleardoublepage"
            | "maketitle"
            | "includegraphics"
            | "to"
    )
}

fn replace_macro_occurrences(src: &str, macros: &HashMap<String, String>) -> (String, bool) {
    let mut out = String::with_capacity(src.len());
    let mut pos = 0usize;
    let mut changed = false;

    while pos < src.len() {
        let tail = &src[pos..];
        if tail.starts_with('\\')
            && let Some((name, consumed)) = parse_control_word(tail)
            && let Some(replacement) = macros.get(name)
        {
            out.push_str(replacement);
            pos += consumed;
            changed = true;
            continue;
        }

        let Some(ch) = tail.chars().next() else {
            break;
        };
        out.push(ch);
        pos += ch.len_utf8();
    }

    (out, changed)
}

// ---------------------------------------------------------------------------
// Shared line-level helpers
// ---------------------------------------------------------------------------

/// Extract the document body between `\begin{document}` and `\end{document}`.
///
/// If `\begin{document}` is present, everything before it (preamble) is
/// discarded.  If `\end{document}` is present, everything after it is
/// discarded as well.  When neither marker exists the full source is returned
/// unchanged — this keeps the parser usable on bare LaTeX fragments and test
/// fixtures.
fn extract_document_body(src: &str) -> &str {
    let start = src
        .find("\\begin{document}")
        .map(|i| {
            let after = i + "\\begin{document}".len();
            // Skip optional trailing whitespace / newline.
            src[after..]
                .find(|c: char| !c.is_whitespace())
                .map_or(after, |ws| after + ws)
        })
        .unwrap_or(0);

    let end = src[start..]
        .find("\\end{document}")
        .map(|i| start + i)
        .unwrap_or(src.len());

    &src[start..end]
}

/// Remove lines that are purely preamble directives or environment delimiters.
fn filter_skippable_lines(src: &str) -> String {
    src.lines()
        .map(|line| if is_skippable(line.trim()) { "" } else { line })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Remove `%`-style LaTeX comments (to end of line).
fn strip_comments(src: &str) -> String {
    src.lines()
        .map(|line| {
            let mut result = String::with_capacity(line.len());
            let mut chars = line.chars().peekable();
            while let Some(ch) = chars.next() {
                if ch == '\\' {
                    result.push(ch);
                    if let Some(&next) = chars.peek() {
                        result.push(next);
                        chars.next();
                    }
                } else if ch == '%' {
                    break;
                } else {
                    result.push(ch);
                }
            }
            result
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Split source into paragraph-sized chunks on blank lines.
fn split_paragraphs(src: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();

    for line in src.lines() {
        if line.trim().is_empty() {
            let trimmed = current.trim().to_string();
            if !trimmed.is_empty() {
                chunks.push(trimmed);
            }
            current.clear();
        } else {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(line.trim());
        }
    }
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        chunks.push(trimmed);
    }
    chunks
}

#[derive(Debug, Clone, Default)]
struct TextFlowState {
    in_titlingpage: bool,
    alignment_stack: Vec<Option<String>>,
    current_alignment: Option<String>,
    current_line_spacing_twips: Option<i32>,
    pending_space_before_twips: Option<i32>,
}

#[derive(Debug, Clone)]
struct PreparedTextChunk {
    text: String,
    style: Option<ParagraphStyle>,
}

fn prepare_text_chunk(
    raw_chunk: &str,
    state: &mut TextFlowState,
    layout: &DocumentLayout,
) -> Option<PreparedTextChunk> {
    let chunk = raw_chunk.trim();
    if chunk.is_empty() {
        return None;
    }

    let begins_titlingpage =
        chunk.contains("\\begin{titlingpage}") || chunk.contains("\\begin{titlepage}");
    let ends_titlingpage =
        chunk.contains("\\end{titlingpage}") || chunk.contains("\\end{titlepage}");
    if begins_titlingpage {
        state.in_titlingpage = true;
        state.current_alignment = None;
        state.current_line_spacing_twips = None;
        state.alignment_stack.clear();
    }

    let begins_flushright = chunk.contains("\\begin{flushright}");
    let begins_flushleft = chunk.contains("\\begin{flushleft}");
    let ends_flush = chunk.contains("\\end{flushright}") || chunk.contains("\\end{flushleft}");
    let chunk_alignment_override = if begins_flushright {
        Some("right".to_string())
    } else if begins_flushleft {
        Some("left".to_string())
    } else {
        None
    };

    if begins_flushright {
        state.alignment_stack.push(state.current_alignment.clone());
        state.current_alignment = Some("right".to_string());
    } else if begins_flushleft {
        state.alignment_stack.push(state.current_alignment.clone());
        state.current_alignment = Some("left".to_string());
    }

    if chunk.contains("\\centering") {
        state.current_alignment = Some("center".to_string());
    } else if chunk.contains("\\raggedleft")
        || (chunk.contains("\\flushright")
            && !chunk.contains("\\begin{flushright}")
            && !chunk.contains("\\end{flushright}"))
    {
        state.current_alignment = Some("right".to_string());
    } else if chunk.contains("\\raggedright")
        || (chunk.contains("\\flushleft")
            && !chunk.contains("\\begin{flushleft}")
            && !chunk.contains("\\end{flushleft}"))
    {
        state.current_alignment = Some("left".to_string());
    }

    if let Some(line_spacing_twips) =
        extract_spacing_directive_twips(chunk, layout.font_size_body_hp)
    {
        state.current_line_spacing_twips = Some(line_spacing_twips.max(1));
    }

    if let Some(vspace_twips) = parse_vspace_only_chunk_twips(chunk, layout.font_size_body_hp) {
        state.pending_space_before_twips = Some(vspace_twips.max(0));
        return None;
    }
    let leading_vspace_twips =
        extract_leading_vspace_twips(chunk, layout.font_size_body_hp).map(|twips| twips.max(0));

    let leading_hfill = chunk.trim_start().starts_with("\\hfill");
    let mut style = ParagraphStyle::default();
    let paragraph_in_titlingpage = state.in_titlingpage;

    if paragraph_in_titlingpage {
        style.first_line_indent_twips = Some(0);
        style.line_spacing_twips = state
            .current_line_spacing_twips
            .or(layout.body_line_spacing_twips);
    }

    if leading_hfill {
        style.alignment = Some("right".to_string());
    } else if let Some(alignment) = chunk_alignment_override {
        style.alignment = Some(alignment);
    } else if let Some(alignment) = state.current_alignment.clone() {
        style.alignment = Some(alignment);
    }

    style.space_before_twips = match (
        state.pending_space_before_twips.take(),
        leading_vspace_twips,
    ) {
        (Some(pending), Some(leading)) => Some(pending.saturating_add(leading)),
        (Some(pending), None) => Some(pending),
        (None, Some(leading)) => Some(leading),
        (None, None) => None,
    };

    let (font_size_hp, fontsize_line_spacing_twips) = extract_fontsize_settings(chunk);
    style.font_size_hp = font_size_hp;
    if let Some(fontsize_line) = fontsize_line_spacing_twips {
        style.line_spacing_twips = Some(fontsize_line.max(1));
    }

    if ends_flush {
        state.current_alignment = state.alignment_stack.pop().flatten();
    }
    if ends_titlingpage {
        state.in_titlingpage = false;
        state.current_alignment = None;
        state.current_line_spacing_twips = None;
        state.alignment_stack.clear();
    }

    Some(PreparedTextChunk {
        text: chunk.to_string(),
        style: has_nondefault_paragraph_style(&style).then_some(style),
    })
}

fn has_nondefault_paragraph_style(style: &ParagraphStyle) -> bool {
    style.alignment.is_some()
        || style.left_indent_twips.is_some()
        || style.first_line_indent_twips.is_some()
        || style.line_spacing_twips.is_some()
        || style.space_before_twips.is_some()
        || style.space_after_twips.is_some()
        || style.font_size_hp.is_some()
}

/// Trim whitespace around explicit `\\` line-break commands.
///
/// This preserves line-break semantics while preventing synthetic leading
/// spaces at the start of wrapped title-page lines.
fn trim_spaces_around_manual_linebreaks(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut pos = 0usize;

    while pos < src.len() {
        let rest = &src[pos..];
        if let Some(after_break) = rest.strip_prefix("\\\\") {
            while out.chars().last().is_some_and(|ch| ch.is_whitespace()) {
                out.pop();
            }
            out.push_str("\\\\");
            pos += 2;

            if after_break.starts_with('[')
                && let Some(arg_len) = bracketed_len(after_break)
            {
                out.push_str(&after_break[..arg_len]);
                pos += arg_len;
            }

            let mut ws_len = 0usize;
            for (idx, ch) in src[pos..].char_indices() {
                if ch.is_whitespace() {
                    ws_len = idx + ch.len_utf8();
                } else {
                    break;
                }
            }
            pos += ws_len;
            continue;
        }

        if let Some(ch) = rest.chars().next() {
            out.push(ch);
            pos += ch.len_utf8();
        } else {
            break;
        }
    }

    out
}

fn estimate_flushright_tabular_left_indent_twips(
    raw_chunk: &str,
    inlines: &[Inline],
    layout: &DocumentLayout,
    paragraph_font_size_hp: Option<usize>,
) -> Option<i32> {
    if !raw_chunk.contains("\\begin{flushright}")
        || !raw_chunk.contains("\\begin{tabular")
        || !raw_chunk.contains("\\end{tabular")
    {
        return None;
    }

    let max_chars = visible_line_width_chars(inlines).into_iter().max()?;
    if max_chars == 0 {
        return None;
    }

    let page_width_twips = layout
        .page_width_twips
        .map(|value| value as i32)
        .unwrap_or(DEFAULT_PAGE_WIDTH_TWIPS);
    let margin_left_twips = layout
        .page_margin_left_twips
        .unwrap_or(DEFAULT_PAGE_MARGIN_LEFT_TWIPS)
        .max(0);
    let margin_right_twips = layout
        .page_margin_right_twips
        .unwrap_or(DEFAULT_PAGE_MARGIN_RIGHT_TWIPS)
        .max(0);
    let text_width_twips = (page_width_twips - margin_left_twips - margin_right_twips).max(0);
    if text_width_twips == 0 {
        return None;
    }

    let font_size_hp = paragraph_font_size_hp
        .or(layout.font_size_body_hp)
        .unwrap_or(DEFAULT_BODY_FONT_SIZE_HP);
    let font_size_pt = font_size_hp as f64 / 2.0;
    if !font_size_pt.is_finite() || font_size_pt <= 0.0 {
        return None;
    }

    let avg_char_width_twips = font_size_pt * 20.0 * ESTIMATED_AVERAGE_CHAR_WIDTH_EM;
    let estimated_block_width_twips = ((max_chars as f64) * avg_char_width_twips).round() as i32;
    Some((text_width_twips - estimated_block_width_twips).max(0))
}

fn visible_line_width_chars(inlines: &[Inline]) -> Vec<usize> {
    let mut plain = String::new();
    push_plain_with_linebreaks(inlines, &mut plain);
    plain
        .split('\n')
        .map(normalize_whitespace)
        .map(|line| line.trim().chars().count())
        .filter(|count| *count > 0)
        .collect()
}

fn push_plain_with_linebreaks(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Text(text) => out.push_str(text),
            Inline::LineBreak => out.push('\n'),
            Inline::Bold(children) | Inline::Italic(children) | Inline::Footnote(children) => {
                push_plain_with_linebreaks(children, out);
            }
            Inline::InlineMath(text) | Inline::Reference(text) => out.push_str(text),
        }
    }
}

fn parse_vspace_only_chunk_twips(chunk: &str, body_font_size_hp: Option<usize>) -> Option<i32> {
    let mut rest = chunk.trim();
    let mut saw_vspace = false;
    let mut last_twips = None;

    loop {
        let after_cmd = if let Some(value) = rest.strip_prefix("\\vspace*") {
            value
        } else if let Some(value) = rest.strip_prefix("\\vspace") {
            value
        } else {
            break;
        };
        let after_cmd = after_cmd.trim_start();
        let arg_len = braced_len(after_cmd)?;
        let payload = &after_cmd[1..arg_len - 1];
        if let Some(twips) = parse_latex_length_to_twips_with_body_font(payload, body_font_size_hp)
        {
            last_twips = Some(twips);
        }
        saw_vspace = true;
        rest = after_cmd[arg_len..].trim_start();
    }

    if saw_vspace && rest.is_empty() {
        last_twips
    } else {
        None
    }
}

fn extract_leading_vspace_twips(chunk: &str, body_font_size_hp: Option<usize>) -> Option<i32> {
    let mut rest = chunk.trim_start();
    let mut last_twips = None;

    loop {
        let mut advanced = false;

        for marker in [
            "\\begin{titlingpage}",
            "\\begin{titlepage}",
            "\\end{titlingpage}",
            "\\end{titlepage}",
        ] {
            if let Some(after) = rest.strip_prefix(marker) {
                rest = after.trim_start();
                advanced = true;
                break;
            }
        }
        if advanced {
            continue;
        }

        if let Some((twips, after)) = consume_vspace_command(rest, body_font_size_hp) {
            if let Some(twips) = twips {
                last_twips = Some(twips);
            }
            rest = after.trim_start();
            continue;
        }

        for command in [
            "\\centering",
            "\\raggedright",
            "\\raggedleft",
            "\\flushleft",
            "\\flushright",
            "\\selectfont",
            "\\OnehalfSpacing",
            "\\DoubleSpacing",
            "\\SingleSpacing",
            "\\hfill",
            "\\par",
        ] {
            if let Some(after) = consume_control_word(rest, command) {
                rest = after.trim_start();
                advanced = true;
                break;
            }
        }
        if advanced {
            continue;
        }

        for command in ["\\setSpacing", "\\setstretch", "\\linespread"] {
            if let Some(after) = consume_control_with_braced_arg(rest, command) {
                rest = after.trim_start();
                advanced = true;
                break;
            }
        }
        if advanced {
            continue;
        }

        if let Some(after) = rest.strip_prefix("\\\\") {
            let mut after = after;
            if after.starts_with('[')
                && let Some(arg_len) = bracketed_len(after)
            {
                after = &after[arg_len..];
            }
            rest = after.trim_start();
            continue;
        }

        break;
    }

    last_twips
}

fn consume_control_word<'a>(src: &'a str, command: &str) -> Option<&'a str> {
    let rest = src.strip_prefix(command)?;
    if rest.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
        None
    } else {
        Some(rest)
    }
}

fn consume_control_with_braced_arg<'a>(src: &'a str, command: &str) -> Option<&'a str> {
    let rest = consume_control_word(src, command)?.trim_start();
    if !rest.starts_with('{') {
        return None;
    }
    let arg_len = braced_len(rest)?;
    Some(&rest[arg_len..])
}

fn consume_vspace_command(
    src: &str,
    body_font_size_hp: Option<usize>,
) -> Option<(Option<i32>, &str)> {
    let after_cmd = if let Some(value) = consume_control_word(src, "\\vspace*") {
        value
    } else {
        consume_control_word(src, "\\vspace")?
    };
    let after_cmd = after_cmd.trim_start();
    if !after_cmd.starts_with('{') {
        return None;
    }
    let arg_len = braced_len(after_cmd)?;
    let payload = &after_cmd[1..arg_len - 1];
    Some((
        parse_latex_length_to_twips_with_body_font(payload, body_font_size_hp),
        &after_cmd[arg_len..],
    ))
}

fn extract_spacing_directive_twips(chunk: &str, body_font_size_hp: Option<usize>) -> Option<i32> {
    extract_last_spacing_factor(chunk, body_font_size_hp).and_then(spacing_factor_to_twips)
}

fn extract_fontsize_settings(src: &str) -> (Option<usize>, Option<i32>) {
    let mut last_size_hp = None;
    let mut last_line_twips = None;
    let mut pos = 0usize;
    while let Some(rel) = src[pos..].find("\\fontsize") {
        let cmd_start = pos + rel + "\\fontsize".len();
        let mut tail = src[cmd_start..].trim_start();
        if !tail.starts_with('{') {
            pos = cmd_start;
            continue;
        }
        let Some(first_len) = braced_len(tail) else {
            pos = cmd_start;
            continue;
        };
        let font_arg = tail[1..first_len - 1].trim();
        let font_value = font_arg.trim_end_matches("pt").trim().replace(',', ".");
        if let Ok(pt) = font_value.parse::<f64>()
            && pt.is_finite()
            && pt > 0.0
        {
            last_size_hp = Some((pt * 2.0).round() as usize);
        }

        tail = tail[first_len..].trim_start();
        if tail.starts_with('{')
            && let Some(second_len) = braced_len(tail)
        {
            let line_arg = tail[1..second_len - 1].trim();
            if let Some(line_value) = parse_fontsize_line_spacing_twips(line_arg, last_size_hp) {
                last_line_twips = Some(line_value.max(1));
            }
            tail = &tail[second_len..];
        }
        if tail.trim().is_empty() {
            break;
        }
        pos = cmd_start + (src[cmd_start..].len() - tail.len());
    }
    (last_size_hp, last_line_twips)
}

fn parse_fontsize_line_spacing_twips(raw: &str, font_size_hp: Option<usize>) -> Option<i32> {
    if let Some(twips) = parse_latex_length_to_twips_with_body_font(raw, font_size_hp) {
        if let Some(size_hp) = font_size_hp {
            let font_pt = size_hp as f64 / 2.0;
            if font_pt > 0.0 {
                // DOCX auto line-spacing units: 240 == 1.0 line.
                // Convert LaTeX baseline distance (twips) to a relative line factor.
                let baseline_pt = twips as f64 / 20.0;
                let factor = baseline_pt / font_pt;
                if factor.is_finite() && factor > 0.0 {
                    return Some((factor * DOCX_AUTO_LINE_SPACING_UNIT_TWIPS).round() as i32);
                }
            }
        }
        // Fallback when font size is unknown: keep the raw value as an explicit auto-like unit.
        return Some(twips);
    }

    let numeric = raw
        .trim()
        .replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value > 0.0)?;
    if let Some(size_hp) = font_size_hp {
        let font_pt = size_hp as f64 / 2.0;
        if font_pt > 0.0 {
            let factor = numeric / font_pt;
            if factor.is_finite() && factor > 0.0 {
                return Some((factor * DOCX_AUTO_LINE_SPACING_UNIT_TWIPS).round() as i32);
            }
        }
    }
    // If only a bare numeric baseline is present and font size is unknown,
    // treat it as points.
    Some((numeric * 20.0).round() as i32)
}

/// Return `true` for preamble lines we want to discard entirely.
fn is_skippable(chunk: &str) -> bool {
    let c = chunk.trim_start();
    c.starts_with("\\documentclass")
        || c.starts_with("\\usepackage")
        || c.starts_with("\\begin{document}")
        || c.starts_with("\\end{document}")
        || c.starts_with("\\maketitle")
        || c.starts_with("\\printnomenclature")
        || c.starts_with("\\makenomenclature")
        || c.starts_with("\\listoffigures")
        || c.starts_with("\\listoftables")
        || c.starts_with("\\addcontentsline")
        || c.starts_with("\\addtocontents")
        || c.starts_with("\\counterwithout")
        || c.starts_with("\\counterwithin")
        || c.starts_with("\\setcounter")
        || c.starts_with("\\setlength")
        || c.starts_with("\\refstepcounter")
        || c.starts_with("\\newcommand")
        || c.starts_with("\\renewcommand")
        || c.starts_with("\\DeclareMathOperator")
        || c.starts_with("\\ifdefmacro")
        || c.starts_with("\\captionsetup")
        || c.starts_with("\\DefTblrTemplate")
        || c.starts_with("\\SetTblrTemplate")
        || c.starts_with("\\UseTblrTemplate")
        || c.starts_with("\\SetCell")
        || c.starts_with("\\begingroup")
        || c.starts_with("\\endgroup")
        || c.starts_with("\\appendix")
        || c.starts_with("\\endTOCtrue")
        || c.starts_with("\\pagestyle")
        || c.starts_with("\\thispagestyle")
}

fn assign_section_numbers(blocks: &mut [Block]) {
    let mut chapter_no = 0usize;
    let mut section_no = 0usize;
    let mut subsection_no = 0usize;

    for block in blocks.iter_mut() {
        let Block::Section { level, number, .. } = block else {
            continue;
        };

        if number.is_none() {
            continue;
        }

        match *level {
            1 => {
                chapter_no += 1;
                section_no = 0;
                subsection_no = 0;
                *number = Some(format!("{chapter_no}."));
            }
            2 => {
                section_no += 1;
                subsection_no = 0;
                *number = if chapter_no > 0 {
                    Some(format!("{chapter_no}.{section_no}"))
                } else {
                    Some(section_no.to_string())
                };
            }
            _ => {
                subsection_no += 1;
                *number = if chapter_no > 0 && section_no > 0 {
                    Some(format!("{chapter_no}.{section_no}.{subsection_no}"))
                } else if section_no > 0 {
                    Some(format!("{section_no}.{subsection_no}"))
                } else {
                    Some(subsection_no.to_string())
                };
            }
        }
    }
}

fn collect_declared_labels(source: &str) -> Vec<String> {
    extract_labels(source)
}

fn extract_labels(source: &str) -> Vec<String> {
    let mut labels = Vec::new();
    let mut pos = 0usize;
    while pos < source.len() {
        let Some(rel) = source[pos..].find("\\label") else {
            break;
        };
        let cmd_start = pos + rel;
        let cmd_end = cmd_start + "\\label".len();
        if source[cmd_end..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
        {
            pos = cmd_end;
            continue;
        }

        let mut arg_pos = cmd_end;
        while arg_pos < source.len() && source.as_bytes()[arg_pos].is_ascii_whitespace() {
            arg_pos += 1;
        }
        if arg_pos < source.len()
            && source.as_bytes()[arg_pos] == b'{'
            && let Some(len) = braced_len(&source[arg_pos..])
        {
            let value = source[arg_pos + 1..arg_pos + len - 1].trim();
            if !value.is_empty() {
                labels.push(value.to_string());
            }
            pos = arg_pos + len;
            continue;
        }
        pos = cmd_end;
    }
    labels
}

fn resolve_references(
    blocks: &mut [Block],
    declared_labels: &[String],
    layout: &crate::model::DocumentLayout,
    preserve_reference_nodes: bool,
) {
    let labels = build_label_registry(blocks, declared_labels, layout);

    for block in blocks.iter_mut() {
        match block {
            Block::Section { title, .. }
            | Block::Paragraph(title)
            | Block::StyledParagraph { inlines: title, .. } => {
                resolve_inline_references(title, &labels, preserve_reference_nodes);
            }
            Block::Table(table) => {
                resolve_inline_references(&mut table.caption, &labels, preserve_reference_nodes);
                resolve_inline_references(&mut table.source, &labels, preserve_reference_nodes);
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        resolve_inline_references(
                            &mut cell.content,
                            &labels,
                            preserve_reference_nodes,
                        );
                    }
                }
            }
            Block::Figure(figure) => {
                resolve_inline_references(&mut figure.caption, &labels, preserve_reference_nodes);
                resolve_inline_references(&mut figure.source, &labels, preserve_reference_nodes);
            }
            Block::List(list) => {
                for item in &mut list.items {
                    resolve_inline_references(item, &labels, preserve_reference_nodes);
                }
            }
            Block::DisplayMath(_)
            | Block::PageBreak
            | Block::PageOrientationSwitch { .. }
            | Block::BibliographyHeading { .. }
            | Block::TableOfContents => {}
        }
    }
}

fn build_label_registry(
    blocks: &[Block],
    declared_labels: &[String],
    layout: &crate::model::DocumentLayout,
) -> HashMap<String, String> {
    // Determine counter scoping from layout (parsed from LaTeX sources).
    // Default: within-chapter (standard LaTeX behaviour).
    let figure_within_chapter = layout.figure_counter_within_chapter.unwrap_or(true);
    let table_within_chapter = layout.table_counter_within_chapter.unwrap_or(true);
    let equation_within_chapter = layout.equation_counter_within_chapter.unwrap_or(true);

    let mut labels = HashMap::new();
    let mut chapter_no = 0usize;
    let mut figure_no = 0usize;
    let mut table_no = 0usize;
    let mut equation_no = 0usize;

    for block in blocks {
        match block {
            Block::Section {
                level,
                number,
                label,
                ..
            } => {
                if *level == 1 && number.is_some() {
                    chapter_no += 1;
                    // Reset float counters only when the LaTeX project requests
                    // within-chapter scoping for each float type.
                    if figure_within_chapter {
                        figure_no = 0;
                    }
                    if table_within_chapter {
                        table_no = 0;
                    }
                    if equation_within_chapter {
                        equation_no = 0;
                    }
                }

                if let (Some(label), Some(number)) = (label.as_ref(), number.as_ref()) {
                    let value = if *level == 1 {
                        number.trim_end_matches('.').to_string()
                    } else {
                        number.clone()
                    };
                    labels.insert(label.clone(), value);
                }
            }
            Block::Figure(figure) => {
                if let Some(label) = figure.label.as_ref() {
                    figure_no += 1;
                    let value = if figure_within_chapter && chapter_no > 0 {
                        format!("{chapter_no}.{figure_no}")
                    } else {
                        figure_no.to_string()
                    };
                    labels.insert(label.clone(), value);
                }
            }
            Block::Table(table) => {
                if let Some(label) = table.label.as_ref() {
                    table_no += 1;
                    let value = if table_within_chapter && chapter_no > 0 {
                        format!("{chapter_no}.{table_no}")
                    } else {
                        table_no.to_string()
                    };
                    labels.insert(label.clone(), value);
                }
            }
            Block::DisplayMath(body) => {
                equation_no += 1;
                let value = if equation_within_chapter && chapter_no > 0 {
                    format!("{chapter_no}.{equation_no}")
                } else {
                    equation_no.to_string()
                };
                for label in extract_labels(body) {
                    labels.insert(label, value.clone());
                }
            }
            Block::Paragraph(_)
            | Block::StyledParagraph { .. }
            | Block::List(_)
            | Block::PageBreak
            | Block::PageOrientationSwitch { .. }
            | Block::BibliographyHeading { .. }
            | Block::TableOfContents => {}
        }
    }

    for label in declared_labels {
        if labels.contains_key(label) {
            continue;
        }
        if let Some(value) = infer_appendix_label_value(label) {
            labels.insert(label.clone(), value);
        }
    }

    labels
}

fn infer_appendix_label_value(label: &str) -> Option<String> {
    let suffix = label
        .strip_prefix("app:")
        .or_else(|| label.strip_prefix("appendix:"))?;
    let token = suffix.trim();
    if token.is_empty() {
        return None;
    }

    let candidate = token
        .chars()
        .take_while(|c| c.is_alphanumeric())
        .collect::<String>();
    if candidate.is_empty() {
        None
    } else {
        Some(candidate)
    }
}

fn resolve_inline_references(
    inlines: &mut [Inline],
    labels: &HashMap<String, String>,
    preserve_reference_nodes: bool,
) {
    for inline in inlines {
        match inline {
            Inline::Reference(label) => {
                if preserve_reference_nodes {
                    if !labels.contains_key(label) {
                        *inline = Inline::Text(format!("[ref:{label}]"));
                    }
                } else {
                    let text = labels
                        .get(label)
                        .cloned()
                        .unwrap_or_else(|| format!("[ref:{label}]"));
                    *inline = Inline::Text(text);
                }
            }
            Inline::Bold(children) | Inline::Italic(children) | Inline::Footnote(children) => {
                resolve_inline_references(children, labels, preserve_reference_nodes);
            }
            Inline::Text(_) | Inline::InlineMath(_) | Inline::LineBreak => {}
        }
    }
}

/// Try to parse `\chapter{…}` / `\section{…}` / `\subsection{…}` / `\subsubsection{…}`.
///
/// Heading level mapping:
/// - `\chapter`       → level 1 (Heading1)
/// - `\section`       → level 2 (Heading2)
/// - `\subsection`    → level 3 (Heading3)
/// - `\subsubsection` → level 3 (Heading3, clamped)
fn try_parse_section(
    chunk: &str,
    autocite_mode: AutociteMode,
    metadata: &ParseMetadata,
    preserve_dynamic_markers: bool,
) -> Option<Block> {
    let (level, rest) = if let Some(r) = chunk.strip_prefix("\\subsubsection") {
        (3u8, r)
    } else if let Some(r) = chunk.strip_prefix("\\subsection") {
        (3u8, r)
    } else if let Some(r) = chunk.strip_prefix("\\section") {
        (2u8, r)
    } else if let Some(r) = chunk.strip_prefix("\\chapter") {
        (1u8, r)
    } else {
        return None;
    };

    let (is_starred, rest) = if let Some(rest) = rest.strip_prefix('*') {
        (true, rest)
    } else {
        (false, rest)
    };
    let rest = rest.trim_start();
    let title_len = braced_len(rest)?;
    let title_src = extract_braced(rest)?;
    let tail = &rest[title_len..];
    let title = parse_inlines(title_src, autocite_mode, metadata, preserve_dynamic_markers);
    Some(Block::Section {
        level,
        number: if is_starred {
            None
        } else {
            Some(String::new())
        },
        label: extract_label_macro(tail),
        title,
    })
}

fn try_parse_plain_heading(
    chunk: &str,
    autocite_mode: AutociteMode,
    metadata: &ParseMetadata,
    document_language: Option<&str>,
    preserve_dynamic_markers: bool,
) -> Option<Block> {
    if !supports_plain_cyrillic_heading_detection(document_language) {
        return None;
    }

    let mut chunk = strip_leading_layout_markers(chunk);
    if chunk.is_empty() {
        return None;
    }

    let mut force_unnumbered = false;
    if let Some(rest) = chunk.strip_prefix('*') {
        force_unnumbered = true;
        chunk = rest.trim_start();
    }
    if chunk.is_empty() {
        return None;
    }

    let lower = chunk.to_lowercase();
    let mut number: Option<String> = None;
    let normalized = if lower.starts_with("глава ") {
        let mut title = chunk.to_string();
        if let Some((chapter_no, chapter_title)) = parse_plain_chapter_heading(chunk) {
            number = Some(format!("{chapter_no}."));
            title = chapter_title.to_string();
        }
        title
    } else if is_plain_front_matter_heading(&lower) || force_unnumbered {
        chunk.to_string()
    } else {
        return None;
    };

    let title = parse_inlines(
        normalized.trim(),
        autocite_mode,
        metadata,
        preserve_dynamic_markers,
    );
    if title.is_empty() {
        return None;
    }

    Some(Block::Section {
        level: 1,
        number: if force_unnumbered {
            None
        } else {
            number.or(Some(String::new()))
        },
        label: None,
        title,
    })
}

/// Consume structural commands that appear at the beginning of a text chunk.
///
/// This preserves flow markers when a chunk starts with commands followed by
/// regular content in the same paragraph, for example:
/// `\clearpage \landscape Some heading`.
fn consume_leading_structural_blocks(mut chunk: &str) -> (Vec<Block>, &str) {
    let mut blocks = Vec::new();

    loop {
        let trimmed = chunk.trim_start();
        if trimmed.is_empty() {
            return (blocks, trimmed);
        }

        if let Some(after) = consume_leading_page_break_command(trimmed) {
            blocks.push(Block::PageBreak);
            chunk = after;
            continue;
        }
        if let Some((orientation, after)) = consume_leading_landscape_switch_command(trimmed) {
            blocks.push(Block::PageOrientationSwitch { orientation });
            chunk = after;
            continue;
        }
        if let Some(after) = consume_leading_tableofcontents_command(trimmed) {
            blocks.push(Block::TableOfContents);
            chunk = after;
            continue;
        }

        return (blocks, trimmed);
    }
}

fn consume_leading_page_break_command(src: &str) -> Option<&str> {
    ["\\newpage", "\\clearpage", "\\cleardoublepage"]
        .iter()
        .find_map(|command| consume_control_word(src, command))
}

fn consume_leading_tableofcontents_command(src: &str) -> Option<&str> {
    let mut rest = consume_control_word(src, "\\tableofcontents")?;
    if let Some(after_star) = rest.strip_prefix('*') {
        rest = after_star;
    }
    Some(rest)
}

fn consume_leading_landscape_switch_command(src: &str) -> Option<(PageOrientation, &str)> {
    if let Some(after) = src.strip_prefix("\\begin{landscape}") {
        return Some((PageOrientation::Landscape, after));
    }
    if let Some(after) = src.strip_prefix("\\end{landscape}") {
        return Some((PageOrientation::Portrait, after));
    }

    if let Some(after_begin) = consume_control_word(src, "\\begin") {
        let after_begin = after_begin.trim_start();
        if let Some(after) = after_begin.strip_prefix("{landscape}") {
            return Some((PageOrientation::Landscape, after));
        }
    }
    if let Some(after_end) = consume_control_word(src, "\\end") {
        let after_end = after_end.trim_start();
        if let Some(after) = after_end.strip_prefix("{landscape}") {
            return Some((PageOrientation::Portrait, after));
        }
    }

    if let Some(after) = consume_control_word(src, "\\landscape") {
        return Some((PageOrientation::Landscape, after));
    }
    if let Some(after) = consume_control_word(src, "\\endlandscape") {
        return Some((PageOrientation::Portrait, after));
    }

    None
}

/// Detect structural control commands and emit dedicated block nodes.
///
/// Supported commands:
/// - `\tableofcontents[*]` -> [`Block::TableOfContents`]
/// - `\newpage`, `\clearpage`, `\cleardoublepage` -> [`Block::PageBreak`]
/// - `\begin{landscape}`, `\end{landscape}`, `\landscape`, `\endlandscape`
///   -> [`Block::PageOrientationSwitch`]
fn try_parse_structural_heading_command(chunk: &str) -> Option<Block> {
    let command = chunk.trim_start();
    if is_standalone_page_break_command(command) {
        Some(Block::PageBreak)
    } else if let Some(orientation) = parse_standalone_landscape_switch_command(command) {
        Some(Block::PageOrientationSwitch { orientation })
    } else if is_standalone_tableofcontents_command(command) {
        Some(Block::TableOfContents)
    } else {
        None
    }
}

fn is_standalone_page_break_command(chunk: &str) -> bool {
    let mut rest = chunk.trim();
    let mut matched = false;

    loop {
        let mut consumed_any = false;
        for command in ["\\newpage", "\\clearpage", "\\cleardoublepage"] {
            if let Some(after) = consume_control_word(rest, command) {
                matched = true;
                rest = after.trim_start();
                consumed_any = true;
                break;
            }
        }
        if !consumed_any {
            break;
        }
    }

    matched && rest.is_empty()
}

fn parse_standalone_landscape_switch_command(chunk: &str) -> Option<PageOrientation> {
    let mut rest = chunk.trim();
    let mut orientation = None;

    loop {
        let next = if let Some(after) = rest.strip_prefix("\\begin{landscape}") {
            orientation = Some(PageOrientation::Landscape);
            Some(after)
        } else if let Some(after) = rest.strip_prefix("\\end{landscape}") {
            orientation = Some(PageOrientation::Portrait);
            Some(after)
        } else if let Some(after) = consume_control_word(rest, "\\landscape") {
            orientation = Some(PageOrientation::Landscape);
            Some(after)
        } else if let Some(after) = consume_control_word(rest, "\\endlandscape") {
            orientation = Some(PageOrientation::Portrait);
            Some(after)
        } else {
            None
        };

        let Some(after) = next else {
            break;
        };

        rest = after.trim_start();
    }

    if rest.is_empty() { orientation } else { None }
}

fn is_standalone_tableofcontents_command(chunk: &str) -> bool {
    let mut rest = chunk.trim_start();
    let Some(after_command) = consume_control_word(rest, "\\tableofcontents") else {
        return false;
    };
    rest = after_command.trim_start();
    if let Some(after_star) = rest.strip_prefix('*') {
        rest = after_star.trim_start();
    }
    rest.is_empty()
}

/// Detect bibliography-rendering commands and emit a `Block::BibliographyHeading`.
///
/// Recognised commands:
/// - `\printbibliography[title=...]` — title from optional arg or default "СПИСОК ЛИТЕРАТУРЫ"
/// - `\insertbibliofullsorted` — default title "СПИСОК ЛИТЕРАТУРЫ"
/// - `\insertbiblioauthor` — same default
///
/// Only the heading is rendered; no `.bib` file entries are parsed.
/// Detect bibliography-rendering commands and emit a `Block::BibliographyHeading`.
///
/// Recognised commands:
/// - `\printbibliography[title=...]` — title from optional arg or language default
/// - `\insertbibliofullsorted` — language-derived default title
/// - `\insertbiblioauthor` — same default
///
/// The `language` parameter is a BCP-47 tag (e.g. `"ru-RU"`, `"en-US"`) derived from
/// `\usepackage[...]{babel}` or `\setmainlanguage{...}`. When `None`, defaults to
/// `"REFERENCES"`. Explicit `title=` arguments always take precedence over the default.
///
/// Only the heading is rendered; no `.bib` file entries are parsed.
fn try_parse_bibliography_command(chunk: &str, language: Option<&str>) -> Option<Block> {
    let s = chunk.trim_start();
    let default_title = || default_bibliography_title_for_language(language).to_string();

    if let Some(rest) = s.strip_prefix("\\printbibliography") {
        // Try to extract title from optional [title=...] argument.
        let after = rest.trim_start();
        let (title, has_nobibheading) = if after.starts_with('[') {
            if let Some(close) = after.find(']') {
                let args = &after[1..close];
                let no_heading = args.contains("nobibheading");
                let t = extract_printbibliography_title(args).unwrap_or_else(default_title);
                (t, no_heading)
            } else {
                (default_title(), false)
            }
        } else {
            (default_title(), false)
        };
        // Skip `heading=nobibheading` — those have no visible heading.
        if has_nobibheading || title.trim().is_empty() {
            return None;
        }
        return Some(Block::BibliographyHeading { title });
    }

    if s.starts_with("\\insertbibliofullsorted")
        || s.starts_with("\\insertbiblioauthor")
        || s.starts_with("\\insertbibliofull")
    {
        return Some(Block::BibliographyHeading {
            title: default_title(),
        });
    }

    None
}

/// Detects plain-text (non-LaTeX-command) headings in unnumbered paragraph text.
///
/// These strings are Russian-specific because they match a known corpus where
/// front-matter sections are sometimes written as plain text without LaTeX commands.
/// Detection is language-gated and enabled only for Cyrillic document languages
/// (`ru-RU`, `uk-UA`, `be-BY`).
///
/// **Known limitation**: non-Russian plain-text documents that use this path will
/// need their own patterns. This function is NOT called for `\tableofcontents` —
/// that command always emits [`Block::TableOfContents`] via
/// [`try_parse_structural_heading_command`]. The `"оглавление"` entry here only
/// matches a literal plain-text heading with no preceding LaTeX command.
fn is_plain_front_matter_heading(lower: &str) -> bool {
    let trimmed = lower.trim();
    matches!(
        trimmed,
        "введение"
            | "заключение"
            | "оглавление"
            | "список сокращений и условных обозначений"
            | "список сокращений"
            | "список условных обозначений"
            | "библиографический список"
            | "список литературы"
    ) || trimmed.starts_with("приложение")
}

fn supports_plain_cyrillic_heading_detection(language: Option<&str>) -> bool {
    matches!(language, Some("ru-RU" | "uk-UA" | "be-BY"))
}

fn parse_plain_chapter_heading(chunk: &str) -> Option<(usize, &str)> {
    let rest = chunk.strip_prefix("ГЛАВА ")?;
    let mut digits_end = 0usize;
    for (i, ch) in rest.char_indices() {
        if ch.is_ascii_digit() {
            digits_end = i + ch.len_utf8();
        } else {
            break;
        }
    }
    if digits_end == 0 {
        return None;
    }

    let chapter_no = rest[..digits_end].parse::<usize>().ok()?;
    let mut tail = rest[digits_end..].trim_start();
    if let Some(after_dot) = tail.strip_prefix('.') {
        tail = after_dot.trim_start();
    }
    Some((chapter_no, tail))
}

fn strip_leading_layout_markers(mut chunk: &str) -> &str {
    loop {
        chunk = chunk.trim_start();
        if !chunk.starts_with('[') {
            return chunk;
        }
        let Some(marker_len) = bracketed_len(chunk) else {
            return chunk;
        };
        let marker = &chunk[1..marker_len - 1];
        if looks_like_dimension_marker(marker) {
            chunk = &chunk[marker_len..];
            continue;
        }
        return chunk;
    }
}

fn looks_like_dimension_marker(marker: &str) -> bool {
    let normalized: String = marker
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    !normalized.is_empty()
        && normalized.chars().all(|ch| {
            ch.is_ascii_digit()
                || matches!(
                    ch,
                    '.' | ',' | '+' | '-' | 'c' | 'm' | 'p' | 't' | 'e' | 'x'
                )
        })
        && (normalized.contains("cm")
            || normalized.contains("mm")
            || normalized.contains("pt")
            || normalized.contains("em")
            || normalized.contains("ex"))
}

/// Extract the content of the first `{…}` group from the start of `src`.
fn extract_braced(src: &str) -> Option<&str> {
    let src = src.trim_start();
    if !src.starts_with('{') {
        return None;
    }
    let inner = &src[1..];
    let mut depth = 1usize;
    for (i, ch) in inner.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&inner[..i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Parse a string of mixed text and inline LaTeX commands into [`Inline`] nodes.
fn parse_inlines(
    src: &str,
    autocite_mode: AutociteMode,
    metadata: &ParseMetadata,
    preserve_dynamic_markers: bool,
) -> Vec<Inline> {
    #[derive(Debug, Clone, Copy, Default)]
    struct InlineStyleState {
        bold: bool,
        italic: bool,
    }

    let mut result = Vec::new();
    let mut pos = 0;
    let bytes = src.as_bytes();
    let len = src.len();
    let mut style_state = InlineStyleState::default();

    while pos < len {
        match bytes[pos] {
            b'$' => {
                // Inline math: $…$
                // Find the closing `$` (skip escaped `\$`).
                let mut end = pos + 1;
                while end < len {
                    if bytes[end] == b'\\' {
                        end += 2; // skip escaped char
                    } else if bytes[end] == b'$' {
                        break;
                    } else {
                        end += 1;
                    }
                }
                if end < len {
                    let math_src = src[pos + 1..end].trim().to_string();
                    push_inline_with_style(
                        &mut result,
                        Inline::InlineMath(math_src),
                        style_state.bold,
                        style_state.italic,
                    );
                    pos = end + 1;
                } else {
                    // No closing $ — emit as plain text.
                    push_inline_with_style(
                        &mut result,
                        Inline::Text("$".to_string()),
                        style_state.bold,
                        style_state.italic,
                    );
                    pos += 1;
                }
            }
            b'\\' => {
                let rest = &src[pos..];
                if let Some((inlines, consumed)) = try_parse_inline_command(
                    rest,
                    autocite_mode,
                    metadata,
                    preserve_dynamic_markers,
                ) {
                    if inlines.is_empty() {
                        apply_declaration_style_directive(
                            &rest[..consumed],
                            &mut style_state.bold,
                            &mut style_state.italic,
                        );
                    } else {
                        extend_inlines_with_style(
                            &mut result,
                            inlines,
                            style_state.bold,
                            style_state.italic,
                        );
                    }
                    pos += consumed;
                } else {
                    let end = rest[1..]
                        .find(|c: char| !c.is_ascii_alphabetic())
                        .map(|i| pos + 1 + i)
                        .unwrap_or(len);
                    pos = end;
                    loop {
                        let remaining = src[pos..].trim_start();
                        pos += src[pos..].len() - remaining.len();

                        if remaining.starts_with('{')
                            && let Some(arg_len) = braced_len(remaining)
                        {
                            pos += arg_len;
                            continue;
                        }
                        if remaining.starts_with('[')
                            && let Some(arg_len) = bracketed_len(remaining)
                        {
                            pos += arg_len;
                            continue;
                        }
                        break;
                    }
                }
            }
            b'{' => {
                let rest = &src[pos..];
                if let Some((inlines, consumed)) =
                    try_parse_brace_group(rest, autocite_mode, metadata, preserve_dynamic_markers)
                {
                    extend_inlines_with_style(
                        &mut result,
                        inlines,
                        style_state.bold,
                        style_state.italic,
                    );
                    pos += consumed;
                } else if let Some(inner_len) = braced_len(rest) {
                    let inner = &rest[1..inner_len - 1];
                    let inlines =
                        parse_inlines(inner, autocite_mode, metadata, preserve_dynamic_markers);
                    extend_inlines_with_style(
                        &mut result,
                        inlines,
                        style_state.bold,
                        style_state.italic,
                    );
                    pos += inner_len;
                } else {
                    push_inline_with_style(
                        &mut result,
                        Inline::Text("{".to_string()),
                        style_state.bold,
                        style_state.italic,
                    );
                    pos += 1;
                }
            }
            _ => {
                let start = pos;
                while pos < len && bytes[pos] != b'\\' && bytes[pos] != b'{' && bytes[pos] != b'$' {
                    pos += 1;
                }
                let text = &src[start..pos];
                let normalized = normalize_whitespace(text);
                if !normalized.is_empty() {
                    push_inline_with_style(
                        &mut result,
                        Inline::Text(normalized),
                        style_state.bold,
                        style_state.italic,
                    );
                }
            }
        }
    }

    result
}

fn push_inline_with_style(result: &mut Vec<Inline>, inline: Inline, bold: bool, italic: bool) {
    if matches!(inline, Inline::LineBreak) {
        result.push(inline);
        return;
    }

    let mut wrapped = inline;
    if bold {
        wrapped = Inline::Bold(vec![wrapped]);
    }
    if italic {
        wrapped = Inline::Italic(vec![wrapped]);
    }
    result.push(wrapped);
}

fn extend_inlines_with_style(
    result: &mut Vec<Inline>,
    inlines: Vec<Inline>,
    bold: bool,
    italic: bool,
) {
    for inline in inlines {
        push_inline_with_style(result, inline, bold, italic);
    }
}

fn apply_declaration_style_directive(src: &str, bold: &mut bool, italic: &mut bool) {
    let src = src.trim_start();
    if src.starts_with("\\bfseries") || src.starts_with("\\bf") {
        *bold = true;
        return;
    }
    if src.starts_with("\\itshape") || src.starts_with("\\it") {
        *italic = true;
        return;
    }
    if src.starts_with("\\normalfont")
        || src.starts_with("\\rmfamily")
        || src.starts_with("\\upshape")
    {
        *bold = false;
        *italic = false;
    }
}

fn try_parse_inline_command(
    src: &str,
    autocite_mode: AutociteMode,
    metadata: &ParseMetadata,
    preserve_dynamic_markers: bool,
) -> Option<(Vec<Inline>, usize)> {
    if let Some(rest) = src.strip_prefix("\\\\") {
        let mut consumed = 2usize;
        if rest.starts_with('[')
            && let Some(arg_len) = bracketed_len(rest)
        {
            consumed += arg_len;
        }
        return Some((vec![Inline::LineBreak], consumed));
    }
    for cmd in ["\\linebreak", "\\newline"] {
        if let Some(rest) = src.strip_prefix(cmd)
            && !rest.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        {
            return Some((vec![Inline::LineBreak], cmd.len()));
        }
    }
    // Control-space command (`\ `) produces an explicit space.
    if src.starts_with('\\')
        && let Some(space_char) = src[1..].chars().next()
        && space_char.is_ascii_whitespace()
    {
        return Some((
            vec![Inline::Text(" ".to_string())],
            1 + space_char.len_utf8(),
        ));
    }
    for cmd in ["\\hfill", "\\allowbreak"] {
        if let Some(rest) = src.strip_prefix(cmd)
            && !rest.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        {
            return Some((vec![Inline::Text(" ".to_string())], cmd.len()));
        }
    }
    if let Some(rest) = src.strip_prefix("\\par")
        && !rest.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
    {
        return Some((vec![Inline::Text(" ".to_string())], "\\par".len()));
    }
    for declaration in [
        "\\centering",
        "\\raggedright",
        "\\raggedleft",
        "\\flushleft",
        "\\flushright",
        "\\selectfont",
        "\\OnehalfSpacing",
        "\\DoubleSpacing",
        "\\SingleSpacing",
    ] {
        if let Some(rest) = src.strip_prefix(declaration)
            && !rest.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        {
            return Some((Vec::new(), declaration.len()));
        }
    }
    if let Some(rest) = src.strip_prefix("\\cdot")
        && !rest.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
    {
        return Some((vec![Inline::Text("·".to_string())], "\\cdot".len()));
    }
    if let Some(rest) = src.strip_prefix("\\hbox")
        && !rest.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
    {
        let mut consumed = "\\hbox".len();
        let mut tail = rest;

        let trimmed = tail.trim_start();
        consumed += tail.len() - trimmed.len();
        tail = trimmed;

        if let Some(after_to) = tail.strip_prefix("to")
            && !after_to
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic())
        {
            consumed += "to".len();
            tail = after_to;

            let trimmed = tail.trim_start();
            consumed += tail.len() - trimmed.len();
            tail = trimmed;

            if tail.starts_with('{') {
                if let Some(arg_len) = braced_len(tail) {
                    consumed += arg_len;
                    tail = &tail[arg_len..];
                }
            } else if tail.starts_with('\\') {
                if let Some((_, token_len)) = parse_control_word(tail) {
                    consumed += token_len;
                    tail = &tail[token_len..];
                }
            } else {
                let token_len = tail
                    .find(|c: char| c.is_whitespace() || c == '{' || c == '[')
                    .unwrap_or(tail.len());
                consumed += token_len;
                tail = &tail[token_len..];
            }

            let trimmed = tail.trim_start();
            consumed += tail.len() - trimmed.len();
            tail = trimmed;
        }

        if let Some(arg_len) = braced_len(tail) {
            let inner = &tail[1..arg_len - 1];
            consumed += arg_len;
            return Some((
                parse_inlines(inner, autocite_mode, metadata, preserve_dynamic_markers),
                consumed,
            ));
        }

        return Some((Vec::new(), consumed));
    }

    if let Some(r) = src.strip_prefix("\\textbf") {
        let r = r.trim_start_matches(' ');
        if let Some(arg_len) = braced_len(r) {
            let inner = &r[1..arg_len - 1];
            let consumed = src.len() - r.len() + arg_len;
            return Some((
                vec![Inline::Bold(parse_inlines(
                    inner,
                    autocite_mode,
                    metadata,
                    preserve_dynamic_markers,
                ))],
                consumed,
            ));
        }
    }
    if let Some(r) = src
        .strip_prefix("\\textit")
        .or_else(|| src.strip_prefix("\\emph"))
    {
        let r = r.trim_start_matches(' ');
        if let Some(arg_len) = braced_len(r) {
            let inner = &r[1..arg_len - 1];
            let consumed = src.len() - r.len() + arg_len;
            return Some((
                vec![Inline::Italic(parse_inlines(
                    inner,
                    autocite_mode,
                    metadata,
                    preserve_dynamic_markers,
                ))],
                consumed,
            ));
        }
    }
    for (cmd, is_bold) in [("\\bfseries", true), ("\\itshape", false)] {
        if let Some(r) = src.strip_prefix(cmd) {
            let r = r.trim_start_matches(' ');
            if let Some(arg_len) = braced_len(r) {
                let inner = &r[1..arg_len - 1];
                let consumed = src.len() - r.len() + arg_len;
                let children =
                    parse_inlines(inner, autocite_mode, metadata, preserve_dynamic_markers);
                let wrapped = if is_bold {
                    Inline::Bold(children)
                } else {
                    Inline::Italic(children)
                };
                return Some((vec![wrapped], consumed));
            }
            return Some((Vec::new(), cmd.len()));
        }
    }
    if let Some(r) = src.strip_prefix("\\texorpdfstring") {
        let r = r.trim_start_matches(' ');
        if let Some(first_len) = braced_len(r) {
            let first = &r[1..first_len - 1];
            let mut consumed = src.len() - r.len() + first_len;
            let out = parse_inlines(first, autocite_mode, metadata, preserve_dynamic_markers);

            let rest = r[first_len..].trim_start_matches(' ');
            consumed += r[first_len..].len() - rest.len();
            if let Some(second_len) = braced_len(rest) {
                consumed += second_len;
            }

            return Some((out, consumed));
        }
    }
    for textual_cmd in &[
        "\\texttt",
        "\\textrm",
        "\\textnormal",
        "\\textup",
        "\\MakeUppercase",
        "\\MakeLowercase",
        "\\mbox",
        "\\enquote",
        "\\uline",
        "\\url",
    ] {
        if let Some(r) = src.strip_prefix(textual_cmd) {
            let r = r.trim_start_matches(' ');
            if let Some(arg_len) = braced_len(r) {
                let inner = &r[1..arg_len - 1];
                let consumed = src.len() - r.len() + arg_len;
                return Some((
                    parse_inlines(inner, autocite_mode, metadata, preserve_dynamic_markers),
                    consumed,
                ));
            }
        }
    }
    if let Some(r) = src.strip_prefix("\\footnote") {
        let r = r.trim_start_matches(' ');
        if let Some(arg_len) = braced_len(r) {
            let inner = &r[1..arg_len - 1];
            let consumed = src.len() - r.len() + arg_len;
            return Some((
                vec![Inline::Footnote(parse_inlines(
                    inner,
                    autocite_mode,
                    metadata,
                    preserve_dynamic_markers,
                ))],
                consumed,
            ));
        }
    }
    if let Some(r) = src.strip_prefix("\\ifnumequal") {
        let mut consumed = src.len() - r.len();
        let mut args: Vec<String> = Vec::new();

        for _ in 0..4 {
            while consumed < src.len() && src.as_bytes()[consumed].is_ascii_whitespace() {
                consumed += 1;
            }
            let Some(arg_len) = braced_len(&src[consumed..]) else {
                break;
            };
            args.push(src[consumed + 1..consumed + arg_len - 1].to_string());
            consumed += arg_len;
        }

        if args.len() >= 3 {
            let selected = if args.len() == 4 {
                match (
                    evaluate_counter_expression(&args[0], metadata),
                    evaluate_counter_expression(&args[1], metadata),
                ) {
                    (Some(lhs), Some(rhs)) => {
                        if lhs == rhs {
                            args[2].as_str()
                        } else {
                            args[3].as_str()
                        }
                    }
                    _ => args[2].as_str(),
                }
            } else {
                args[2].as_str()
            };

            let final_consumed = if args.len() == 4 { consumed } else { src.len() };
            return Some((
                parse_inlines(selected, autocite_mode, metadata, preserve_dynamic_markers),
                final_consumed,
            ));
        }
    }
    if src.starts_with("\\ifnum") {
        let mut pos = "\\ifnum".len();
        while pos < src.len() && src.as_bytes()[pos].is_ascii_whitespace() {
            pos += 1;
        }

        let (lhs_consumed, lhs_value) = consume_ifnum_operand(&src[pos..], metadata)?;
        pos += lhs_consumed;
        while pos < src.len() && src.as_bytes()[pos].is_ascii_whitespace() {
            pos += 1;
        }

        let cmp = src[pos..].chars().next()?;
        if !matches!(cmp, '<' | '=' | '>') {
            return None;
        }
        pos += cmp.len_utf8();

        while pos < src.len() && src.as_bytes()[pos].is_ascii_whitespace() {
            pos += 1;
        }

        let (rhs_consumed, rhs_value) = consume_ifnum_operand(&src[pos..], metadata)?;
        pos += rhs_consumed;

        let Some(fi_rel) = src[pos..].find("\\fi") else {
            return Some((Vec::new(), src.len()));
        };
        let fi_pos = pos + fi_rel;
        let else_pos = src[pos..].find("\\else").map(|rel| pos + rel);

        let (then_branch, else_branch) = if let Some(else_pos) = else_pos {
            if else_pos < fi_pos {
                (
                    &src[pos..else_pos],
                    Some(&src[else_pos + "\\else".len()..fi_pos]),
                )
            } else {
                (&src[pos..fi_pos], None)
            }
        } else {
            (&src[pos..fi_pos], None)
        };

        let condition_true = match (lhs_value, rhs_value) {
            (Some(lhs), Some(rhs)) => match cmp {
                '<' => lhs < rhs,
                '=' => lhs == rhs,
                '>' => lhs > rhs,
                _ => true,
            },
            _ => true,
        };
        let chosen = if condition_true {
            then_branch
        } else {
            else_branch.unwrap_or("")
        };

        return Some((
            parse_inlines(
                chosen.trim(),
                autocite_mode,
                metadata,
                preserve_dynamic_markers,
            ),
            fi_pos + "\\fi".len(),
        ));
    }
    if src.starts_with("\\else") {
        return Some((Vec::new(), "\\else".len()));
    }
    if src.starts_with("\\fi") {
        return Some((Vec::new(), "\\fi".len()));
    }
    if let Some(r) = src.strip_prefix("\\label") {
        let r = r.trim_start_matches(' ');
        if let Some(arg_len) = braced_len(r) {
            let consumed = src.len() - r.len() + arg_len;
            return Some((Vec::new(), consumed));
        }
    }
    if let Some(r) = src.strip_prefix("\\formbytotal") {
        let mut consumed = src.len() - r.len();
        let mut args: Vec<String> = Vec::new();

        for _ in 0..5 {
            while consumed < src.len() && src.as_bytes()[consumed].is_ascii_whitespace() {
                consumed += 1;
            }

            let Some(arg_len) = braced_len(&src[consumed..]) else {
                break;
            };
            args.push(src[consumed + 1..consumed + arg_len - 1].to_string());
            consumed += arg_len;
        }

        if args.len() == 5 {
            let replacement = if let Some(total) = metadata.counters.get(args[0].trim()) {
                render_formbytotal(
                    *total,
                    args[1].trim(),
                    args[2].trim(),
                    args[3].trim(),
                    args[4].trim(),
                )
            } else if preserve_dynamic_markers {
                formbytotal_marker(
                    args[0].trim(),
                    args[1].trim(),
                    args[2].trim(),
                    args[3].trim(),
                    args[4].trim(),
                )
            } else {
                let stem = args[1].trim();
                let chosen_suffix = [&args[4], &args[3], &args[2]]
                    .iter()
                    .map(|s| s.trim())
                    .find(|s| !s.is_empty())
                    .unwrap_or("");
                format!("{stem}{chosen_suffix}")
            };
            return Some((vec![Inline::Text(replacement)], consumed));
        }
    }
    if let Some(rest) = src.strip_prefix("\\the")
        && !rest.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
    {
        let mut consumed = "\\the".len();
        let trimmed = rest.trim_start();
        consumed += rest.len() - trimmed.len();

        if let Some((counter_name, counter_len)) = parse_control_word(trimmed) {
            consumed += counter_len;
            if let Some(value) = counter_text(metadata, counter_name) {
                return Some((vec![Inline::Text(value)], consumed));
            }
            if preserve_dynamic_markers {
                return Some((vec![Inline::Text(counter_marker(counter_name))], consumed));
            }
            return Some((Vec::new(), consumed));
        }
        return Some((Vec::new(), consumed));
    }
    if let Some(rest) = src.strip_prefix("\\year")
        && !rest.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
    {
        if let Some(value) = counter_text(metadata, "year") {
            return Some((vec![Inline::Text(value)], "\\year".len()));
        }
        if preserve_dynamic_markers {
            return Some((vec![Inline::Text(counter_marker("year"))], "\\year".len()));
        }
        return Some((Vec::new(), "\\year".len()));
    }
    for cmd in &["\\total", "\\totvalue", "\\arabic", "\\value"] {
        if let Some(r) = src.strip_prefix(cmd) {
            let r = r.trim_start_matches(' ');
            if let Some(arg_len) = braced_len(r) {
                let key = r[1..arg_len - 1].trim();
                let consumed = src.len() - r.len() + arg_len;
                if let Some(value) = counter_text(metadata, key) {
                    return Some((vec![Inline::Text(value)], consumed));
                }
                if preserve_dynamic_markers {
                    return Some((vec![Inline::Text(counter_marker(key))], consumed));
                }
                return Some((Vec::new(), consumed));
            }
        }
    }
    if src.starts_with("\\formatpl") {
        let consumed = "\\formatpl".len();
        if let Some(value) = counter_text(metadata, "citeauthorpl") {
            return Some((vec![Inline::Text(value)], consumed));
        }
        if preserve_dynamic_markers {
            return Some((vec![Inline::Text(counter_marker("citeauthorpl"))], consumed));
        }
        return Some((Vec::new(), consumed));
    }
    for cmd in &["\\ref", "\\autoref", "\\cref", "\\Cref"] {
        if let Some(r) = src.strip_prefix(cmd) {
            let r = r.trim_start_matches(' ');
            if let Some(arg_len) = braced_len(r) {
                let inner = r[1..arg_len - 1].trim().to_string();
                let consumed = src.len() - r.len() + arg_len;
                return Some((vec![Inline::Reference(inner)], consumed));
            }
        }
    }
    if let Some(r) = src.strip_prefix("\\eqref") {
        let r = r.trim_start_matches(' ');
        if let Some(arg_len) = braced_len(r) {
            let inner = r[1..arg_len - 1].trim().to_string();
            let consumed = src.len() - r.len() + arg_len;
            return Some((
                vec![
                    Inline::Text("(".to_string()),
                    Inline::Reference(inner),
                    Inline::Text(")".to_string()),
                ],
                consumed,
            ));
        }
    }
    if let Some(r) = src.strip_prefix("\\cite") {
        let r = r.trim_start_matches(' ');
        if let Some(arg_len) = braced_len(r) {
            let inner = &r[1..arg_len - 1];
            let placeholder = format!("[{}]", inner);
            let consumed = src.len() - r.len() + arg_len;
            return Some((vec![Inline::Text(placeholder)], consumed));
        }
    }
    if let Some(r) = src.strip_prefix("\\autocite") {
        let r = r.trim_start_matches(' ');
        if let Some(arg_len) = braced_len(r) {
            let inner = &r[1..arg_len - 1];
            let consumed = src.len() - r.len() + arg_len;
            let placeholder = format!("[{}]", inner);
            return Some(match autocite_mode {
                AutociteMode::InlinePlaceholder => (vec![Inline::Text(placeholder)], consumed),
                AutociteMode::FootnotePlaceholder => (
                    vec![Inline::Footnote(vec![Inline::Text(placeholder)])],
                    consumed,
                ),
            });
        }
    }
    None
}

fn try_parse_brace_group(
    src: &str,
    autocite_mode: AutociteMode,
    metadata: &ParseMetadata,
    preserve_dynamic_markers: bool,
) -> Option<(Vec<Inline>, usize)> {
    if !src.starts_with('{') {
        return None;
    }
    let inner_src = &src[1..];
    let (wrapper, content_src): (fn(Vec<Inline>) -> Inline, &str) = if let Some(r) = inner_src
        .strip_prefix("\\bf")
        .filter(|r| !r.chars().next().is_some_and(|c| c.is_ascii_alphabetic()))
    {
        (|v| Inline::Bold(v), r.trim_start())
    } else if let Some(r) = inner_src
        .strip_prefix("\\it")
        .filter(|r| !r.chars().next().is_some_and(|c| c.is_ascii_alphabetic()))
    {
        (|v| Inline::Italic(v), r.trim_start())
    } else if let Some(r) = inner_src
        .strip_prefix("\\bfseries")
        .filter(|r| !r.chars().next().is_some_and(|c| c.is_ascii_alphabetic()))
    {
        (|v| Inline::Bold(v), r.trim_start())
    } else if let Some(r) = inner_src
        .strip_prefix("\\itshape")
        .filter(|r| !r.chars().next().is_some_and(|c| c.is_ascii_alphabetic()))
    {
        (|v| Inline::Italic(v), r.trim_start())
    } else {
        return None;
    };

    let total_group_len = braced_len(src)?;
    let content_end = total_group_len - 1;
    let content_start = src.len() - inner_src.len() + (inner_src.len() - content_src.len());
    let content = &src[content_start..content_end];
    let inlines = parse_inlines(content, autocite_mode, metadata, preserve_dynamic_markers);
    Some((vec![wrapper(inlines)], total_group_len))
}

fn braced_len(src: &str) -> Option<usize> {
    if !src.starts_with('{') {
        return None;
    }
    let mut depth = 0usize;
    for (i, ch) in src.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

fn bracketed_len(src: &str) -> Option<usize> {
    if !src.starts_with('[') {
        return None;
    }
    let mut depth = 0usize;
    for (i, ch) in src.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

fn consume_ifnum_operand(src: &str, metadata: &ParseMetadata) -> Option<(usize, Option<i64>)> {
    if src.is_empty() {
        return None;
    }

    if let Some(value_len) = parse_leading_int_len(src) {
        return Some((value_len, src[..value_len].parse::<i64>().ok()));
    }

    if src.starts_with('\\') {
        if let Some((consumed, value)) = consume_counter_command_operand(src, metadata) {
            return Some((consumed, Some(value)));
        }

        let mut consumed = 1usize;
        while consumed < src.len() && src.as_bytes()[consumed].is_ascii_alphabetic() {
            consumed += 1;
        }
        loop {
            let remaining = &src[consumed..];
            let trimmed = remaining.trim_start();
            consumed += remaining.len() - trimmed.len();

            if trimmed.starts_with('{')
                && let Some(arg_len) = braced_len(trimmed)
            {
                consumed += arg_len;
                continue;
            }
            if trimmed.starts_with('[')
                && let Some(arg_len) = bracketed_len(trimmed)
            {
                consumed += arg_len;
                continue;
            }
            break;
        }
        return Some((consumed, None));
    }

    None
}

fn consume_counter_command_operand(src: &str, metadata: &ParseMetadata) -> Option<(usize, i64)> {
    if let Some(rest) = src.strip_prefix("\\year")
        && !rest.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
    {
        let value = metadata.counters.get("year").copied()?;
        return Some(("\\year".len(), value));
    }
    for cmd in ["\\value", "\\totvalue", "\\total", "\\arabic"] {
        if let Some(rest) = src.strip_prefix(cmd) {
            let mut consumed = cmd.len();
            let trimmed = rest.trim_start();
            consumed += rest.len() - trimmed.len();
            let arg_len = braced_len(trimmed)?;
            let key = trimmed[1..arg_len - 1].trim();
            let value = metadata.counters.get(key).copied()?;
            consumed += arg_len;
            return Some((consumed, value));
        }
    }
    None
}

fn evaluate_counter_expression(expr: &str, metadata: &ParseMetadata) -> Option<i64> {
    let expr = expr.trim();
    if let Ok(value) = expr.parse::<i64>() {
        return Some(value);
    }
    if expr.starts_with('\\')
        && let Some((_, value)) = consume_counter_command_operand(expr, metadata)
    {
        return Some(value);
    }
    metadata.counters.get(expr).copied()
}

fn counter_text(metadata: &ParseMetadata, key: &str) -> Option<String> {
    if let Some(value) = metadata.text_counters.get(key) {
        return Some(value.clone());
    }
    metadata.counters.get(key).map(|value| value.to_string())
}

fn counter_marker(key: &str) -> String {
    format!("[[COUNTER:{}]]", key.trim())
}

fn formbytotal_marker(key: &str, stem: &str, singular: &str, few: &str, many: &str) -> String {
    format!(
        "[[FORMBYTOTAL:{}|{}|{}|{}|{}]]",
        key.trim(),
        stem.trim(),
        singular.trim(),
        few.trim(),
        many.trim()
    )
}

fn render_formbytotal(total: i64, stem: &str, singular: &str, few: &str, many: &str) -> String {
    let abs_total = total.abs();
    let last_two = abs_total % 100;
    let last = abs_total % 10;
    let suffix = if (11..=19).contains(&last_two) {
        many
    } else if last == 1 {
        singular
    } else if (2..=4).contains(&last) {
        few
    } else {
        many
    };
    format!("{total} {}{}", stem.trim(), suffix.trim())
}

fn parse_leading_int_len(src: &str) -> Option<usize> {
    let mut chars = src.char_indices();
    let mut end = 0usize;

    if let Some((_, '-')) = chars.next() {
        end = 1;
    } else {
        chars = src.char_indices();
    }

    let mut saw_digit = false;
    for (i, ch) in chars {
        if ch.is_ascii_digit() {
            saw_digit = true;
            end = i + ch.len_utf8();
        } else {
            break;
        }
    }

    if saw_digit { Some(end) } else { None }
}

/// Returns `true` when every inline in the slice is a `LineBreak`.
///
/// Used to suppress spurious empty `Block::Paragraph` entries produced by
/// a lone `\\` on its own paragraph-chunk (e.g. between two blank lines).
fn is_only_linebreaks(inlines: &[Inline]) -> bool {
    !inlines.is_empty() && inlines.iter().all(|i| matches!(i, Inline::LineBreak))
}

fn is_single_brace_paragraph(inlines: &[Inline]) -> bool {
    if inlines
        .iter()
        .any(|inline| !matches!(inline, Inline::Text(_)))
    {
        return false;
    }

    let text = inlines
        .iter()
        .filter_map(|inline| match inline {
            Inline::Text(value) => Some(value.as_str()),
            _ => None,
        })
        .collect::<String>();
    let trimmed = text.trim();
    trimmed == "{" || trimmed == "}"
}

fn normalize_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        if ch.is_whitespace() || ch == '~' {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out = out.replace("<<", "«").replace(">>", "»");
    out = out.replace("\"---", " —");
    out = out.replace("---", "—");
    out = out.replace(" -- ", " — ");
    out = out.replace("--", "–");
    while out.contains("  ") {
        out = out.replace("  ", " ");
    }
    out
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/unit/parser_latex_tests.rs"
    ));
}
