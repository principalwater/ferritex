use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use crate::model::{Block, Document, Figure, Inline, List, Table, TableCell, TableRow};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutociteMode {
    InlinePlaceholder,
    FootnotePlaceholder,
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
pub fn parse_latex(source: &str) -> Document {
    let autocite_mode = detect_autocite_mode(source);
    parse_latex_with_mode(source, autocite_mode)
}

/// Parse an entry `.tex` file with recursive `\input{...}` / `\include{...}` expansion.
///
/// Missing input files are kept as-is in the expanded source (best-effort mode).
pub fn parse_latex_file(input_path: &Path) -> anyhow::Result<Document> {
    let root_dir = input_path.parent().unwrap_or_else(|| Path::new("."));
    let mut stack = Vec::new();
    let expanded = expand_inputs_recursive(input_path, root_dir, &mut stack)?;
    Ok(parse_latex(&expanded))
}

fn parse_latex_with_mode(source: &str, autocite_mode: AutociteMode) -> Document {
    let source = strip_comments(source);
    let declared_labels = collect_declared_labels(&source);
    let body = extract_document_body(&source);
    let filtered = filter_skippable_lines(body);
    // Segment the source into typed spans before paragraph-splitting,
    // so that multi-line environments are kept intact.
    let segments = segment(&filtered);
    let mut blocks = Vec::new();

    for seg in segments {
        match seg {
            Segment::Float(content) => {
                if let Some(block) = parse_float(&content, autocite_mode) {
                    blocks.push(block);
                }
            }
            Segment::Text(content) => {
                for chunk in split_paragraphs(&content) {
                    let chunk = chunk.trim();
                    if chunk.is_empty() {
                        continue;
                    }
                    if let Some(block) = try_parse_section(chunk, autocite_mode) {
                        blocks.push(block);
                    } else {
                        let inlines = parse_inlines(chunk, autocite_mode);
                        if !inlines.is_empty() && !is_single_brace_paragraph(&inlines) {
                            blocks.push(Block::Paragraph(inlines));
                        }
                    }
                }
            }
        }
    }

    assign_section_numbers(&mut blocks);
    resolve_references(&mut blocks, &declared_labels);
    Document { blocks }
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
                            out.push_str(&expanded);
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
    "tabular",
    "tabular*",
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
    for prefix in &["\\tablesource", "\\figuresource"] {
        if let Some(rest) = src.strip_prefix(prefix) {
            let rest = rest.trim_start_matches([' ', '\t']);
            if let Some(len) = braced_len(rest) {
                return format!("{}{}", prefix, &rest[..len]);
            }
        }
    }
    String::new()
}

// ---------------------------------------------------------------------------
// Float parser
// ---------------------------------------------------------------------------

/// Parse a block environment segment into a [`Block`].
fn parse_float(src: &str, autocite_mode: AutociteMode) -> Option<Block> {
    let src = src.trim();
    if src.starts_with("\\[") || src.starts_with("\\begin{equation") {
        Some(Block::DisplayMath(extract_display_math_body(src)))
    } else if src.starts_with("\\begin{refsection") {
        None
    } else if src.starts_with("\\begin{figure") {
        Some(Block::Figure(parse_figure(src, autocite_mode)))
    } else if src.starts_with("\\begin{itemize") {
        Some(Block::List(parse_list(src, false, autocite_mode)))
    } else if src.starts_with("\\begin{enumerate") {
        Some(Block::List(parse_list(src, true, autocite_mode)))
    } else if src.starts_with("\\begin{table")
        || src.starts_with("\\begin{tabular")
        || src.starts_with("\\begin{tblr")
        || src.starts_with("\\begin{longtblr")
    {
        Some(Block::Table(parse_table(src, autocite_mode)))
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
fn parse_table(src: &str, autocite_mode: AutociteMode) -> Table {
    let mut caption = extract_caption(src, autocite_mode);
    if caption.is_empty()
        && let Some(value) = extract_option_value(src, "caption")
    {
        caption = parse_inlines(value.as_str(), autocite_mode);
    }
    let label = extract_label_macro(src).or_else(|| extract_option_label(src));
    let source = extract_source_macro(src, autocite_mode);

    // Find the inner tabular/tblr/longtblr environment.
    let rows = extract_table_rows(src, autocite_mode);

    Table {
        caption,
        label,
        source,
        rows,
    }
}

/// Parse a `\begin{figure}…\end{figure}` segment into a [`Figure`].
fn parse_figure(src: &str, autocite_mode: AutociteMode) -> Figure {
    let caption = extract_caption(src, autocite_mode);
    let label = extract_label_macro(src).or_else(|| extract_option_label(src));
    let source = extract_source_macro(src, autocite_mode);
    let image_path = extract_includegraphics_path(src);
    let width_permille = extract_includegraphics_width_permille(src);
    Figure {
        image_path,
        width_permille,
        caption,
        label,
        source,
    }
}

/// Parse a `\begin{itemize}…\end{itemize}` or `\begin{enumerate}…\end{enumerate}` segment.
fn parse_list(src: &str, ordered: bool, autocite_mode: AutociteMode) -> List {
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
            parse_inlines(&text, autocite_mode)
        })
        .filter(|inlines| !inlines.is_empty())
        .collect();

    List { ordered, items }
}

/// Extract `\caption{…}` content from a float.
fn extract_caption(src: &str, autocite_mode: AutociteMode) -> Vec<Inline> {
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
            return parse_inlines(content, autocite_mode);
        }
    }
    Vec::new()
}

