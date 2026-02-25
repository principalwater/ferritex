use std::{
    collections::HashMap,
    fs::File,
    io::{Cursor, Read, Write},
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
};

use anyhow::{Context, anyhow};
use docx_rs::{
    AbstractNumbering, AlignmentType, BreakType, Docx, Footnote, Header, Hyperlink, HyperlinkType,
    IndentLevel, Level, LevelJc, LevelText, LineSpacing, LineSpacingType, NumberFormat, Numbering,
    NumberingId, PageMargin, PageNum, Paragraph, Pic, Run, RunChild, RunFonts, SpecialIndentType,
    Start, Style, StyleType, Tab, TabLeaderType, TabValueType, Table as DocxTable,
    TableAlignmentType, TableCell as DocxCell, TableRow as DocxRow, VertAlignType,
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

use ferritex_core::model::{
    Block, Document, DocumentLayout, Figure, Inline, ParagraphStyle, Table,
};

const PAGE_A4_WIDTH_TWIPS: u32 = 11_906;
const PAGE_A4_HEIGHT_TWIPS: u32 = 16_838;

const DEFAULT_PAGE_MARGIN_TOP_TWIPS: i32 = 979;
const DEFAULT_PAGE_MARGIN_BOTTOM_TWIPS: i32 = 922;
const DEFAULT_PAGE_MARGIN_LEFT_TWIPS: i32 = 1_138;
const DEFAULT_PAGE_MARGIN_RIGHT_TWIPS: i32 = 288;
const DEFAULT_PAGE_MARGIN_HEADER_TWIPS: i32 = 342;
const DEFAULT_PAGE_MARGIN_FOOTER_TWIPS: i32 = 342;
const DEFAULT_PAGE_GUTTER_TWIPS: i32 = 0;
const FONT_SIZE_BODY_HP: usize = 28;
const FONT_SIZE_TABLE_HP: usize = 24;
const FONT_SIZE_FOOTNOTE_HP: usize = 20;
const DEFAULT_TOC_DEPTH: i32 = 2;
const DEFAULT_HYPERLINK_TEXT_COLOR: &str = "000000";
const DEFAULT_HYPERLINK_UNDERLINE: bool = false;
const DEFAULT_CAPTION_LABEL_BOLD: bool = true;

const LINE_SPACING_SINGLE_TWIPS: i32 = 240;
const LINE_SPACING_DEFAULT_BODY_TWIPS: i32 = 360;
/// DOCX conversion constant: 1 point = 20 twips.
const DOCX_TWIPS_PER_POINT_F64: f64 = 20.0;

const FIRST_LINE_INDENT_TWIPS: i32 = 709;
const DEFAULT_CAPTION_LABEL_SEPARATOR: &str = ". ";
const DEFAULT_CAPTION_SKIP_TWIPS: i32 = 0;
const DEFAULT_CAPTION_INDENT_TWIPS: i32 = 0;
/// Heuristic character budget used to estimate "single-line" captions when
/// `singlelinecheck=true` and no layout engine metrics are available.
const CAPTION_SINGLELINE_ESTIMATE_CHARS: usize = 80;
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "bmp", "tif", "tiff", "gif", "webp"];
const EMU_PER_TWIP: u32 = 635;
const IMAGE_SAFE_SCALE_NUM: u32 = 100;
const IMAGE_SAFE_SCALE_DEN: u32 = 100;
/// Default list left indent = body first-line indent (1.25 cm = 709 twips).
const DEFAULT_LIST_LEFT_TWIPS: i32 = 709;
/// Default list hanging indent = labelsep + labelwidth ≈ 2 × 0.5em at 14pt ≈ 284 twips.
const DEFAULT_LIST_HANGING_TWIPS: i32 = 284;
/// LaTeX default list label separator (enumitem/list geometry baseline): `0.5em`.
const DEFAULT_LIST_LABEL_SEP_EM: f64 = 0.5;
/// Default bullet character for unordered lists.
const DEFAULT_LIST_BULLET: &str = "•";
/// Default vertical space above `\tablesource` line in twips (4pt).
const DEFAULT_SOURCE_VSPACE_TABLE_TWIPS: i32 = 80;
/// Default vertical space above `\figuresource` line in twips (2pt).
const DEFAULT_SOURCE_VSPACE_FIGURE_TWIPS: i32 = 40;
const LIST_NUM_ID_BASE: usize = 100;
const DEFAULT_TOC_RIGHT_MARGIN_TWIPS: i32 = 0;
/// Default: chapter TOC entry text is bold (memoir/tocloft default when no `\cftchapterfont` override).
const DEFAULT_TOC_CHAPTER_ENTRY_BOLD: bool = true;
/// Default: chapter page number in TOC is bold (memoir/tocloft default when no `\cftchapterpagefont` override).
const DEFAULT_TOC_CHAPTER_PAGE_BOLD: bool = true;
/// Approximate glyph width for TOC prefix-width estimation (in `em`).
const TOC_PREFIX_ESTIMATED_CHAR_WIDTH_EM: f64 = 0.45;
const FOOTNOTE_MARKER_RUN_XML: &str = "<w:r><w:rPr><w:vertAlign w:val=\"superscript\" /></w:rPr><w:footnoteRef/></w:r><w:r><w:t xml:space=\"preserve\"> </w:t></w:r>";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptionPosition {
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy)]
struct CaptionRenderSettings {
    default_alignment: AlignmentType,
    indent_twips: i32,
    skip_twips: i32,
    position: CaptionPosition,
    singlelinecheck: bool,
    label_bold: bool,
    footnote_font_size_hp: usize,
}

/// Fully resolved rendering settings derived from [`DocumentLayout`].
///
/// Every field has a concrete value — `from_layout()` substitutes fallback
/// defaults for any setting the LaTeX source did not express.
#[derive(Debug, Clone)]
struct RenderProfile {
    // ── Page geometry ──────────────────────────────────────────────────
    page_margin_top_twips: i32,
    page_margin_bottom_twips: i32,
    page_margin_left_twips: i32,
    page_margin_right_twips: i32,
    page_margin_header_twips: i32,
    page_margin_footer_twips: i32,
    page_gutter_twips: i32,
    body_line_spacing_twips: i32,

    // ── Float counter scoping ──────────────────────────────────────────
    figure_counter_within_chapter: bool,
    table_counter_within_chapter: bool,

    // ── Font ───────────────────────────────────────────────────────────
    font_family_body: String,
    font_size_body_hp: usize,
    font_size_table_hp: usize,
    font_size_footnote_hp: usize,
    font_size_caption_hp: usize,

    // ── Paragraph indent ───────────────────────────────────────────────
    body_first_line_indent_twips: i32,

    // ── Caption / float labels ─────────────────────────────────────────
    caption_label_figure: String,
    caption_label_table: String,
    caption_label_separator_figure: String,
    caption_label_separator_table: String,
    caption_label_bold_figure: bool,
    caption_label_bold_table: bool,
    caption_skip_twips_figure: i32,
    caption_skip_twips_table: i32,
    caption_position_figure: CaptionPosition,
    caption_position_table: CaptionPosition,
    caption_singlelinecheck_figure: bool,
    caption_singlelinecheck_table: bool,
    caption_indent_twips_figure: i32,
    caption_indent_twips_table: i32,

    // ── Heading formatting ─────────────────────────────────────────────
    chapter_name: String,
    heading_uppercase: bool,
    heading_alignment: AlignmentType,
    heading_number_delimiter: String,
    heading_number_delimiter_section: String,
    heading_number_delimiter_subsection: String,
    heading_number_delimiter_subsubsection: String,
    heading_indent_section_twips: i32,
    heading_indent_subsection_twips: i32,
    heading_indent_subsubsection_twips: i32,
    heading_space_before_chapter_twips: i32,
    heading_space_after_chapter_twips: i32,
    heading_space_before_section_twips: i32,
    heading_space_after_section_twips: i32,
    heading_space_before_subsection_twips: i32,
    heading_space_after_subsection_twips: i32,
    heading_space_before_subsubsection_twips: i32,
    heading_space_after_subsubsection_twips: i32,
    toc_right_margin_twips: i32,
    toc_depth: i32,
    toc_use_dot_leader: bool,
    toc_chapter_name_prefix: String,
    toc_indent_chapter_twips: i32,
    toc_numwidth_chapter_twips: i32,
    toc_chapter_space_before_twips: i32,
    toc_section_space_before_twips: i32,
    toc_subsection_space_before_twips: i32,
    toc_subsubsection_space_before_twips: i32,
    toc_indent_section_twips: i32,
    toc_numwidth_section_twips: i32,
    toc_indent_subsection_twips: i32,
    toc_numwidth_subsection_twips: i32,
    toc_indent_subsubsection_twips: i32,
    toc_numwidth_subsubsection_twips: i32,
    /// Whether chapter TOC entry text is bold. Driven by `\cftchapterfont`.
    toc_chapter_entry_bold: bool,
    /// Whether chapter TOC page number is bold. Driven by `\cftchapterpagefont`.
    toc_chapter_page_bold: bool,
    /// Separator after chapter number in TOC (e.g. `". "`). Driven by `\cftchapteraftersnum`.
    toc_aftersnum_chapter: String,
    /// Separator after section number in TOC. Driven by `\cftsectionaftersnum`.
    toc_aftersnum_section: String,
    /// Separator after subsection number in TOC. Driven by `\cftsubsectionaftersnum`.
    toc_aftersnum_subsection: String,
    /// Separator after subsubsection number in TOC. Driven by `\cftsubsubsectionaftersnum`.
    toc_aftersnum_subsubsection: String,
    /// Prefix for appendix entries in TOC. Driven by `\cftappendixname`.
    /// Applied for level-1 entries when chapter numbering looks appendix-like (`A`, `B`, ...).
    toc_appendix_name: String,

    // ── List formatting ──────────────────────────────────────────────
    /// Text indent for list paragraphs in twips (where list item text starts).
    list_left_indent_twips: i32,
    /// Hanging indent width for list paragraphs in twips (label box + separator span).
    list_hanging_indent_twips: i32,
    /// Bullet character for unordered list items. Driven by `\renewcommand{\labelitemi}{...}`.
    list_bullet_char: String,

    // ── Source attribution lines ─────────────────────────────────────
    /// Vertical space above table source line in twips. Driven by `\tablesource` vspace.
    source_vspace_table_twips: i32,
    /// Vertical space above figure source line in twips. Driven by `\figuresource` vspace.
    source_vspace_figure_twips: i32,

    // ── Title page ──────────────────────────────────────────────────
    /// Whether to suppress page number on the first page. Driven by `\thispagestyle{empty}`.
    title_page_suppress_number: bool,
    page_number_alignment: AlignmentType,

    // ── Caption alignment ────────────────────────────────────────────
    caption_alignment: AlignmentType,
    body_text_alignment: AlignmentType,

    // ── Hyperlinks ────────────────────────────────────────────────────
    hyperlink_text_color: String,
    hyperlink_underline: bool,

    // ── Page size ────────────────────────────────────────────────────
    page_width_twips: u32,
    page_height_twips: u32,

    // ── Graphics search paths ────────────────────────────────────────
    graphics_search_paths: Vec<String>,

    // ── Document language ──────────────────────────────────────────────
    document_language: Option<String>,
}

