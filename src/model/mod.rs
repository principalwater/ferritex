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
    /// Optional table of contents entries extracted from sidecar files (e.g. `.toc`).
    ///
    /// When empty, renderer should fall back to deriving TOC from section blocks.
    pub toc_entries: Vec<TocEntry>,
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
    /// Left indent for section headings (`\section`) in twips.
    /// Parsed from `\setsecindent{...}`.
    pub heading_indent_section_twips: Option<i32>,
    /// Left indent for subsection headings (`\subsection`) in twips.
    /// Parsed from `\setsubsecindent{...}`.
    pub heading_indent_subsection_twips: Option<i32>,
    /// Left indent for subsubsection headings (`\subsubsection`) in twips.
    /// Parsed from `\setsubsubsecindent{...}`.
    pub heading_indent_subsubsection_twips: Option<i32>,

    // ── Table of contents formatting ────────────────────────────────────
    /// Extra right margin for TOC title lines in twips.
    /// Parsed from `\setrmarg{...}` when present.
    pub toc_right_margin_twips: Option<i32>,
    /// Whether TOC page numbers should use dot leaders.
    /// Parsed from `\cft...leader` / `\cftdotfill` customizations.
    pub toc_use_dot_leader: Option<bool>,
    /// Optional chapter-name prefix for numbered chapter entries in TOC.
    /// Parsed from `\renewcommand*{\cftchaptername}{...}`.
    pub toc_chapter_name_prefix: Option<String>,
    /// TOC chapter entry left indent in twips.
    /// Parsed from `\setlength{\cftchapterindent}{...}` or `\cftsetindents{chapter}{...}{...}`.
    pub toc_indent_chapter_twips: Option<i32>,
    /// TOC chapter entry number width in twips.
    /// Parsed from `\setlength{\cftchapternumwidth}{...}` or `\cftsetindents{chapter}{...}{...}`.
    pub toc_numwidth_chapter_twips: Option<i32>,
    /// TOC section entry left indent in twips.
    /// Parsed from `\setlength{\cftsectionindent}{...}` or `\cftsetindents{section}{...}{...}`.
    pub toc_indent_section_twips: Option<i32>,
    /// TOC section entry number width in twips.
    /// Parsed from `\setlength{\cftsectionnumwidth}{...}` or `\cftsetindents{section}{...}{...}`.
    pub toc_numwidth_section_twips: Option<i32>,
    /// TOC subsection entry left indent in twips.
    /// Parsed from `\setlength{\cftsubsectionindent}{...}` or `\cftsetindents{subsection}{...}{...}`.
    pub toc_indent_subsection_twips: Option<i32>,
    /// TOC subsection entry number width in twips.
    /// Parsed from `\setlength{\cftsubsectionnumwidth}{...}` or `\cftsetindents{subsection}{...}{...}`.
    pub toc_numwidth_subsection_twips: Option<i32>,
    /// TOC subsubsection entry left indent in twips.
    /// Parsed from `\setlength{\cftsubsubsectionindent}{...}` or `\cftsetindents{subsubsection}{...}{...}`.
    pub toc_indent_subsubsection_twips: Option<i32>,
    /// TOC subsubsection entry number width in twips.
    /// Parsed from `\setlength{\cftsubsubsectionnumwidth}{...}` or `\cftsetindents{subsubsection}{...}{...}`.
    pub toc_numwidth_subsubsection_twips: Option<i32>,
    /// Whether chapter TOC entry text should be bold.
    /// `Some(false)` when `\renewcommand{\cftchapterfont}{\normalfont}` is present.
    pub toc_chapter_entry_bold: Option<bool>,
    /// Whether chapter TOC page-number should be bold.
    /// `Some(false)` when `\renewcommand{\cftchapterpagefont}{\normalfont}` is present.
    pub toc_chapter_page_bold: Option<bool>,
    /// Separator appended after chapter number in TOC.
    /// Extracted from `\renewcommand\cftchapteraftersnum{...}`.
    pub toc_aftersnum_chapter: Option<String>,
    /// Separator after section number in TOC.
    /// Extracted from `\renewcommand\cftsectionaftersnum{...}`.
    pub toc_aftersnum_section: Option<String>,
    /// Separator after subsection number in TOC.
    /// Extracted from `\renewcommand\cftsubsectionaftersnum{...}`.
    pub toc_aftersnum_subsection: Option<String>,
    /// Separator after subsubsection number in TOC.
    /// Extracted from `\renewcommand\cftsubsubsectionaftersnum{...}`.
    pub toc_aftersnum_subsubsection: Option<String>,
    /// Prefix for appendix entries in TOC.
    /// Extracted from `\renewcommand{\cftappendixname}{...}`.
    pub toc_appendix_name: Option<String>,

    // ── List formatting ──────────────────────────────────────────────
    /// Left indent for list items in twips.
    pub list_left_indent_twips: Option<i32>,
    /// Hanging indent for list items in twips.
    pub list_hanging_indent_twips: Option<i32>,
    /// Label separator in twips (from `\setlist{labelsep=...}`).
    pub list_label_sep_twips: Option<i32>,
    /// Label width in twips (from `\setlist{labelwidth=...}`; `None` if `!`/auto).
    pub list_label_width_twips: Option<i32>,
    /// Bullet character for unordered list items (from `\renewcommand{\labelitemi}{...}`).
    pub list_bullet_char: Option<String>,

    // ── Caption alignment ────────────────────────────────────────────
    /// Caption alignment (e.g. `"center"`, `"left"`, `"both"`).
    /// Parsed from `\captionsetup{justification=centering}`.
    pub caption_alignment: Option<String>,

    // ── Source attribution lines ─────────────────────────────────────
    /// Vertical space above `\tablesource` line in twips.
    /// Parsed from `\newcommand{\tablesource}[1]{\par\vspace{...}...}`.
    pub source_vspace_table_twips: Option<i32>,
    /// Vertical space above `\figuresource` line in twips.
    /// Parsed from `\newcommand{\figuresource}[1]{\par\vspace{...}...}`.
    pub source_vspace_figure_twips: Option<i32>,

    // ── Title page ──────────────────────────────────────────────────
    /// Whether to suppress the page number on the first (title) page.
    /// Detected from `\thispagestyle{empty}` inside `\begin{titlingpage}`.
    pub title_page_suppress_number: Option<bool>,

    // ── Graphics paths ───────────────────────────────────────────────
    /// Search paths for included graphics, parsed from `\graphicspath{{./figures/}{./img/}}`.
    pub graphics_search_paths: Vec<String>,

    // ── Document language ──────────────────────────────────────────────
    /// BCP-47 language tag (e.g. `"ru-RU"`, `"en-US"`).
    /// Parsed from `\usepackage[russian]{babel}` or `\setdefaultlanguage{...}`.
    pub document_language: Option<String>,
}