/// Extract `\tablesource{…}` or `\figuresource{…}` content.
fn extract_source_macro(src: &str, autocite_mode: AutociteMode) -> Vec<Inline> {
    for macro_name in &["\\tablesource", "\\figuresource"] {
        if let Some(pos) = src.find(macro_name) {
            let after = src[pos + macro_name.len()..].trim_start_matches([' ', '\t']);
            if let Some(content) = extract_braced(after) {
                return parse_inlines(content, autocite_mode);
            }
        }
    }
    Vec::new()
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
fn extract_table_rows(src: &str, autocite_mode: AutociteMode) -> Vec<TableRow> {
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
                content: parse_inlines(cell.trim(), autocite_mode),
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
        || c.starts_with("\\tableofcontents")
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
        || c.starts_with("\\vspace")
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
        || c.starts_with("\\centering")
        || c.starts_with("\\raggedright")
        || c.starts_with("\\raggedleft")
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

fn resolve_references(blocks: &mut [Block], declared_labels: &[String]) {
    let labels = build_label_registry(blocks, declared_labels);

    for block in blocks.iter_mut() {
        match block {
            Block::Section { title, .. } | Block::Paragraph(title) => {
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
            Block::DisplayMath(_) => {}
        }
    }
}

fn build_label_registry(blocks: &[Block], declared_labels: &[String]) -> HashMap<String, String> {
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
                    figure_no = 0;
                    table_no = 0;
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
                    let value = if chapter_no > 0 {
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
                    let value = if chapter_no > 0 {
                        format!("{chapter_no}.{table_no}")
                    } else {
                        table_no.to_string()
                    };
                    labels.insert(label.clone(), value);
                }
            }
            Block::DisplayMath(body) => {
                equation_no += 1;
                let value = equation_no.to_string();
                for label in extract_labels(body) {
                    labels.insert(label, value.clone());
                }
            }
            Block::Paragraph(_) | Block::List(_) => {}
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
            Inline::Text(_) | Inline::InlineMath(_) => {}
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
fn try_parse_section(chunk: &str, autocite_mode: AutociteMode) -> Option<Block> {
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
    let title = parse_inlines(title_src, autocite_mode);
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
fn parse_inlines(src: &str, autocite_mode: AutociteMode) -> Vec<Inline> {
    let mut result = Vec::new();
    let mut pos = 0;
    let bytes = src.as_bytes();
    let len = src.len();

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
                    result.push(Inline::InlineMath(math_src));
                    pos = end + 1;
                } else {
                    // No closing $ — emit as plain text.
                    result.push(Inline::Text("$".to_string()));
                    pos += 1;
                }
            }
            b'\\' => {
                let rest = &src[pos..];
                if let Some((inlines, consumed)) = try_parse_inline_command(rest, autocite_mode) {
                    result.extend(inlines);
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
                if let Some((inlines, consumed)) = try_parse_brace_group(rest, autocite_mode) {
                    result.extend(inlines);
                    pos += consumed;
                } else if let Some(inner_len) = braced_len(rest) {
                    let inner = &rest[1..inner_len - 1];
                    result.extend(parse_inlines(inner, autocite_mode));
                    pos += inner_len;
                } else {
                    result.push(Inline::Text("{".to_string()));
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
                    result.push(Inline::Text(normalized));
                }
            }
        }
    }

    result
}

fn try_parse_inline_command(
    src: &str,
    autocite_mode: AutociteMode,
) -> Option<(Vec<Inline>, usize)> {
    if let Some(rest) = src.strip_prefix("\\\\") {
        let mut consumed = 2usize;
        if rest.starts_with('[')
            && let Some(arg_len) = bracketed_len(rest)
        {
            consumed += arg_len;
        }
        return Some((vec![Inline::Text(" ".to_string())], consumed));
    }
    if let Some(rest) = src.strip_prefix("\\cdot")
        && !rest.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
    {
        return Some((vec![Inline::Text("·".to_string())], "\\cdot".len()));
    }

    if let Some(r) = src.strip_prefix("\\textbf") {
        let r = r.trim_start_matches(' ');
        if let Some(arg_len) = braced_len(r) {
            let inner = &r[1..arg_len - 1];
            let consumed = src.len() - r.len() + arg_len;
            return Some((
                vec![Inline::Bold(parse_inlines(inner, autocite_mode))],
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
                vec![Inline::Italic(parse_inlines(inner, autocite_mode))],
                consumed,
            ));
        }
    }
    for textual_cmd in &[
        "\\texttt",
        "\\textrm",
        "\\textnormal",
        "\\textup",
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
                return Some((parse_inlines(inner, autocite_mode), consumed));
            }
        }
    }
    if let Some(r) = src.strip_prefix("\\footnote") {
        let r = r.trim_start_matches(' ');
        if let Some(arg_len) = braced_len(r) {
            let inner = &r[1..arg_len - 1];
            let consumed = src.len() - r.len() + arg_len;
            return Some((
                vec![Inline::Footnote(parse_inlines(inner, autocite_mode))],
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
            // Prefer the "equal" branch. This keeps human-readable text and
            // avoids leaking control-flow internals into output.
            let final_consumed = if args.len() == 4 { consumed } else { src.len() };
            return Some((
                parse_inlines(args[2].as_str(), autocite_mode),
                final_consumed,
            ));
        }
    }
    if src.starts_with("\\ifnum") {
        let mut pos = "\\ifnum".len();
        while pos < src.len() && src.as_bytes()[pos].is_ascii_whitespace() {
            pos += 1;
        }

        let (lhs_consumed, lhs_value) = consume_ifnum_operand(&src[pos..])?;
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

        let (rhs_consumed, rhs_value) = consume_ifnum_operand(&src[pos..])?;
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
            parse_inlines(chosen.trim(), autocite_mode),
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
            let stem = args[1].trim();
            let chosen_suffix = [&args[4], &args[3], &args[2]]
                .iter()
                .map(|s| s.trim())
                .find(|s| !s.is_empty())
                .unwrap_or("");
            let replacement = format!("{stem}{chosen_suffix}");
            return Some((vec![Inline::Text(replacement)], consumed));
        }
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

fn try_parse_brace_group(src: &str, autocite_mode: AutociteMode) -> Option<(Vec<Inline>, usize)> {
    if !src.starts_with('{') {
        return None;
    }
    let inner_src = &src[1..];
    let (wrapper, content_src): (fn(Vec<Inline>) -> Inline, &str) = if let Some(r) =
        inner_src.strip_prefix("\\bf").and_then(|r| {
            if r.starts_with(|c: char| c.is_whitespace() || c == '{') {
                Some(r)
            } else {
                None
            }
        }) {
        (|v| Inline::Bold(v), r.trim_start())
    } else if let Some(r) = inner_src.strip_prefix("\\it").and_then(|r| {
        if r.starts_with(|c: char| c.is_whitespace() || c == '{') {
            Some(r)
        } else {
            None
        }
    }) {
        (|v| Inline::Italic(v), r.trim_start())
    } else if let Some(r) = inner_src.strip_prefix("\\bfseries").and_then(|r| {
        if r.starts_with(|c: char| c.is_whitespace() || c == '{') {
            Some(r)
        } else {
            None
        }
    }) {
        (|v| Inline::Bold(v), r.trim_start())
    } else if let Some(r) = inner_src.strip_prefix("\\itshape").and_then(|r| {
        if r.starts_with(|c: char| c.is_whitespace() || c == '{') {
            Some(r)
        } else {
            None
        }
    }) {
        (|v| Inline::Italic(v), r.trim_start())
    } else {
        return None;
    };

    let total_group_len = braced_len(src)?;
    let content_end = total_group_len - 1;
    let content_start = src.len() - inner_src.len() + (inner_src.len() - content_src.len());
    let content = &src[content_start..content_end];
    let inlines = parse_inlines(content, autocite_mode);
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

fn consume_ifnum_operand(src: &str) -> Option<(usize, Option<i64>)> {
    if src.is_empty() {
        return None;
    }

    if let Some(value_len) = parse_leading_int_len(src) {
        return Some((value_len, src[..value_len].parse::<i64>().ok()));
    }

    if src.starts_with('\\') {
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
    out = out.replace("\"---", "—");
    out.replace(" -- ", " — ")
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
                // 3 rows: header + 2 data rows (hline rules are stripped, not rows)
                assert_eq!(t.rows.len(), 3, "expected 3 rows (header + 2 data)");
                assert_eq!(t.rows[0].cells.len(), 2, "expected 2 cells per row");
            }
            other => panic!("expected Table, got {other:?}"),
        }
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
}