impl RenderProfile {
    fn from_layout(layout: &DocumentLayout) -> Self {
        let derived_list_hanging_indent_twips =
            layout.list_hanging_indent_twips.unwrap_or_else(|| {
                if layout.list_label_sep_twips.is_none() && layout.list_label_width_twips.is_none()
                {
                    DEFAULT_LIST_HANGING_TWIPS
                } else {
                    let sep = layout.list_label_sep_twips.unwrap_or_else(|| {
                        // Fallback when `labelsep` is not present in LaTeX source.
                        // Mirrors parser-side em conversion based on current body font size.
                        em_to_twips(
                            layout.font_size_body_hp.unwrap_or(FONT_SIZE_BODY_HP),
                            DEFAULT_LIST_LABEL_SEP_EM,
                        )
                    });
                    let width = layout.list_label_width_twips.unwrap_or(sep); // auto (!) = labelsep
                    sep + width
                }
            });
        let body_first_line_indent_twips = layout
            .body_first_line_indent_twips
            .unwrap_or(DEFAULT_LIST_LEFT_TWIPS);
        let list_left_margin_twips =
            sanitize_nonnegative_twips(layout.list_left_indent_twips, body_first_line_indent_twips);
        let list_hanging_indent_twips = sanitize_nonnegative_twips(
            layout.list_hanging_indent_twips,
            derived_list_hanging_indent_twips,
        );
        let list_item_indent_twips = sanitize_nonnegative_twips(
            layout.list_item_indent_twips,
            if layout.list_left_indent_twips.is_some() {
                list_hanging_indent_twips
            } else {
                0
            },
        );
        let list_text_indent_twips = list_left_margin_twips.saturating_add(list_item_indent_twips);
        Self {
            page_margin_top_twips: sanitize_twips(
                layout.page_margin_top_twips,
                DEFAULT_PAGE_MARGIN_TOP_TWIPS,
            ),
            page_margin_bottom_twips: sanitize_twips(
                layout.page_margin_bottom_twips,
                DEFAULT_PAGE_MARGIN_BOTTOM_TWIPS,
            ),
            page_margin_left_twips: sanitize_twips(
                layout.page_margin_left_twips,
                DEFAULT_PAGE_MARGIN_LEFT_TWIPS,
            ),
            page_margin_right_twips: sanitize_twips(
                layout.page_margin_right_twips,
                DEFAULT_PAGE_MARGIN_RIGHT_TWIPS,
            ),
            page_margin_header_twips: sanitize_twips(
                layout.page_margin_header_twips,
                DEFAULT_PAGE_MARGIN_HEADER_TWIPS,
            ),
            page_margin_footer_twips: sanitize_twips(
                layout.page_margin_footer_twips,
                DEFAULT_PAGE_MARGIN_FOOTER_TWIPS,
            ),
            page_gutter_twips: sanitize_nonnegative_twips(
                layout.page_gutter_twips,
                DEFAULT_PAGE_GUTTER_TWIPS,
            ),
            body_line_spacing_twips: sanitize_twips(
                layout.body_line_spacing_twips,
                LINE_SPACING_DEFAULT_BODY_TWIPS,
            ),
            // Default: within-chapter scoping (matches standard LaTeX behaviour).
            figure_counter_within_chapter: layout.figure_counter_within_chapter.unwrap_or(true),
            table_counter_within_chapter: layout.table_counter_within_chapter.unwrap_or(true),
            // Font: fallback to Times New Roman 14pt (common academic default).
            font_family_body: layout
                .font_family_body
                .clone()
                .unwrap_or_else(|| "Times New Roman".to_string()),
            font_size_body_hp: layout.font_size_body_hp.unwrap_or(FONT_SIZE_BODY_HP),
            font_size_table_hp: layout.font_size_table_hp.unwrap_or(FONT_SIZE_TABLE_HP),
            font_size_footnote_hp: layout
                .font_size_footnote_hp
                .unwrap_or(FONT_SIZE_FOOTNOTE_HP),
            font_size_caption_hp: layout.font_size_caption_hp.unwrap_or(FONT_SIZE_BODY_HP),
            // Paragraph indent: fallback 1.25 cm.
            body_first_line_indent_twips: layout
                .body_first_line_indent_twips
                .unwrap_or(FIRST_LINE_INDENT_TWIPS),
            // Caption labels: fallback to English.
            caption_label_figure: layout
                .caption_label_figure
                .clone()
                .unwrap_or_else(|| "Figure".to_string()),
            caption_label_table: layout
                .caption_label_table
                .clone()
                .unwrap_or_else(|| "Table".to_string()),
            caption_label_separator_figure: layout
                .caption_label_separator_figure
                .clone()
                .unwrap_or_else(|| DEFAULT_CAPTION_LABEL_SEPARATOR.to_string()),
            caption_label_separator_table: layout
                .caption_label_separator_table
                .clone()
                .unwrap_or_else(|| DEFAULT_CAPTION_LABEL_SEPARATOR.to_string()),
            caption_label_bold_figure: layout
                .caption_label_bold_figure
                .unwrap_or(DEFAULT_CAPTION_LABEL_BOLD),
            caption_label_bold_table: layout
                .caption_label_bold_table
                .unwrap_or(DEFAULT_CAPTION_LABEL_BOLD),
            caption_skip_twips_figure: sanitize_nonnegative_twips(
                layout.caption_skip_twips_figure,
                DEFAULT_CAPTION_SKIP_TWIPS,
            ),
            caption_skip_twips_table: sanitize_nonnegative_twips(
                layout.caption_skip_twips_table,
                DEFAULT_CAPTION_SKIP_TWIPS,
            ),
            caption_position_figure: parse_caption_position(
                layout.caption_position_figure.as_deref(),
                CaptionPosition::Bottom,
            ),
            caption_position_table: parse_caption_position(
                layout.caption_position_table.as_deref(),
                CaptionPosition::Top,
            ),
            caption_singlelinecheck_figure: layout.caption_singlelinecheck_figure.unwrap_or(true),
            caption_singlelinecheck_table: layout.caption_singlelinecheck_table.unwrap_or(true),
            caption_indent_twips_figure: sanitize_nonnegative_twips(
                layout.caption_indent_twips_figure,
                DEFAULT_CAPTION_INDENT_TWIPS,
            ),
            caption_indent_twips_table: sanitize_nonnegative_twips(
                layout.caption_indent_twips_table,
                DEFAULT_CAPTION_INDENT_TWIPS,
            ),
            // Heading: no chapter prefix by default, no uppercase, left-aligned.
            chapter_name: layout.chapter_name.clone().unwrap_or_default(),
            heading_uppercase: layout.heading_uppercase.unwrap_or(false),
            heading_alignment: parse_alignment(
                layout.heading_alignment.as_deref().unwrap_or("left"),
            ),
            heading_number_delimiter: layout
                .heading_number_delimiter
                .clone()
                .unwrap_or_else(|| ".".to_string()),
            heading_number_delimiter_section: layout
                .heading_number_delimiter_section
                .clone()
                .unwrap_or_default(),
            heading_number_delimiter_subsection: layout
                .heading_number_delimiter_subsection
                .clone()
                .unwrap_or_default(),
            heading_number_delimiter_subsubsection: layout
                .heading_number_delimiter_subsubsection
                .clone()
                .unwrap_or_default(),
            heading_indent_section_twips: sanitize_nonnegative_twips(
                layout.heading_indent_section_twips,
                0,
            ),
            heading_indent_subsection_twips: sanitize_nonnegative_twips(
                layout.heading_indent_subsection_twips,
                0,
            ),
            heading_indent_subsubsection_twips: sanitize_nonnegative_twips(
                layout.heading_indent_subsubsection_twips,
                0,
            ),
            heading_space_before_chapter_twips: sanitize_nonnegative_twips(
                layout.heading_space_before_chapter_twips,
                0,
            ),
            heading_space_after_chapter_twips: sanitize_nonnegative_twips(
                layout.heading_space_after_chapter_twips,
                0,
            ),
            heading_space_before_section_twips: sanitize_nonnegative_twips(
                layout.heading_space_before_section_twips,
                0,
            ),
            heading_space_after_section_twips: sanitize_nonnegative_twips(
                layout.heading_space_after_section_twips,
                0,
            ),
            heading_space_before_subsection_twips: sanitize_nonnegative_twips(
                layout.heading_space_before_subsection_twips,
                0,
            ),
            heading_space_after_subsection_twips: sanitize_nonnegative_twips(
                layout.heading_space_after_subsection_twips,
                0,
            ),
            heading_space_before_subsubsection_twips: sanitize_nonnegative_twips(
                layout.heading_space_before_subsubsection_twips,
                0,
            ),
            heading_space_after_subsubsection_twips: sanitize_nonnegative_twips(
                layout.heading_space_after_subsubsection_twips,
                0,
            ),
            toc_right_margin_twips: sanitize_nonnegative_twips(
                layout.toc_right_margin_twips,
                DEFAULT_TOC_RIGHT_MARGIN_TWIPS,
            ),
            toc_depth: layout.toc_depth.unwrap_or(DEFAULT_TOC_DEPTH).clamp(-1, 6),
            toc_use_dot_leader: layout.toc_use_dot_leader.unwrap_or(true),
            toc_chapter_name_prefix: layout.toc_chapter_name_prefix.clone().unwrap_or_default(),
            toc_indent_chapter_twips: sanitize_nonnegative_twips(
                layout.toc_indent_chapter_twips,
                0,
            ),
            toc_numwidth_chapter_twips: sanitize_nonnegative_twips(
                layout.toc_numwidth_chapter_twips,
                0,
            ),
            toc_chapter_space_before_twips: sanitize_nonnegative_twips(
                layout.toc_chapter_space_before_twips,
                0,
            ),
            toc_section_space_before_twips: sanitize_nonnegative_twips(
                layout.toc_section_space_before_twips,
                0,
            ),
            toc_subsection_space_before_twips: sanitize_nonnegative_twips(
                layout.toc_subsection_space_before_twips,
                0,
            ),
            toc_subsubsection_space_before_twips: sanitize_nonnegative_twips(
                layout.toc_subsubsection_space_before_twips,
                0,
            ),
            toc_indent_section_twips: sanitize_nonnegative_twips(
                layout.toc_indent_section_twips,
                layout
                    .body_first_line_indent_twips
                    .unwrap_or(FIRST_LINE_INDENT_TWIPS)
                    / 2,
            ),
            toc_numwidth_section_twips: sanitize_nonnegative_twips(
                layout.toc_numwidth_section_twips,
                0,
            ),
            toc_indent_subsection_twips: sanitize_nonnegative_twips(
                layout.toc_indent_subsection_twips,
                layout
                    .body_first_line_indent_twips
                    .unwrap_or(FIRST_LINE_INDENT_TWIPS),
            ),
            toc_numwidth_subsection_twips: sanitize_nonnegative_twips(
                layout.toc_numwidth_subsection_twips,
                0,
            ),
            toc_indent_subsubsection_twips: sanitize_nonnegative_twips(
                layout.toc_indent_subsubsection_twips,
                layout
                    .body_first_line_indent_twips
                    .unwrap_or(FIRST_LINE_INDENT_TWIPS)
                    + layout
                        .body_first_line_indent_twips
                        .unwrap_or(FIRST_LINE_INDENT_TWIPS)
                        / 2,
            ),
            toc_numwidth_subsubsection_twips: sanitize_nonnegative_twips(
                layout.toc_numwidth_subsubsection_twips,
                0,
            ),
            // TOC entry bold/font: fallback = bold (memoir/tocloft default).
            toc_chapter_entry_bold: layout
                .toc_chapter_entry_bold
                .unwrap_or(DEFAULT_TOC_CHAPTER_ENTRY_BOLD),
            toc_chapter_page_bold: layout
                .toc_chapter_page_bold
                .unwrap_or(DEFAULT_TOC_CHAPTER_PAGE_BOLD),
            // TOC aftersnum separators: empty string = no explicit separator.
            toc_aftersnum_chapter: layout.toc_aftersnum_chapter.clone().unwrap_or_default(),
            toc_aftersnum_section: layout.toc_aftersnum_section.clone().unwrap_or_default(),
            toc_aftersnum_subsection: layout.toc_aftersnum_subsection.clone().unwrap_or_default(),
            toc_aftersnum_subsubsection: layout
                .toc_aftersnum_subsubsection
                .clone()
                .unwrap_or_default(),
            // TOC appendix prefix: empty = no appendix prefix.
            toc_appendix_name: layout.toc_appendix_name.clone().unwrap_or_default(),
            // List indentation: text-left = leftmargin + itemindent, hanging = labelsep + labelwidth.
            list_left_indent_twips: list_text_indent_twips,
            list_hanging_indent_twips,
            list_bullet_char: layout
                .list_bullet_char
                .clone()
                .unwrap_or_else(|| DEFAULT_LIST_BULLET.to_string()),
            // Source attribution spacing: driven by \tablesource / \figuresource vspace.
            source_vspace_table_twips: layout
                .source_vspace_table_twips
                .unwrap_or(DEFAULT_SOURCE_VSPACE_TABLE_TWIPS),
            source_vspace_figure_twips: layout
                .source_vspace_figure_twips
                .unwrap_or(DEFAULT_SOURCE_VSPACE_FIGURE_TWIPS),
            // Title page: suppress page number when \thispagestyle{empty} detected.
            title_page_suppress_number: layout.title_page_suppress_number.unwrap_or(false),
            page_number_alignment: parse_page_number_alignment(
                layout.page_number_alignment.as_deref(),
            ),
            // Caption alignment: default centred.
            caption_alignment: parse_alignment(
                layout.caption_alignment.as_deref().unwrap_or("center"),
            ),
            body_text_alignment: parse_body_alignment(layout.body_text_alignment.as_deref()),
            hyperlink_text_color: normalize_docx_color(
                layout.hyperlink_text_color.as_deref(),
                DEFAULT_HYPERLINK_TEXT_COLOR,
            ),
            hyperlink_underline: layout
                .hyperlink_underline
                .unwrap_or(DEFAULT_HYPERLINK_UNDERLINE),
            // Page size: default A4.
            page_width_twips: layout.page_width_twips.unwrap_or(PAGE_A4_WIDTH_TWIPS),
            page_height_twips: layout.page_height_twips.unwrap_or(PAGE_A4_HEIGHT_TWIPS),
            // Graphics search paths.
            graphics_search_paths: if layout.graphics_search_paths.is_empty() {
                vec![
                    "images".to_string(),
                    "figures".to_string(),
                    "img".to_string(),
                ]
            } else {
                layout.graphics_search_paths.clone()
            },
            // Language: None → no explicit language tag in DOCX.
            document_language: layout.document_language.clone(),
        }
    }

