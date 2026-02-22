use crate::model::{Block, Document, Figure, Inline, List, Table, TableCell, TableRow};

/// Parse a LaTeX source string into a [`Document`] AST.
///
/// Supported constructs:
/// - `\section{…}`, `\subsection{…}`, `\subsubsection{…}`
/// - Plain paragraphs (blank-line separated)
/// - `\textbf{…}`, `\textit{…}`, `{\bf …}`, `{\it …}`
/// - `\label{…}`, `\ref{…}`, `\cite{…}` — emitted as placeholder text
/// - `\begin{table}…\end{table}` with `\begin{tabular}` / `\begin{tblr}` / `\begin{longtblr}`
/// - `\begin{figure}…\end{figure}` with `\includegraphics` and `\caption`
/// - `\tablesource{…}` / `\figuresource{…}` — stored as source attribution
/// - Preamble directives (`\documentclass`, `\usepackage`, `\begin{document}`,
///   `\end{document}`) are silently skipped.
pub fn parse_latex(source: &str) -> Document {
    let source = strip_comments(source);
    let filtered = filter_skippable_lines(&source);
    // Segment the source into typed spans before paragraph-splitting,
    // so that multi-line environments are kept intact.
    let segments = segment(&filtered);
    let mut blocks = Vec::new();

    for seg in segments {
        match seg {
            Segment::Float(content) => {
                if let Some(block) = parse_float(&content) {
                    blocks.push(block);
                }
            }
            Segment::Text(content) => {
                for chunk in split_paragraphs(&content) {
                    let chunk = chunk.trim();
                    if chunk.is_empty() {
                        continue;
                    }
                    if let Some(block) = try_parse_section(chunk) {
                        blocks.push(block);
                    } else {
                        let inlines = parse_inlines(chunk);
                        if !inlines.is_empty() {
                            blocks.push(Block::Paragraph(inlines));
                        }
                    }
                }
            }
        }
    }

    Document { blocks }
}

// ---------------------------------------------------------------------------
// Segmenter — splits source into float environments vs plain text
// ---------------------------------------------------------------------------

enum Segment {
    /// A `\begin{table}…\end{table}` or `\begin{figure}…\end{figure}` block,
    /// plus any immediately following `\tablesource`/`\figuresource` call.
    Float(String),
    /// Everything else (paragraphs, sections, etc.).
    Text(String),
}

/// Block-level environments extracted before paragraph splitting.
const BLOCK_ENVS: &[&str] = &[
    "table",
    "figure",
    "table*",
    "figure*",
    "itemize",
    "enumerate",
];

fn segment(src: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut pos = 0;
    let bytes = src.as_bytes();
    let len = src.len();

    while pos < len {
        // Look for \begin{<float_env>}
        if let Some((env_name, begin_pos)) = find_begin_float(src, pos) {
            // Flush text before this float.
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
                // Unmatched \begin — treat as text.
                segments.push(Segment::Text(src[begin_pos..].to_string()));
                pos = len;
            }
        } else {
            // No more floats — rest is text.
            segments.push(Segment::Text(src[pos..].to_string()));
            pos = len;
        }
        _ = bytes; // suppress unused warning
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
fn parse_float(src: &str) -> Option<Block> {
    let src = src.trim();
    if src.starts_with("\\begin{figure") {
        Some(Block::Figure(parse_figure(src)))
    } else if src.starts_with("\\begin{itemize") {
        Some(Block::List(parse_list(src, false)))
    } else if src.starts_with("\\begin{enumerate") {
        Some(Block::List(parse_list(src, true)))
    } else {
        Some(Block::Table(parse_table(src)))
    }
}

/// Parse a `\begin{table}…\end{table}` segment into a [`Table`].
fn parse_table(src: &str) -> Table {
    let caption = extract_caption(src);
    let source = extract_source_macro(src);

    // Find the inner tabular/tblr/longtblr environment.
    let rows = extract_table_rows(src);

    Table {
        caption,
        source,
        rows,
    }
}

/// Parse a `\begin{figure}…\end{figure}` segment into a [`Figure`].
fn parse_figure(src: &str) -> Figure {
    let caption = extract_caption(src);
    let source = extract_source_macro(src);
    let image_path = extract_includegraphics_path(src);
    Figure {
        image_path,
        caption,
        source,
    }
}

/// Parse a `\begin{itemize}…\end{itemize}` or `\begin{enumerate}…\end{enumerate}` segment.
fn parse_list(src: &str, ordered: bool) -> List {
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
            parse_inlines(&text)
        })
        .filter(|inlines| !inlines.is_empty())
        .collect();

    List { ordered, items }
}

/// Extract `\caption{…}` content from a float.
fn extract_caption(src: &str) -> Vec<Inline> {
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
            return parse_inlines(content);
        }
    }
    Vec::new()
}

