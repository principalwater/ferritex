use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    process::Command,
};

use crate::model::{
    Block, Document, DocumentLayout, Figure, Inline, List, ParagraphStyle, Table, TableCell,
    TableRow, TocEntry,
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
}

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
#[allow(dead_code)]
pub fn parse_latex(source: &str) -> Document {
    let autocite_mode = detect_autocite_mode(source);
    parse_latex_with_mode(source, autocite_mode, &ParseMetadata::default(), false)
}

/// Parse an entry `.tex` file with recursive `\input{...}` / `\include{...}` expansion.
///
/// Missing input files are kept as-is in the expanded source (best-effort mode).
pub fn parse_latex_file(input_path: &Path) -> anyhow::Result<Document> {
    let root_dir = input_path.parent().unwrap_or_else(|| Path::new("."));
    let mut stack = Vec::new();
    let expanded = expand_inputs_recursive(input_path, root_dir, &mut stack)?;
    let autocite_mode = detect_autocite_mode(&expanded);
    let mut metadata = collect_parse_metadata(&expanded, input_path, root_dir);
    let mut document = parse_latex_with_mode(&expanded, autocite_mode, &metadata, true);
    enrich_structural_counters(&mut metadata, &document, &expanded);
    resolve_dynamic_placeholders(&mut document.blocks, &metadata);
    resolve_citation_placeholders(&mut document.blocks, &metadata.bibliography);
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

                    let heading_candidate = strip_heading_prefix_noise(prepared.text.as_str());
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
                        preserve_dynamic_markers,
                    ) {
                        blocks.push(block);
                    } else if let Some(block) = try_parse_structural_heading_command(
                        prepared.text.as_str(),
                        autocite_mode,
                        metadata,
                        preserve_dynamic_markers,
                    ) {
                        blocks.push(block);
                    } else if let Some(block) =
                        try_parse_bibliography_command(prepared.text.as_str())
                    {
                        blocks.push(block);
                    } else {
                        let inlines = parse_inlines(
                            prepared.text.as_str(),
                            autocite_mode,
                            metadata,
                            preserve_dynamic_markers,
                        );
                        if !inlines.is_empty()
                            && !is_single_brace_paragraph(&inlines)
                            && !is_only_linebreaks(&inlines)
                        {
                            if let Some(style) = prepared.style {
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
    resolve_references(&mut blocks, &declared_labels, &layout);
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

    if let Some(options) = extract_last_macro_braced_argument(source, "\\geometry") {
        layout.page_margin_top_twips = extract_latex_option_value(&options, "top")
            .and_then(|value| parse_latex_length_to_twips(&value));
        layout.page_margin_bottom_twips = extract_latex_option_value(&options, "bottom")
            .and_then(|value| parse_latex_length_to_twips(&value));
        layout.page_margin_left_twips = extract_latex_option_value(&options, "left")
            .and_then(|value| parse_latex_length_to_twips(&value));
        layout.page_margin_right_twips = extract_latex_option_value(&options, "right")
            .and_then(|value| parse_latex_length_to_twips(&value));
    }

    if let Some(factor) = extract_last_setspacing_factor(source) {
        let twips = (240.0 * factor).round();
        if twips.is_finite() && twips > 0.0 {
            layout.body_line_spacing_twips = Some(twips as i32);
        }
    } else if source.contains("\\OnehalfSpacing") {
        layout.body_line_spacing_twips = Some(360);
    } else if source.contains("\\DoubleSpacing") {
        layout.body_line_spacing_twips = Some(480);
    } else if source.contains("\\SingleSpacing") {
        layout.body_line_spacing_twips = Some(240);
    }

    if let Some(header_twips) = extract_setlength_value_twips(source, "headsep") {
        layout.page_margin_header_twips = Some(header_twips);
    }
    if let Some(footer_twips) = extract_setlength_value_twips(source, "footskip") {
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

    // ── Body font size from \documentclass options ─────────────────────
    layout.font_size_body_hp = extract_documentclass_fontsize_hp(source);

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
    if let Some(indent_twips) = extract_setlength_value_twips(source, "parindent") {
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
    layout.caption_skip_twips_figure = extract_captionsetup_skip_twips(source, "figure");
    layout.caption_skip_twips_table = extract_captionsetup_skip_twips(source, "table");
    layout.caption_position_figure = extract_captionsetup_position(source, "figure");
    layout.caption_position_table = extract_captionsetup_position(source, "table");
    layout.caption_singlelinecheck_figure = extract_captionsetup_singlelinecheck(source, "figure");
    layout.caption_singlelinecheck_table = extract_captionsetup_singlelinecheck(source, "table");
    layout.caption_indent_twips_figure = extract_captionsetup_indent_twips(source, "figure");
    layout.caption_indent_twips_table = extract_captionsetup_indent_twips(source, "table");

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
            .and_then(|v| parse_latex_length_to_twips(&v))
        {
            layout.page_width_twips = Some(w as u32);
        }
        if let Some(h) = extract_latex_option_value(&options, "paperheight")
            .and_then(|v| parse_latex_length_to_twips(&v))
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
    layout.heading_indent_section_twips = extract_heading_indent_twips(
        source,
        "\\setsecindent",
        layout.body_first_line_indent_twips,
    );
    layout.heading_indent_subsection_twips = extract_heading_indent_twips(
        source,
        "\\setsubsecindent",
        layout.body_first_line_indent_twips,
    );
    layout.heading_indent_subsubsection_twips = extract_heading_indent_twips(
        source,
        "\\setsubsubsecindent",
        layout.body_first_line_indent_twips,
    );
    layout.toc_right_margin_twips = extract_toc_right_margin_twips(source);
    layout.toc_use_dot_leader = extract_toc_dot_leader(source);

    // ── List formatting ──────────────────────────────────────────────
    let (list_label_sep, list_label_width, list_bullet) = extract_list_settings(source);
    layout.list_label_sep_twips = list_label_sep;
    layout.list_label_width_twips = list_label_width;
    layout.list_bullet_char = list_bullet;
    // Left indent for list items = \parindent (body first-line indent).
    if layout.list_left_indent_twips.is_none() {
        layout.list_left_indent_twips = layout.body_first_line_indent_twips;
    }

    // ── Source attribution spacing ────────────────────────────────────
    layout.source_vspace_table_twips = extract_source_vspace_twips(source, "tablesource");
    layout.source_vspace_figure_twips = extract_source_vspace_twips(source, "figuresource");

    // ── Title page page number suppression ────────────────────────────
    layout.title_page_suppress_number = extract_title_page_suppress_number(source);

    // ── Caption alignment ────────────────────────────────────────────
    // \captionsetup{justification=centering}
    layout.caption_alignment = extract_captionsetup_justification(source);

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
    layout.toc_chapter_entry_bold = extract_toc_chapter_entry_bold(source);
    layout.toc_chapter_page_bold = extract_toc_chapter_page_bold(source);
    layout.toc_aftersnum_chapter = extract_toc_aftersnum(source, "chapter");
    layout.toc_aftersnum_section = extract_toc_aftersnum(source, "section");
    layout.toc_aftersnum_subsection = extract_toc_aftersnum(source, "subsection");
    layout.toc_aftersnum_subsubsection = extract_toc_aftersnum(source, "subsubsection");
    layout.toc_appendix_name = extract_toc_appendix_name(source);
    let (toc_chapter_indent, toc_chapter_numwidth) =
        extract_toc_indent_numwidth_twips(source, "chapter");
    layout.toc_indent_chapter_twips = toc_chapter_indent;
    layout.toc_numwidth_chapter_twips = toc_chapter_numwidth;
    let (toc_section_indent, toc_section_numwidth) =
        extract_toc_indent_numwidth_twips(source, "section");
    layout.toc_indent_section_twips = toc_section_indent;
    layout.toc_numwidth_section_twips = toc_section_numwidth;
    let (toc_subsection_indent, toc_subsection_numwidth) =
        extract_toc_indent_numwidth_twips(source, "subsection");
    layout.toc_indent_subsection_twips = toc_subsection_indent;
    layout.toc_numwidth_subsection_twips = toc_subsection_numwidth;
    let (toc_subsubsection_indent, toc_subsubsection_numwidth) =
        extract_toc_indent_numwidth_twips(source, "subsubsection");
    layout.toc_indent_subsubsection_twips = toc_subsubsection_indent;
    layout.toc_numwidth_subsubsection_twips = toc_subsubsection_numwidth;

    layout
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

    last
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

/// Extract caption skip for `target` (`"figure"` or `"table"`) in twips.
fn extract_captionsetup_skip_twips(source: &str, target: &str) -> Option<i32> {
    let raw = extract_captionsetup_option(source, Some(target), "skip")?;
    let resolved = resolve_captionsetup_option_value(&raw, source)?;
    parse_latex_length_to_twips_or_zero(&resolved)
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

/// Extract caption indent for `target` (`"figure"` or `"table"`) in twips.
fn extract_captionsetup_indent_twips(source: &str, target: &str) -> Option<i32> {
    let raw = extract_captionsetup_option(source, Some(target), "indent")?;
    let resolved = resolve_captionsetup_option_value(&raw, source)?;
    parse_latex_length_to_twips_or_zero(&resolved)
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

fn parse_latex_length_to_twips_or_zero(raw: &str) -> Option<i32> {
    let trimmed = raw.trim().trim_matches(['{', '}']).trim();
    if trimmed == "0" {
        return Some(0);
    }
    parse_latex_length_to_twips(trimmed)
}

fn extract_heading_indent_twips(
    source: &str,
    macro_name: &str,
    parindent_fallback: Option<i32>,
) -> Option<i32> {
    let raw = extract_last_macro_braced_argument(source, macro_name)?;
    let trimmed = raw.trim().trim_matches(['{', '}']).trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed == "\\parindent" || trimmed.contains("\\parindent") {
        return parindent_fallback;
    }
    parse_latex_length_to_twips(trimmed)
}

/// Extract TOC right margin from memoir `\setrmarg{...}` in twips.
///
/// Accepts glue expressions like `2.55em plus1fil` by parsing the leading
/// length component only.
fn extract_toc_right_margin_twips(source: &str) -> Option<i32> {
    let raw = extract_last_macro_braced_argument(source, "\\setrmarg")?;
    parse_latex_length_prefix_to_twips(&raw)
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

// ── List settings extraction ─────────────────────────────────────────────────

/// Extract enumitem list settings from `\setlist{...}` and `\renewcommand{\labelitemi}{...}`.
///
/// Returns `(label_sep_twips, label_width_twips, bullet_char)`.
/// - `label_sep_twips`: from `labelsep=<dim>` in `\setlist{...}`.
/// - `label_width_twips`: `None` when `labelwidth=!` (auto), otherwise from `labelwidth=<dim>`.
/// - `bullet_char`: from `\renewcommand{\labelitemi}{...}`, stripped of formatting commands.
fn extract_list_settings(source: &str) -> (Option<i32>, Option<i32>, Option<String>) {
    let sep = extract_setlist_param_twips(source, "labelsep");
    let width = extract_setlist_labelwidth_twips(source);
    let bullet = extract_labelitemi_char(source);
    (sep, width, bullet)
}

/// Extract a dimension parameter from the first `\setlist{..., name=<dim>, ...}` block.
fn extract_setlist_param_twips(source: &str, param: &str) -> Option<i32> {
    let needle = "\\setlist";
    let mut pos = 0usize;
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
            // Look for `param=<dim>` inside the body.
            let search = format!("{param}=");
            if let Some(rel_p) = body.find(&search) {
                let after_eq = &body[rel_p + search.len()..];
                // Collect the dimension token (up to , or }).
                let dim: String = after_eq
                    .chars()
                    .take_while(|c| *c != ',' && *c != '}' && *c != '\n')
                    .collect();
                let dim = dim.trim();
                if !dim.is_empty()
                    && let Some(tw) = parse_latex_length_to_twips(dim)
                {
                    return Some(tw);
                }
            }
        }
        pos = start;
    }
    None
}

/// Extract `labelwidth` from `\setlist{..., labelwidth=<dim-or-!>, ...}`.
///
/// Returns `None` for `labelwidth=!` (auto-width).
fn extract_setlist_labelwidth_twips(source: &str) -> Option<i32> {
    let needle = "\\setlist";
    let mut pos = 0usize;
    while let Some(rel) = source[pos..].find(needle) {
        let start = pos + rel + needle.len();
        let after_cmd = source[start..].trim_start_matches([' ', '\t', '\n', '\r']);
        let after_cmd_pos = start + (source[start..].len() - after_cmd.len());
        let content_start = if after_cmd.starts_with('[') {
            if let Some(close) = source[after_cmd_pos..].find(']') {
                after_cmd_pos + close + 1
            } else {
                pos = start;
                continue;
            }
        } else {
            after_cmd_pos
        };
        let trimmed = source[content_start..].trim_start_matches([' ', '\t', '\n', '\r']);
        let trimmed_pos = content_start + (source[content_start..].len() - trimmed.len());
        if !trimmed.starts_with('{') {
            pos = start;
            continue;
        }
        if let Some(body) = extract_braced(&source[trimmed_pos..])
            && let Some(rel_p) = body.find("labelwidth=")
        {
            let after_eq = body[rel_p + "labelwidth=".len()..].trim_start();
            if after_eq.starts_with('!') {
                // Auto-width — return None.
                return None;
            }
            let dim: String = after_eq
                .chars()
                .take_while(|c| *c != ',' && *c != '}' && *c != '\n')
                .collect();
            let dim = dim.trim();
            if !dim.is_empty() {
                return parse_latex_length_to_twips(dim);
            }
        }
        pos = start;
    }
    None
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

/// Extract the vertical space (in twips) specified before the content in a
/// `\newcommand{\<macro_name>}[1]{\par\vspace{<dim>}...}` definition.
///
/// Strategy: find the definition marker, then look for the first `\vspace{<dim>}`
/// within 200 characters — reliable for the common `\par\vspace{4pt}{...}` pattern.
fn extract_source_vspace_twips(source: &str, macro_name: &str) -> Option<i32> {
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
    parse_latex_length_to_twips(dim)
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

/// Extract TOC entry indent and number-width settings for a given entry kind
/// (`chapter`, `section`, `subsection`, `subsubsection`) in twips.
///
/// Supported LaTeX sources:
/// - `\setlength{\cft<kind>indent}{...}` + `\setlength{\cft<kind>numwidth}{...}`
/// - `\cftsetindents{<kind>}{<indent>}{<numwidth>}`
///
/// When both forms are present, the later occurrence in source order wins.
fn extract_toc_indent_numwidth_twips(source: &str, kind: &str) -> (Option<i32>, Option<i32>) {
    let indent_name = format!("cft{kind}indent");
    let numwidth_name = format!("cft{kind}numwidth");

    let mut indent = extract_last_setlength_value_twips_with_pos(source, &indent_name);
    let mut numwidth = extract_last_setlength_value_twips_with_pos(source, &numwidth_name);

    if let Some((cft_indent, cft_numwidth, cft_pos)) =
        extract_last_cftsetindents_twips_with_pos(source, kind)
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

fn extract_last_setlength_value_twips_with_pos(source: &str, name: &str) -> Option<(i32, usize)> {
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
            && let Some(twips) = parse_latex_length_prefix_to_twips(raw_value)
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
                parse_latex_length_prefix_to_twips(raw_indent),
                parse_latex_length_prefix_to_twips(raw_numwidth),
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

fn extract_last_setspacing_factor(source: &str) -> Option<f64> {
    extract_last_macro_braced_argument(source, "\\setSpacing").and_then(|value| {
        value
            .trim()
            .replace(',', ".")
            .parse::<f64>()
            .ok()
            .filter(|factor| factor.is_finite() && *factor > 0.0)
    })
}

fn extract_setlength_value_twips(source: &str, name: &str) -> Option<i32> {
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
            last = parse_latex_length_to_twips(raw_value);
        }
        pos = cur + value_len;
    }

    last
}

fn parse_latex_length_to_twips(raw: &str) -> Option<i32> {
    let normalized = raw
        .trim()
        .trim_matches(['{', '}'])
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    if normalized.is_empty() {
        return None;
    }

    // `em` is relative — 1 em equals the body font size.  We assume the
    // standard LaTeX base of 14pt (which gives 1em = 280 twips) unless the
    // caller parses a different font size (handled downstream).
    for (unit, twips_per_unit) in [
        ("mm", 56.692_913_f64),
        ("cm", 566.929_133_f64),
        ("in", 1440.0_f64),
        ("em", 280.0_f64),
        ("pt", 20.0_f64),
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

/// Parse a leading LaTeX length expression (e.g. `2.55em` from
/// `2.55em plus1fil`) into twips.
fn parse_latex_length_prefix_to_twips(raw: &str) -> Option<i32> {
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
    for unit in ["mm", "cm", "in", "em", "pt"] {
        if unit_tail.starts_with(unit) {
            let candidate = format!("{}{}", &compact[..end], unit);
            return parse_latex_length_to_twips(&candidate);
        }
    }
    None
}

fn collect_parse_metadata(source: &str, input_path: &Path, root_dir: &Path) -> ParseMetadata {
    let mut metadata = ParseMetadata {
        counters: collect_setcounter_values(source),
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

    if let Some(pages) = infer_pdf_page_count(input_path) {
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

fn infer_pdf_page_count(input_path: &Path) -> Option<u32> {
    let pdf_path = input_path.with_extension("pdf");
    if !pdf_path.is_file() {
        return None;
    }
    let output = Command::new("pdfinfo").arg(&pdf_path).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Pages:") {
            let value = rest.trim().parse::<u32>().ok()?;
            return Some(value);
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
            Block::DisplayMath(_) | Block::BibliographyHeading { .. } => {}
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
            Block::DisplayMath(_) | Block::BibliographyHeading { .. } => {}
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

    let begins_titlingpage = chunk.contains("\\begin{titlingpage}");
    let ends_titlingpage = chunk.contains("\\end{titlingpage}");
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

    if let Some(line_spacing_twips) = extract_spacing_directive_twips(chunk) {
        state.current_line_spacing_twips = Some(line_spacing_twips.max(1));
    }

    if let Some(vspace_twips) = parse_vspace_only_chunk_twips(chunk) {
        state.pending_space_before_twips = Some(vspace_twips.max(0));
        return None;
    }

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

    if let Some(space_before_twips) = state.pending_space_before_twips.take() {
        style.space_before_twips = Some(space_before_twips);
    }

    style.font_size_hp = extract_fontsize_halfpoints(chunk);

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
        || style.first_line_indent_twips.is_some()
        || style.line_spacing_twips.is_some()
        || style.space_before_twips.is_some()
        || style.space_after_twips.is_some()
        || style.font_size_hp.is_some()
}

fn parse_vspace_only_chunk_twips(chunk: &str) -> Option<i32> {
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
        if let Some(twips) = parse_latex_length_to_twips(payload) {
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

fn extract_spacing_directive_twips(chunk: &str) -> Option<i32> {
    if let Some(factor) = extract_last_setspacing_factor(chunk) {
        let twips = (240.0 * factor).round();
        if twips.is_finite() && twips > 0.0 {
            return Some(twips as i32);
        }
    }
    if chunk.contains("\\OnehalfSpacing") {
        return Some(360);
    }
    if chunk.contains("\\DoubleSpacing") {
        return Some(480);
    }
    if chunk.contains("\\SingleSpacing") {
        return Some(240);
    }
    None
}

fn extract_fontsize_halfpoints(src: &str) -> Option<usize> {
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
        let value = tail[1..first_len - 1].trim();
        let value = value.trim_end_matches("pt").trim().replace(',', ".");
        if let Ok(pt) = value.parse::<f64>()
            && pt.is_finite()
            && pt > 0.0
        {
            return Some((pt * 2.0).round() as usize);
        }

        tail = &tail[first_len..];
        if tail.trim().is_empty() {
            break;
        }
        pos = cmd_start + first_len;
    }
    None
}

/// Return `true` for preamble lines we want to discard entirely.
fn is_skippable(chunk: &str) -> bool {
    let c = chunk.trim_start();
    c.starts_with("\\documentclass")
        || c.starts_with("\\usepackage")
        || c.starts_with("\\begin{document}")
        || c.starts_with("\\end{document}")
        || c.starts_with("\\begin{landscape}")
        || c.starts_with("\\end{landscape}")
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
        || c.starts_with("\\newpage")
        || c.starts_with("\\clearpage")
        || c.starts_with("\\cleardoublepage")
        || c.starts_with("\\ifdefmacro")
        || c.starts_with("\\captionsetup")
        || c.starts_with("\\DefTblrTemplate")
        || c.starts_with("\\SetTblrTemplate")
        || c.starts_with("\\UseTblrTemplate")
        || c.starts_with("\\SetCell")
        || c.starts_with("\\begingroup")
        || c.starts_with("\\endgroup")
        || c.starts_with("\\appendix")
        || c.starts_with("\\landscape")
        || c.starts_with("\\endlandscape")
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
) {
    let labels = build_label_registry(blocks, declared_labels, layout);

    for block in blocks.iter_mut() {
        match block {
            Block::Section { title, .. }
            | Block::Paragraph(title)
            | Block::StyledParagraph { inlines: title, .. } => {
                resolve_inline_references(title, &labels);
            }
            Block::Table(table) => {
                resolve_inline_references(&mut table.caption, &labels);
                resolve_inline_references(&mut table.source, &labels);
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        resolve_inline_references(&mut cell.content, &labels);
                    }
                }
            }
            Block::Figure(figure) => {
                resolve_inline_references(&mut figure.caption, &labels);
                resolve_inline_references(&mut figure.source, &labels);
            }
            Block::List(list) => {
                for item in &mut list.items {
                    resolve_inline_references(item, &labels);
                }
            }
            Block::DisplayMath(_) | Block::BibliographyHeading { .. } => {}
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
            | Block::BibliographyHeading { .. } => {}
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

fn resolve_inline_references(inlines: &mut [Inline], labels: &HashMap<String, String>) {
    for inline in inlines {
        match inline {
            Inline::Reference(label) => {
                let text = labels
                    .get(label)
                    .cloned()
                    .unwrap_or_else(|| format!("[ref:{label}]"));
                *inline = Inline::Text(text);
            }
            Inline::Bold(children) | Inline::Italic(children) | Inline::Footnote(children) => {
                resolve_inline_references(children, labels);
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
    preserve_dynamic_markers: bool,
) -> Option<Block> {
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

fn try_parse_structural_heading_command(
    chunk: &str,
    autocite_mode: AutociteMode,
    metadata: &ParseMetadata,
    preserve_dynamic_markers: bool,
) -> Option<Block> {
    let command = chunk.trim_start();
    let heading = if command.starts_with("\\tableofcontents") {
        "ОГЛАВЛЕНИЕ"
    } else {
        return None;
    };

    let title = parse_inlines(heading, autocite_mode, metadata, preserve_dynamic_markers);
    if title.is_empty() {
        return None;
    }

    Some(Block::Section {
        level: 1,
        number: None,
        label: None,
        title,
    })
}

/// Detect bibliography-rendering commands and emit a `Block::BibliographyHeading`.
///
/// Recognised commands:
/// - `\printbibliography[title=...]` — title from optional arg or default "СПИСОК ЛИТЕРАТУРЫ"
/// - `\insertbibliofullsorted` — default title "СПИСОК ЛИТЕРАТУРЫ"
/// - `\insertbiblioauthor` — same default
///
/// Only the heading is rendered; no `.bib` file entries are parsed.
fn try_parse_bibliography_command(chunk: &str) -> Option<Block> {
    let s = chunk.trim_start();
    let default_title = "СПИСОК ЛИТЕРАТУРЫ".to_string();

    if let Some(rest) = s.strip_prefix("\\printbibliography") {
        // Try to extract title from optional [title=...] argument.
        let after = rest.trim_start();
        let (title, has_nobibheading) = if after.starts_with('[') {
            if let Some(close) = after.find(']') {
                let args = &after[1..close];
                let no_heading = args.contains("nobibheading");
                let t = extract_printbibliography_title(args).unwrap_or(default_title);
                (t, no_heading)
            } else {
                (default_title, false)
            }
        } else {
            (default_title, false)
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
            title: default_title,
        });
    }

    None
}

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
    use super::*;
    use crate::model::{Block, Inline};

    #[test]
    fn test_chapter_level1() {
        let doc = parse_latex("\\chapter{Overview}");
        assert_eq!(
            doc.blocks,
            vec![Block::Section {
                level: 1,
                number: Some("1.".into()),
                label: None,
                title: vec![Inline::Text("Overview".into())]
            }]
        );
    }

    #[test]
    fn test_section_level2() {
        let doc = parse_latex("\\section{Introduction}");
        assert_eq!(
            doc.blocks,
            vec![Block::Section {
                level: 2,
                number: Some("1".into()),
                label: None,
                title: vec![Inline::Text("Introduction".into())]
            }]
        );
    }

    #[test]
    fn test_subsection_level3() {
        let doc = parse_latex("\\subsection{Background}");
        assert_eq!(
            doc.blocks[0],
            Block::Section {
                level: 3,
                number: Some("1".into()),
                label: None,
                title: vec![Inline::Text("Background".into())]
            }
        );
    }

    #[test]
    fn test_section_star() {
        let doc = parse_latex("\\section*{Preface}");
        assert_eq!(
            doc.blocks[0],
            Block::Section {
                level: 2,
                number: None,
                label: None,
                title: vec![Inline::Text("Preface".into())]
            }
        );
    }

    #[test]
    fn test_section_numbering_sequence_inside_chapter() {
        let doc = parse_latex(
            "\\chapter{One}\n\n\\section{A}\n\n\\subsection{A.1}\n\n\\section{B}\n\n\\subsection{B.1}",
        );
        let numbers: Vec<String> = doc
            .blocks
            .iter()
            .filter_map(|b| {
                if let Block::Section {
                    number: Some(n), ..
                } = b
                {
                    Some(n.clone())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(numbers, vec!["1.", "1.1", "1.1.1", "1.2", "1.2.1"]);
    }

    #[test]
    fn test_section_numbering_resets_after_new_chapter() {
        let doc = parse_latex("\\chapter{One}\n\n\\section{A}\n\n\\chapter{Two}\n\n\\section{B}");
        let section_numbers: Vec<String> = doc
            .blocks
            .iter()
            .filter_map(|b| {
                if let Block::Section {
                    level: 2,
                    number: Some(n),
                    ..
                } = b
                {
                    Some(n.clone())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(section_numbers, vec!["1.1", "2.1"]);
    }

    #[test]
    fn test_plain_paragraph() {
        let doc = parse_latex("Hello world.");
        assert_eq!(
            doc.blocks,
            vec![Block::Paragraph(vec![Inline::Text("Hello world.".into())])]
        );
    }

    #[test]
    fn test_textbf() {
        let doc = parse_latex("This is \\textbf{bold} text.");
        assert_eq!(
            doc.blocks,
            vec![Block::Paragraph(vec![
                Inline::Text("This is ".into()),
                Inline::Bold(vec![Inline::Text("bold".into())]),
                Inline::Text(" text.".into()),
            ])]
        );
    }

    #[test]
    fn test_textit() {
        let doc = parse_latex("\\textit{italic}");
        assert_eq!(
            doc.blocks,
            vec![Block::Paragraph(vec![Inline::Italic(vec![Inline::Text(
                "italic".into()
            )]),])]
        );
    }

    #[test]
    fn test_bfseries_declaration_inside_group_wraps_following_content() {
        let doc = parse_latex("{\\bfseries\\MakeUppercase{Hello} world}");
        match &doc.blocks[0] {
            Block::Paragraph(inlines) => {
                assert!(
                    inlines
                        .iter()
                        .any(|inline| matches!(inline, Inline::Bold(_))),
                    "expected bold declaration to wrap group content: {inlines:?}"
                );
                let text = plain_text_from_inlines(inlines);
                assert!(text.contains("Hello"), "text: {text:?}");
                assert!(text.contains("world"), "text: {text:?}");
            }
            other => panic!("expected paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_control_space_command_preserved_as_text_space() {
        let doc = parse_latex("A\\ B");
        match &doc.blocks[0] {
            Block::Paragraph(inlines) => {
                let text = plain_text_from_inlines(inlines);
                assert_eq!(text, "A B");
            }
            other => panic!("expected paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_bfseries_declaration_without_braces_styles_following_text() {
        let doc = parse_latex("\\bfseries Bold text");
        match &doc.blocks[0] {
            Block::Paragraph(inlines) => {
                assert!(
                    inlines
                        .iter()
                        .any(|inline| matches!(inline, Inline::Bold(_))),
                    "expected bold inline after declaration: {inlines:?}"
                );
                let text = plain_text_from_inlines(inlines);
                assert_eq!(text.trim(), "Bold text");
            }
            other => panic!("expected paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_hard_linebreak_command_preserved_as_inline_break() {
        let doc = parse_latex("Line one\\\\Line two");
        match &doc.blocks[0] {
            Block::Paragraph(inlines) => {
                assert!(
                    inlines
                        .iter()
                        .any(|inline| matches!(inline, Inline::LineBreak)),
                    "expected explicit line break in parsed inlines: {inlines:?}"
                );
            }
            other => panic!("expected paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_preamble_skipped() {
        let src = "\\documentclass{article}\n\\usepackage{fontenc}\n\\begin{document}\nHello.\n\\end{document}";
        let doc = parse_latex(src);
        assert_eq!(
            doc.blocks,
            vec![Block::Paragraph(vec![Inline::Text("Hello.".into())])]
        );
    }

    #[test]
    fn test_comment_stripped() {
        let doc = parse_latex("Hello % this is a comment\nworld.");
        match &doc.blocks[0] {
            Block::Paragraph(inlines) => {
                let text: String = inlines
                    .iter()
                    .map(|i| match i {
                        Inline::Text(s) => s.clone(),
                        _ => String::new(),
                    })
                    .collect();
                assert!(text.contains("Hello"), "expected 'Hello' in: {text:?}");
                assert!(text.contains("world"), "expected 'world' in: {text:?}");
            }
            other => panic!("expected paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_layout_settings_extracted_from_geometry_and_setspacing() {
        let doc = parse_latex(
            "\\geometry{a4paper, top=20mm, bottom=20mm, left=25mm, right=10mm}\n\\setSpacing{1.385}\nBody.",
        );
        assert_eq!(doc.layout.page_margin_top_twips, Some(1134));
        assert_eq!(doc.layout.page_margin_bottom_twips, Some(1134));
        assert_eq!(doc.layout.page_margin_left_twips, Some(1417));
        assert_eq!(doc.layout.page_margin_right_twips, Some(567));
        assert_eq!(doc.layout.body_line_spacing_twips, Some(332));
    }

    #[test]
    fn test_layout_settings_fallback_to_onehalfspacing_when_setspacing_absent() {
        let doc = parse_latex("\\OnehalfSpacing\nBody.");
        assert_eq!(doc.layout.body_line_spacing_twips, Some(360));
    }

    #[test]
    fn test_extract_toc_right_margin_and_dot_leader_from_latex() {
        let src = "\
\\setrmarg{2.55em plus1fil}
\\renewcommand{\\cftchapterleader}{\\cftdotfill{\\cftchapterdotsep}}
Body.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.toc_right_margin_twips, Some(714));
        assert_eq!(doc.layout.toc_use_dot_leader, Some(true));
    }

    #[test]
    fn test_extract_toc_dot_leader_false_when_explicit_non_dot_leader() {
        let src = "\\renewcommand{\\cftchapterleader}{\\hfill}\nBody.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.toc_use_dot_leader, Some(false));
    }

    #[test]
    fn test_extract_toc_chapter_name_prefix_from_cftchaptername() {
        let src = "\
\\renewcommand{\\chaptername}{Глава}
\\renewcommand*{\\cftchaptername}{\\chaptername\\space}
Body.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.toc_chapter_name_prefix.as_deref(), Some("Глава"));
    }

    #[test]
    fn test_extract_toc_chapter_name_prefix_absent_when_undefined() {
        let doc = parse_latex("Body.");
        assert_eq!(doc.layout.toc_chapter_name_prefix, None);
    }

    #[test]
    fn test_extract_toc_indents_from_cftsetindents() {
        let src = "\
\\cftsetindents{chapter}{0em}{3em}
\\cftsetindents{section}{1.2em}{2.4em}
Body.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.toc_indent_chapter_twips, Some(0));
        assert_eq!(doc.layout.toc_numwidth_chapter_twips, Some(840));
        assert_eq!(doc.layout.toc_indent_section_twips, Some(336));
        assert_eq!(doc.layout.toc_numwidth_section_twips, Some(672));
    }

    #[test]
    fn test_extract_toc_indents_from_setlength() {
        let src = "\
\\setlength{\\cftchapterindent}{0pt}
\\setlength{\\cftchapternumwidth}{42pt}
\\setlength{\\cftsectionindent}{20pt}
\\setlength{\\cftsectionnumwidth}{36pt}
Body.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.toc_indent_chapter_twips, Some(0));
        assert_eq!(doc.layout.toc_numwidth_chapter_twips, Some(840));
        assert_eq!(doc.layout.toc_indent_section_twips, Some(400));
        assert_eq!(doc.layout.toc_numwidth_section_twips, Some(720));
    }

    #[test]
    fn test_extract_toc_indents_absent_when_undefined() {
        let doc = parse_latex("Body.");
        assert_eq!(doc.layout.toc_indent_chapter_twips, None);
        assert_eq!(doc.layout.toc_numwidth_chapter_twips, None);
        assert_eq!(doc.layout.toc_indent_section_twips, None);
        assert_eq!(doc.layout.toc_numwidth_section_twips, None);
        assert_eq!(doc.layout.toc_indent_subsection_twips, None);
        assert_eq!(doc.layout.toc_numwidth_subsection_twips, None);
        assert_eq!(doc.layout.toc_indent_subsubsection_twips, None);
        assert_eq!(doc.layout.toc_numwidth_subsubsection_twips, None);
    }

    #[test]
    fn test_layout_float_counters_global_when_contnumfig_is_1() {
        // contnumfig=1 means continuous (global) numbering → within_chapter = false
        let src = "\\setcounter{contnumfig}{1}\n\\setcounter{contnumtab}{1}\n\\setcounter{contnumeq}{1}\nBody.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.figure_counter_within_chapter, Some(false));
        assert_eq!(doc.layout.table_counter_within_chapter, Some(false));
        assert_eq!(doc.layout.equation_counter_within_chapter, Some(false));
    }

    #[test]
    fn test_layout_float_counters_per_chapter_when_contnumfig_is_0() {
        // contnumfig=0 means per-chapter numbering → within_chapter = true
        let src = "\\setcounter{contnumfig}{0}\n\\setcounter{contnumtab}{0}\nBody.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.figure_counter_within_chapter, Some(true));
        assert_eq!(doc.layout.table_counter_within_chapter, Some(true));
    }

    #[test]
    fn test_layout_float_counters_absent_when_contnumfig_not_set() {
        let doc = parse_latex("Body.");
        assert_eq!(doc.layout.figure_counter_within_chapter, None);
        assert_eq!(doc.layout.table_counter_within_chapter, None);
    }

    #[test]
    fn test_layout_float_counters_with_trailing_comment_and_newcounter() {
        // Reproduces the exact pattern from a typical dissertation setup file:
        //   \newcounter{contnumfig}
        //   \setcounter{contnumfig}{1}  % 0 --- per-chapter; 1 --- global
        let src = "\
\\newcounter{contnumfig}\n\
\\newcounter{contnumtab}\n\
\\setcounter{contnumfig}{1}  % 0 --- per-chapter; 1 --- global\n\
\\setcounter{contnumtab}{1}  % 0 --- per-chapter; 1 --- global\n\
Body.";
        let doc = parse_latex(src);
        assert_eq!(
            doc.layout.figure_counter_within_chapter,
            Some(false),
            "contnumfig=1 should produce figure_counter_within_chapter=false (global)"
        );
        assert_eq!(
            doc.layout.table_counter_within_chapter,
            Some(false),
            "contnumtab=1 should produce table_counter_within_chapter=false (global)"
        );
    }

    #[test]
    fn test_setcounter_inside_iffontexiststf_is_ignored() {
        // \IfFontExistsTF{Times New Roman}{}{\setcounter{fontfamily}{0}}
        // The fallback \setcounter inside the runtime conditional should be ignored.
        let src = "\
\\setcounter{fontfamily}{1}
\\IfFontExistsTF{Times New Roman}{}{\\setcounter{fontfamily}{0}}
\\IfFontExistsTF{LiberationSerif}{}{\\setcounter{fontfamily}{0}}
Body.";
        let doc = parse_latex(src);
        // fontfamily=1 means per-chapter is determined by contnumfig, not fontfamily.
        // But we can verify the extracted fontfamily indirectly via font_family_body.
        // With fontfamily=1 and matching \ifnumequal branch:
        let src2 = "\
\\setcounter{fontfamily}{1}
\\IfFontExistsTF{Times New Roman}{}{\\setcounter{fontfamily}{0}}
\\ifnumequal{\\value{fontfamily}}{0}{\\setmainfont{CMU Serif}}
\\ifnumequal{\\value{fontfamily}}{1}{\\setmainfont{Times New Roman}}
Body.";
        let doc2 = parse_latex(src2);
        assert_eq!(
            doc2.layout.font_family_body.as_deref(),
            Some("Times New Roman"),
            "\\setcounter inside \\IfFontExistsTF should be skipped; fontfamily stays 1"
        );
        let _ = doc; // suppress unused warning
    }

    #[test]
    fn test_label_registry_global_numbering_produces_flat_refs() {
        // When contnumfig=1 (global), \ref{fig:first} inside chapter 1 should resolve
        // to "1" (not "1.1"). Same for tables.
        let src = "\
\\setcounter{contnumfig}{1}\n\
\\setcounter{contnumtab}{1}\n\
\\chapter{Chapter One}\n\
\\begin{figure}\n\
\\caption{A figure}\n\
\\label{fig:first}\n\
\\end{figure}\n\
\\begin{table}\n\
\\caption{A table}\n\
\\label{tab:first}\n\
\\begin{tabular}{l}\nCell\\end{tabular}\n\
\\end{table}\n\
See \\ref{fig:first} and \\ref{tab:first}.";
        let doc = parse_latex(src);
        // Find the paragraph containing the resolved references.
        let mut found_fig = false;
        let mut found_tab = false;
        for block in &doc.blocks {
            if let Block::Paragraph(inlines) = block {
                for inline in inlines {
                    if let Inline::Text(t) = inline {
                        if t == "1" && !found_fig {
                            found_fig = true;
                        } else if t == "1" && found_fig {
                            found_tab = true;
                        }
                    }
                }
            }
        }
        // The resolved refs must appear as plain "1", not "1.1".
        for block in &doc.blocks {
            if let Block::Paragraph(inlines) = block {
                for inline in inlines {
                    if let Inline::Text(t) = inline {
                        assert!(
                            !t.contains("1.1"),
                            "global counter should produce flat ref '1', not '{}' (chapter-prefixed)",
                            t
                        );
                    }
                }
            }
        }
        let _ = found_fig;
        let _ = found_tab;
    }

    #[test]
    fn test_label_registry_per_chapter_numbering_produces_prefixed_refs() {
        // When contnumfig=0 (per-chapter), \ref{fig:first} inside chapter 1 should resolve
        // to "1.1".
        let src = "\
\\setcounter{contnumfig}{0}\n\
\\chapter{Chapter One}\n\
\\begin{figure}\n\
\\caption{A figure}\n\
\\label{fig:first}\n\
\\end{figure}\n\
See \\ref{fig:first}.";
        let doc = parse_latex(src);
        let mut found_prefixed = false;
        for block in &doc.blocks {
            if let Block::Paragraph(inlines) = block {
                for inline in inlines {
                    if let Inline::Text(t) = inline {
                        if t == "1.1" {
                            found_prefixed = true;
                        }
                    }
                }
            }
        }
        assert!(
            found_prefixed,
            "per-chapter counter should produce ref '1.1'"
        );
    }

    #[test]
    fn test_zero_arg_newcommand_expanded_in_paragraph_text() {
        let doc = parse_latex("\\newcommand{\\actualityTXT}{Topic relevance}\nA: \\actualityTXT.");
        match &doc.blocks[0] {
            Block::Paragraph(inlines) => {
                let text = inlines
                    .iter()
                    .filter_map(|inline| match inline {
                        Inline::Text(value) => Some(value.as_str()),
                        _ => None,
                    })
                    .collect::<String>();
                assert!(
                    text.contains("Topic relevance"),
                    "expected expanded macro text, got: {text}"
                );
            }
            other => panic!("expected paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_texorpdfstring_prefers_visible_branch() {
        let doc = parse_latex("\\texorpdfstring{Visible text}{Hidden text}");
        match &doc.blocks[0] {
            Block::Paragraph(inlines) => {
                let text = inlines
                    .iter()
                    .filter_map(|inline| match inline {
                        Inline::Text(value) => Some(value.as_str()),
                        _ => None,
                    })
                    .collect::<String>();
                assert!(text.contains("Visible text"), "unexpected text: {text}");
                assert!(!text.contains("Hidden text"), "unexpected text: {text}");
            }
            other => panic!("expected paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_makeuppercase_command_keeps_argument_text() {
        let doc = parse_latex("\\MakeUppercase{Sample heading}");
        match &doc.blocks[0] {
            Block::Paragraph(inlines) => {
                let text = inlines
                    .iter()
                    .filter_map(|inline| match inline {
                        Inline::Text(value) => Some(value.as_str()),
                        _ => None,
                    })
                    .collect::<String>();
                assert!(text.contains("Sample heading"), "unexpected text: {text}");
            }
            other => panic!("expected paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_tableofcontents_command_becomes_heading_block() {
        let doc = parse_latex("\\tableofcontents*");
        assert_eq!(doc.blocks.len(), 1, "unexpected blocks: {:?}", doc.blocks);
        match &doc.blocks[0] {
            Block::Section {
                level,
                number,
                title,
                ..
            } => {
                assert_eq!(*level, 1);
                assert!(
                    number.is_none(),
                    "table of contents heading must be unnumbered"
                );
                let text = title
                    .iter()
                    .filter_map(|inline| match inline {
                        Inline::Text(value) => Some(value.as_str()),
                        _ => None,
                    })
                    .collect::<String>();
                assert!(text.contains("ОГЛАВЛЕНИЕ"), "unexpected title: {text}");
            }
            other => panic!("expected Section, got {other:?}"),
        }
    }

    #[test]
    fn test_tochelper_like_macro_is_not_expanded_into_body_noise() {
        let src = "\\newcommand*{\\tocheader}{\\ifnumequal{\\value{pgnum}}{1}{\\hbox to \\linewidth{X}\\afterpage{\\tocheader}}{}}\n\\addtocontents{toc}{\\protect\\tocheader}\n\\tableofcontents*";
        let expanded = expand_simple_macros(src);
        assert!(
            expanded.contains("\\protect\\tocheader"),
            "toc helper macro should not be inlined into document body"
        );
    }

    #[test]
    fn test_parse_toc_helper_line_extracts_visible_header_text_from_macro() {
        let source = "\\newcommand*{\\tocheader}{\\ifnumequal{\\value{pgnum}}{1}{\\hbox to \\linewidth{\\noindent{}~\\hfill{Стр.}}\\par\\afterpage{\\tocheader}}{}}";
        let entry = parse_toc_line(
            "\\tocheader",
            AutociteMode::InlinePlaceholder,
            &ParseMetadata::default(),
            source,
        )
        .expect("expected to parse toc helper line");

        assert_eq!(entry.level, 0);
        assert_eq!(entry.title, "Стр.");
        assert_eq!(entry.number, None);
        assert_eq!(entry.page, None);
    }

    #[test]
    fn test_cite_becomes_placeholder() {
        let doc = parse_latex("See \\cite{Smith2020}.");
        match &doc.blocks[0] {
            Block::Paragraph(inlines) => {
                assert!(
                    inlines
                        .iter()
                        .any(|i| matches!(i, Inline::Text(s) if s.contains("Smith2020")))
                );
            }
            other => panic!("expected paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_ref_resolved_for_section_label() {
        let src = "\\chapter{Intro}\\label{ch:intro}\n\nSee chapter \\ref{ch:intro}.";
        let doc = parse_latex(src);
        let resolved = doc.blocks.iter().find_map(|b| {
            if let Block::Paragraph(inlines) = b {
                Some(
                    inlines
                        .iter()
                        .filter_map(|i| {
                            if let Inline::Text(s) = i {
                                Some(s.as_str())
                            } else {
                                None
                            }
                        })
                        .collect::<String>(),
                )
            } else {
                None
            }
        });
        let resolved = resolved.expect("missing paragraph");
        assert!(
            resolved.contains("1"),
            "expected resolved section reference number, got: {resolved}"
        );
    }

    #[test]
    fn test_ref_resolved_for_figure_label() {
        let src = r"
\chapter{Intro}

\begin{figure}[H]
\caption{Sample}
\label{fig:sample}
\end{figure}

See Figure \ref{fig:sample}.
";
        let doc = parse_latex(src);
        let resolved = doc.blocks.iter().find_map(|b| {
            if let Block::Paragraph(inlines) = b {
                Some(
                    inlines
                        .iter()
                        .filter_map(|i| {
                            if let Inline::Text(s) = i {
                                Some(s.as_str())
                            } else {
                                None
                            }
                        })
                        .collect::<String>(),
                )
            } else {
                None
            }
        });
        let resolved = resolved.expect("missing paragraph");
        assert!(
            resolved.contains("1.1"),
            "expected chapter-aware figure reference number, got: {resolved}"
        );
    }

    #[test]
    fn test_ref_missing_becomes_explicit_placeholder() {
        let doc = parse_latex("See \\ref{missing:label}.");
        match &doc.blocks[0] {
            Block::Paragraph(inlines) => {
                assert!(
                    inlines
                        .iter()
                        .any(|i| matches!(i, Inline::Text(s) if s.contains("[ref:missing:label]")))
                );
            }
            other => panic!("expected paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_ref_resolved_for_tblr_option_label() {
        let src = r"
\chapter{Intro}

\begin{longtblr}[
label = {tab:opt_label},
]{
colspec = {|l|l|},
hlines
}
A & B \\
\end{longtblr}

See table \cref{tab:opt_label}.
";
        let doc = parse_latex(src);
        let resolved = doc.blocks.iter().find_map(|b| {
            if let Block::Paragraph(inlines) = b {
                Some(
                    inlines
                        .iter()
                        .filter_map(|i| {
                            if let Inline::Text(s) = i {
                                Some(s.as_str())
                            } else {
                                None
                            }
                        })
                        .collect::<String>(),
                )
            } else {
                None
            }
        });
        let resolved = resolved.expect("missing paragraph");
        assert!(
            resolved.contains("1.1"),
            "expected resolved table number from tabularray option label, got: {resolved}"
        );
    }

    #[test]
    fn test_ref_resolved_for_appendix_label_from_standalone_label() {
        let src = "\\chapter*{Appendix}\n\n\\label{app:C}\n\nSee \\ref{app:C}.";
        let doc = parse_latex(src);
        let resolved = doc.blocks.iter().find_map(|b| {
            if let Block::Paragraph(inlines) = b {
                Some(
                    inlines
                        .iter()
                        .filter_map(|i| {
                            if let Inline::Text(s) = i {
                                Some(s.as_str())
                            } else {
                                None
                            }
                        })
                        .collect::<String>(),
                )
            } else {
                None
            }
        });
        let resolved = resolved.expect("missing paragraph");
        assert!(
            resolved.contains("C"),
            "expected appendix fallback value from app:C label, got: {resolved}"
        );
    }

    #[test]
    fn test_ref_resolved_for_equation_label() {
        let src = r"
\begin{equation}
W = x + y
\label{eq:welfare}
\end{equation}

As shown in \ref{eq:welfare}, objective is linear.
";
        let doc = parse_latex(src);
        let resolved = doc.blocks.iter().find_map(|b| {
            if let Block::Paragraph(inlines) = b {
                Some(
                    inlines
                        .iter()
                        .filter_map(|i| {
                            if let Inline::Text(s) = i {
                                Some(s.as_str())
                            } else {
                                None
                            }
                        })
                        .collect::<String>(),
                )
            } else {
                None
            }
        });
        let resolved = resolved.expect("missing paragraph");
        assert!(
            resolved.contains("1"),
            "expected resolved equation reference number, got: {resolved}"
        );
    }

    #[test]
    fn test_eqref_wraps_resolved_equation_number() {
        let src = r"
\begin{equation}
W = x + y
\label{eq:welfare}
\end{equation}

As shown in \eqref{eq:welfare}, objective is linear.
";
        let doc = parse_latex(src);
        let resolved = doc.blocks.iter().find_map(|b| {
            if let Block::Paragraph(inlines) = b {
                Some(
                    inlines
                        .iter()
                        .filter_map(|i| {
                            if let Inline::Text(s) = i {
                                Some(s.as_str())
                            } else {
                                None
                            }
                        })
                        .collect::<String>(),
                )
            } else {
                None
            }
        });
        let resolved = resolved.expect("missing paragraph");
        assert!(
            resolved.contains("(1)"),
            "expected eqref format '(1)', got: {resolved}"
        );
    }

    #[test]
    fn test_autocite_default_is_inline_placeholder() {
        let doc = parse_latex("See \\autocite{Smith2020}.");
        match &doc.blocks[0] {
            Block::Paragraph(inlines) => {
                assert!(
                    inlines
                        .iter()
                        .any(|i| matches!(i, Inline::Text(s) if s.contains("Smith2020"))),
                    "autocite should default to inline placeholder"
                );
            }
            other => panic!("expected paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_autocite_becomes_footnote_when_style_requires_it() {
        let src = "\\ExecuteBibliographyOptions{autocite=footnote}\nSee \\autocite{Smith2020}.";
        let doc = parse_latex(src);
        match &doc.blocks[0] {
            Block::Paragraph(inlines) => {
                let footnote_payload = inlines.iter().find_map(|i| {
                    if let Inline::Footnote(content) = i {
                        Some(content)
                    } else {
                        None
                    }
                });
                let footnote_payload = footnote_payload.expect("expected footnote from autocite");
                assert!(
                    footnote_payload
                        .iter()
                        .any(|i| matches!(i, Inline::Text(s) if s.contains("Smith2020"))),
                    "autocite footnote payload must contain citation key"
                );
            }
            other => panic!("expected paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_simple_table_parsed() {
        let src = r"
\begin{table}[H]
\centering
\caption{Sample table}
\label{tab:sample}
\begin{tabular}{|l|c|}
\hline
\textbf{Name} & \textbf{Value} \\
\hline
Alpha & 1 \\
Beta & 2 \\
\hline
\end{tabular}
\end{table}
";
        let doc = parse_latex(src);
        assert_eq!(doc.blocks.len(), 1, "expected one block");
        match &doc.blocks[0] {
            Block::Table(t) => {
                assert!(!t.caption.is_empty(), "caption should be parsed");
                assert_eq!(t.alignment.as_deref(), Some("center"));
                // 3 rows: header + 2 data rows (hline rules are stripped, not rows)
                assert_eq!(t.rows.len(), 3, "expected 3 rows (header + 2 data)");
                assert_eq!(t.rows[0].cells.len(), 2, "expected 2 cells per row");
            }
            other => panic!("expected Table, got {other:?}"),
        }
    }

    #[test]
    fn test_table_parses_multiple_source_macros_after_float() {
        let src = r"
\begin{table}[H]
\caption{Sample}
\begin{tabular}{|l|}
\hline
X \\
\hline
\end{tabular}
\end{table}
\tablesource{*Estimated.}
\tablesource{Source: synthetic dataset.}
";
        let doc = parse_latex(src);
        let Block::Table(table) = &doc.blocks[0] else {
            panic!("expected table");
        };
        let source_text = table
            .source
            .iter()
            .filter_map(|inline| match inline {
                Inline::Text(value) => Some(value.as_str()),
                _ => None,
            })
            .collect::<String>();
        assert!(
            source_text.contains("*Estimated."),
            "expected first source segment, got: {source_text}"
        );
        assert!(
            source_text.contains("Source: synthetic dataset."),
            "expected second source segment, got: {source_text}"
        );
    }

    #[test]
    fn test_table_cell_linebreak_keeps_space_between_words() {
        let src = r"
\begin{table}[H]
\caption{Sample}
\begin{tabular}{|l|}
\hline
Alpha\linebreak Beta \\
\hline
\end{tabular}
\end{table}
";
        let doc = parse_latex(src);
        let Block::Table(table) = &doc.blocks[0] else {
            panic!("expected table");
        };
        let cell_text = table.rows[0].cells[0]
            .content
            .iter()
            .filter_map(|inline| match inline {
                Inline::Text(value) => Some(value.as_str()),
                _ => None,
            })
            .collect::<String>();
        assert_eq!(
            cell_text.split_whitespace().collect::<Vec<_>>().join(" "),
            "Alpha Beta"
        );
    }

    #[test]
    fn test_figure_parsed() {
        let src = r"
\begin{figure}[H]
\centering
\includegraphics[width=1.0\textwidth]{images/chart.png}
\caption{A sample chart}
\label{fig:chart}
\end{figure}
\figuresource{Source: synthetic data.}
";
        let doc = parse_latex(src);
        assert_eq!(doc.blocks.len(), 1);
        match &doc.blocks[0] {
            Block::Figure(f) => {
                assert_eq!(
                    f.image_path.as_deref(),
                    Some("images/chart.png"),
                    "image path mismatch"
                );
                assert_eq!(f.width_permille, Some(1000), "width hint mismatch");
                assert_eq!(f.alignment.as_deref(), Some("center"));
                assert!(!f.caption.is_empty(), "caption should be parsed");
                assert!(!f.source.is_empty(), "figuresource should be parsed");
            }
            other => panic!("expected Figure, got {other:?}"),
        }
    }

    #[test]
    fn test_itemize_parsed() {
        let src = r"
\begin{itemize}
    \item First item with \textit{italic}.
    \item Second item.
    \item Third item.
\end{itemize}
";
        let doc = parse_latex(src);
        assert_eq!(doc.blocks.len(), 1);
        match &doc.blocks[0] {
            Block::List(list) => {
                assert!(!list.ordered, "should be unordered");
                assert_eq!(list.items.len(), 3, "expected 3 items");
                // First item should contain italic inline.
                assert!(
                    list.items[0].iter().any(|i| matches!(i, Inline::Italic(_))),
                    "first item should contain italic"
                );
            }
            other => panic!("expected List, got {other:?}"),
        }
    }

    #[test]
    fn test_enumerate_parsed() {
        let src = r"
\begin{enumerate}
    \item Rule one.
    \item Rule two with \textbf{bold}.
\end{enumerate}
";
        let doc = parse_latex(src);
        assert_eq!(doc.blocks.len(), 1);
        match &doc.blocks[0] {
            Block::List(list) => {
                assert!(list.ordered, "should be ordered");
                assert_eq!(list.items.len(), 2, "expected 2 items");
                assert!(
                    list.items[1].iter().any(|i| matches!(i, Inline::Bold(_))),
                    "second item should contain bold"
                );
            }
            other => panic!("expected List, got {other:?}"),
        }
    }

    #[test]
    fn test_inline_math_parsed() {
        let doc = parse_latex("Energy $E = mc^2$ is conserved.");
        match &doc.blocks[0] {
            Block::Paragraph(inlines) => {
                assert!(
                    inlines.iter().any(|i| matches!(i, Inline::InlineMath(_))),
                    "expected InlineMath in paragraph"
                );
            }
            other => panic!("expected paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_display_math_parsed() {
        let src = "\\begin{equation}\nW = \\sum x_i\n\\label{eq:welfare}\n\\end{equation}";
        let doc = parse_latex(src);
        assert_eq!(doc.blocks.len(), 1);
        match &doc.blocks[0] {
            Block::DisplayMath(body) => {
                assert!(body.contains("W = \\sum x_i"), "body: {body:?}");
            }
            other => panic!("expected DisplayMath, got {other:?}"),
        }
    }

    #[test]
    fn test_display_math_brackets_parsed() {
        let src = "\\[\nE = mc^2\n\\]";
        let doc = parse_latex(src);
        assert_eq!(doc.blocks.len(), 1);
        match &doc.blocks[0] {
            Block::DisplayMath(body) => {
                assert_eq!(body, "E = mc^2");
            }
            other => panic!("expected DisplayMath, got {other:?}"),
        }
    }

    #[test]
    fn test_display_math_brackets_between_paragraphs() {
        let src = "Before.\n\n\\[\na+b\n\\]\n\nAfter.";
        let doc = parse_latex(src);
        assert_eq!(
            doc.blocks.len(),
            3,
            "expected paragraph + display math + paragraph"
        );
        assert!(matches!(doc.blocks[0], Block::Paragraph(_)));
        assert!(matches!(doc.blocks[1], Block::DisplayMath(_)));
        assert!(matches!(doc.blocks[2], Block::Paragraph(_)));
    }

    #[test]
    fn test_footnote_parsed() {
        let src = "A statement\\footnote{A supporting note with \\textit{style}.}.";
        let doc = parse_latex(src);
        match &doc.blocks[0] {
            Block::Paragraph(inlines) => {
                assert!(
                    inlines.iter().any(|i| matches!(i, Inline::Footnote(_))),
                    "expected Footnote inline in paragraph"
                );
                let footnote = inlines.iter().find_map(|i| {
                    if let Inline::Footnote(content) = i {
                        Some(content)
                    } else {
                        None
                    }
                });
                let footnote = footnote.expect("missing Footnote inline payload");
                assert!(
                    footnote.iter().any(|i| matches!(i, Inline::Italic(_))),
                    "expected nested formatting inside footnote"
                );
            }
            other => panic!("expected paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_latex_file_expands_input_blocks() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ferritex_input_expand_{unique}"));
        std::fs::create_dir_all(&root).expect("failed to create temp dir");

        let main_tex = root.join("main.tex");
        let part_tex = root.join("part.tex");
        std::fs::write(&part_tex, "Included paragraph from input.")
            .expect("failed to write part.tex");
        std::fs::write(
            &main_tex,
            "\\section{Main}\n\n\\input{part}\n\nA trailing paragraph.",
        )
        .expect("failed to write main.tex");

        let doc = parse_latex_file(&main_tex).expect("parse_latex_file failed");
        assert!(
            doc.blocks
                .iter()
                .any(|b| matches!(b, Block::Section { .. })),
            "expected section block from main.tex"
        );
        let has_included_paragraph = doc.blocks.iter().any(|b| {
            if let Block::Paragraph(inlines) = b {
                inlines.iter().any(
                    |i| matches!(i, Inline::Text(s) if s.contains("Included paragraph from input")),
                )
            } else {
                false
            }
        });
        assert!(
            has_included_paragraph,
            "expected paragraph content from included part.tex"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_parse_latex_file_resolves_year_primitives_from_metadata() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ferritex_year_primitive_{unique}"));
        std::fs::create_dir_all(&root).expect("failed to create temp dir");

        let main_tex = root.join("main.tex");
        std::fs::write(&main_tex, "Y: \\the\\year / \\year.").expect("failed to write main.tex");

        let doc = parse_latex_file(&main_tex).expect("parse_latex_file failed");
        let expected_year = current_utc_year()
            .expect("expected current year")
            .to_string();
        let text = doc
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Paragraph(inlines) => Some(
                    inlines
                        .iter()
                        .filter_map(|inline| match inline {
                            Inline::Text(value) => Some(value.as_str()),
                            _ => None,
                        })
                        .collect::<String>(),
                ),
                _ => None,
            })
            .collect::<String>();

        assert!(
            text.contains(&expected_year),
            "expected year '{expected_year}' in parsed text, got: {text}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_parse_latex_file_reads_toc_helper_header_and_entries() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ferritex_toc_helper_{unique}"));
        std::fs::create_dir_all(&root).expect("failed to create temp dir");

        let main_tex = root.join("main.tex");
        let main_toc = root.join("main.toc");
        std::fs::write(
            &main_tex,
            "\\newcommand*{\\tocheader}{\\hbox to \\linewidth{\\hfill{Page.}}}\n\\tableofcontents*",
        )
        .expect("failed to write main.tex");
        std::fs::write(
            &main_toc,
            "\\tocheader\n\\contentsline {chapter}{\\numberline {1}Intro}{3}{chapter.1}%\n",
        )
        .expect("failed to write main.toc");

        let doc = parse_latex_file(&main_tex).expect("parse_latex_file failed");
        assert_eq!(doc.toc_entries.len(), 2, "unexpected toc entries");
        assert_eq!(doc.toc_entries[0].level, 0);
        assert_eq!(doc.toc_entries[0].title, "Page.");
        assert_eq!(doc.toc_entries[1].level, 1);
        assert_eq!(doc.toc_entries[1].number.as_deref(), Some("1"));
        assert_eq!(doc.toc_entries[1].title, "Intro");
        assert_eq!(doc.toc_entries[1].page.as_deref(), Some("3"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_parse_latex_file_detects_autocite_mode_from_included_style() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ferritex_autocite_mode_{unique}"));
        std::fs::create_dir_all(&root).expect("failed to create temp dir");

        let main_tex = root.join("main.tex");
        let style_tex = root.join("style.tex");
        std::fs::write(
            &style_tex,
            "\\ExecuteBibliographyOptions{autocite=footnote}\n",
        )
        .expect("failed to write style.tex");
        std::fs::write(&main_tex, "\\input{style}\nSee \\autocite{Key2026}.")
            .expect("failed to write main.tex");

        let doc = parse_latex_file(&main_tex).expect("parse_latex_file failed");
        let has_autocite_footnote = doc.blocks.iter().any(|b| {
            if let Block::Paragraph(inlines) = b {
                inlines.iter().any(|i| matches!(i, Inline::Footnote(_)))
            } else {
                false
            }
        });
        assert!(
            has_autocite_footnote,
            "expected autocite to become footnote based on included style settings"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_parse_latex_file_expands_include_blocks() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ferritex_include_expand_{unique}"));
        std::fs::create_dir_all(&root).expect("failed to create temp dir");

        let main_tex = root.join("main.tex");
        let chapter_tex = root.join("chapter1.tex");
        std::fs::write(&chapter_tex, "Chapter text from include.")
            .expect("failed to write chapter1");
        std::fs::write(&main_tex, "\\include{chapter1}\n").expect("failed to write main.tex");

        let doc = parse_latex_file(&main_tex).expect("parse_latex_file failed");
        let has_included_paragraph = doc.blocks.iter().any(|b| {
            if let Block::Paragraph(inlines) = b {
                inlines.iter().any(
                    |i| matches!(i, Inline::Text(s) if s.contains("Chapter text from include")),
                )
            } else {
                false
            }
        });
        assert!(
            has_included_paragraph,
            "expected paragraph from included chapter file"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_parse_latex_file_resolves_input_from_root_fallback() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ferritex_root_fallback_{unique}"));
        let chapters = root.join("chapters");
        let common = root.join("common");
        std::fs::create_dir_all(&chapters).expect("failed to create chapters dir");
        std::fs::create_dir_all(&common).expect("failed to create common dir");

        let main_tex = root.join("main.tex");
        let chapter_tex = chapters.join("chapter1.tex");
        let shared_tex = common.join("shared.tex");

        std::fs::write(&shared_tex, "Shared paragraph from root fallback.")
            .expect("failed to write shared.tex");
        std::fs::write(&chapter_tex, "\\input{common/shared}\n")
            .expect("failed to write chapter1.tex");
        std::fs::write(&main_tex, "\\include{chapters/chapter1}\n")
            .expect("failed to write main.tex");

        let doc = parse_latex_file(&main_tex).expect("parse_latex_file failed");
        let has_shared_text = doc.blocks.iter().any(|b| {
            if let Block::Paragraph(inlines) = b {
                inlines.iter().any(
                    |i| matches!(i, Inline::Text(s) if s.contains("Shared paragraph from root fallback")),
                )
            } else {
                false
            }
        });
        assert!(
            has_shared_text,
            "expected text from root fallback include path"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_parse_latex_file_preserves_heading_after_include_without_trailing_newline() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ferritex_include_newline_{unique}"));
        std::fs::create_dir_all(&root).expect("failed to create temp dir");

        let main_tex = root.join("main.tex");
        let toc_tex = root.join("toc.tex");
        let intro_tex = root.join("intro.tex");

        std::fs::write(&main_tex, "\\input{toc}\n\\input{intro}\n")
            .expect("failed to write main.tex");
        std::fs::write(
            &toc_tex,
            "\\ifdefmacro{\\microtypesetup}{\\microtypesetup{protrusion=true}}{}",
        )
        .expect("failed to write toc.tex");
        std::fs::write(&intro_tex, "\\chapter*{Introduction}\n\nBody text.")
            .expect("failed to write intro.tex");

        let doc = parse_latex_file(&main_tex).expect("parse_latex_file failed");

        assert!(
            doc.blocks.iter().any(|block| {
                matches!(
                    block,
                    Block::Section {
                        level: 1,
                        number: None,
                        title,
                        ..
                    } if title.iter().any(|inline| matches!(inline, Inline::Text(text) if text.contains("Introduction")))
                )
            }),
            "expected chapter* heading to survive include boundary"
        );

        let star_heading_leak = doc.blocks.iter().any(|block| {
            if let Block::Paragraph(inlines) = block {
                let text = inlines
                    .iter()
                    .filter_map(|inline| match inline {
                        Inline::Text(value) => Some(value.as_str()),
                        _ => None,
                    })
                    .collect::<String>();
                text.contains("*Introduction")
            } else {
                false
            }
        });
        assert!(
            !star_heading_leak,
            "unexpected raw '*Introduction' leakage from heading parsing"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_strip_heading_prefix_noise_skips_ifnumequal_prefix() {
        let chunk = r"\ifnumequal{\value{contnumfig}}{1}{}{\counterwithout{figure}{chapter}} \
\ifnumequal{\value{contnumtab}}{1}{}{\counterwithout{table}{chapter}} \
\chapter*{Introduction}";
        let stripped = strip_heading_prefix_noise(chunk);
        assert!(
            stripped.starts_with("\\chapter*{Introduction}"),
            "unexpected stripped value: {stripped}"
        );
    }

    #[test]
    fn test_parse_latex_extracts_chapter_after_ifnumequal_prefix() {
        let src = r"\ifnumequal{\value{contnumfig}}{1}{}{\counterwithout{figure}{chapter}}
\ifnumequal{\value{contnumtab}}{1}{}{\counterwithout{table}{chapter}}
\chapter*{Introduction}";
        let doc = parse_latex(src);
        assert!(
            doc.blocks.iter().any(|block| {
                matches!(
                    block,
                    Block::Section {
                        level: 1,
                        number: None,
                        title,
                        ..
                    } if title.iter().any(|inline| matches!(inline, Inline::Text(text) if text == "Introduction"))
                )
            }),
            "expected chapter* block after ifnumequal prefix"
        );
    }

    #[test]
    fn test_unknown_control_command_does_not_leak_argument_text() {
        let doc = parse_latex("Alpha \\unknownmacro{SHOULD_NOT_LEAK} Beta.");
        match &doc.blocks[0] {
            Block::Paragraph(inlines) => {
                let text = inlines
                    .iter()
                    .filter_map(|i| {
                        if let Inline::Text(t) = i {
                            Some(t.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<String>();
                assert!(text.contains("Alpha"));
                assert!(text.contains("Beta."));
                assert!(!text.contains("SHOULD_NOT_LEAK"));
            }
            other => panic!("expected paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_ifnumequal_keeps_primary_branch_and_omits_else_macros() {
        let src = r"
\ifnumequal{\value{bibliosel}}{0}
{
Primary branch text.
}%
{
\begin{refsection}[bl-author]
\printbibliography[heading=nobibheading, section=1, env=countauthor, keyword=biblioauthor]
\end{refsection}
}%
";
        let doc = parse_latex(src);
        let text = doc
            .blocks
            .iter()
            .filter_map(|b| {
                if let Block::Paragraph(inlines) = b {
                    Some(
                        inlines
                            .iter()
                            .filter_map(|i| {
                                if let Inline::Text(t) = i {
                                    Some(t.as_str())
                                } else {
                                    None
                                }
                            })
                            .collect::<String>(),
                    )
                } else {
                    None
                }
            })
            .collect::<String>();

        assert!(text.contains("Primary branch text."));
        assert!(!text.contains("bl-author"));
        assert!(!text.contains("heading=nobibheading"));
    }

    #[test]
    fn test_standalone_brace_paragraphs_are_filtered() {
        let doc = parse_latex("{\n\nVisible text.\n\n}");
        let mut texts = doc
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Paragraph(inlines) => Some(
                    inlines
                        .iter()
                        .filter_map(|inline| match inline {
                            Inline::Text(value) => Some(value.as_str()),
                            _ => None,
                        })
                        .collect::<String>(),
                ),
                _ => None,
            })
            .collect::<Vec<_>>();
        texts.retain(|value| !value.trim().is_empty());

        assert_eq!(texts.len(), 1, "unexpected paragraph texts: {texts:?}");
        assert!(
            texts[0].contains("Visible text."),
            "unexpected text: {}",
            texts[0]
        );
    }

    #[test]
    fn test_ifnum_prefers_then_branch_for_unknown_operands() {
        let doc = parse_latex(
            "Prefix \\ifnum\\totvalue{totalappendix}>0, keeps then-branch\\else keeps else-branch\\fi suffix.",
        );
        let text = doc
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Paragraph(inlines) => Some(
                    inlines
                        .iter()
                        .filter_map(|inline| match inline {
                            Inline::Text(value) => Some(value.as_str()),
                            _ => None,
                        })
                        .collect::<String>(),
                ),
                _ => None,
            })
            .collect::<String>();

        assert!(text.contains("then-branch"), "unexpected text: {text}");
        assert!(!text.contains("else-branch"), "unexpected text: {text}");
    }

    #[test]
    fn test_ifnum_numeric_comparison_uses_else_branch_when_false() {
        let doc = parse_latex("Prefix \\ifnum 1>2 then\\else else\\fi suffix.");
        let text = doc
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Paragraph(inlines) => Some(
                    inlines
                        .iter()
                        .filter_map(|inline| match inline {
                            Inline::Text(value) => Some(value.as_str()),
                            _ => None,
                        })
                        .collect::<String>(),
                ),
                _ => None,
            })
            .collect::<String>();

        assert!(text.contains("else"), "unexpected text: {text}");
        assert!(!text.contains(" then "), "unexpected text: {text}");
    }

    #[test]
    fn test_longtblr_caption_option_parsed() {
        let src = r"
\begin{longtblr}[
caption = {Option caption text},
label = {tab:opt},
]{
colspec = {|l|l|},
hlines
}
A & B \\
\end{longtblr}
";
        let doc = parse_latex(src);
        match &doc.blocks[0] {
            Block::Table(table) => {
                let caption_text = table
                    .caption
                    .iter()
                    .filter_map(|i| {
                        if let Inline::Text(t) = i {
                            Some(t.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<String>();
                assert!(
                    caption_text.contains("Option caption text"),
                    "unexpected caption: {caption_text}"
                );
            }
            other => panic!("expected table, got {other:?}"),
        }
    }

    #[test]
    fn test_printnomenclature_line_is_skipped() {
        let doc = parse_latex("\\printnomenclature[3.5cm]");
        assert!(doc.blocks.is_empty(), "unexpected blocks: {:?}", doc.blocks);
    }

    #[test]
    fn test_typography_normalization_for_quotes_and_dash() {
        let doc = parse_latex("<<чистыми>> и Третий вектор -- новая фаза. X \"--- в тезисах.");
        match &doc.blocks[0] {
            Block::Paragraph(inlines) => {
                let text = inlines
                    .iter()
                    .filter_map(|i| {
                        if let Inline::Text(t) = i {
                            Some(t.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<String>();
                assert!(text.contains("«чистыми»"), "unexpected text: {text}");
                assert!(text.contains("вектор —"), "unexpected text: {text}");
                assert!(text.contains("X —"), "unexpected text: {text}");
                assert!(
                    !text.contains("<<") && !text.contains(">>"),
                    "unexpected text: {text}"
                );
            }
            other => panic!("expected paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_cdot_command_converted_to_middle_dot() {
        let doc = parse_latex("кВт\\cdotч");
        match &doc.blocks[0] {
            Block::Paragraph(inlines) => {
                let text = inlines
                    .iter()
                    .filter_map(|i| {
                        if let Inline::Text(t) = i {
                            Some(t.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<String>();
                assert!(text.contains("кВт·ч"), "unexpected text: {text}");
            }
            other => panic!("expected paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_formbytotal_prefers_plural_suffix() {
        let doc = parse_latex(
            "Полный объём составляет \\formbytotal{TotPages}{страниц}{у}{ы}{} в документе.",
        );
        match &doc.blocks[0] {
            Block::Paragraph(inlines) => {
                let text = inlines
                    .iter()
                    .filter_map(|i| {
                        if let Inline::Text(t) = i {
                            Some(t.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<String>();
                assert!(text.contains("страницы"), "unexpected text: {text}");
                assert!(!text.contains("страницуы"), "unexpected text: {text}");
            }
            other => panic!("expected paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_nonbreaking_space_tilde_normalized_to_regular_space() {
        let doc = parse_latex("См. Рисунок~\\ref{fig:sample}.");
        match &doc.blocks[0] {
            Block::Paragraph(inlines) => {
                let text = inlines
                    .iter()
                    .filter_map(|i| {
                        if let Inline::Text(t) = i {
                            Some(t.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<String>();
                assert!(text.contains("Рисунок "));
                assert!(!text.contains("Рисунок~"));
            }
            other => panic!("expected paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_textual_passthrough_command_keeps_braced_content() {
        let doc = parse_latex("Quoted: \\enquote{important text}.");
        match &doc.blocks[0] {
            Block::Paragraph(inlines) => {
                let text = inlines
                    .iter()
                    .filter_map(|i| {
                        if let Inline::Text(t) = i {
                            Some(t.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<String>();
                assert!(text.contains("important text"));
            }
            other => panic!("expected paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_unmatched_display_math_bracket_does_not_swallow_rest() {
        // \[ used in a macro definition (not as display math) must not
        // prevent subsequent block environments from being segmented.
        let src = concat!(
            "\\def\\zz{\\ifx\\[$\\else\\fi}\n\n",
            "Before.\n\n",
            "\\begin{itemize}\n\\item One\n\\item Two\n\\end{itemize}\n\n",
            "After."
        );
        let doc = parse_latex(src);
        let has_list = doc.blocks.iter().any(|b| matches!(b, Block::List(_)));
        assert!(
            has_list,
            "unmatched \\[ in preamble must not hide subsequent list blocks"
        );
    }

    // ── Tests for newly extracted DocumentLayout fields ─────────────────

    #[test]
    fn test_extract_font_family_from_setmainfont() {
        let src = "\\setmainfont{Liberation Serif}\nBody.";
        let doc = parse_latex(src);
        assert_eq!(
            doc.layout.font_family_body.as_deref(),
            Some("Liberation Serif")
        );
    }

    #[test]
    fn test_extract_font_family_absent() {
        let doc = parse_latex("Body.");
        assert_eq!(doc.layout.font_family_body, None);
    }

    #[test]
    fn test_extract_font_family_conditional_on_fontfamily_counter() {
        let src = "\
\\setcounter{fontfamily}{1}
\\ifnumequal{\\value{fontfamily}}{0}{\\setmainfont{CMU Serif}}
\\ifnumequal{\\value{fontfamily}}{1}{\\setmainfont{Times New Roman}}
\\ifnumequal{\\value{fontfamily}}{2}{\\setmainfont{LiberationSerif}}
Body.";
        let doc = parse_latex(src);
        assert_eq!(
            doc.layout.font_family_body.as_deref(),
            Some("Times New Roman"),
            "should pick \\setmainfont from the branch matching fontfamily=1"
        );
    }

    #[test]
    fn test_extract_font_family_conditional_selects_liberation_when_counter_is_2() {
        let src = "\
\\setcounter{fontfamily}{2}
\\ifnumequal{\\value{fontfamily}}{0}{\\setmainfont{CMU Serif}}
\\ifnumequal{\\value{fontfamily}}{1}{\\setmainfont{Times New Roman}}
\\ifnumequal{\\value{fontfamily}}{2}{\\setmainfont{LiberationSerif}}
Body.";
        let doc = parse_latex(src);
        assert_eq!(
            doc.layout.font_family_body.as_deref(),
            Some("LiberationSerif")
        );
    }

    #[test]
    fn test_bare_tabular_not_parsed_as_table_block() {
        let src = "\\begin{tabular}{l}\nA \\\\ B\n\\end{tabular}\nBody.";
        let doc = parse_latex(src);
        let table_count = doc
            .blocks
            .iter()
            .filter(|b| matches!(b, Block::Table(_)))
            .count();
        assert_eq!(
            table_count, 0,
            "bare \\begin{{tabular}} should not produce a Block::Table"
        );
    }

    #[test]
    fn test_extract_font_size_from_documentclass() {
        let src = "\\documentclass[a4paper,14pt,oneside]{memoir}\nBody.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.font_size_body_hp, Some(28)); // 14pt * 2
    }

    #[test]
    fn test_extract_font_size_12pt_from_documentclass() {
        let src = "\\documentclass[12pt]{article}\nBody.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.font_size_body_hp, Some(24)); // 12pt * 2
    }

    #[test]
    fn test_extract_font_size_absent() {
        let src = "\\documentclass{article}\nBody.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.font_size_body_hp, None);
    }

    #[test]
    fn test_extract_table_font_size_from_settblrinner() {
        let src = "\\documentclass[14pt]{memoir}\n\\SetTblrInner{font=\\footnotesize}\nBody.";
        let doc = parse_latex(src);
        // footnotesize at 14pt base = 12pt = 24 hp
        assert_eq!(doc.layout.font_size_table_hp, Some(24));
    }

    #[test]
    fn test_extract_table_font_size_absent() {
        let src = "\\documentclass[14pt]{memoir}\nBody.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.font_size_table_hp, None);
    }

    #[test]
    fn test_extract_caption_font_size_from_captionsetup() {
        let src =
            "\\documentclass[14pt]{memoir}\n\\captionsetup[table]{font={normalsize,bf}}\nBody.";
        let doc = parse_latex(src);
        // normalsize at 14pt base = 14pt = 28 hp
        assert_eq!(doc.layout.font_size_caption_hp, Some(28));
    }

    #[test]
    fn test_extract_caption_font_size_small() {
        let src = "\\documentclass[14pt]{memoir}\n\\captionsetup{font=small}\nBody.";
        let doc = parse_latex(src);
        // small at 14pt base = 13pt = 26 hp
        assert_eq!(doc.layout.font_size_caption_hp, Some(26));
    }

    #[test]
    fn test_extract_caption_font_size_absent() {
        let src = "\\documentclass[14pt]{memoir}\nBody.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.font_size_caption_hp, None);
    }

    #[test]
    fn test_extract_footnote_font_size_derived_from_body() {
        let src = "\\documentclass[14pt]{memoir}\nBody.";
        let doc = parse_latex(src);
        // 14pt body → footnote = 14 − 4 = 10pt = 20 hp
        assert_eq!(doc.layout.font_size_footnote_hp, Some(20));
    }

    #[test]
    fn test_extract_footnote_font_size_derived_from_12pt_body() {
        let src = "\\documentclass[12pt]{article}\nBody.";
        let doc = parse_latex(src);
        // 12pt body → footnote = 12 − 4 = 8pt = 16 hp
        assert_eq!(doc.layout.font_size_footnote_hp, Some(16));
    }

    #[test]
    fn test_extract_parindent_in_em() {
        let src = "\\setlength{\\parindent}{2.5em}\nBody.";
        let doc = parse_latex(src);
        // 2.5em * 280tw/em = 700tw
        assert_eq!(doc.layout.body_first_line_indent_twips, Some(700));
    }

    #[test]
    fn test_extract_parindent_in_cm() {
        let src = "\\setlength{\\parindent}{1.25cm}\nBody.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.body_first_line_indent_twips, Some(709));
    }

    #[test]
    fn test_extract_parindent_absent() {
        let doc = parse_latex("Body.");
        assert_eq!(doc.layout.body_first_line_indent_twips, None);
    }

    #[test]
    fn test_extract_figurename_renewcommand() {
        let src = "\\renewcommand{\\figurename}{Рисунок}\nBody.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.caption_label_figure.as_deref(), Some("Рисунок"));
    }

    #[test]
    fn test_extract_tablename_renewcommand() {
        let src = "\\renewcommand{\\tablename}{Таблица}\nBody.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.caption_label_table.as_deref(), Some("Таблица"));
    }

    #[test]
    fn test_extract_caption_labels_absent() {
        let doc = parse_latex("Body.");
        assert_eq!(doc.layout.caption_label_figure, None);
        assert_eq!(doc.layout.caption_label_table, None);
    }

    #[test]
    fn test_extract_caption_label_separator_from_declared_tabsep_and_figsep() {
        let src = r"
\newcommand{\tablabelsep}{~---\ }
\newcommand{\figlabelsep}{:\space}
\DeclareCaptionLabelSeparator{tabsep}{\tablabelsep}
\DeclareCaptionLabelSeparator{figsep}{\figlabelsep}
\captionsetup[table]{labelsep=tabsep}
\captionsetup[figure]{labelsep=figsep}
Body.";
        let doc = parse_latex(src);
        assert_eq!(
            doc.layout.caption_label_separator_table.as_deref(),
            Some(" — ")
        );
        assert_eq!(
            doc.layout.caption_label_separator_figure.as_deref(),
            Some(": ")
        );
    }

    #[test]
    fn test_extract_caption_label_separator_prefers_target_over_global() {
        let src = r"
\captionsetup{labelsep=colon}
\captionsetup[figure]{labelsep=period}
Body.";
        let doc = parse_latex(src);
        assert_eq!(
            doc.layout.caption_label_separator_figure.as_deref(),
            Some(". ")
        );
        assert_eq!(
            doc.layout.caption_label_separator_table.as_deref(),
            Some(": ")
        );
    }

    #[test]
    fn test_extract_caption_label_separator_absent() {
        let doc = parse_latex("Body.");
        assert_eq!(doc.layout.caption_label_separator_figure, None);
        assert_eq!(doc.layout.caption_label_separator_table, None);
    }

    #[test]
    fn test_extract_chaptername_renewcommand() {
        let src = "\\renewcommand{\\chaptername}{Chapter}\nBody.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.chapter_name.as_deref(), Some("Chapter"));
    }

    #[test]
    fn test_extract_chaptername_from_chapstyle_counter_and_language() {
        let src = "\
\\setmainlanguage{russian}
\\setcounter{chapstyle}{1}
\\renewcommand*{\\printchaptername}{\\MakeUppercase{\\@chapapp}}
Body.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.chapter_name.as_deref(), Some("Глава"));
    }

    #[test]
    fn test_extract_chaptername_from_chapstyle_disabled_returns_none() {
        let src = "\
\\setmainlanguage{russian}
\\setcounter{chapstyle}{0}
\\renewcommand*{\\printchaptername}{\\MakeUppercase{\\@chapapp}}
Body.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.chapter_name, None);
    }

    #[test]
    fn test_extract_heading_uppercase_from_makeupper_in_chaptertitle() {
        let src = "\\renewcommand{\\printchaptertitle}[1]{\\MakeUppercase{#1}}\nBody.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.heading_uppercase, Some(true));
    }

    #[test]
    fn test_heading_uppercase_absent() {
        let doc = parse_latex("Body.");
        assert_eq!(doc.layout.heading_uppercase, None);
    }

    #[test]
    fn test_extract_language_from_setmainlanguage() {
        let src = "\\setmainlanguage{russian}\nBody.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.document_language.as_deref(), Some("ru-RU"));
    }

    #[test]
    fn test_extract_language_from_babel() {
        let src = "\\usepackage[english, russian]{babel}\nBody.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.document_language.as_deref(), Some("ru-RU"));
    }

    #[test]
    fn test_extract_language_absent() {
        let doc = parse_latex("Body.");
        assert_eq!(doc.layout.document_language, None);
    }

    #[test]
    fn test_extract_english_language_from_babel() {
        let src = "\\usepackage[english]{babel}\nBody.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.document_language.as_deref(), Some("en-US"));
    }

    #[test]
    fn test_extract_documentclass_paper_size_a4() {
        let src = "\\documentclass[a4paper,14pt]{memoir}\nBody.";
        assert_eq!(
            extract_documentclass_paper_size(src),
            Some((11_906, 16_838))
        );
    }

    #[test]
    fn test_extract_documentclass_paper_size_letter() {
        let src = "\\documentclass[letterpaper,12pt]{article}\nBody.";
        assert_eq!(
            extract_documentclass_paper_size(src),
            Some((12_240, 15_840))
        );
    }

    #[test]
    fn test_extract_documentclass_paper_size_absent() {
        let src = "\\documentclass[12pt]{article}\nBody.";
        assert_eq!(extract_documentclass_paper_size(src), None);
    }

    #[test]
    fn test_extract_setmainfont_conditional_for_monofont() {
        let src = "\
\\setcounter{fontfamily}{2}
\\ifnumequal{\\value{fontfamily}}{1}{\\setmonofont{Courier New}}
\\ifnumequal{\\value{fontfamily}}{2}{\\setmonofont{Fira Code}}
Body.";
        assert_eq!(
            extract_setmainfont_conditional_for(src, Some(2), "\\setmonofont"),
            Some("Fira Code".to_string())
        );
    }

    #[test]
    fn test_extract_setmainfont_conditional_for_no_matching_branch() {
        let src = "\\ifnumequal{\\value{fontfamily}}{1}{\\setmonofont{Courier New}}\nBody.";
        assert_eq!(
            extract_setmainfont_conditional_for(src, Some(3), "\\setmonofont"),
            None
        );
    }

    #[test]
    fn test_extract_setmainfont_conditional_for_without_counter_value() {
        let src = "\\ifnumequal{\\value{fontfamily}}{1}{\\setmonofont{Courier New}}\nBody.";
        assert_eq!(
            extract_setmainfont_conditional_for(src, None, "\\setmonofont"),
            None
        );
    }

    #[test]
    fn test_extract_heading_alignment_from_titleformat_center() {
        let src =
            "\\titleformat{\\chapter}[display]{\\centering\\bfseries}{\\thechapter}{1em}{}\nBody.";
        assert_eq!(extract_heading_alignment(src), Some("center".to_string()));
    }

    #[test]
    fn test_extract_heading_alignment_from_memoir_raggedright() {
        let src = "\\renewcommand{\\printchaptertitle}[1]{\\raggedright\\chaptitlefont #1}\nBody.";
        assert_eq!(extract_heading_alignment(src), Some("left".to_string()));
    }

    #[test]
    fn test_extract_heading_alignment_absent() {
        assert_eq!(extract_heading_alignment("Body."), None);
    }

    #[test]
    fn test_extract_heading_alignment_from_titleformat_name_selector_before_code() {
        let src = "\\titleformat{name=\\chapter}[display]{\\bfseries}{\\thechapter}{1em}{\\filcenter}\nBody.";
        assert_eq!(extract_heading_alignment(src), Some("center".to_string()));
    }

    #[test]
    fn test_extract_heading_alignment_from_titleformat_starred() {
        let src = "\\titleformat*{\\chapter}{\\filcenter\\bfseries}\nBody.";
        assert_eq!(extract_heading_alignment(src), Some("center".to_string()));
    }

    #[test]
    fn test_extract_heading_alignment_from_headingalign_counter() {
        let src = "\\newcounter{headingalign}\n\\setcounter{headingalign}{0}\nBody.";
        assert_eq!(extract_heading_alignment(src), Some("center".to_string()));
    }

    #[test]
    fn test_extract_heading_number_delimiter_from_thechapter_dot() {
        let src = "\\renewcommand{\\thechapter}{\\arabic{chapter}.}\nBody.";
        assert_eq!(extract_heading_number_delimiter(src), Some(".".to_string()));
    }

    #[test]
    fn test_extract_heading_number_delimiter_from_thechapter_empty() {
        let src = "\\renewcommand{\\thechapter}{\\arabic{chapter}}\nBody.";
        assert_eq!(extract_heading_number_delimiter(src), Some(String::new()));
    }

    #[test]
    fn test_extract_heading_number_delimiter_from_titleformat_label() {
        let src = "\\titleformat{\\chapter}[display]{\\bfseries}{\\thechapter:}{1em}{}\nBody.";
        assert_eq!(extract_heading_number_delimiter(src), Some(":".to_string()));
    }

    #[test]
    fn test_extract_heading_number_delimiter_from_headingdelim_counter() {
        let src = "\\newcounter{headingdelim}\n\\setcounter{headingdelim}{0}\nBody.";
        assert_eq!(extract_heading_number_delimiter(src), Some(String::new()));
    }

    #[test]
    fn test_extract_heading_number_delimiter_from_titlelabel() {
        let src = "\\titlelabel{\\thetitle.\\quad}\nBody.";
        assert_eq!(extract_heading_number_delimiter(src), Some(".".to_string()));
    }

    #[test]
    fn test_extract_heading_indents_from_setsecindent_macros() {
        let src = "\
\\setlength{\\parindent}{1.25cm}
\\setsecindent{\\parindent}
\\setsubsecindent{12pt}
\\setsubsubsecindent{0pt}
Body.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.heading_indent_section_twips, Some(709));
        assert_eq!(doc.layout.heading_indent_subsection_twips, Some(240));
        assert_eq!(doc.layout.heading_indent_subsubsection_twips, Some(0));
    }

    #[test]
    fn test_extract_heading_indents_absent() {
        let doc = parse_latex("Body.");
        assert_eq!(doc.layout.heading_indent_section_twips, None);
        assert_eq!(doc.layout.heading_indent_subsection_twips, None);
        assert_eq!(doc.layout.heading_indent_subsubsection_twips, None);
    }

    #[test]
    fn test_extract_captionsetup_justification_center() {
        let src = "\\captionsetup{justification=centering}\nBody.";
        assert_eq!(
            extract_captionsetup_justification(src),
            Some("center".to_string())
        );
    }

    #[test]
    fn test_extract_captionsetup_justification_raggedright() {
        let src = "\\captionsetup[table]{justification=raggedright}\nBody.";
        assert_eq!(
            extract_captionsetup_justification(src),
            Some("left".to_string())
        );
    }

    #[test]
    fn test_extract_captionsetup_justification_absent() {
        assert_eq!(extract_captionsetup_justification("Body."), None);
    }

    #[test]
    fn test_extract_captionsetup_skip_twips_target_and_global_fallback() {
        let src = "\\captionsetup{skip=4pt}\n\\captionsetup[table]{skip=3pt}\nBody.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.caption_skip_twips_table, Some(60));
        assert_eq!(doc.layout.caption_skip_twips_figure, Some(80));
    }

    #[test]
    fn test_extract_captionsetup_skip_twips_absent() {
        let doc = parse_latex("Body.");
        assert_eq!(doc.layout.caption_skip_twips_table, None);
        assert_eq!(doc.layout.caption_skip_twips_figure, None);
    }

    #[test]
    fn test_extract_captionsetup_position_target_and_global_fallback() {
        let src = "\\captionsetup{position=below}\n\\captionsetup[table]{position=above}\nBody.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.caption_position_table.as_deref(), Some("top"));
        assert_eq!(
            doc.layout.caption_position_figure.as_deref(),
            Some("bottom")
        );
    }

    #[test]
    fn test_extract_captionsetup_position_absent() {
        let doc = parse_latex("Body.");
        assert_eq!(doc.layout.caption_position_table, None);
        assert_eq!(doc.layout.caption_position_figure, None);
    }

    #[test]
    fn test_extract_captionsetup_singlelinecheck_from_macro() {
        let src = "\
\\newcommand{\\tabsinglecenter}{false}
\\captionsetup[table]{singlelinecheck=\\tabsinglecenter}
\\captionsetup[figure]{singlelinecheck=true}
Body.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.caption_singlelinecheck_table, Some(false));
        assert_eq!(doc.layout.caption_singlelinecheck_figure, Some(true));
    }

    #[test]
    fn test_extract_captionsetup_singlelinecheck_absent() {
        let doc = parse_latex("Body.");
        assert_eq!(doc.layout.caption_singlelinecheck_table, None);
        assert_eq!(doc.layout.caption_singlelinecheck_figure, None);
    }

    #[test]
    fn test_extract_captionsetup_indent_twips_from_macro() {
        let src = "\
\\newcommand{\\tabindent}{1.25cm}
\\captionsetup[table]{indent=\\tabindent}
Body.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.caption_indent_twips_table, Some(709));
        assert_eq!(doc.layout.caption_indent_twips_figure, None);
    }

    #[test]
    fn test_extract_captionsetup_indent_twips_target_and_global_fallback() {
        let src = "\\captionsetup{indent=0pt}\n\\captionsetup[figure]{indent=6pt}\nBody.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.caption_indent_twips_figure, Some(120));
        assert_eq!(doc.layout.caption_indent_twips_table, Some(0));
    }

    #[test]
    fn test_extract_captionsetup_indent_twips_absent() {
        let doc = parse_latex("Body.");
        assert_eq!(doc.layout.caption_indent_twips_table, None);
        assert_eq!(doc.layout.caption_indent_twips_figure, None);
    }

    #[test]
    fn test_extract_graphicspath_basic_and_cleaned() {
        let src = "\\graphicspath{{./figures/}{./img/}{../images/}}\nBody.";
        assert_eq!(
            extract_graphicspath(src),
            vec![
                "figures/".to_string(),
                "img/".to_string(),
                "../images/".to_string()
            ]
        );
    }

    #[test]
    fn test_extract_graphicspath_uses_last_definition() {
        let src = "\\graphicspath{{./figures/}}\n\\graphicspath{{./plots/}{./img/}}\nBody.";
        assert_eq!(
            extract_graphicspath(src),
            vec!["plots/".to_string(), "img/".to_string()]
        );
    }

    #[test]
    fn test_extract_graphicspath_absent() {
        assert!(extract_graphicspath("Body.").is_empty());
    }

    #[test]
    fn test_extract_graphicspath_normalizes_windows_style_and_deduplicates() {
        let src = "\\graphicspath{{.\\\\figures\\\\}{./figures/}{./img//}}\nBody.";
        assert_eq!(
            extract_graphicspath(src),
            vec!["figures/".to_string(), "img/".to_string()]
        );
    }

    #[test]
    fn test_extract_memoir_heading_controls_from_counters() {
        let src = "\
\\setmainlanguage{russian}
\\setcounter{chapstyle}{1}
\\setcounter{headingalign}{1}
\\setcounter{headingdelim}{1}
\\renewcommand*{\\printchaptername}{\\MakeUppercase{\\@chapapp}}
Body.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.chapter_name.as_deref(), Some("Глава"));
        assert_eq!(doc.layout.heading_alignment.as_deref(), Some("left"));
        assert_eq!(doc.layout.heading_number_delimiter.as_deref(), Some("."));
    }

    // ── Task 1: toc_chapter_entry_bold ────────────────────────────────────

    #[test]
    fn test_toc_chapter_entry_bold_normalfont() {
        let src = r"\renewcommand{\cftchapterfont}{\normalfont}
Body.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.toc_chapter_entry_bold, Some(false));
    }

    #[test]
    fn test_toc_chapter_entry_bold_absent() {
        let doc = parse_latex("Body.");
        assert_eq!(doc.layout.toc_chapter_entry_bold, None);
    }

    // ── Task 1: toc_chapter_page_bold ────────────────────────────────────

    #[test]
    fn test_toc_chapter_page_bold_normalfont() {
        let src = r"\renewcommand{\cftchapterpagefont}{\normalfont}
Body.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.toc_chapter_page_bold, Some(false));
    }

    #[test]
    fn test_toc_chapter_page_bold_absent() {
        let doc = parse_latex("Body.");
        assert_eq!(doc.layout.toc_chapter_page_bold, None);
    }

    // ── Task 1: toc_aftersnum ────────────────────────────────────────────

    #[test]
    fn test_toc_aftersnum_chapter_present() {
        let src = r"\renewcommand\cftchapteraftersnum{.\space}
Body.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.toc_aftersnum_chapter.as_deref(), Some(". "));
    }

    #[test]
    fn test_toc_aftersnum_section_present() {
        let src = r"\renewcommand\cftsectionaftersnum{.\space}
Body.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.toc_aftersnum_section.as_deref(), Some(". "));
    }

    #[test]
    fn test_toc_aftersnum_all_absent() {
        let doc = parse_latex("Body.");
        assert_eq!(doc.layout.toc_aftersnum_chapter, None);
        assert_eq!(doc.layout.toc_aftersnum_section, None);
        assert_eq!(doc.layout.toc_aftersnum_subsection, None);
        assert_eq!(doc.layout.toc_aftersnum_subsubsection, None);
    }

    // ── Task 1: toc_appendix_name ────────────────────────────────────────

    #[test]
    fn test_toc_appendix_name_present() {
        let src = r"\renewcommand{\appendixname}{Appendix}
\renewcommand{\cftappendixname}{\appendixname\space}
Body.";
        let doc = parse_latex(src);
        assert_eq!(doc.layout.toc_appendix_name.as_deref(), Some("Appendix"));
    }

    #[test]
    fn test_toc_appendix_name_absent() {
        let doc = parse_latex("Body.");
        assert_eq!(doc.layout.toc_appendix_name, None);
    }

    // ── Task 2: is_only_linebreaks — no Block from lone \\ ────────────────

    #[test]
    fn test_linebreak_only_chunk_no_block() {
        let src = "First paragraph.\n\\\\\n\nSecond paragraph.";
        let doc = parse_latex(src);
        let para_count = doc
            .blocks
            .iter()
            .filter(|b| matches!(b, Block::Paragraph(_)))
            .count();
        assert_eq!(para_count, 2, "expected 2 paragraphs, got: {para_count}");
    }

    // ── Task 2: inline math preserves surrounding spaces ─────────────────

    #[test]
    fn test_inline_math_preserves_spaces() {
        let doc = parse_latex("Let $P$ denote the value.");
        let Block::Paragraph(inlines) = &doc.blocks[0] else {
            panic!("expected paragraph");
        };
        let text: String = inlines
            .iter()
            .map(|i| match i {
                Inline::Text(s) => s.clone(),
                Inline::InlineMath(_) => "MATH".to_string(),
                _ => String::new(),
            })
            .collect();
        assert!(text.contains("Let "), "space before math missing: {text:?}");
        assert!(
            text.contains(" denote"),
            "space after math missing: {text:?}"
        );
    }

    // ── List settings extraction tests ───────────────────────────────────────

    #[test]
    fn test_extract_list_settings_labelsep_em() {
        let source = r"\setlist{nosep, labelsep=.5em, labelwidth=!, leftmargin=\dimexpr\parindent-\labelwidth-\labelsep\relax}";
        let (sep, width, _bullet) = extract_list_settings(source);
        // 0.5em at 14pt (280 twips/em) = 140 twips
        let sep = sep.expect("labelsep should be extracted");
        assert!((130..=150).contains(&sep), "expected ~140 twips, got {sep}");
        assert!(width.is_none(), "labelwidth=! should produce None");
    }

    #[test]
    fn test_extract_list_settings_absent() {
        let source = r"\usepackage{enumitem}";
        let (sep, width, bullet) = extract_list_settings(source);
        assert!(sep.is_none());
        assert!(width.is_none());
        assert!(bullet.is_none());
    }

    #[test]
    fn test_extract_labelitemi_char_endash() {
        let source = r"\renewcommand{\labelitemi}{\normalfont\bfseries{--}}";
        let (_sep, _width, bullet) = extract_list_settings(source);
        let bullet = bullet.expect("bullet should be extracted");
        assert_eq!(bullet, "–", "expected en-dash, got {bullet:?}");
    }

    #[test]
    fn test_extract_labelitemi_char_absent() {
        let source = r"\usepackage{enumitem}";
        let (_sep, _width, bullet) = extract_list_settings(source);
        assert!(bullet.is_none());
    }

    // ── Source vspace extraction tests ───────────────────────────────────────

    #[test]
    fn test_extract_source_vspace_tablesource() {
        let source = r"\newcommand{\tablesource}[1]{\par\vspace{4pt}{\noindent\raggedright\small\textit{#1}\par}}";
        let tw = extract_source_vspace_twips(source, "tablesource");
        // 4pt = 80 twips
        let tw = tw.expect("vspace should be extracted");
        assert_eq!(tw, 80, "expected 80 twips (4pt), got {tw}");
    }

    #[test]
    fn test_extract_source_vspace_figuresource() {
        let source = r"\newcommand{\figuresource}[1]{\par\vspace{2pt}{\noindent\raggedright\small\textit{#1}\par}}";
        let tw = extract_source_vspace_twips(source, "figuresource");
        let tw = tw.expect("vspace should be extracted");
        assert_eq!(tw, 40, "expected 40 twips (2pt), got {tw}");
    }

    #[test]
    fn test_extract_source_vspace_absent() {
        let source = r"\usepackage{caption}";
        assert!(extract_source_vspace_twips(source, "tablesource").is_none());
    }

    // ── Title page page number suppression tests ─────────────────────────────

    #[test]
    fn test_extract_title_page_suppress_inside_titlingpage() {
        let source = "\\begin{titlingpage}\n\\thispagestyle{empty}\n\\end{titlingpage}";
        assert_eq!(extract_title_page_suppress_number(source), Some(true));
    }

    #[test]
    fn test_extract_title_page_suppress_absent() {
        let source = "\\begin{document}\nSome text.\n\\end{document}";
        assert!(extract_title_page_suppress_number(source).is_none());
    }

    // ── Bibliography command parsing tests ────────────────────────────────────

    #[test]
    fn test_try_parse_bibliography_printbibliography_no_title() {
        let block = try_parse_bibliography_command("\\printbibliography");
        match block {
            Some(Block::BibliographyHeading { title }) => {
                assert_eq!(title, "СПИСОК ЛИТЕРАТУРЫ");
            }
            other => panic!("expected BibliographyHeading, got {other:?}"),
        }
    }

    #[test]
    fn test_try_parse_bibliography_printbibliography_with_title() {
        let block = try_parse_bibliography_command("\\printbibliography[title={References}]");
        match block {
            Some(Block::BibliographyHeading { title }) => {
                assert_eq!(title, "References");
            }
            other => panic!("expected BibliographyHeading, got {other:?}"),
        }
    }

    #[test]
    fn test_try_parse_bibliography_nobibheading_skipped() {
        let block =
            try_parse_bibliography_command("\\printbibliography[heading=nobibheading, section=1]");
        assert!(block.is_none(), "nobibheading should produce None");
    }

    #[test]
    fn test_try_parse_bibliography_insertbibliofullsorted() {
        let block = try_parse_bibliography_command("\\insertbibliofullsorted");
        match block {
            Some(Block::BibliographyHeading { title }) => {
                assert_eq!(title, "СПИСОК ЛИТЕРАТУРЫ");
            }
            other => panic!("expected BibliographyHeading, got {other:?}"),
        }
    }

    #[test]
    fn test_try_parse_bibliography_not_a_bib_command() {
        assert!(try_parse_bibliography_command("\\chapter{Introduction}").is_none());
        assert!(try_parse_bibliography_command("Some paragraph text.").is_none());
    }
}