    fn max_image_width_emu(&self) -> u32 {
        let text_width_twips = self.page_width_twips.saturating_sub(
            self.page_margin_left_twips.max(0) as u32 + self.page_margin_right_twips.max(0) as u32,
        );
        text_width_twips * EMU_PER_TWIP * IMAGE_SAFE_SCALE_NUM / IMAGE_SAFE_SCALE_DEN
    }

    fn toc_page_tab_stop_twips(&self) -> usize {
        let text_width_twips = self.page_width_twips.saturating_sub(
            self.page_margin_left_twips.max(0) as u32 + self.page_margin_right_twips.max(0) as u32,
        );
        // Keep a minimal positive tab-stop even for malformed/degenerate geometry
        // to avoid emitting invalid TOC paragraph tabs in DOCX.
        let stop_twips = (text_width_twips as i32 - self.toc_right_margin_twips.max(0)).max(1_000);
        stop_twips as usize
    }

    /// Return the `aftersnum` separator string for a given TOC level (1-based).
    ///
    /// Driven by `\cft<level>aftersnum` in the LaTeX source.
    fn toc_level_aftersnum(&self, level: u8) -> &str {
        match level {
            1 => &self.toc_aftersnum_chapter,
            2 => &self.toc_aftersnum_section,
            3 => &self.toc_aftersnum_subsection,
            _ => &self.toc_aftersnum_subsubsection,
        }
    }

    fn toc_level_indent_twips(&self, level: u8) -> i32 {
        match level {
            1 => self.toc_indent_chapter_twips,
            2 => self.toc_indent_section_twips,
            3 => self.toc_indent_subsection_twips,
            4 => self.toc_indent_subsubsection_twips,
            5 => self.toc_indent_subsubsection_twips + self.body_first_line_indent_twips / 2,
            _ => self.toc_indent_subsubsection_twips + self.body_first_line_indent_twips,
        }
    }

    fn toc_level_numwidth_twips(&self, level: u8) -> i32 {
        match level {
            1 => self.toc_numwidth_chapter_twips,
            2 => self.toc_numwidth_section_twips,
            3 => self.toc_numwidth_subsection_twips,
            _ => self.toc_numwidth_subsubsection_twips,
        }
    }

    fn toc_level_space_before_twips(&self, level: u8) -> i32 {
        match level {
            1 => self.toc_chapter_space_before_twips,
            2 => self.toc_section_space_before_twips,
            3 => self.toc_subsection_space_before_twips,
            _ => self.toc_subsubsection_space_before_twips,
        }
    }

    fn heading_spacing_before_after(&self, level: u8) -> (Option<i32>, Option<i32>) {
        let (before, after) = match level {
            1 => (
                self.heading_space_before_chapter_twips,
                self.heading_space_after_chapter_twips,
            ),
            2 => (
                self.heading_space_before_section_twips,
                self.heading_space_after_section_twips,
            ),
            3 => (
                self.heading_space_before_subsection_twips
                    .max(self.heading_space_before_subsubsection_twips),
                self.heading_space_after_subsection_twips
                    .max(self.heading_space_after_subsubsection_twips),
            ),
            _ => (
                self.heading_space_before_subsubsection_twips,
                self.heading_space_after_subsubsection_twips,
            ),
        };
        ((before > 0).then_some(before), (after > 0).then_some(after))
    }

    fn heading_number_delimiter_for_level(&self, level: u8) -> &str {
        match level {
            1 => &self.heading_number_delimiter,
            2 => &self.heading_number_delimiter_section,
            3 => &self.heading_number_delimiter_subsection,
            _ => &self.heading_number_delimiter_subsubsection,
        }
    }

    fn toc_max_level(&self) -> u8 {
        (self.toc_depth + 1).clamp(0, 6) as u8
    }
}

#[derive(Debug, Clone, Default)]
struct ReferenceRenderIndex {
    label_values: HashMap<String, String>,
    label_bookmarks: HashMap<String, String>,
    section_bookmark_by_index: HashMap<usize, String>,
    toc_anchor_by_key: HashMap<String, String>,
}

impl ReferenceRenderIndex {
    fn resolve_reference(&self, label: &str) -> (String, Option<String>) {
        if let Some(value) = self.label_values.get(label) {
            return (value.clone(), self.label_bookmarks.get(label).cloned());
        }
        if let Some(value) = infer_appendix_label_value(label) {
            return (value, self.label_bookmarks.get(label).cloned());
        }
        (format!("[ref:{label}]"), None)
    }

    fn toc_anchor_for_entry(&self, level: u8, number: Option<&str>, title: &str) -> Option<String> {
        let key = toc_entry_lookup_key(level, number, title);
        self.toc_anchor_by_key.get(&key).cloned()
    }
}

fn build_reference_render_index(
    document: &Document,
    profile: &RenderProfile,
) -> ReferenceRenderIndex {
    let mut index = ReferenceRenderIndex::default();

    let mut chapter_no = 0usize;
    let mut figure_no = 0usize;
    let mut table_no = 0usize;
    let mut equation_no = 0usize;
    let mut synthetic_section_seq = 0usize;

    for (block_index, block) in document.blocks.iter().enumerate() {
        match block {
            Block::Section {
                level,
                number,
                label,
                title,
            } => {
                if *level == 1 && number.is_some() {
                    chapter_no += 1;
                    if profile.figure_counter_within_chapter {
                        figure_no = 0;
                    }
                    if profile.table_counter_within_chapter {
                        table_no = 0;
                    }
                    equation_no = 0;
                }

                let anchor = if let Some(label) = label {
                    bookmark_name_for_label(label)
                } else {
                    synthetic_section_seq += 1;
                    format!("fxt_sec_{synthetic_section_seq}")
                };
                index
                    .section_bookmark_by_index
                    .insert(block_index, anchor.clone());

                let title_text = collect_inline_text(title);
                let toc_key = toc_entry_lookup_key(*level, number.as_deref(), &title_text);
                index
                    .toc_anchor_by_key
                    .entry(toc_key)
                    .or_insert(anchor.clone());

                if let Some(label) = label {
                    index
                        .label_bookmarks
                        .entry(label.clone())
                        .or_insert(anchor.clone());
                    if let Some(number) = number.as_ref() {
                        let value = if *level == 1 {
                            number.trim_end_matches('.').to_string()
                        } else {
                            number.clone()
                        };
                        index.label_values.insert(label.clone(), value);
                    } else if let Some(app_value) = infer_appendix_label_value(label) {
                        index.label_values.insert(label.clone(), app_value);
                    }
                }
            }
            Block::Figure(figure) => {
                if let Some(label) = figure.label.as_ref() {
                    figure_no += 1;
                    let value = if profile.figure_counter_within_chapter && chapter_no > 0 {
                        format!("{chapter_no}.{figure_no}")
                    } else {
                        figure_no.to_string()
                    };
                    index.label_values.insert(label.clone(), value);
                    index
                        .label_bookmarks
                        .insert(label.clone(), bookmark_name_for_label(label));
                }
            }
            Block::Table(table) => {
                if let Some(label) = table.label.as_ref() {
                    table_no += 1;
                    let value = if profile.table_counter_within_chapter && chapter_no > 0 {
                        format!("{chapter_no}.{table_no}")
                    } else {
                        table_no.to_string()
                    };
                    index.label_values.insert(label.clone(), value);
                    index
                        .label_bookmarks
                        .insert(label.clone(), bookmark_name_for_label(label));
                }
            }
            Block::DisplayMath(body) => {
                equation_no += 1;
                let value = if chapter_no > 0 {
                    format!("{chapter_no}.{equation_no}")
                } else {
                    equation_no.to_string()
                };
                for label in extract_labels_from_display_math(body) {
                    index.label_values.insert(label.clone(), value.clone());
                    index
                        .label_bookmarks
                        .insert(label.clone(), bookmark_name_for_label(&label));
                }
            }
            Block::Paragraph(_)
            | Block::StyledParagraph { .. }
            | Block::List(_)
            | Block::BibliographyHeading { .. }
            | Block::TableOfContents => {}
        }
    }

    index
}

fn toc_entry_lookup_key(level: u8, number: Option<&str>, title: &str) -> String {
    let number = number.unwrap_or("").trim().trim_end_matches('.');
    let title = normalize_space_compact(title).to_lowercase();
    format!("{level}|{number}|{title}")
}

