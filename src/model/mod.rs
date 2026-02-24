/// Intermediate representation of a parsed LaTeX document.
///
/// This AST is the only contract between the parser and the renderer.
/// Neither stage should import types from the other.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Document {
    /// Top-level blocks in document order.
    pub blocks: Vec<Block>,
    /// Effective layout settings extracted from LaTeX project sources.
    pub layout: DocumentLayout,
}

/// Rendering-related layout settings extracted from LaTeX sources.
///
/// Every `Option` field defaults to `None`, meaning "the LaTeX source did not
/// express a preference — the renderer should use its own fallback default."
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DocumentLayout {
    // ── Page geometry ──────────────────────────────────────────────────
    /// Top page margin in twips.
    pub page_margin_top_twips: Option<i32>,
    /// Bottom page margin in twips.
    pub page_margin_bottom_twips: Option<i32>,
    /// Left page margin in twips.
    pub page_margin_left_twips: Option<i32>,
    /// Right page margin in twips.
    pub page_margin_right_twips: Option<i32>,
    /// Header distance in twips.
    pub page_margin_header_twips: Option<i32>,
    /// Footer distance in twips.
    pub page_margin_footer_twips: Option<i32>,

    // ── Line spacing ───────────────────────────────────────────────────
    /// Body paragraph line spacing in twips (`240 = single`, `360 = 1.5`).
    pub body_line_spacing_twips: Option<i32>,

    // ── Float counter scoping ──────────────────────────────────────────
    /// Whether figure numbers are scoped per chapter (`true`) or global (`false`).
    ///
    /// Maps to `\counterwithin{figure}{chapter}` vs `\counterwithout{figure}{chapter}`.
    pub figure_counter_within_chapter: Option<bool>,
    /// Whether table numbers are scoped per chapter (`true`) or global (`false`).
    pub table_counter_within_chapter: Option<bool>,
    /// Whether equation numbers are scoped per chapter (`true`) or global (`false`).
    pub equation_counter_within_chapter: Option<bool>,

    // ── Page size ─────────────────────────────────────────────────────
    /// Page width in twips. Parsed from `\geometry{paperwidth=...}` or
    /// `\documentclass[a4paper]`.
    pub page_width_twips: Option<u32>,
    /// Page height in twips.
    pub page_height_twips: Option<u32>,

    // ── Font ───────────────────────────────────────────────────────────
    /// Main (serif) font family name. Parsed from `\setmainfont{...}` or
    /// `\renewcommand{\rmdefault}{...}`.
    pub font_family_body: Option<String>,
    /// Monospace font family name. Parsed from `\setmonofont{...}`.
    pub font_family_mono: Option<String>,
    /// Body font size in half-points (e.g. `28` = 14 pt).
    /// Parsed from `\documentclass[14pt]` or `\fontsize`.
    pub font_size_body_hp: Option<usize>,
    /// Table cell font size in half-points.
    pub font_size_table_hp: Option<usize>,
    /// Footnote font size in half-points.
    pub font_size_footnote_hp: Option<usize>,
    /// Caption font size in half-points.
    pub font_size_caption_hp: Option<usize>,

    // ── Paragraph indent ───────────────────────────────────────────────
    /// First-line indent for body paragraphs in twips.
    /// Parsed from `\setlength{\parindent}{...}` or `\parindent=...`.
    pub body_first_line_indent_twips: Option<i32>,

    // ── Caption / float labels ─────────────────────────────────────────
    /// Figure caption prefix, e.g. `"Figure"` or `"Рисунок"`.
    /// Parsed from `\renewcommand{\figurename}{...}`.
    pub caption_label_figure: Option<String>,
    /// Table caption prefix, e.g. `"Table"` or `"Таблица"`.
    /// Parsed from `\renewcommand{\tablename}{...}`.
    pub caption_label_table: Option<String>,
    /// Figure caption label separator text (e.g. `". "`, `": "`, `" --- "`).
    /// Parsed from `\captionsetup[figure]{labelsep=...}` and related declarations.
    pub caption_label_separator_figure: Option<String>,
    /// Table caption label separator text.
    /// Parsed from `\captionsetup[table]{labelsep=...}` and related declarations.
    pub caption_label_separator_table: Option<String>,
    /// Figure caption spacing (`skip`) in twips.
    /// Parsed from `\captionsetup[figure]{skip=...}` with global fallback.
    pub caption_skip_twips_figure: Option<i32>,
    /// Table caption spacing (`skip`) in twips.
    /// Parsed from `\captionsetup[table]{skip=...}` with global fallback.
    pub caption_skip_twips_table: Option<i32>,
    /// Figure caption position (`"top"` or `"bottom"`).
    /// Parsed from `\captionsetup[figure]{position=...}` with global fallback.
    pub caption_position_figure: Option<String>,
    /// Table caption position (`"top"` or `"bottom"`).
    /// Parsed from `\captionsetup[table]{position=...}` with global fallback.
    pub caption_position_table: Option<String>,
    /// Whether single-line figure captions should be centered.
    /// Parsed from `\captionsetup[figure]{singlelinecheck=...}` with global fallback.
    pub caption_singlelinecheck_figure: Option<bool>,
    /// Whether single-line table captions should be centered.
    /// Parsed from `\captionsetup[table]{singlelinecheck=...}` with global fallback.
    pub caption_singlelinecheck_table: Option<bool>,
    /// Figure caption indent in twips.
    /// Parsed from `\captionsetup[figure]{indent=...}` with global fallback.
    pub caption_indent_twips_figure: Option<i32>,
    /// Table caption indent in twips.
    /// Parsed from `\captionsetup[table]{indent=...}` with global fallback.
    pub caption_indent_twips_table: Option<i32>,

    // ── Heading formatting ─────────────────────────────────────────────
    /// Chapter name prefix (e.g. `"Глава"`, `"Chapter"`, `""`).
    /// Parsed from `\renewcommand{\chaptername}{...}`.
    pub chapter_name: Option<String>,
    /// Whether chapter titles should be rendered in uppercase.
    /// Detected from `\MakeUppercase` in chapter format definitions.
    pub heading_uppercase: Option<bool>,
    /// Heading alignment (e.g. `"left"`, `"center"`, `"both"`).
    /// Detected from titlesec/memoir heading format commands.
    pub heading_alignment: Option<String>,
    /// Delimiter after heading number (e.g. `"."`, `""`, `":"`, `" —"`).
    /// Parsed from `\thechapter` or heading format definitions.
    pub heading_number_delimiter: Option<String>,

    // ── List formatting ──────────────────────────────────────────────
    /// Left indent for list items in twips.
    pub list_left_indent_twips: Option<i32>,
    /// Hanging indent for list items in twips.
    pub list_hanging_indent_twips: Option<i32>,

    // ── Caption alignment ────────────────────────────────────────────
    /// Caption alignment (e.g. `"center"`, `"left"`, `"both"`).
    /// Parsed from `\captionsetup{justification=centering}`.
    pub caption_alignment: Option<String>,

    // ── Graphics paths ───────────────────────────────────────────────
    /// Search paths for included graphics, parsed from `\graphicspath{{./figures/}{./img/}}`.
    pub graphics_search_paths: Vec<String>,

    // ── Document language ──────────────────────────────────────────────
    /// BCP-47 language tag (e.g. `"ru-RU"`, `"en-US"`).
    /// Parsed from `\usepackage[russian]{babel}` or `\setdefaultlanguage{...}`.
    pub document_language: Option<String>,
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
