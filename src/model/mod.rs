/// Intermediate representation of a parsed LaTeX document.
///
/// This AST is the only contract between the parser and the renderer.
/// Neither stage should import types from the other.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Document {
    /// Top-level blocks in document order.
    pub blocks: Vec<Block>,
}

/// A block-level element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// A section heading at the given nesting level.
    ///
    /// `level` is 1-based: 1 = `\section`, 2 = `\subsection`, 3 = `\subsubsection`.
    Section { level: u8, title: Vec<Inline> },
    /// A body paragraph consisting of inline elements.
    Paragraph(Vec<Inline>),
}

/// An inline text element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inline {
    /// Plain text span.
    Text(String),
    /// Bold text — may contain nested inlines.
    Bold(Vec<Inline>),
    /// Italic text — may contain nested inlines.
    Italic(Vec<Inline>),
}