fn normalize_space_compact(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
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

fn bookmark_name_for_label(label: &str) -> String {
    let mut stem = String::new();
    for ch in label.chars() {
        if ch.is_ascii_alphanumeric() {
            stem.push(ch.to_ascii_lowercase());
        } else if matches!(ch, '_' | '-' | ':' | '.') && !stem.ends_with('_') {
            stem.push('_');
        }
    }
    if stem.is_empty() {
        stem.push_str("label");
    }
    let hash = label.bytes().fold(0xcbf29ce484222325u64, |acc, b| {
        (acc ^ b as u64).wrapping_mul(0x100000001b3)
    });
    let mut out = format!("fxt_{stem}_{:08x}", (hash & 0xffff_ffff) as u32);
    if out.len() > 40 {
        out.truncate(40);
    }
    if out.chars().next().is_some_and(|c| !c.is_ascii_alphabetic()) {
        out.insert(0, 'b');
    }
    out
}

fn extract_labels_from_display_math(src: &str) -> Vec<String> {
    let mut labels = Vec::new();
    let mut pos = 0usize;
    while pos < src.len() {
        let Some(rel) = src[pos..].find("\\label") else {
            break;
        };
        let start = pos + rel + "\\label".len();
        let mut cur = start;
        while cur < src.len() && src.as_bytes()[cur].is_ascii_whitespace() {
            cur += 1;
        }
        if cur < src.len()
            && src.as_bytes()[cur] == b'{'
            && let Some(close_rel) = src[cur + 1..].find('}')
        {
            let end = cur + 1 + close_rel;
            let value = src[cur + 1..end].trim();
            if !value.is_empty() {
                labels.push(value.to_string());
            }
            pos = end + 1;
            continue;
        }
        pos = start;
    }
    labels
}

/// Convert a string alignment name to a docx-rs [`AlignmentType`].
fn parse_alignment(s: &str) -> AlignmentType {
    match s.to_lowercase().as_str() {
        "left" | "flushleft" | "raggedright" => AlignmentType::Left,
        "center" | "centering" => AlignmentType::Center,
        "right" | "flushright" | "raggedleft" => AlignmentType::Right,
        "both" | "justify" | "justified" => AlignmentType::Both,
        _ => AlignmentType::Left,
    }
}

fn parse_body_alignment(value: Option<&str>) -> AlignmentType {
    match value.map(str::to_ascii_lowercase).as_deref() {
        Some("left") | Some("flushleft") | Some("raggedright") => AlignmentType::Left,
        Some("center") | Some("centering") => AlignmentType::Center,
        Some("right") | Some("flushright") | Some("raggedleft") => AlignmentType::Right,
        Some("both") | Some("justify") | Some("justified") => AlignmentType::Both,
        _ => AlignmentType::Both,
    }
}

fn parse_page_number_alignment(value: Option<&str>) -> AlignmentType {
    match value.map(str::to_ascii_lowercase).as_deref() {
        Some("left") => AlignmentType::Left,
        Some("right") => AlignmentType::Right,
        Some("center") | Some("centering") => AlignmentType::Center,
        _ => AlignmentType::Center,
    }
}

fn normalize_docx_color(value: Option<&str>, fallback: &str) -> String {
    let Some(raw) = value else {
        return fallback.to_string();
    };
    let token = raw.trim().trim_matches(['{', '}']).trim();
    if token.is_empty() {
        return fallback.to_string();
    }

    let normalized = token.trim_start_matches('#').to_ascii_lowercase();
    if normalized.len() == 6 && normalized.chars().all(|c| c.is_ascii_hexdigit()) {
        return normalized.to_ascii_uppercase();
    }

    let named = match normalized.as_str() {
        "black" => Some("000000"),
        "white" => Some("FFFFFF"),
        "red" => Some("FF0000"),
        "green" => Some("008000"),
        "blue" => Some("0000FF"),
        "cyan" => Some("00FFFF"),
        "magenta" => Some("FF00FF"),
        "yellow" => Some("FFFF00"),
        "gray" | "grey" => Some("808080"),
        "darkgray" | "darkgrey" => Some("404040"),
        "lightgray" | "lightgrey" => Some("C0C0C0"),
        "brown" => Some("A52A2A"),
        "orange" => Some("FFA500"),
        "purple" => Some("800080"),
        "teal" => Some("008080"),
        "violet" => Some("EE82EE"),
        _ => None,
    };

    named.unwrap_or(fallback).to_string()
}

fn apply_hyperlink_run_style(run: Run, profile: &RenderProfile) -> Run {
    let underline = if profile.hyperlink_underline {
        "single"
    } else {
        "none"
    };
    run.underline(underline)
        .color(&profile.hyperlink_text_color)
}

fn parse_table_alignment(s: Option<&str>) -> TableAlignmentType {
    match s.map(str::to_ascii_lowercase).as_deref() {
        Some("center") | Some("centering") => TableAlignmentType::Center,
        Some("right") | Some("raggedleft") | Some("flushright") => TableAlignmentType::Right,
        _ => TableAlignmentType::Left,
    }
}

fn parse_caption_position(value: Option<&str>, fallback: CaptionPosition) -> CaptionPosition {
    match value.map(str::to_ascii_lowercase).as_deref() {
        Some("top") | Some("above") => CaptionPosition::Top,
        Some("bottom") | Some("below") => CaptionPosition::Bottom,
        _ => fallback,
    }
}

fn sanitize_twips(value: Option<i32>, fallback: i32) -> i32 {
    match value {
        Some(v) if v > 0 => v,
        _ => fallback,
    }
}

fn sanitize_nonnegative_twips(value: Option<i32>, fallback: i32) -> i32 {
    match value {
        Some(v) if v >= 0 => v,
        _ => fallback,
    }
}

fn table_caption_settings(profile: &RenderProfile) -> CaptionRenderSettings {
    CaptionRenderSettings {
        default_alignment: profile.caption_alignment,
        indent_twips: profile.caption_indent_twips_table,
        skip_twips: profile.caption_skip_twips_table,
        position: profile.caption_position_table,
        singlelinecheck: profile.caption_singlelinecheck_table,
        label_bold: profile.caption_label_bold_table,
        footnote_font_size_hp: profile.font_size_footnote_hp,
    }
}

fn figure_caption_settings(profile: &RenderProfile) -> CaptionRenderSettings {
    CaptionRenderSettings {
        default_alignment: profile.caption_alignment,
        indent_twips: profile.caption_indent_twips_figure,
        skip_twips: profile.caption_skip_twips_figure,
        position: profile.caption_position_figure,
        singlelinecheck: profile.caption_singlelinecheck_figure,
        label_bold: profile.caption_label_bold_figure,
        footnote_font_size_hp: profile.font_size_footnote_hp,
    }
}

/// Render the intermediate [`Document`] AST to a `.docx` file at `output_path`.
///
/// DOCX structure rules (see AGENTS.md):
/// - Document defaults are configured for GOST-like typography (A4, margins, TNR 14pt, 1.5x spacing).
/// - Body paragraphs use the `Normal` style with justify alignment and first-line indent.
/// - Section headings use `Heading1` / `Heading2` / `Heading3` styles.
/// - Bold runs use `Run::bold()`, italic runs use `Run::italic()`.
/// - No `<w:sectPr>` is inserted inside paragraph properties.
pub fn render_docx(document: &Document, output_path: &Path) -> anyhow::Result<()> {
    render_docx_with_context(document, output_path, None)
}

/// Render the intermediate [`Document`] AST to a `.docx` file at `output_path`,
/// using `input_tex_path` as base for resolving figure files.
pub fn render_docx_with_context(
    document: &Document,
    output_path: &Path,
    input_tex_path: Option<&Path>,
) -> anyhow::Result<()> {
    let profile = RenderProfile::from_layout(&document.layout);
    let ref_index = build_reference_render_index(document, &profile);
    let mut docx = create_styled_docx(&profile);
    let figure_base_dir = input_tex_path.and_then(Path::parent);
    let mut chapter_no = 0usize;
    let mut table_no = 0usize;
    let mut figure_no = 0usize;
    let mut bookmark_ids: HashMap<String, usize> = HashMap::new();
    let mut next_bookmark_id: usize = 1;

    // Assign a stable numbering ID for each list block we encounter.
    // abstractNumId == numId for simplicity (one-to-one mapping).
    let mut next_num_id: usize = LIST_NUM_ID_BASE;
    let mut rendered_any_block = false;

    for (index, block) in document.blocks.iter().enumerate() {
        match block {
            Block::Section { level, number, .. } => {
                if *level == 1
                    && let Some(number) = number
                {
                    let parsed = number.trim_end_matches('.').parse::<usize>().ok();
                    chapter_no = parsed.unwrap_or(chapter_no + 1);
                    // Only reset float counters when the project requests within-chapter scoping.
                    // When the LaTeX source uses global (continuous) counters, never reset.
                    if profile.table_counter_within_chapter {
                        table_no = 0;
                    }
                    if profile.figure_counter_within_chapter {
                        figure_no = 0;
                    }
                }
                if *level == 1 && rendered_any_block {
                    docx = docx.add_paragraph(
                        Paragraph::new().add_run(Run::new().add_break(BreakType::Page)),
                    );
                }
                let section_bookmark = ref_index.section_bookmark_by_index.get(&index).cloned();
                let para = attach_bookmark_to_paragraph(
                    build_paragraph(block, &profile, &ref_index),
                    section_bookmark.as_deref(),
                    &mut bookmark_ids,
                    &mut next_bookmark_id,
                );
                docx = docx.add_paragraph(para);
                rendered_any_block = true;
            }
            Block::TableOfContents => {
                for toc_para in generated_toc_paragraphs(document, index + 1, &profile, &ref_index)
                {
                    docx = docx.add_paragraph(toc_para);
                }
                rendered_any_block = true;
            }
            Block::Paragraph(_) => {
                let para = build_paragraph(block, &profile, &ref_index);
                docx = docx.add_paragraph(para);
                rendered_any_block = true;
            }
            Block::StyledParagraph { .. } => {
                let para = build_paragraph(block, &profile, &ref_index);
                docx = docx.add_paragraph(para);
                rendered_any_block = true;
            }
            Block::Table(t) => {
                table_no += 1;
                let eff_chapter_tab = if profile.table_counter_within_chapter {
                    chapter_no
                } else {
                    0
                };
                let table_number = float_number(eff_chapter_tab, table_no);
                let caption_settings = table_caption_settings(&profile);
                let table_bookmark = t
                    .label
                    .as_ref()
                    .and_then(|label| ref_index.label_bookmarks.get(label))
                    .cloned();
                let mut bookmark_consumed = false;
                if !t.caption.is_empty() && caption_settings.position == CaptionPosition::Top {
                    let para = attach_bookmark_to_paragraph(
                        caption_paragraph(
                            &profile.caption_label_table,
                            Some(table_number.as_str()),
                            &profile.caption_label_separator_table,
                            &t.caption,
                            caption_settings,
                            &profile,
                            &ref_index,
                        ),
                        table_bookmark.as_deref(),
                        &mut bookmark_ids,
                        &mut next_bookmark_id,
                    );
                    docx = docx.add_paragraph(para);
                    bookmark_consumed = table_bookmark.is_some();
                }
                if table_bookmark.is_some() && !bookmark_consumed {
                    docx = docx.add_paragraph(attach_bookmark_to_paragraph(
                        Paragraph::new().style("BodyText"),
                        table_bookmark.as_deref(),
                        &mut bookmark_ids,
                        &mut next_bookmark_id,
                    ));
                }
                docx = docx.add_table(build_table(t, &profile, &ref_index));
                if !t.caption.is_empty() && caption_settings.position == CaptionPosition::Bottom {
                    let para = attach_bookmark_to_paragraph(
                        caption_paragraph(
                            &profile.caption_label_table,
                            Some(table_number.as_str()),
                            &profile.caption_label_separator_table,
                            &t.caption,
                            caption_settings,
                            &profile,
                            &ref_index,
                        ),
                        if bookmark_consumed {
                            None
                        } else {
                            table_bookmark.as_deref()
                        },
                        &mut bookmark_ids,
                        &mut next_bookmark_id,
                    );
                    docx = docx.add_paragraph(para);
                }
                if !t.source.is_empty() {
                    docx = docx.add_paragraph(source_paragraph(
                        &t.source,
                        &profile,
                        profile.source_vspace_table_twips,
                        &ref_index,
                    ));
                }
                rendered_any_block = true;
            }
            Block::Figure(f) => {
                figure_no += 1;
                let eff_chapter_fig = if profile.figure_counter_within_chapter {
                    chapter_no
                } else {
                    0
                };
                let figure_number = float_number(eff_chapter_fig, figure_no);
                docx = render_figure_block(
                    docx,
                    f,
                    FigureRenderMeta {
                        base_dir: figure_base_dir,
                        figure_number: Some(figure_number.as_str()),
                    },
                    &profile,
                    &ref_index,
                    &mut bookmark_ids,
                    &mut next_bookmark_id,
                );
                rendered_any_block = true;
            }
            Block::List(list) => {
                let num_id = next_num_id;
                next_num_id += 1;
                docx = register_numbering(docx, num_id, list.ordered, &profile);
                for item_inlines in &list.items {
                    let para = build_list_item(item_inlines, num_id, &profile, &ref_index);
                    docx = docx.add_paragraph(para);
                }
                rendered_any_block = true;
            }
            Block::DisplayMath(src) => {
                let equation_bookmark = extract_labels_from_display_math(src)
                    .into_iter()
                    .find_map(|label| ref_index.label_bookmarks.get(&label).cloned());
                let para = attach_bookmark_to_paragraph(
                    build_display_math_paragraph(src),
                    equation_bookmark.as_deref(),
                    &mut bookmark_ids,
                    &mut next_bookmark_id,
                );
                docx = docx.add_paragraph(para);
                rendered_any_block = true;
            }
            Block::BibliographyHeading { title } => {
                docx = docx.add_paragraph(build_bibliography_heading(title, &profile));
                rendered_any_block = true;
            }
        }
    }

    let file = File::create(output_path)?;
    docx.build().pack(file)?;
    postprocess_docx(output_path, profile.document_language.as_deref())?;
    Ok(())
}

fn create_styled_docx(profile: &RenderProfile) -> Docx {
    let page_margin = PageMargin::new()
        .top(profile.page_margin_top_twips)
        .bottom(profile.page_margin_bottom_twips)
        .left(profile.page_margin_left_twips)
        .right(profile.page_margin_right_twips)
        .header(profile.page_margin_header_twips)
        .footer(profile.page_margin_footer_twips)
        .gutter(profile.page_gutter_twips);
    let fonts = build_run_fonts(profile);

    let mut docx = Docx::new()
        .page_size(profile.page_width_twips, profile.page_height_twips)
        .page_margin(page_margin)
        .default_size(profile.font_size_body_hp)
        .default_fonts(fonts.clone())
        .default_line_spacing(line_spacing(profile.body_line_spacing_twips));

    for style in gost_styles(fonts, profile) {
        docx = docx.add_style(style);
    }

    // Default (non-first-page) header carries the page number.
    docx = docx.header(page_number_header(profile.page_number_alignment));

    // When the title page has \thispagestyle{empty}, enable different-first-page mode.
    // The first-page header is left empty (Word default), so no number appears on page 1.
    if profile.title_page_suppress_number {
        docx = docx.title_pg();
    }

    docx
}

fn gost_styles(fonts: RunFonts, profile: &RenderProfile) -> Vec<Style> {
    let mut footnote_reference = Style::new("FootnoteReference", StyleType::Character)
        .name("Footnote Reference")
        .based_on("DefaultParagraphFont")
        .fonts(fonts.clone())
        .size(profile.font_size_footnote_hp);
    footnote_reference.run_property = footnote_reference
        .run_property
        .vert_align(VertAlignType::SuperScript);

    vec![
        Style::new("BodyText", StyleType::Paragraph)
            .name("Body Text")
            .based_on("Normal")
            .next("BodyText")
            .fonts(fonts.clone())
            .size(profile.font_size_body_hp)
            .align(profile.body_text_alignment)
            .line_spacing(line_spacing(profile.body_line_spacing_twips))
            .indent(
                Some(0),
                Some(SpecialIndentType::FirstLine(
                    profile.body_first_line_indent_twips,
                )),
                None,
                None,
            ),
        heading_style_definition("Heading1", "Heading 1", 0, fonts.clone(), profile),
        heading_style_definition("Heading2", "Heading 2", 1, fonts.clone(), profile),
        heading_style_definition("Heading3", "Heading 3", 2, fonts.clone(), profile),
        toc_style_definition(
            "TOC1",
            "TOC 1",
            profile.toc_level_indent_twips(1),
            fonts.clone(),
            profile,
        ),
        toc_style_definition(
            "TOC2",
            "TOC 2",
            profile.toc_level_indent_twips(2),
            fonts.clone(),
            profile,
        ),
        toc_style_definition(
            "TOC3",
            "TOC 3",
            profile.toc_level_indent_twips(3),
            fonts.clone(),
            profile,
        ),
        toc_style_definition(
            "TOC4",
            "TOC 4",
            profile.toc_level_indent_twips(4),
            fonts.clone(),
            profile,
        ),
        toc_style_definition(
            "TOC5",
            "TOC 5",
            profile.toc_level_indent_twips(5),
            fonts.clone(),
            profile,
        ),
        toc_style_definition(
            "TOC6",
            "TOC 6",
            profile.toc_level_indent_twips(6),
            fonts.clone(),
            profile,
        ),
        Style::new("Caption", StyleType::Paragraph)
            .name("Caption")
            .based_on("BodyText")
            .next("BodyText")
            .fonts(fonts.clone())
            .size(profile.font_size_caption_hp)
            .align(profile.caption_alignment)
            .line_spacing(single_spacing())
            .indent(Some(0), None, None, None),
        Style::new("FootnoteText", StyleType::Paragraph)
            .name("Footnote Text")
            .based_on("Normal")
            .next("FootnoteText")
            .fonts(fonts.clone())
            .size(profile.font_size_footnote_hp)
            .align(profile.body_text_alignment)
            .line_spacing(single_spacing())
            .indent(Some(0), None, None, None),
        Style::new("ListParagraph", StyleType::Paragraph)
            .name("List Paragraph")
            .based_on("BodyText")
            .next("BodyText")
            .fonts(fonts.clone())
            .size(profile.font_size_body_hp)
            .align(profile.body_text_alignment)
            .line_spacing(line_spacing(profile.body_line_spacing_twips))
            .indent(Some(0), None, None, None),
        Style::new("TableParagraph", StyleType::Paragraph)
            .name("Table Paragraph")
            .based_on("BodyText")
            .next("TableParagraph")
            .fonts(fonts.clone())
            .size(profile.font_size_table_hp)
            .align(AlignmentType::Both)
            .line_spacing(single_spacing())
            .indent(Some(0), Some(SpecialIndentType::FirstLine(0)), None, None),
        footnote_reference,
    ]
}

fn heading_style_definition(
    style_id: &str,
    style_name: &str,
    outline_level: usize,
    fonts: RunFonts,
    profile: &RenderProfile,
) -> Style {
    let level = (outline_level + 1) as u8;
    let (space_before, space_after) = profile.heading_spacing_before_after(level);
    Style::new(style_id, StyleType::Paragraph)
        .name(style_name)
        .based_on("Normal")
        .next("BodyText")
        .fonts(fonts)
        .size(profile.font_size_body_hp)
        .bold()
        .align(profile.heading_alignment)
        .line_spacing(line_spacing_with_spacing(
            profile.body_line_spacing_twips,
            space_before,
            space_after,
        ))
        .indent(
            Some(0),
            Some(SpecialIndentType::FirstLine(heading_left_indent_twips(
                level, profile,
            ))),
            None,
            None,
        )
        .outline_lvl(outline_level)
}

fn toc_style_definition(
    style_id: &str,
    style_name: &str,
    left_indent_twips: i32,
    fonts: RunFonts,
    profile: &RenderProfile,
) -> Style {
    Style::new(style_id, StyleType::Paragraph)
        .name(style_name)
        .based_on("BodyText")
        .next(style_id)
        .fonts(fonts)
        .size(profile.font_size_body_hp)
        .align(AlignmentType::Left)
        .line_spacing(line_spacing(profile.body_line_spacing_twips))
        .indent(
            Some(left_indent_twips.max(0)),
            Some(SpecialIndentType::FirstLine(0)),
            None,
            None,
        )
}

fn heading_left_indent_twips(level: u8, profile: &RenderProfile) -> i32 {
    match level {
        2 => profile.heading_indent_section_twips,
        3 => profile
            .heading_indent_subsection_twips
            .max(profile.heading_indent_subsubsection_twips),
        _ => 0,
    }
}

fn page_number_header(alignment: AlignmentType) -> Header {
    Header::new().add_paragraph(
        Paragraph::new()
            .align(alignment)
            .line_spacing(single_spacing())
            .indent(Some(0), None, None, None)
            .add_page_num(PageNum::new()),
    )
}

/// Build `RunFonts` from the profile's body font family.
fn build_run_fonts(profile: &RenderProfile) -> RunFonts {
    RunFonts::new()
        .ascii(&profile.font_family_body)
        .hi_ansi(&profile.font_family_body)
        .cs(&profile.font_family_body)
}

fn single_spacing() -> LineSpacing {
    line_spacing(LINE_SPACING_SINGLE_TWIPS)
}

fn line_spacing(twips: i32) -> LineSpacing {
    LineSpacing::new()
        .line_rule(LineSpacingType::Auto)
        .line(twips.max(1))
}

fn attach_bookmark_to_paragraph(
    mut para: Paragraph,
    bookmark_name: Option<&str>,
    bookmark_ids: &mut HashMap<String, usize>,
    next_bookmark_id: &mut usize,
) -> Paragraph {
    let Some(name) = bookmark_name else {
        return para;
    };
    let bookmark_id = if let Some(existing) = bookmark_ids.get(name) {
        *existing
    } else {
        let id = *next_bookmark_id;
        *next_bookmark_id = next_bookmark_id.saturating_add(1);
        bookmark_ids.insert(name.to_string(), id);
        id
    };
    para = para.add_bookmark_start(bookmark_id, name);
    para.add_bookmark_end(bookmark_id)
}

// ---------------------------------------------------------------------------
// List rendering
// ---------------------------------------------------------------------------

/// Register an AbstractNumbering + Numbering pair for one list instance.
fn register_numbering(docx: Docx, num_id: usize, ordered: bool, profile: &RenderProfile) -> Docx {
    let abs_id = num_id - 1; // abstract IDs are 0-based by convention

    let (format, text): (&str, &str) = if ordered {
        ("decimal", "%1.")
    } else {
        ("bullet", &profile.list_bullet_char)
    };

    let level = Level::new(
        0,
        Start::new(1),
        NumberFormat::new(format),
        LevelText::new(text),
        LevelJc::new("left"),
    )
    .indent(
        Some(profile.list_left_indent_twips),
        Some(SpecialIndentType::Hanging(
            profile.list_hanging_indent_twips,
        )),
        None,
        None,
    );

    let abs_num = AbstractNumbering::new(abs_id).add_level(level);
    let num = Numbering::new(num_id, abs_id);

    docx.add_abstract_numbering(abs_num).add_numbering(num)
}

/// Build a single list-item paragraph with numbering applied.
fn build_list_item(
    inlines: &[Inline],
    num_id: usize,
    profile: &RenderProfile,
    refs: &ReferenceRenderIndex,
) -> Paragraph {
    let para = Paragraph::new()
        .style("ListParagraph")
        .align(profile.body_text_alignment)
        .line_spacing(line_spacing(profile.body_line_spacing_twips))
        .indent(
            Some(profile.list_left_indent_twips),
            Some(SpecialIndentType::Hanging(
                profile.list_hanging_indent_twips,
            )),
            None,
            None,
        )
        .numbering(NumberingId::new(num_id), IndentLevel::new(0));
    append_inlines_to_paragraph(
        para,
        inlines,
        InlineRenderState {
            bold: false,
            italic: false,
            force_italic: false,
            footnote_hp: profile.font_size_footnote_hp,
        },
        refs,
        profile,
    )
}

// ---------------------------------------------------------------------------
// Table rendering
// ---------------------------------------------------------------------------

fn build_table(table: &Table, profile: &RenderProfile, refs: &ReferenceRenderIndex) -> DocxTable {
    let rows: Vec<DocxRow> = table
        .rows
        .iter()
        .map(|row| {
            let cells: Vec<DocxCell> = row
                .cells
                .iter()
                .map(|cell| {
                    let para = Paragraph::new()
                        .style("TableParagraph")
                        .align(AlignmentType::Both)
                        .line_spacing(single_spacing())
                        .indent(Some(0), Some(SpecialIndentType::FirstLine(0)), None, None);
                    let para = append_inlines_to_paragraph(
                        para,
                        &cell.content,
                        InlineRenderState {
                            bold: false,
                            italic: false,
                            force_italic: false,
                            footnote_hp: profile.font_size_footnote_hp,
                        },
                        refs,
                        profile,
                    );
                    DocxCell::new()
                        .add_paragraph(resize_paragraph_runs(para, profile.font_size_table_hp))
                })
                .collect();
            DocxRow::new(cells)
        })
        .collect();
    DocxTable::new(rows)
        .style("TableGrid")
        .align(parse_table_alignment(table.alignment.as_deref()))
}

fn float_number(chapter_no: usize, local_no: usize) -> String {
    if chapter_no > 0 {
        format!("{chapter_no}.{local_no}")
    } else {
        local_no.to_string()
    }
}

/// A caption paragraph with alignment/indent/spacing derived from `RenderProfile`.
fn caption_paragraph(
    kind: &str,
    number: Option<&str>,
    separator: &str,
    inlines: &[Inline],
    settings: CaptionRenderSettings,
    profile: &RenderProfile,
    refs: &ReferenceRenderIndex,
) -> Paragraph {
    let alignment = effective_caption_alignment(
        settings.default_alignment,
        settings.singlelinecheck,
        inlines,
    );
    let mut para = Paragraph::new()
        .style("Caption")
        .align(alignment)
        .line_spacing(caption_line_spacing(settings.skip_twips, settings.position))
        .indent(Some(settings.indent_twips), None, None, None);

    let prefixed = caption_is_prefixed(kind, inlines);
    if !prefixed && let Some(prefix) = caption_prefix_text(kind, number, separator) {
        let run = if settings.label_bold {
            Run::new().add_text(prefix).bold()
        } else {
            Run::new().add_text(prefix)
        };
        para = para.add_run(run);
    }

    append_inlines_to_paragraph(
        para,
        inlines,
        InlineRenderState {
            bold: false,
            italic: false,
            force_italic: false,
            footnote_hp: settings.footnote_font_size_hp,
        },
        refs,
        profile,
    )
}

fn caption_line_spacing(skip_twips: i32, position: CaptionPosition) -> LineSpacing {
    let mut spacing = single_spacing();
    let skip = skip_twips.max(0) as u32;
    if skip == 0 {
        return spacing;
    }
    match position {
        CaptionPosition::Top => spacing = spacing.after(skip),
        CaptionPosition::Bottom => spacing = spacing.before(skip),
    }
    spacing
}

fn effective_caption_alignment(
    default_alignment: AlignmentType,
    singlelinecheck: bool,
    inlines: &[Inline],
) -> AlignmentType {
    if singlelinecheck && caption_is_single_line_candidate(inlines) {
        AlignmentType::Center
    } else {
        default_alignment
    }
}

fn caption_is_single_line_candidate(inlines: &[Inline]) -> bool {
    let mut chars = 0usize;
    let mut has_hard_break = false;
    collect_caption_text_stats(inlines, &mut chars, &mut has_hard_break);
    !has_hard_break && chars <= CAPTION_SINGLELINE_ESTIMATE_CHARS
}

fn collect_caption_text_stats(inlines: &[Inline], chars: &mut usize, has_hard_break: &mut bool) {
    for inline in inlines {
        match inline {
            Inline::Text(value) => {
                if value.chars().any(|ch| ch == '\n' || ch == '\r') {
                    *has_hard_break = true;
                }
                *chars += value.chars().filter(|ch| !ch.is_whitespace()).count();
            }
            Inline::LineBreak => {
                *has_hard_break = true;
            }
            Inline::Bold(children) | Inline::Italic(children) => {
                collect_caption_text_stats(children, chars, has_hard_break);
            }
            Inline::Footnote(children) => {
                collect_caption_text_stats(children, chars, has_hard_break);
            }
            Inline::InlineMath(value) | Inline::Reference(value) => {
                if value.chars().any(|ch| ch == '\n' || ch == '\r') {
                    *has_hard_break = true;
                }
                *chars += value.chars().filter(|ch| !ch.is_whitespace()).count();
            }
        }
    }
}

fn caption_prefix_text(kind: &str, number: Option<&str>, separator: &str) -> Option<String> {
    if kind.is_empty() {
        return None;
    }

    let sep = if separator.is_empty() {
        " ".to_string()
    } else {
        separator.to_string()
    };

    if let Some(number) = number {
        Some(format!("{kind} {number}{sep}"))
    } else {
        Some(format!("{kind}{sep}"))
    }
}

fn caption_is_prefixed(kind: &str, inlines: &[Inline]) -> bool {
    let mut text = String::new();
    for inline in inlines {
        match inline {
            Inline::Text(value) => text.push_str(value),
            Inline::LineBreak => text.push(' '),
            Inline::Bold(children) | Inline::Italic(children) => {
                for child in children {
                    if let Inline::Text(value) = child {
                        text.push_str(value);
                    }
                }
            }
            Inline::Footnote(_) | Inline::InlineMath(_) | Inline::Reference(_) => {}
        }
        if text.len() >= 64 {
            break;
        }
    }
    text.trim_start()
        .to_lowercase()
        .starts_with(&kind.to_lowercase())
}

/// A left-aligned small italic paragraph used for source attributions (`\figuresource`/`\tablesource`).
///
/// Matches the LaTeX definition `{\noindent\raggedright\small\textit{#1}}`.
/// `vspace_twips` is added as `space_before` to replicate `\vspace{...}` before the line.
fn source_paragraph(
    inlines: &[Inline],
    profile: &RenderProfile,
    vspace_twips: i32,
    refs: &ReferenceRenderIndex,
) -> Paragraph {
    let spacing = {
        let mut s = single_spacing();
        if vspace_twips > 0 {
            s = s.before(vspace_twips as u32);
        }
        s
    };
    let para = Paragraph::new()
        .style("BodyText")
        .align(AlignmentType::Left)
        .line_spacing(spacing)
        .indent(Some(0), Some(SpecialIndentType::FirstLine(0)), None, None);
    let para = append_inlines_to_paragraph(
        para,
        inlines,
        InlineRenderState {
            bold: false,
            italic: false,
            force_italic: true,
            footnote_hp: profile.font_size_footnote_hp,
        },
        refs,
        profile,
    );
    resize_paragraph_runs(para, profile.font_size_footnote_hp)
}

/// A chapter-level heading for a bibliography section.
///
/// Renders `\printbibliography` / `\insertbibliofullsorted` as an unnumbered chapter heading.
fn build_bibliography_heading(title: &str, profile: &RenderProfile) -> Paragraph {
    let text = if profile.heading_uppercase {
        title.to_uppercase()
    } else {
        title.to_string()
    };
    let (space_before, space_after) = profile.heading_spacing_before_after(1);
    Paragraph::new()
        .style("Heading1")
        .align(profile.heading_alignment)
        .line_spacing(line_spacing_with_spacing(
            profile.body_line_spacing_twips,
            space_before,
            space_after,
        ))
        .indent(Some(0), Some(SpecialIndentType::FirstLine(0)), None, None)
        .add_run(Run::new().add_text(text))
}

/// A centred italic paragraph for display-math blocks.
fn build_display_math_paragraph(src: &str) -> Paragraph {
    let visible = strip_label_commands(src);
    Paragraph::new()
        .style("BodyText")
        .align(AlignmentType::Center)
        .line_spacing(single_spacing())
        .indent(Some(0), None, None, None)
        .add_run(Run::new().add_text(visible))
}

fn strip_label_commands(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut pos = 0usize;

    while pos < src.len() {
        let Some(rel) = src[pos..].find("\\label") else {
            out.push_str(&src[pos..]);
            break;
        };

        let start = pos + rel;
        out.push_str(&src[pos..start]);
        let cmd_end = start + "\\label".len();
        let mut arg_pos = cmd_end;
        while arg_pos < src.len() && src.as_bytes()[arg_pos].is_ascii_whitespace() {
            arg_pos += 1;
        }

        if arg_pos < src.len() && src.as_bytes()[arg_pos] == b'{' {
            if let Some(end_rel) = src[arg_pos + 1..].find('}') {
                pos = arg_pos + 1 + end_rel + 1;
            } else {
                pos = cmd_end;
            }
        } else {
            pos = cmd_end;
        }
    }

    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_math_text(src: &str) -> String {
    let mut out = src.to_string();
    for (from, to) in [
        ("\\cdot", "·"),
        ("\\times", "×"),
        ("\\leq", "≤"),
        ("\\geq", "≥"),
        ("\\neq", "≠"),
        ("\\sim", "≈"),
        ("\\approx", "≈"),
        ("\\to", "→"),
        ("\\rightarrow", "→"),
        ("\\left", ""),
        ("\\right", ""),
        ("\\,", " "),
        ("\\;", " "),
        ("\\!", ""),
    ] {
        out = out.replace(from, to);
    }
    out = out.replace("---", "—");
    out = out.replace("--", "–");
    out = out.replace(['{', '}'], "");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------------------
// Figure rendering
// ---------------------------------------------------------------------------

struct FigureRenderMeta<'a> {
    base_dir: Option<&'a Path>,
    figure_number: Option<&'a str>,
}

fn render_figure_block(
    mut docx: Docx,
    figure: &Figure,
    meta: FigureRenderMeta<'_>,
    profile: &RenderProfile,
    refs: &ReferenceRenderIndex,
    bookmark_ids: &mut HashMap<String, usize>,
    next_bookmark_id: &mut usize,
) -> Docx {
    let mut embedded = false;
    let max_image_width_emu = profile.max_image_width_emu();
    let caption_settings = figure_caption_settings(profile);
    let render_caption_before_figure = caption_settings.position == CaptionPosition::Top;
    let figure_alignment = parse_alignment(figure.alignment.as_deref().unwrap_or("center"));
    let figure_bookmark = figure
        .label
        .as_ref()
        .and_then(|label| refs.label_bookmarks.get(label))
        .cloned();
    let mut bookmark_consumed = false;

    if render_caption_before_figure && !figure.caption.is_empty() {
        let para = attach_bookmark_to_paragraph(
            caption_paragraph(
                &profile.caption_label_figure,
                meta.figure_number,
                &profile.caption_label_separator_figure,
                &figure.caption,
                caption_settings,
                profile,
                refs,
            ),
            figure_bookmark.as_deref(),
            bookmark_ids,
            next_bookmark_id,
        );
        docx = docx.add_paragraph(para);
        bookmark_consumed = figure_bookmark.is_some();
    }

    if figure_bookmark.is_some() && !bookmark_consumed {
        docx = docx.add_paragraph(attach_bookmark_to_paragraph(
            Paragraph::new().style("BodyText"),
            figure_bookmark.as_deref(),
            bookmark_ids,
            next_bookmark_id,
        ));
    }

    if let Some(raw_path) = figure.image_path.as_deref() {
        if let Some(resolved) =
            resolve_figure_path(raw_path, meta.base_dir, &profile.graphics_search_paths)
        {
            match read_figure_pic(&resolved, figure.width_permille, max_image_width_emu) {
                Ok(pic) => {
                    docx = docx.add_paragraph(figure_image_paragraph(pic, figure_alignment));
                    embedded = true;
                }
                Err(error) => {
                    log::warn!(
                        "Failed to embed figure image {}: {error}",
                        resolved.display()
                    );
                }
            }
        } else {
            log::warn!("Figure image not found for includegraphics path: {raw_path}");
        }
    } else {
        log::warn!("Figure block has no includegraphics path");
    }

    if !embedded {
        let fallback = figure
            .image_path
            .as_deref()
            .unwrap_or("missing includegraphics path");
        docx = docx.add_paragraph(figure_placeholder_paragraph(fallback, figure_alignment));
    }

    if !render_caption_before_figure && !figure.caption.is_empty() {
        let para = attach_bookmark_to_paragraph(
            caption_paragraph(
                &profile.caption_label_figure,
                meta.figure_number,
                &profile.caption_label_separator_figure,
                &figure.caption,
                caption_settings,
                profile,
                refs,
            ),
            if bookmark_consumed {
                None
            } else {
                figure_bookmark.as_deref()
            },
            bookmark_ids,
            next_bookmark_id,
        );
        docx = docx.add_paragraph(para);
    }

    if !figure.source.is_empty() {
        docx = docx.add_paragraph(source_paragraph(
            &figure.source,
            profile,
            profile.source_vspace_figure_twips,
            refs,
        ));
    }

    docx
}

fn read_figure_pic(
    path: &Path,
    width_permille: Option<u16>,
    max_image_width_emu: u32,
) -> anyhow::Result<Pic> {
    let image = std::fs::read(path)
        .with_context(|| format!("failed to read image bytes from {}", path.display()))?;

    let pic = catch_unwind(AssertUnwindSafe(|| Pic::new(&image))).map_err(|_| {
        anyhow!(
            "docx-rs failed to decode image {} (unsupported or corrupt image format)",
            path.display()
        )
    })?;

    Ok(scale_pic_to_text_width(
        pic,
        width_permille,
        max_image_width_emu,
    ))
}

fn scale_pic_to_text_width(pic: Pic, width_permille: Option<u16>, max_image_width_emu: u32) -> Pic {
    let (width_emu, height_emu) = pic.size;
    if width_emu == 0 {
        return pic;
    }

    let latex_target_emu = width_permille.map(|permille| {
        (max_image_width_emu as u64 * IMAGE_SAFE_SCALE_DEN as u64 * permille as u64
            / (IMAGE_SAFE_SCALE_NUM as u64 * 1000))
            .max(1) as u32
    });
    let target_width_emu = latex_target_emu
        .unwrap_or(max_image_width_emu)
        .min(max_image_width_emu);

    if width_emu <= target_width_emu {
        return pic;
    }

    let scaled_height =
        ((height_emu as u64) * (target_width_emu as u64) / (width_emu as u64)).max(1) as u32;
    pic.size(target_width_emu, scaled_height)
}

fn figure_image_paragraph(pic: Pic, alignment: AlignmentType) -> Paragraph {
    Paragraph::new()
        .style("BodyText")
        .align(alignment)
        .line_spacing(single_spacing())
        .indent(Some(0), Some(SpecialIndentType::FirstLine(0)), None, None)
        .add_run(Run::new().add_image(pic))
}

fn figure_placeholder_paragraph(path_hint: &str, alignment: AlignmentType) -> Paragraph {
    Paragraph::new()
        .style("BodyText")
        .align(alignment)
        .line_spacing(single_spacing())
        .indent(Some(0), Some(SpecialIndentType::FirstLine(0)), None, None)
        .add_run(
            Run::new()
                .add_text(format!("[Figure image not embedded: {path_hint}]"))
                .italic(),
        )
}

fn resolve_figure_path(
    raw: &str,
    base_dir: Option<&Path>,
    search_paths: &[String],
) -> Option<PathBuf> {
    let raw = raw.trim().trim_matches('"');
    if raw.is_empty() {
        return None;
    }

    let input = Path::new(raw);
    let mut candidates = Vec::new();

    if input.is_absolute() {
        candidates.push(input.to_path_buf());
    } else if let Some(base_dir) = base_dir {
        candidates.push(base_dir.join(input));
        for asset_dir in search_paths {
            candidates.push(base_dir.join(asset_dir).join(input));
        }
    } else {
        candidates.push(input.to_path_buf());
    }

    if input.extension().is_none() {
        let mut with_extensions = Vec::new();
        for candidate in &candidates {
            for ext in IMAGE_EXTENSIONS {
                with_extensions.push(candidate.with_extension(ext));
            }
        }
        candidates.extend(with_extensions);
    }

    candidates.into_iter().find(|candidate| candidate.is_file())
}

// ---------------------------------------------------------------------------
// Paragraph / section rendering
// ---------------------------------------------------------------------------

/// Convert a [`Block::Section`] or body paragraph into a docx-rs [`Paragraph`].
fn build_paragraph(
    block: &Block,
    profile: &RenderProfile,
    refs: &ReferenceRenderIndex,
) -> Paragraph {
    match block {
        Block::Section {
            level,
            number,
            title,
            ..
        } => {
            let style = heading_style(*level);
            let heading_indent = heading_left_indent_twips(*level, profile);
            let (space_before, space_after) = profile.heading_spacing_before_after(*level);
            let mut para = Paragraph::new()
                .style(style)
                .align(profile.heading_alignment)
                .line_spacing(line_spacing_with_spacing(
                    profile.body_line_spacing_twips,
                    space_before,
                    space_after,
                ))
                .indent(
                    Some(0),
                    Some(SpecialIndentType::FirstLine(heading_indent)),
                    None,
                    None,
                );
            let section_title = if *level == 1 && profile.heading_uppercase {
                uppercase_inlines(title)
            } else {
                title.to_vec()
            };
            if let Some(number) = number {
                let delim = profile.heading_number_delimiter_for_level(*level);
                let number = number.trim();
                let number_core = number.trim_end_matches('.');
                let mut prefix = if *level == 1 && !profile.chapter_name.is_empty() {
                    let chap_name = if profile.heading_uppercase {
                        profile.chapter_name.to_uppercase()
                    } else {
                        profile.chapter_name.clone()
                    };
                    format!("{chap_name} {number_core}{delim}")
                } else {
                    format!("{number_core}{delim}")
                };
                if !title.is_empty() {
                    prefix.push(' ');
                }
                para = para.add_run(Run::new().add_text(prefix).bold());
            }
            append_inlines_to_paragraph(
                para,
                &section_title,
                InlineRenderState {
                    bold: true,
                    italic: false,
                    force_italic: false,
                    footnote_hp: profile.font_size_footnote_hp,
                },
                refs,
                profile,
            )
        }
        Block::Paragraph(inlines) => build_default_body_paragraph(inlines, profile, refs),
        Block::StyledParagraph { inlines, style } => {
            build_styled_body_paragraph(inlines, style, profile, refs)
        }
        // Table, Figure, List, DisplayMath, BibliographyHeading, TableOfContents
        // are handled separately in the render loop — unreachable here.
        Block::Table(_)
        | Block::Figure(_)
        | Block::List(_)
        | Block::DisplayMath(_)
        | Block::BibliographyHeading { .. }
        | Block::TableOfContents => {
            unreachable!()
        }
    }
}

fn build_default_body_paragraph(
    inlines: &[Inline],
    profile: &RenderProfile,
    refs: &ReferenceRenderIndex,
) -> Paragraph {
    let para = Paragraph::new()
        .style("BodyText")
        .align(profile.body_text_alignment)
        .line_spacing(line_spacing(profile.body_line_spacing_twips))
        .indent(
            Some(0),
            Some(SpecialIndentType::FirstLine(
                profile.body_first_line_indent_twips,
            )),
            None,
            None,
        );
    append_inlines_to_paragraph(
        para,
        inlines,
        InlineRenderState {
            bold: false,
            italic: false,
            force_italic: false,
            footnote_hp: profile.font_size_footnote_hp,
        },
        refs,
        profile,
    )
}

fn build_styled_body_paragraph(
    inlines: &[Inline],
    style: &ParagraphStyle,
    profile: &RenderProfile,
    refs: &ReferenceRenderIndex,
) -> Paragraph {
    let alignment = style
        .alignment
        .as_deref()
        .map(parse_alignment)
        .unwrap_or(profile.body_text_alignment);
    let left_indent = style.left_indent_twips.unwrap_or(0).max(0);
    let line_twips = style
        .line_spacing_twips
        .unwrap_or(profile.body_line_spacing_twips);
    let first_line_indent = style
        .first_line_indent_twips
        .unwrap_or(profile.body_first_line_indent_twips);
    let para = Paragraph::new()
        .style("BodyText")
        .align(alignment)
        .line_spacing(line_spacing_with_spacing(
            line_twips,
            style.space_before_twips,
            style.space_after_twips,
        ))
        .indent(
            Some(left_indent),
            Some(SpecialIndentType::FirstLine(first_line_indent)),
            None,
            None,
        );
    let para = append_inlines_to_paragraph(
        para,
        inlines,
        InlineRenderState {
            bold: false,
            italic: false,
            force_italic: false,
            footnote_hp: profile.font_size_footnote_hp,
        },
        refs,
        profile,
    );
    if let Some(size_hp) = style.font_size_hp {
        resize_paragraph_runs(para, size_hp)
    } else {
        para
    }
}

fn line_spacing_with_spacing(
    line_twips: i32,
    before_twips: Option<i32>,
    after_twips: Option<i32>,
) -> LineSpacing {
    let mut spacing = line_spacing(line_twips);
    if let Some(before) = before_twips
        && before > 0
    {
        spacing = spacing.before(before as u32);
    }
    if let Some(after) = after_twips
        && after > 0
    {
        spacing = spacing.after(after as u32);
    }
    spacing
}

fn em_to_twips(font_size_hp: usize, em: f64) -> i32 {
    let point_size = font_size_hp as f64 / 2.0;
    (point_size * DOCX_TWIPS_PER_POINT_F64 * em).round() as i32
}

fn estimate_toc_prefix_width_twips(prefix: &str, font_size_hp: usize) -> i32 {
    let chars = prefix.chars().count() as f64;
    let em_twips = em_to_twips(font_size_hp, 1.0) as f64;
    let width = (chars * TOC_PREFIX_ESTIMATED_CHAR_WIDTH_EM * em_twips).round();
    if width.is_finite() && width > 0.0 {
        width as i32
    } else {
        0
    }
}

/// Map a heading level (1-based) to a docx-rs style id string.
fn heading_style(level: u8) -> &'static str {
    match level {
        1 => "Heading1",
        2 => "Heading2",
        _ => "Heading3",
    }
}

fn generated_toc_paragraphs(
    document: &Document,
    start_index: usize,
    profile: &RenderProfile,
    refs: &ReferenceRenderIndex,
) -> Vec<Paragraph> {
    if !document.toc_entries.is_empty() {
        return generated_toc_paragraphs_from_entries(document, profile, refs);
    }

    let mut paragraphs = Vec::new();

    for (index, block) in document.blocks.iter().enumerate().skip(start_index) {
        let Block::Section {
            level,
            number,
            title,
            ..
        } = block
        else {
            continue;
        };

        if *level == 0 || *level > profile.toc_max_level() {
            continue;
        }

        paragraphs.push(build_toc_entry_paragraph(
            *level,
            number.as_deref(),
            title,
            None,
            refs.section_bookmark_by_index
                .get(&index)
                .map(String::as_str),
            profile,
            refs,
        ));
    }

    paragraphs
}

fn generated_toc_paragraphs_from_entries(
    document: &Document,
    profile: &RenderProfile,
    refs: &ReferenceRenderIndex,
) -> Vec<Paragraph> {
    let mut paragraphs = Vec::new();
    for entry in &document.toc_entries {
        if entry.level == 0 {
            if !entry.title.trim().is_empty() {
                paragraphs.push(build_toc_page_header_paragraph(&entry.title, profile));
            }
            continue;
        }
        if entry.level > profile.toc_max_level() {
            continue;
        }
        let title_inlines = vec![Inline::Text(entry.title.clone())];
        let target_anchor =
            refs.toc_anchor_for_entry(entry.level, entry.number.as_deref(), &entry.title);
        paragraphs.push(build_toc_entry_paragraph(
            entry.level,
            entry.number.as_deref(),
            &title_inlines,
            entry.page.as_deref(),
            target_anchor.as_deref(),
            profile,
            refs,
        ));
    }
    paragraphs
}

fn build_toc_page_header_paragraph(text: &str, profile: &RenderProfile) -> Paragraph {
    let mut para = Paragraph::new()
        .style("TOC1")
        .align(AlignmentType::Right)
        .line_spacing(line_spacing(profile.body_line_spacing_twips));
    let inlines = vec![Inline::Text(text.to_string())];
    for run in inline_runs_with_footnote_size(&inlines, false, false, profile.font_size_body_hp) {
        para = para.add_run(run);
    }
    para
}

fn is_appendix_like_chapter_number(number: &str) -> bool {
    let core = number.trim().trim_end_matches('.');
    core.chars().next().is_some_and(char::is_alphabetic)
}

fn toc_level_one_name_prefix(profile: &RenderProfile, number: &str) -> Option<String> {
    let raw_prefix = if is_appendix_like_chapter_number(number)
        && !profile.toc_appendix_name.trim().is_empty()
    {
        profile.toc_appendix_name.trim()
    } else {
        profile.toc_chapter_name_prefix.trim()
    };
    if raw_prefix.is_empty() {
        return None;
    }
    if profile.heading_uppercase {
        Some(raw_prefix.to_uppercase())
    } else {
        Some(raw_prefix.to_string())
    }
}

fn build_toc_entry_paragraph(
    level: u8,
    number: Option<&str>,
    title: &[Inline],
    page: Option<&str>,
    target_anchor: Option<&str>,
    profile: &RenderProfile,
    _refs: &ReferenceRenderIndex,
) -> Paragraph {
    let style = match level {
        1 => "TOC1",
        2 => "TOC2",
        3 => "TOC3",
        4 => "TOC4",
        5 => "TOC5",
        _ => "TOC6",
    };
    let mut para = Paragraph::new()
        .style(style)
        .align(AlignmentType::Left)
        .line_spacing(line_spacing(profile.body_line_spacing_twips));
    let toc_space_before = profile.toc_level_space_before_twips(level);
    if toc_space_before > 0 {
        para = para.line_spacing(line_spacing_with_spacing(
            profile.body_line_spacing_twips,
            Some(toc_space_before),
            None,
        ));
    }
    let toc_indent = profile.toc_level_indent_twips(level);
    let mut toc_numwidth = profile.toc_level_numwidth_twips(level);
    let level_one_name_prefix = if level == 1 {
        number.and_then(|value| toc_level_one_name_prefix(profile, value))
    } else {
        None
    };
    if let Some(name_prefix) = level_one_name_prefix.as_deref() {
        let prefix = format!("{name_prefix} ");
        toc_numwidth = toc_numwidth.saturating_add(estimate_toc_prefix_width_twips(
            &prefix,
            profile.font_size_body_hp,
        ));
    }

    if let Some(number) = number {
        if toc_numwidth > 0 {
            para = para.indent(
                Some((toc_indent + toc_numwidth).max(0)),
                Some(SpecialIndentType::Hanging(toc_numwidth)),
                None,
                None,
            );
        }

        let aftersnum = profile.toc_level_aftersnum(level);
        let mut prefix = String::new();
        if level == 1 {
            if let Some(name_prefix) = level_one_name_prefix.as_deref() {
                prefix.push_str(name_prefix);
                prefix.push(' ');
            }
            let number_core = number.trim_end_matches('.');
            prefix.push_str(number_core);
            // Use aftersnum separator if available, else fall back to heading delimiter.
            if !aftersnum.is_empty() {
                prefix.push_str(aftersnum);
            } else {
                prefix.push_str(profile.heading_number_delimiter_for_level(1));
            }
        } else {
            prefix.push_str(number);
            if !aftersnum.is_empty() {
                // Replace trailing dot/space from LaTeX number with aftersnum.
                let trimmed = prefix.trim_end_matches(['.', ' ']);
                prefix = format!("{trimmed}{aftersnum}");
            }
        }
        if !prefix.is_empty() {
            // Ensure single space after prefix if not already ending with whitespace.
            if !prefix.ends_with(|c: char| c.is_whitespace()) {
                prefix.push(' ');
            }
            para = para.add_run(Run::new().add_text(prefix));
        }
    }

    let title = if level == 1 && profile.heading_uppercase {
        uppercase_inlines(title)
    } else {
        title.to_vec()
    };
    // Apply bold/non-bold per toc_chapter_entry_bold (level-1 only).
    let title_runs =
        inline_runs_with_footnote_size(&title, false, false, profile.font_size_body_hp);
    if let Some(anchor) = target_anchor {
        let mut link = Hyperlink::new(anchor, HyperlinkType::Anchor);
        if level == 1 && !profile.toc_chapter_entry_bold {
            for run in title_runs {
                link = link.add_run(apply_hyperlink_run_style(run.disable_bold(), profile));
            }
        } else {
            for run in title_runs {
                link = link.add_run(apply_hyperlink_run_style(run, profile));
            }
        }
        para = para.add_hyperlink(link);
    } else if level == 1 && !profile.toc_chapter_entry_bold {
        for run in title_runs {
            para = para.add_run(run.disable_bold());
        }
    } else {
        for run in title_runs {
            para = para.add_run(run);
        }
    }

    if let Some(page) = page {
        let mut tab = Tab::new()
            .val(TabValueType::Right)
            .pos(profile.toc_page_tab_stop_twips());
        if profile.toc_use_dot_leader {
            tab = tab.leader(TabLeaderType::Dot);
        }
        let page_run = Run::new().add_text(page);
        let page_run = if level == 1 && !profile.toc_chapter_page_bold {
            page_run.disable_bold()
        } else {
            page_run
        };
        para = para
            .add_tab(tab)
            .add_run(Run::new().add_tab())
            .add_run(page_run);
    }

    para
}

fn collect_inline_text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for inline in inlines {
        match inline {
            Inline::Text(text) => out.push_str(text),
            Inline::LineBreak => out.push(' '),
            Inline::Bold(children) | Inline::Italic(children) | Inline::Footnote(children) => {
                out.push_str(&collect_inline_text(children))
            }
            Inline::InlineMath(text) | Inline::Reference(text) => out.push_str(text),
        }
    }
    out
}