/// Extract `\tablesource{…}` or `\figuresource{…}` content.
fn extract_source_macro(src: &str) -> Vec<Inline> {
    for macro_name in &["\\tablesource", "\\figuresource"] {
        if let Some(pos) = src.find(macro_name) {
            let after = src[pos + macro_name.len()..].trim_start_matches([' ', '\t']);
            if let Some(content) = extract_braced(after) {
                return parse_inlines(content);
            }
        }
    }
    Vec::new()
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

/// Parse rows from within a `tabular`, `tblr`, or `longtblr` environment.
///
/// Strategy:
/// 1. Locate the innermost table environment body (after the column spec).
/// 2. Split on `\\` (row separator).
/// 3. Split each row on `&` (cell separator).
/// 4. Parse each cell as inlines.
fn extract_table_rows(src: &str) -> Vec<TableRow> {
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
                content: parse_inlines(cell.trim()),
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
// Shared line-level helpers (unchanged from v0.1)
// ---------------------------------------------------------------------------

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
        || c.starts_with("\\maketitle")
        || c.starts_with("\\tableofcontents")
}

/// Try to parse `\section{…}` / `\subsection{…}` / `\subsubsection{…}`.
fn try_parse_section(chunk: &str) -> Option<Block> {
    let (level, rest) = if let Some(r) = chunk.strip_prefix("\\subsubsection") {
        (3u8, r)
    } else if let Some(r) = chunk.strip_prefix("\\subsection") {
        (2u8, r)
    } else if let Some(r) = chunk.strip_prefix("\\section") {
        (1u8, r)
    } else {
        return None;
    };

    let rest = rest.strip_prefix('*').unwrap_or(rest).trim_start();
    let title_src = extract_braced(rest)?;
    let title = parse_inlines(title_src);
    Some(Block::Section { level, title })
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
pub(crate) fn parse_inlines(src: &str) -> Vec<Inline> {
    let mut result = Vec::new();
    let mut pos = 0;
    let bytes = src.as_bytes();
    let len = src.len();

    while pos < len {
        match bytes[pos] {
            b'\\' => {
                let rest = &src[pos..];
                if let Some((inlines, consumed)) = try_parse_inline_command(rest) {
                    result.extend(inlines);
                    pos += consumed;
                } else {
                    let end = rest[1..]
                        .find(|c: char| !c.is_ascii_alphabetic())
                        .map(|i| pos + 1 + i)
                        .unwrap_or(len);
                    pos = end;
                    let remaining = src[pos..].trim_start();
                    if remaining.starts_with('{')
                        && let Some(arg_len) = braced_len(remaining)
                    {
                        pos += src[pos..].len() - remaining.len() + arg_len;
                    }
                }
            }
            b'{' => {
                let rest = &src[pos..];
                if let Some((inlines, consumed)) = try_parse_brace_group(rest) {
                    result.extend(inlines);
                    pos += consumed;
                } else if let Some(inner_len) = braced_len(rest) {
                    let inner = &rest[1..inner_len - 1];
                    result.extend(parse_inlines(inner));
                    pos += inner_len;
                } else {
                    result.push(Inline::Text("{".to_string()));
                    pos += 1;
                }
            }
            _ => {
                let start = pos;
                while pos < len && bytes[pos] != b'\\' && bytes[pos] != b'{' {
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

fn try_parse_inline_command(src: &str) -> Option<(Vec<Inline>, usize)> {
    if let Some(r) = src.strip_prefix("\\textbf") {
        let r = r.trim_start_matches(' ');
        if let Some(arg_len) = braced_len(r) {
            let inner = &r[1..arg_len - 1];
            let consumed = src.len() - r.len() + arg_len;
            return Some((vec![Inline::Bold(parse_inlines(inner))], consumed));
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
            return Some((vec![Inline::Italic(parse_inlines(inner))], consumed));
        }
    }
    for cmd in &["\\label", "\\ref", "\\cite", "\\autocite"] {
        if let Some(r) = src.strip_prefix(cmd) {
            let r = r.trim_start_matches(' ');
            if let Some(arg_len) = braced_len(r) {
                let inner = &r[1..arg_len - 1];
                let placeholder = format!("[{}]", inner);
                let consumed = src.len() - r.len() + arg_len;
                return Some((vec![Inline::Text(placeholder)], consumed));
            }
        }
    }
    None
}

fn try_parse_brace_group(src: &str) -> Option<(Vec<Inline>, usize)> {
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
    let inlines = parse_inlines(content);
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

fn normalize_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(ch);
            prev_space = false;
        }
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
    fn test_section_level1() {
        let doc = parse_latex("\\section{Introduction}");
        assert_eq!(
            doc.blocks,
            vec![Block::Section {
                level: 1,
                title: vec![Inline::Text("Introduction".into())]
            }]
        );
    }

    #[test]
    fn test_section_level2() {
        let doc = parse_latex("\\subsection{Background}");
        assert_eq!(
            doc.blocks[0],
            Block::Section {
                level: 2,
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
                level: 1,
                title: vec![Inline::Text("Preface".into())]
            }
        );
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
}
