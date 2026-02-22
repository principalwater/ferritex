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
    /// A table with an optional caption and rows of cells.
    Table(Table),
    /// A figure with an optional caption (image embedding is not yet supported).
    Figure(Figure),
    /// A bullet or numbered list.
    List(List),
}

/// A table block.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Table {
    /// Caption text, if any (`\caption{…}` before or inside the float).
    pub caption: Vec<Inline>,
    /// Source/attribution line (`\tablesource{…}`), if present.
    pub source: Vec<Inline>,
    /// Rows of cells; each cell contains inline content.
    pub rows: Vec<TableRow>,
}

/// A single row in a table.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TableRow {
    /// Ordered cells in this row.
    pub cells: Vec<TableCell>,
}

/// A single cell in a table row.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TableCell {
    /// Inline content of the cell.
    pub content: Vec<Inline>,
}

/// A figure block (image reference + caption).
///
/// ferritex does not embed images in v0.2; the image path is stored for
/// future use when image embedding is implemented.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Figure {
    /// Relative path from `\includegraphics[…]{path}`, if present.
    pub image_path: Option<String>,
    /// Caption text from `\caption{…}`, if any.
    pub caption: Vec<Inline>,
    /// Source/attribution line (`\figuresource{…}`), if present.
    pub source: Vec<Inline>,
}

/// A bullet or numbered list block.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct List {
    /// `true` = `\begin{enumerate}` (numbered), `false` = `\begin{itemize}` (bullet).
    pub ordered: bool,
    /// The list items in document order.
    pub items: Vec<Vec<Inline>>,
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