#[derive(Clone, Copy)]
struct InlineRenderState {
    bold: bool,
    italic: bool,
    force_italic: bool,
    footnote_hp: usize,
}

fn append_inlines_to_paragraph(
    mut para: Paragraph,
    inlines: &[Inline],
    state: InlineRenderState,
    refs: &ReferenceRenderIndex,
    profile: &RenderProfile,
) -> Paragraph {
    for inline in inlines {
        match inline {
            Inline::Text(text) => {
                let mut run = Run::new().add_text(text.as_str());
                if state.bold {
                    run = run.bold();
                }
                if state.italic || state.force_italic {
                    run = run.italic();
                }
                para = para.add_run(run);
            }
            Inline::LineBreak => {
                para = para.add_run(Run::new().add_break(BreakType::TextWrapping));
            }
            Inline::Bold(children) => {
                para = append_inlines_to_paragraph(
                    para,
                    children,
                    InlineRenderState {
                        bold: true,
                        ..state
                    },
                    refs,
                    profile,
                );
            }
            Inline::Italic(children) => {
                para = append_inlines_to_paragraph(
                    para,
                    children,
                    InlineRenderState {
                        italic: true,
                        ..state
                    },
                    refs,
                    profile,
                );
            }
            Inline::InlineMath(src) => {
                let visible = normalize_math_text(src);
                let mut run = Run::new().add_text(visible);
                if state.bold {
                    run = run.bold();
                }
                if state.italic || state.force_italic {
                    run = run.italic();
                }
                para = para.add_run(run);
            }
            Inline::Reference(label) => {
                let (resolved_text, anchor) = refs.resolve_reference(label);
                let mut run = Run::new().add_text(resolved_text);
                if state.bold {
                    run = run.bold();
                }
                if state.italic || state.force_italic {
                    run = run.italic();
                }
                if let Some(anchor) = anchor {
                    let link = Hyperlink::new(anchor, HyperlinkType::Anchor)
                        .add_run(apply_hyperlink_run_style(run, profile));
                    para = para.add_hyperlink(link);
                } else {
                    para = para.add_run(run);
                }
            }
            Inline::Footnote(content) => {
                trim_trailing_spaces_from_paragraph_runs(&mut para);
                let mut footnote_para = Paragraph::new()
                    .style("FootnoteText")
                    .align(profile.body_text_alignment)
                    .line_spacing(single_spacing())
                    .indent(Some(0), None, None, None)
                    .keep_lines(true);
                footnote_para = append_inlines_to_paragraph(
                    footnote_para,
                    content,
                    InlineRenderState {
                        bold: false,
                        italic: false,
                        force_italic: false,
                        footnote_hp: state.footnote_hp,
                    },
                    refs,
                    profile,
                );
                // Apply footnote-sized runs recursively.
                footnote_para = resize_paragraph_runs(footnote_para, state.footnote_hp);

                let mut footnote = Footnote::new();
                footnote.add_content(footnote_para);
                let mut footnote_ref_run = Run::new().add_footnote_reference(footnote);
                footnote_ref_run.run_property = footnote_ref_run
                    .run_property
                    .vert_align(VertAlignType::SuperScript);
                para = para.add_run(footnote_ref_run);
            }
        }
    }
    para
}