/// A single table-of-contents entry extracted from LaTeX auxiliary data.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TocEntry {
    /// Heading depth (1 = chapter/front matter, 2 = section, ...).
    pub level: u8,
    /// Optional visible heading number (`"1."`, `"1.2"`...).
    pub number: Option<String>,
    /// Visible entry title text.
    pub title: String,
    /// Optional source page number from `.toc` (`"14"` etc.).
    pub page: Option<String>,
}

/// Per-paragraph style overrides extracted from LaTeX declarations.
///
/// `None` means "inherit renderer defaults/profile".
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParagraphStyle {
    /// Paragraph alignment (`"left"`, `"center"`, `"right"`, `"both"`).
    pub alignment: Option<String>,
    /// First-line indent override in twips.
    pub first_line_indent_twips: Option<i32>,
    /// Line spacing override in twips.
    pub line_spacing_twips: Option<i32>,
    /// Space before paragraph in twips.
    pub space_before_twips: Option<i32>,
    /// Space after paragraph in twips.
    pub space_after_twips: Option<i32>,
    /// Paragraph font size override in half-points.
    pub font_size_hp: Option<usize>,
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
    /// A body paragraph with explicit style overrides extracted from LaTeX.
    StyledParagraph {
        inlines: Vec<Inline>,
        style: ParagraphStyle,
    },
    /// A table with an optional caption and rows of cells.
    Table(Table),
    /// A figure with an optional embedded image and caption.
    Figure(Figure),
    /// A bullet or numbered list.
    List(List),
    /// A display-math block from `\begin{equation}…\end{equation}` or `\[…\]`.
    /// Stored as raw LaTeX source.
    DisplayMath(String),
    /// A bibliography section rendered as a chapter-level heading.
    ///
    /// Emitted for `\printbibliography`, `\insertbibliofullsorted`, and similar commands.
    /// No bibliography entries are parsed — only the section heading is produced.
    BibliographyHeading { title: String },
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
    /// Preferred table alignment (`"left"`, `"center"`, `"right"`), if expressed by LaTeX.
    pub alignment: Option<String>,
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
    /// Preferred figure alignment (`"left"`, `"center"`, `"right"`), if expressed by LaTeX.
    pub alignment: Option<String>,
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
    /// Explicit line break (`\\`, `\newline`, `\linebreak`).
    LineBreak,
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
