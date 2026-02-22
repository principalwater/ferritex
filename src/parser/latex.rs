use crate::model::{Block, Document, Inline};

/// Parse a LaTeX source string into a [`Document`] AST.
///
/// Supported constructs:
/// - `\section{…}`, `\subsection{…}`, `\subsubsection{…}`
/// - Plain paragraphs (blank-line separated)
/// - `\textbf{…}`, `\textit{…}`, `{\bf …}`, `{\it …}`
/// - `\label{…}`, `\ref{…}`, `\cite{…}` — emitted as placeholder text
/// - Preamble directives (`\documentclass`, `\usepackage`, `\begin{document}`,
///   `\end{document}`) are silently skipped.
pub fn parse_latex(source: &str) -> Document {
    // Strip comments first, then filter out skippable lines so that
    // split_paragraphs never sees preamble directives mixed with body text.
    let source = strip_comments(source);
    let filtered = filter_skippable_lines(&source);
    let chunks = split_paragraphs(&filtered);
    let mut blocks = Vec::new();

    for chunk in chunks {
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

    Document { blocks }
}

/// Remove lines that are purely preamble directives or environment delimiters,
/// replacing them with blank lines so that paragraph splitting is unaffected.
fn filter_skippable_lines(src: &str) -> String {
    src.lines()
        .map(|line| if is_skippable(line.trim()) { "" } else { line })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Remove `%`-style LaTeX comments (to end of line).
fn strip_comments(src: &str) -> String {
    src.lines()
        .map(|line| {
            // A `%` that is NOT preceded by `\` starts a comment.
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
/// Returns `None` if the chunk is not a heading.
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

    // Accept optional `*` (unnumbered variant) then `{…}`
    let rest = rest.strip_prefix('*').unwrap_or(rest).trim_start();
    let title_src = extract_braced(rest)?;
    let title = parse_inlines(title_src);
    Some(Block::Section { level, title })
}

/// Extract the content of the first `{…}` group from the start of `src`.
/// Returns the content (without braces) or `None` if the input doesn't start
/// with `{`.
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

/// Parse a string of mixed text and inline LaTeX commands into a list of
/// [`Inline`] nodes.
pub(crate) fn parse_inlines(src: &str) -> Vec<Inline> {
    let mut result = Vec::new();
    let mut pos = 0;
    let bytes = src.as_bytes();
    let len = src.len();

    while pos < len {
        // Look for the next interesting character.
        match bytes[pos] {
            b'\\' => {
                let rest = &src[pos..];
                if let Some((inlines, consumed)) = try_parse_inline_command(rest) {
                    result.extend(inlines);
                    pos += consumed;
                } else {
                    // Unknown command — emit as literal text up to next space/brace.
                    let end = rest[1..]
                        .find(|c: char| !c.is_ascii_alphabetic())
                        .map(|i| pos + 1 + i)
                        .unwrap_or(len);
                    // Skip the command silently (unknown macro).
                    pos = end;
                    // Skip optional `{…}` argument if present.
                    let remaining = src[pos..].trim_start();
                    if remaining.starts_with('{')
                        && let Some(arg_len) = braced_len(remaining)
                    {
                        pos += src[pos..].len() - remaining.len() + arg_len;
                    }
                }
            }
            b'{' => {
                // Group: check for `{\bf …}` / `{\it …}` etc.
                let rest = &src[pos..];
                if let Some((inlines, consumed)) = try_parse_brace_group(rest) {
                    result.extend(inlines);
                    pos += consumed;
                } else {
                    // Plain group — parse contents transparently.
                    if let Some(inner_len) = braced_len(rest) {
                        let inner = &rest[1..inner_len - 1];
                        result.extend(parse_inlines(inner));
                        pos += inner_len;
                    } else {
                        result.push(Inline::Text("{".to_string()));
                        pos += 1;
                    }
                }
            }
            _ => {
                // Collect plain text until the next `\` or `{`.
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

/// Try to parse an inline command starting with `\` from the beginning of
/// `src`. Returns `(inlines, bytes_consumed)` on success.
fn try_parse_inline_command(src: &str) -> Option<(Vec<Inline>, usize)> {
    // \textbf{…}
    if let Some(r) = src.strip_prefix("\\textbf") {
        let r = r.trim_start_matches(' ');
        if let Some(arg_len) = braced_len(r) {
            let inner = &r[1..arg_len - 1];
            let consumed = src.len() - r.len() + arg_len;
            return Some((vec![Inline::Bold(parse_inlines(inner))], consumed));
        }
    }
    // \textit{…}  /  \emph{…}
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
    // \label{…}, \ref{…}, \cite{…} — emit placeholder
    for cmd in &["\\label", "\\ref", "\\cite"] {
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

/// Try to parse a `{…}` group that starts with a font switch like `{\bf …}`
/// or `{\it …}`.
fn try_parse_brace_group(src: &str) -> Option<(Vec<Inline>, usize)> {
    if !src.starts_with('{') {
        return None;
    }
    let inner_src = &src[1..];
    // Look for font switch at the start of the group.
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

    // Find the matching closing `}` for the outer group.
    let total_group_len = braced_len(src)?;
    // content is everything between the font switch and the closing `}`.
    let content_end = total_group_len - 1; // index of closing `}`
    let content_start = src.len() - inner_src.len() + (inner_src.len() - content_src.len());
    let content = &src[content_start..content_end];
    let inlines = parse_inlines(content);
    Some((vec![wrapper(inlines)], total_group_len))
}

/// Return the byte length of a `{…}` group at the start of `src` (including
/// both braces), or `None` if `src` doesn't start with `{`.
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

/// Collapse runs of whitespace (including newlines) into a single space.
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
        // After stripping the comment and joining the paragraph we get "Hello  world."
        // with possible extra space; normalize_whitespace reduces it.
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
}