fn resize_paragraph_runs(mut para: Paragraph, size_hp: usize) -> Paragraph {
    for child in &mut para.children {
        match child {
            docx_rs::ParagraphChild::Run(run) => {
                **run = (**run).clone().size(size_hp);
            }
            docx_rs::ParagraphChild::Hyperlink(link) => {
                let mut updated = link.clone();
                updated.children = updated
                    .children
                    .iter()
                    .map(|c| match c {
                        docx_rs::ParagraphChild::Run(run) => {
                            docx_rs::ParagraphChild::Run(Box::new((**run).clone().size(size_hp)))
                        }
                        other => other.clone(),
                    })
                    .collect();
                *link = updated;
            }
            _ => {}
        }
    }
    para
}

/// Recursively convert a slice of [`Inline`] nodes into a flat list of
/// docx-rs [`Run`]s, inheriting bold/italic state from parent nodes.
///
/// `footnote_hp` controls the font size applied inside footnote paragraphs.
fn inline_runs_with_footnote_size(
    inlines: &[Inline],
    bold: bool,
    italic: bool,
    footnote_hp: usize,
) -> Vec<Run> {
    let mut runs = Vec::new();
    for inline in inlines {
        match inline {
            Inline::Text(text) => {
                let mut run = Run::new().add_text(text.as_str());
                if bold {
                    run = run.bold();
                }
                if italic {
                    run = run.italic();
                }
                runs.push(run);
            }
            Inline::LineBreak => {
                runs.push(Run::new().add_break(BreakType::TextWrapping));
            }
            Inline::Bold(children) => {
                runs.extend(inline_runs_with_footnote_size(
                    children,
                    true,
                    italic,
                    footnote_hp,
                ));
            }
            Inline::Italic(children) => {
                runs.extend(inline_runs_with_footnote_size(
                    children,
                    bold,
                    true,
                    footnote_hp,
                ));
            }
            Inline::InlineMath(src) => {
                // Render inline math with lightweight TeX normalization.
                let visible = normalize_math_text(src);
                let mut run = Run::new().add_text(visible);
                if bold {
                    run = run.bold();
                }
                runs.push(run);
            }
            Inline::Reference(label) => {
                let mut run = Run::new().add_text(label.as_str());
                if bold {
                    run = run.bold();
                }
                if italic {
                    run = run.italic();
                }
                runs.push(run);
            }
            Inline::Footnote(content) => {
                trim_trailing_spaces_from_text_run(&mut runs);
                // Render \footnote{...} as a native DOCX footnote reference + footnotes.xml entry.
                let mut footnote_para = Paragraph::new()
                    .style("FootnoteText")
                    .align(AlignmentType::Both)
                    .line_spacing(single_spacing())
                    .indent(Some(0), None, None, None)
                    .keep_lines(true);
                for run in inline_runs_with_footnote_size(content, false, false, footnote_hp) {
                    footnote_para = footnote_para.add_run(run.size(footnote_hp));
                }

                let mut footnote = Footnote::new();
                footnote.add_content(footnote_para);
                let mut footnote_ref_run = Run::new().add_footnote_reference(footnote);
                footnote_ref_run.run_property = footnote_ref_run
                    .run_property
                    .vert_align(VertAlignType::SuperScript);
                runs.push(footnote_ref_run);
            }
        }
    }
    runs
}

