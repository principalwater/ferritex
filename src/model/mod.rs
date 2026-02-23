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
    /// `level` is 1-based: 1 = `\chapter`, 2 = `\section`, 3 = `\subsection` / `\subsubsection`.
    ///
    /// `number` is `None` for unnumbered headings (`\section*`, `\chapter*`).
    /// For numbered headings, it stores the computed visible number (e.g. `2.1`).
    Section {
        level: u8,
        number: Option<String>,
        /// Optional label from a trailing `\label{...}` attached to heading.
        label: Option<String>,
        title: Vec<Inline>,
    },
    /// A body paragraph consisting of inline elements.
    Paragraph(Vec<Inline>),
    /// A table with an optional caption and rows of cells.
    Table(Table),
    /// A figure with an optional embedded image and caption.
    Figure(Figure),
    /// A bullet or numbered list.
    List(List),
    /// A display-math block from `\begin{equation}…\end{equation}` or `\[…\]`.
    /// Stored as raw LaTeX source.
    DisplayMath(String),
}

/// A table block.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Table {
    /// Caption text, if any (`\caption{…}` before or inside the float).
    pub caption: Vec<Inline>,
    /// Optional `\label{...}` attached to this table float.
    pub label: Option<String>,
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
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Figure {
    /// Relative path from `\includegraphics[…]{path}`, if present.
    pub image_path: Option<String>,
    /// Optional requested width from `\includegraphics[width=...]{...}`.
    ///
    /// Stored as thousandths of text width (`1000` = `1.0\textwidth`).
    pub width_permille: Option<u16>,
    /// Caption text from `\caption{…}`, if any.
    pub caption: Vec<Inline>,
    /// Optional `\label{...}` attached to this figure float.
    pub label: Option<String>,
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
#[allow(clippy::enum_variant_names)]
pub enum Inline {
    /// Plain text span.
    Text(String),
    /// Bold text — may contain nested inlines.
    Bold(Vec<Inline>),
    /// Italic text — may contain nested inlines.
    Italic(Vec<Inline>),
    /// Inline math from `$…$` — stored as raw LaTeX source.
    InlineMath(String),
    /// Cross-reference target from commands like `\ref{label}`.
    ///
    /// Resolved to plain text in a post-parse pass.
    Reference(String),
    /// Footnote from `\footnote{…}`.
    Footnote(Vec<Inline>),
}