fn trim_trailing_spaces_from_text_run(runs: &mut Vec<Run>) {
    while let Some(last_run) = runs.last_mut() {
        let Some(last_child) = last_run.children.last_mut() else {
            runs.pop();
            continue;
        };

        let RunChild::Text(text) = last_child else {
            break;
        };

        text.text = text.text.trim_end().to_string();
        if text.text.is_empty() {
            last_run.children.pop();
            if last_run.children.is_empty() {
                runs.pop();
                continue;
            }
        }
        break;
    }
}

fn trim_trailing_spaces_from_paragraph_runs(para: &mut Paragraph) {
    while let Some(last_child) = para.children.last_mut() {
        match last_child {
            docx_rs::ParagraphChild::Run(run) => {
                if trim_trailing_spaces_from_run(run) {
                    para.children.pop();
                    continue;
                }
            }
            docx_rs::ParagraphChild::Hyperlink(link) => {
                while let Some(last_link_child) = link.children.last_mut() {
                    if let docx_rs::ParagraphChild::Run(run) = last_link_child
                        && trim_trailing_spaces_from_run(run)
                    {
                        link.children.pop();
                        continue;
                    }
                    break;
                }
                if link.children.is_empty() {
                    para.children.pop();
                    continue;
                }
            }
            _ => {}
        }
        break;
    }
}

fn trim_trailing_spaces_from_run(run: &mut Run) -> bool {
    while let Some(last_child) = run.children.last_mut() {
        let RunChild::Text(text) = last_child else {
            break;
        };

        text.text = text.text.trim_end().to_string();
        if text.text.is_empty() {
            run.children.pop();
            continue;
        }
        break;
    }
    run.children.is_empty()
}

fn uppercase_inlines(inlines: &[Inline]) -> Vec<Inline> {
    inlines
        .iter()
        .map(|inline| match inline {
            Inline::Text(text) => Inline::Text(text.to_uppercase()),
            Inline::Bold(children) => Inline::Bold(uppercase_inlines(children)),
            Inline::Italic(children) => Inline::Italic(uppercase_inlines(children)),
            Inline::Footnote(children) => Inline::Footnote(uppercase_inlines(children)),
            Inline::InlineMath(value) => Inline::InlineMath(value.clone()),
            Inline::Reference(value) => Inline::Reference(value.clone()),
            Inline::LineBreak => Inline::LineBreak,
        })
        .collect()
}

fn postprocess_docx(output_path: &Path, document_language: Option<&str>) -> anyhow::Result<()> {
    let original = std::fs::read(output_path)
        .with_context(|| format!("failed to read generated DOCX {}", output_path.display()))?;
    let cursor = Cursor::new(original);
    let mut archive = ZipArchive::new(cursor).context("failed to open DOCX as zip archive")?;

    let temp_path = output_path.with_extension("docx.tmp");
    let temp_file = File::create(&temp_path)
        .with_context(|| format!("failed to create temp DOCX {}", temp_path.display()))?;
    let mut writer = ZipWriter::new(temp_file);

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .with_context(|| format!("failed to read zip entry #{index}"))?;
        let entry_name = entry.name().to_string();
        let options: SimpleFileOptions = entry.options();

        if entry.is_dir() {
            writer
                .add_directory(entry_name, options)
                .context("failed to copy DOCX directory entry")?;
            continue;
        }

        let mut data = Vec::new();
        entry
            .read_to_end(&mut data)
            .with_context(|| format!("failed to read {}", entry.name()))?;

        if entry_name == "word/footnotes.xml" {
            let xml = String::from_utf8(data).context("footnotes.xml is not valid UTF-8")?;
            data = inject_footnote_markers(&xml).into_bytes();
        } else if entry_name == "word/styles.xml" {
            let xml = String::from_utf8(data).context("styles.xml is not valid UTF-8")?;
            data = ensure_default_language(&xml, document_language).into_bytes();
        }

        writer
            .start_file(entry_name, options)
            .context("failed to create output zip entry")?;
        writer
            .write_all(&data)
            .context("failed to write output zip entry bytes")?;
    }

    writer.finish().context("failed to finalize patched DOCX")?;
    std::fs::rename(&temp_path, output_path).with_context(|| {
        format!(
            "failed to replace {} with patched DOCX {}",
            output_path.display(),
            temp_path.display()
        )
    })?;

    Ok(())
}

fn inject_footnote_markers(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len() + 4096);
    let mut pos = 0usize;

    while let Some(rel_start) = xml[pos..].find("<w:footnote ") {
        let start = pos + rel_start;
        out.push_str(&xml[pos..start]);

        let Some(rel_end) = xml[start..].find("</w:footnote>") else {
            out.push_str(&xml[start..]);
            return out;
        };
        let end = start + rel_end + "</w:footnote>".len();
        let mut footnote = xml[start..end].to_string();

        let is_separator = footnote.contains("w:type=\"separator\"")
            || footnote.contains("w:type=\"continuationSeparator\"");
        if !is_separator && !footnote.contains("<w:footnoteRef/>") {
            if let Some(ppr_end) = footnote.find("</w:pPr>") {
                footnote.insert_str(ppr_end + "</w:pPr>".len(), FOOTNOTE_MARKER_RUN_XML);
            } else if let Some(ppr_empty) = footnote.find("<w:pPr/>") {
                footnote.insert_str(ppr_empty + "<w:pPr/>".len(), FOOTNOTE_MARKER_RUN_XML);
            } else if let Some(paragraph_start) = footnote.find("<w:p")
                && let Some(open_end_rel) = footnote[paragraph_start..].find('>')
            {
                let insert_at = paragraph_start + open_end_rel + 1;
                footnote.insert_str(insert_at, FOOTNOTE_MARKER_RUN_XML);
            }
        }

        out.push_str(&footnote);
        pos = end;
    }

    out.push_str(&xml[pos..]);
    out
}

/// Inject `w:lang` into the document default run properties for the given BCP-47 tag.
/// If `lang` is `None`, the function is a no-op and returns the input unchanged.
fn ensure_default_language(xml: &str, lang: Option<&str>) -> String {
    let Some(tag) = lang else {
        return xml.to_string();
    };
    let check = format!("w:lang w:val=\"{tag}\"");
    if xml.contains(&check) {
        return xml.to_string();
    }
    let replacement = format!(
        "<w:lang w:val=\"{tag}\" w:eastAsia=\"{tag}\" w:bidi=\"{tag}\" /></w:rPr></w:rPrDefault>"
    );
    xml.replacen("</w:rPr></w:rPrDefault>", &replacement, 1)
}

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/unit/renderer_docx_tests.rs"
    ));
}
