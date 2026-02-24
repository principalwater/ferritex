use std::{
    fs::File,
    io::{Cursor, Read, Write},
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
};

use anyhow::{Context, anyhow};
use docx_rs::{
    AbstractNumbering, AlignmentType, BreakType, Docx, Footnote, Header, IndentLevel, Level,
    LevelJc, LevelText, LineSpacing, LineSpacingType, NumberFormat, Numbering, NumberingId,
    PageMargin, PageNum, Paragraph, Pic, Run, RunChild, RunFonts, SpecialIndentType, Start, Style,
    StyleType, Tab, TabLeaderType, TabValueType, Table as DocxTable, TableAlignmentType,
    TableCell as DocxCell, TableRow as DocxRow, VertAlignType,
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::model::{Block, Document, DocumentLayout, Figure, Inline, ParagraphStyle, Table};

const PAGE_A4_WIDTH_TWIPS: u32 = 11_906;
const PAGE_A4_HEIGHT_TWIPS: u32 = 16_838;

const DEFAULT_PAGE_MARGIN_TOP_TWIPS: i32 = 979;
const DEFAULT_PAGE_MARGIN_BOTTOM_TWIPS: i32 = 922;
const DEFAULT_PAGE_MARGIN_LEFT_TWIPS: i32 = 1_138;
const DEFAULT_PAGE_MARGIN_RIGHT_TWIPS: i32 = 288;
const DEFAULT_PAGE_MARGIN_HEADER_TWIPS: i32 = 342;
const DEFAULT_PAGE_MARGIN_FOOTER_TWIPS: i32 = 342;
const FONT_SIZE_BODY_HP: usize = 28;
const FONT_SIZE_TABLE_HP: usize = 24;
const FONT_SIZE_FOOTNOTE_HP: usize = 20;

const LINE_SPACING_SINGLE_TWIPS: i32 = 240;
const LINE_SPACING_DEFAULT_BODY_TWIPS: i32 = 360;

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
    heading_indent_section_twips: i32,
    heading_indent_subsection_twips: i32,
    heading_indent_subsubsection_twips: i32,
    toc_right_margin_twips: i32,
    toc_use_dot_leader: bool,
    toc_chapter_name_prefix: String,
    toc_indent_chapter_twips: i32,
    toc_numwidth_chapter_twips: i32,
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
    /// Reserved for future appendix TOC rendering; extracted but not yet applied.
    #[allow(dead_code)]
    toc_appendix_name: String,

    // ── List formatting ──────────────────────────────────────────────
    list_left_indent_twips: i32,
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

    // ── Caption alignment ────────────────────────────────────────────
    caption_alignment: AlignmentType,

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
            toc_right_margin_twips: sanitize_nonnegative_twips(
                layout.toc_right_margin_twips,
                DEFAULT_TOC_RIGHT_MARGIN_TWIPS,
            ),
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
            // List indentation: left = parindent, hanging = labelsep + labelwidth.
            list_left_indent_twips: layout.list_left_indent_twips.unwrap_or_else(|| {
                layout
                    .body_first_line_indent_twips
                    .unwrap_or(DEFAULT_LIST_LEFT_TWIPS)
            }),
            list_hanging_indent_twips: layout.list_hanging_indent_twips.unwrap_or_else(|| {
                if layout.list_label_sep_twips.is_none() && layout.list_label_width_twips.is_none()
                {
                    DEFAULT_LIST_HANGING_TWIPS
                } else {
                    let sep = layout.list_label_sep_twips.unwrap_or(142); // 0.5em at 14pt
                    let width = layout.list_label_width_twips.unwrap_or(sep); // auto (!) = labelsep
                    sep + width
                }
            }),
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
            // Caption alignment: default centred.
            caption_alignment: parse_alignment(
                layout.caption_alignment.as_deref().unwrap_or("center"),
            ),
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
#[allow(dead_code)]
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
    let mut docx = create_styled_docx(&profile);
    let figure_base_dir = input_tex_path.and_then(Path::parent);
    let mut chapter_no = 0usize;
    let mut table_no = 0usize;
    let mut figure_no = 0usize;

    // Assign a stable numbering ID for each list block we encounter.
    // abstractNumId == numId for simplicity (one-to-one mapping).
    let mut next_num_id: usize = LIST_NUM_ID_BASE;
    let mut rendered_any_block = false;

    for (index, block) in document.blocks.iter().enumerate() {
        match block {
            Block::Section {
                level,
                number,
                title,
                ..
            } => {
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
                let para = build_paragraph(block, &profile);
                docx = docx.add_paragraph(para);
                if *level == 1 && number.is_none() && is_toc_heading(title) {
                    for toc_para in generated_toc_paragraphs(document, index + 1, &profile) {
                        docx = docx.add_paragraph(toc_para);
                    }
                }
                rendered_any_block = true;
            }
            Block::Paragraph(_) => {
                let para = build_paragraph(block, &profile);
                docx = docx.add_paragraph(para);
                rendered_any_block = true;
            }
            Block::StyledParagraph { .. } => {
                let para = build_paragraph(block, &profile);
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
                if !t.caption.is_empty() && caption_settings.position == CaptionPosition::Top {
                    docx = docx.add_paragraph(caption_paragraph(
                        &profile.caption_label_table,
                        Some(table_number.as_str()),
                        &profile.caption_label_separator_table,
                        &t.caption,
                        caption_settings,
                    ));
                }
                docx = docx.add_table(build_table(t, &profile));
                if !t.caption.is_empty() && caption_settings.position == CaptionPosition::Bottom {
                    docx = docx.add_paragraph(caption_paragraph(
                        &profile.caption_label_table,
                        Some(table_number.as_str()),
                        &profile.caption_label_separator_table,
                        &t.caption,
                        caption_settings,
                    ));
                }
                if !t.source.is_empty() {
                    docx = docx.add_paragraph(source_paragraph(
                        &t.source,
                        &profile,
                        profile.source_vspace_table_twips,
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
                    figure_base_dir,
                    Some(figure_number.as_str()),
                    &profile,
                );
                rendered_any_block = true;
            }
            Block::List(list) => {
                let num_id = next_num_id;
                next_num_id += 1;
                docx = register_numbering(docx, num_id, list.ordered, &profile);
                for item_inlines in &list.items {
                    let para = build_list_item(item_inlines, num_id, &profile);
                    docx = docx.add_paragraph(para);
                }
                rendered_any_block = true;
            }
            Block::DisplayMath(src) => {
                docx = docx.add_paragraph(build_display_math_paragraph(src));
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
        .gutter(0);
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
    docx = docx.header(page_number_header());

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
            .align(AlignmentType::Both)
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
            .bold()
            .align(profile.caption_alignment)
            .line_spacing(single_spacing())
            .indent(Some(0), None, None, None),
        Style::new("FootnoteText", StyleType::Paragraph)
            .name("Footnote Text")
            .based_on("Normal")
            .next("FootnoteText")
            .fonts(fonts.clone())
            .size(profile.font_size_footnote_hp)
            .align(AlignmentType::Both)
            .line_spacing(single_spacing())
            .indent(Some(0), None, None, None),
        Style::new("ListParagraph", StyleType::Paragraph)
            .name("List Paragraph")
            .based_on("BodyText")
            .next("BodyText")
            .fonts(fonts.clone())
            .size(profile.font_size_body_hp)
            .align(AlignmentType::Both)
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
    Style::new(style_id, StyleType::Paragraph)
        .name(style_name)
        .based_on("Normal")
        .next("BodyText")
        .fonts(fonts)
        .size(profile.font_size_body_hp)
        .bold()
        .align(profile.heading_alignment)
        .line_spacing(line_spacing(profile.body_line_spacing_twips))
        .indent(
            Some(heading_left_indent_twips(level, profile)),
            Some(SpecialIndentType::FirstLine(0)),
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
        .line_spacing(single_spacing())
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

fn page_number_header() -> Header {
    Header::new().add_paragraph(
        Paragraph::new()
            .align(AlignmentType::Center)
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
fn build_list_item(inlines: &[Inline], num_id: usize, profile: &RenderProfile) -> Paragraph {
    let mut para = Paragraph::new()
        .style("ListParagraph")
        .align(AlignmentType::Both)
        .line_spacing(line_spacing(profile.body_line_spacing_twips))
        .numbering(NumberingId::new(num_id), IndentLevel::new(0));
    for run in inline_runs_with_footnote_size(inlines, false, false, profile.font_size_footnote_hp)
    {
        para = para.add_run(run);
    }
    para
}

// ---------------------------------------------------------------------------
// Table rendering
// ---------------------------------------------------------------------------

fn build_table(table: &Table, profile: &RenderProfile) -> DocxTable {
    let rows: Vec<DocxRow> = table
        .rows
        .iter()
        .map(|row| {
            let cells: Vec<DocxCell> = row
                .cells
                .iter()
                .map(|cell| {
                    let mut para = Paragraph::new()
                        .style("TableParagraph")
                        .align(AlignmentType::Both)
                        .line_spacing(single_spacing())
                        .indent(Some(0), Some(SpecialIndentType::FirstLine(0)), None, None);
                    for run in inline_runs_with_footnote_size(
                        &cell.content,
                        false,
                        false,
                        profile.font_size_footnote_hp,
                    ) {
                        para = para.add_run(run.size(profile.font_size_table_hp));
                    }
                    DocxCell::new().add_paragraph(para)
                })
                .collect();
            DocxRow::new(cells)
        })
        .collect();
    DocxTable::new(rows).align(parse_table_alignment(table.alignment.as_deref()))
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
        para = para.add_run(Run::new().add_text(prefix).bold());
    }

    for run in inline_runs_with_footnote_size(inlines, true, false, settings.footnote_font_size_hp)
    {
        para = para.add_run(run);
    }
    para
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
fn source_paragraph(inlines: &[Inline], profile: &RenderProfile, vspace_twips: i32) -> Paragraph {
    let spacing = {
        let mut s = line_spacing(profile.body_line_spacing_twips);
        if vspace_twips > 0 {
            s = s.before(vspace_twips as u32);
        }
        s
    };
    let mut para = Paragraph::new()
        .style("BodyText")
        .align(AlignmentType::Left)
        .line_spacing(spacing)
        .indent(Some(0), Some(SpecialIndentType::FirstLine(0)), None, None);
    for run in inline_runs_with_footnote_size(inlines, false, true, profile.font_size_footnote_hp) {
        para = para.add_run(run);
    }
    para
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
    Paragraph::new()
        .style("Heading1")
        .align(profile.heading_alignment)
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

fn render_figure_block(
    mut docx: Docx,
    figure: &Figure,
    base_dir: Option<&Path>,
    figure_number: Option<&str>,
    profile: &RenderProfile,
) -> Docx {
    let mut embedded = false;
    let max_image_width_emu = profile.max_image_width_emu();
    let caption_settings = figure_caption_settings(profile);
    let render_caption_before_figure = caption_settings.position == CaptionPosition::Top;
    let figure_alignment = parse_alignment(figure.alignment.as_deref().unwrap_or("center"));

    if render_caption_before_figure && !figure.caption.is_empty() {
        docx = docx.add_paragraph(caption_paragraph(
            &profile.caption_label_figure,
            figure_number,
            &profile.caption_label_separator_figure,
            &figure.caption,
            caption_settings,
        ));
    }

    if let Some(raw_path) = figure.image_path.as_deref() {
        if let Some(resolved) =
            resolve_figure_path(raw_path, base_dir, &profile.graphics_search_paths)
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
        docx = docx.add_paragraph(caption_paragraph(
            &profile.caption_label_figure,
            figure_number,
            &profile.caption_label_separator_figure,
            &figure.caption,
            caption_settings,
        ));
    }

    if !figure.source.is_empty() {
        docx = docx.add_paragraph(source_paragraph(
            &figure.source,
            profile,
            profile.source_vspace_figure_twips,
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
fn build_paragraph(block: &Block, profile: &RenderProfile) -> Paragraph {
    match block {
        Block::Section {
            level,
            number,
            title,
            ..
        } => {
            let style = heading_style(*level);
            let heading_indent = heading_left_indent_twips(*level, profile);
            let mut para = Paragraph::new()
                .style(style)
                .align(profile.heading_alignment)
                .line_spacing(line_spacing(profile.body_line_spacing_twips))
                .indent(
                    Some(heading_indent),
                    Some(SpecialIndentType::FirstLine(0)),
                    None,
                    None,
                );
            let section_title = if *level == 1 && profile.heading_uppercase {
                uppercase_inlines(title)
            } else {
                title.to_vec()
            };
            if let Some(number) = number {
                let delim = &profile.heading_number_delimiter;
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
            for run in inline_runs_with_footnote_size(
                &section_title,
                true,
                false,
                profile.font_size_footnote_hp,
            ) {
                para = para.add_run(run);
            }
            para
        }
        Block::Paragraph(inlines) => build_default_body_paragraph(inlines, profile),
        Block::StyledParagraph { inlines, style } => {
            build_styled_body_paragraph(inlines, style, profile)
        }
        // Table, Figure, List, DisplayMath, BibliographyHeading are handled separately — unreachable here.
        Block::Table(_)
        | Block::Figure(_)
        | Block::List(_)
        | Block::DisplayMath(_)
        | Block::BibliographyHeading { .. } => {
            unreachable!()
        }
    }
}

fn build_default_body_paragraph(inlines: &[Inline], profile: &RenderProfile) -> Paragraph {
    let mut para = Paragraph::new()
        .style("BodyText")
        .align(AlignmentType::Both)
        .line_spacing(line_spacing(profile.body_line_spacing_twips))
        .indent(
            Some(0),
            Some(SpecialIndentType::FirstLine(
                profile.body_first_line_indent_twips,
            )),
            None,
            None,
        );
    for run in inline_runs_with_footnote_size(inlines, false, false, profile.font_size_footnote_hp)
    {
        para = para.add_run(run);
    }
    para
}

fn build_styled_body_paragraph(
    inlines: &[Inline],
    style: &ParagraphStyle,
    profile: &RenderProfile,
) -> Paragraph {
    let alignment = style
        .alignment
        .as_deref()
        .map(parse_alignment)
        .unwrap_or(AlignmentType::Both);
    let line_twips = style
        .line_spacing_twips
        .unwrap_or(profile.body_line_spacing_twips);
    let first_line_indent = style
        .first_line_indent_twips
        .unwrap_or(profile.body_first_line_indent_twips);
    let mut para = Paragraph::new()
        .style("BodyText")
        .align(alignment)
        .line_spacing(line_spacing_with_spacing(
            line_twips,
            style.space_before_twips,
            style.space_after_twips,
        ))
        .indent(
            Some(0),
            Some(SpecialIndentType::FirstLine(first_line_indent)),
            None,
            None,
        );

    for run in inline_runs_with_footnote_size(inlines, false, false, profile.font_size_footnote_hp)
    {
        let run = if let Some(size_hp) = style.font_size_hp {
            run.size(size_hp)
        } else {
            run
        };
        para = para.add_run(run);
    }
    para
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

/// Map a heading level (1-based) to a docx-rs style id string.
fn heading_style(level: u8) -> &'static str {
    match level {
        1 => "Heading1",
        2 => "Heading2",
        _ => "Heading3",
    }
}

fn is_toc_heading(title: &[Inline]) -> bool {
    let text = collect_inline_text(title).to_uppercase();
    text.contains("ОГЛАВЛЕНИЕ")
}

fn generated_toc_paragraphs(
    document: &Document,
    start_index: usize,
    profile: &RenderProfile,
) -> Vec<Paragraph> {
    if !document.toc_entries.is_empty() {
        return generated_toc_paragraphs_from_entries(document, profile);
    }

    let mut paragraphs = Vec::new();

    for block in document.blocks.iter().skip(start_index) {
        let Block::Section {
            level,
            number,
            title,
            ..
        } = block
        else {
            continue;
        };

        if *level == 0 || *level > 2 {
            continue;
        }
        if *level == 1 && number.is_none() && is_toc_heading(title) {
            continue;
        }

        paragraphs.push(build_toc_entry_paragraph(
            *level,
            number.as_deref(),
            title,
            None,
            profile,
        ));
    }

    paragraphs
}

fn generated_toc_paragraphs_from_entries(
    document: &Document,
    profile: &RenderProfile,
) -> Vec<Paragraph> {
    let mut paragraphs = Vec::new();
    for entry in &document.toc_entries {
        if entry.level == 0 {
            if !entry.title.trim().is_empty() {
                paragraphs.push(build_toc_page_header_paragraph(&entry.title, profile));
            }
            continue;
        }
        if entry.level > 6 {
            continue;
        }
        let title_inlines = vec![Inline::Text(entry.title.clone())];
        if entry.level == 1 && entry.number.is_none() && is_toc_heading(&title_inlines) {
            continue;
        }
        paragraphs.push(build_toc_entry_paragraph(
            entry.level,
            entry.number.as_deref(),
            &title_inlines,
            entry.page.as_deref(),
            profile,
        ));
    }
    paragraphs
}

fn build_toc_page_header_paragraph(text: &str, profile: &RenderProfile) -> Paragraph {
    let mut para = Paragraph::new()
        .style("TOC1")
        .align(AlignmentType::Right)
        .line_spacing(single_spacing());
    let inlines = vec![Inline::Text(text.to_string())];
    for run in inline_runs_with_footnote_size(&inlines, false, false, profile.font_size_footnote_hp)
    {
        para = para.add_run(run);
    }
    para
}

fn build_toc_entry_paragraph(
    level: u8,
    number: Option<&str>,
    title: &[Inline],
    page: Option<&str>,
    profile: &RenderProfile,
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
        .line_spacing(single_spacing());
    let toc_indent = profile.toc_level_indent_twips(level);
    let toc_numwidth = profile.toc_level_numwidth_twips(level);

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
            let chapter_prefix = profile.toc_chapter_name_prefix.trim();
            if !chapter_prefix.is_empty() {
                prefix.push_str(&chapter_prefix.to_uppercase());
                prefix.push(' ');
            }
            let number_core = number.trim_end_matches('.');
            prefix.push_str(number_core);
            // Use aftersnum separator if available, else fall back to heading delimiter.
            if !aftersnum.is_empty() {
                prefix.push_str(aftersnum);
            } else {
                prefix.push_str(&profile.heading_number_delimiter);
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

    let title = if level == 1 {
        uppercase_inlines(title)
    } else {
        title.to_vec()
    };
    // Apply bold/non-bold per toc_chapter_entry_bold (level-1 only).
    let title_runs =
        inline_runs_with_footnote_size(&title, false, false, profile.font_size_footnote_hp);
    if level == 1 && !profile.toc_chapter_entry_bold {
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
    use super::*;
    use crate::model::{Block, Document, Inline, Table, TableCell, TableRow, TocEntry};
    use docx_rs::BuildXML;

    #[test]
    fn float_number_is_chapter_aware() {
        assert_eq!(float_number(0, 3), "3");
        assert_eq!(float_number(2, 4), "2.4");
    }

    #[test]
    fn caption_prefix_detection_is_case_insensitive() {
        let prefixed = vec![Inline::Text("Таблица 1. Пример".to_string())];
        let plain = vec![Inline::Text("Пример".to_string())];
        assert!(caption_is_prefixed("ТАБЛИЦА", &prefixed));
        assert!(!caption_is_prefixed("Таблица", &plain));
    }

    #[test]
    fn caption_prefix_uses_configured_separator() {
        assert_eq!(
            caption_prefix_text("Table", Some("2"), " --- "),
            Some("Table 2 --- ".to_string())
        );
        assert_eq!(
            caption_prefix_text("Figure", Some("3"), ": "),
            Some("Figure 3: ".to_string())
        );
    }

    #[test]
    fn caption_paragraph_top_position_sets_after_skip_and_indent() {
        let inlines = vec![Inline::Text("Sample caption".to_string())];
        let para = caption_paragraph(
            "Table",
            Some("1"),
            ". ",
            &inlines,
            CaptionRenderSettings {
                default_alignment: AlignmentType::Left,
                indent_twips: 120,
                skip_twips: 60,
                position: CaptionPosition::Top,
                singlelinecheck: false,
                footnote_font_size_hp: 20,
            },
        );
        let xml = String::from_utf8(para.build()).expect("paragraph xml should be utf8");

        assert!(xml.contains("w:after=\"60\""), "xml: {xml}");
        assert!(xml.contains("w:left=\"120\""), "xml: {xml}");
        assert!(xml.contains("w:jc w:val=\"left\""), "xml: {xml}");
    }

    #[test]
    fn caption_paragraph_bottom_position_sets_before_skip() {
        let inlines = vec![Inline::Text("Sample caption".to_string())];
        let para = caption_paragraph(
            "Figure",
            Some("2"),
            ". ",
            &inlines,
            CaptionRenderSettings {
                default_alignment: AlignmentType::Center,
                indent_twips: 0,
                skip_twips: 40,
                position: CaptionPosition::Bottom,
                singlelinecheck: false,
                footnote_font_size_hp: 20,
            },
        );
        let xml = String::from_utf8(para.build()).expect("paragraph xml should be utf8");

        assert!(xml.contains("w:before=\"40\""), "xml: {xml}");
    }

    #[test]
    fn caption_singlelinecheck_centers_short_caption_only() {
        let short = vec![Inline::Text("Short caption".to_string())];
        let centered = caption_paragraph(
            "Figure",
            Some("1"),
            ". ",
            &short,
            CaptionRenderSettings {
                default_alignment: AlignmentType::Left,
                indent_twips: 0,
                skip_twips: 0,
                position: CaptionPosition::Bottom,
                singlelinecheck: true,
                footnote_font_size_hp: 20,
            },
        );
        let centered_xml =
            String::from_utf8(centered.build()).expect("paragraph xml should be utf8");
        assert!(
            centered_xml.contains("w:jc w:val=\"center\""),
            "xml: {centered_xml}"
        );

        let long_text = "very long caption text ".repeat(8);
        let long = vec![Inline::Text(long_text)];
        let kept = caption_paragraph(
            "Figure",
            Some("1"),
            ". ",
            &long,
            CaptionRenderSettings {
                default_alignment: AlignmentType::Left,
                indent_twips: 0,
                skip_twips: 0,
                position: CaptionPosition::Bottom,
                singlelinecheck: true,
                footnote_font_size_hp: 20,
            },
        );
        let kept_xml = String::from_utf8(kept.build()).expect("paragraph xml should be utf8");
        assert!(kept_xml.contains("w:jc w:val=\"left\""), "xml: {kept_xml}");
    }

    #[test]
    fn chapter_prefix_does_not_duplicate_trailing_dot() {
        let profile = RenderProfile::from_layout(&DocumentLayout {
            chapter_name: Some("Глава".to_string()),
            heading_number_delimiter: Some(".".to_string()),
            ..DocumentLayout::default()
        });
        let para = build_paragraph(
            &Block::Section {
                level: 1,
                number: Some("1.".to_string()),
                label: None,
                title: vec![Inline::Text("Заголовок".to_string())],
            },
            &profile,
        );
        let xml = String::from_utf8(para.build()).expect("paragraph xml should be utf8");
        assert!(xml.contains("Глава 1. "), "xml: {xml}");
        assert!(!xml.contains("1.."), "xml: {xml}");
    }

    #[test]
    fn table_alignment_uses_center_when_requested() {
        let profile = RenderProfile::from_layout(&DocumentLayout::default());
        let table = Table {
            caption: Vec::new(),
            label: None,
            source: Vec::new(),
            alignment: Some("center".to_string()),
            rows: vec![TableRow {
                cells: vec![TableCell {
                    content: vec![Inline::Text("X".to_string())],
                }],
            }],
        };
        let xml = String::from_utf8(build_table(&table, &profile).build())
            .expect("table xml should be utf8");
        assert!(xml.contains("w:jc w:val=\"center\""), "xml: {xml}");
    }

    #[test]
    fn section_heading_uses_latex_driven_indent() {
        let profile = RenderProfile::from_layout(&DocumentLayout {
            heading_indent_section_twips: Some(709),
            ..DocumentLayout::default()
        });
        let para = build_paragraph(
            &Block::Section {
                level: 2,
                number: Some("1.1".to_string()),
                label: None,
                title: vec![Inline::Text("Section".to_string())],
            },
            &profile,
        );
        let xml = String::from_utf8(para.build()).expect("paragraph xml should be utf8");
        assert!(xml.contains("w:left=\"709\""), "xml: {xml}");
    }

    #[test]
    fn normalize_math_text_supports_sim_and_double_dash_ranges() {
        assert_eq!(normalize_math_text("\\sim8 300--8 500"), "≈8 300–8 500");
    }

    #[test]
    fn generated_toc_paragraphs_include_chapters_and_sections() {
        let document = Document {
            blocks: vec![
                Block::Section {
                    level: 1,
                    number: None,
                    label: None,
                    title: vec![Inline::Text("ОГЛАВЛЕНИЕ".to_string())],
                },
                Block::Section {
                    level: 1,
                    number: Some("1.".to_string()),
                    label: None,
                    title: vec![Inline::Text("Глава".to_string())],
                },
                Block::Section {
                    level: 2,
                    number: Some("1.1".to_string()),
                    label: None,
                    title: vec![Inline::Text("Раздел".to_string())],
                },
            ],
            layout: DocumentLayout::default(),
            toc_entries: Vec::new(),
        };
        let profile = RenderProfile::from_layout(&document.layout);
        let paragraphs = generated_toc_paragraphs(&document, 1, &profile);
        assert_eq!(paragraphs.len(), 2);

        let first_xml = String::from_utf8(paragraphs[0].build()).expect("paragraph xml utf8");
        let second_xml = String::from_utf8(paragraphs[1].build()).expect("paragraph xml utf8");
        assert!(
            first_xml.contains("w:pStyle w:val=\"TOC1\""),
            "xml: {first_xml}"
        );
        assert!(
            second_xml.contains("w:pStyle w:val=\"TOC2\""),
            "xml: {second_xml}"
        );
        assert!(first_xml.contains("1. "), "xml: {first_xml}");
        assert!(second_xml.contains("1.1 "), "xml: {second_xml}");
    }

    #[test]
    fn generated_toc_paragraphs_from_entries_include_page_header_line() {
        let document = Document {
            blocks: Vec::new(),
            layout: DocumentLayout::default(),
            toc_entries: vec![
                TocEntry {
                    level: 0,
                    number: None,
                    title: "Page.".to_string(),
                    page: None,
                },
                TocEntry {
                    level: 1,
                    number: Some("1.".to_string()),
                    title: "Intro".to_string(),
                    page: Some("3".to_string()),
                },
            ],
        };
        let profile = RenderProfile::from_layout(&document.layout);
        let paragraphs = generated_toc_paragraphs(&document, 0, &profile);
        assert_eq!(paragraphs.len(), 2);

        let header_xml = String::from_utf8(paragraphs[0].build()).expect("paragraph xml utf8");
        let entry_xml = String::from_utf8(paragraphs[1].build()).expect("paragraph xml utf8");
        assert!(
            header_xml.contains("w:jc w:val=\"right\""),
            "xml: {header_xml}"
        );
        assert!(header_xml.contains("Page."), "xml: {header_xml}");
        assert!(
            entry_xml.contains("w:pStyle w:val=\"TOC1\""),
            "xml: {entry_xml}"
        );
        assert!(
            entry_xml.contains("w:tab w:val=\"right\" w:leader=\"dot\""),
            "xml: {entry_xml}"
        );
        assert!(entry_xml.contains("<w:tab "), "xml: {entry_xml}");
        assert!(entry_xml.contains(">3<"), "xml: {entry_xml}");
    }

    #[test]
    fn toc_entry_uses_latex_chapter_name_prefix_when_configured() {
        let layout = DocumentLayout {
            toc_chapter_name_prefix: Some("Глава".to_string()),
            ..DocumentLayout::default()
        };
        let document = Document {
            blocks: Vec::new(),
            layout: layout.clone(),
            toc_entries: vec![TocEntry {
                level: 1,
                number: Some("1.".to_string()),
                title: "Раздел".to_string(),
                page: Some("5".to_string()),
            }],
        };
        let profile = RenderProfile::from_layout(&layout);
        let paragraphs = generated_toc_paragraphs(&document, 0, &profile);
        assert_eq!(paragraphs.len(), 1);

        let xml = String::from_utf8(paragraphs[0].build()).expect("paragraph xml utf8");
        assert!(xml.contains("ГЛАВА 1. "), "xml: {xml}");
        assert!(!xml.contains("1.."), "xml: {xml}");
    }

    #[test]
    fn toc_entry_uses_latex_driven_numwidth_for_hanging_indent() {
        let layout = DocumentLayout {
            toc_indent_section_twips: Some(400),
            toc_numwidth_section_twips: Some(700),
            ..DocumentLayout::default()
        };
        let document = Document {
            blocks: Vec::new(),
            layout: layout.clone(),
            toc_entries: vec![TocEntry {
                level: 2,
                number: Some("1.1".to_string()),
                title: "A very long section title for hanging indent behavior".to_string(),
                page: Some("12".to_string()),
            }],
        };
        let profile = RenderProfile::from_layout(&layout);
        let paragraphs = generated_toc_paragraphs(&document, 0, &profile);
        assert_eq!(paragraphs.len(), 1);

        let xml = String::from_utf8(paragraphs[0].build()).expect("paragraph xml utf8");
        assert!(xml.contains("w:left=\"1100\""), "xml: {xml}");
        assert!(xml.contains("w:hanging=\"700\""), "xml: {xml}");
    }

    #[test]
    fn linebreak_inline_renders_text_wrapping_break() {
        let profile = RenderProfile::from_layout(&DocumentLayout::default());
        let para = build_styled_body_paragraph(
            &[
                Inline::Text("Line 1".to_string()),
                Inline::LineBreak,
                Inline::Text("Line 2".to_string()),
            ],
            &crate::model::ParagraphStyle::default(),
            &profile,
        );
        let xml = String::from_utf8(para.build()).expect("paragraph xml utf8");
        assert!(xml.contains("w:br w:type=\"textWrapping\""), "xml: {xml}");
    }

    #[test]
    fn resolve_figure_path_uses_images_fallback_and_extension_guess() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ferritex_figure_path_{unique}"));
        let images_dir = root.join("images").join("part2");
        std::fs::create_dir_all(&images_dir).expect("failed to create images dir");
        let image_path = images_dir.join("chart.png");
        std::fs::write(&image_path, b"fake").expect("failed to write image");

        let default_paths = vec![
            "images".to_string(),
            "figures".to_string(),
            "img".to_string(),
        ];
        let resolved = resolve_figure_path("part2/chart", Some(&root), &default_paths);
        assert_eq!(resolved, Some(image_path.clone()));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_figure_path_accepts_absolute_path() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ferritex_figure_abs_{unique}"));
        std::fs::create_dir_all(&root).expect("failed to create dir");
        let image_path = root.join("figure.jpg");
        std::fs::write(&image_path, b"fake").expect("failed to write image");

        let absolute = image_path.to_string_lossy().to_string();
        let resolved = resolve_figure_path(&absolute, None, &[]);
        assert_eq!(resolved, Some(image_path.clone()));

        let _ = std::fs::remove_dir_all(&root);
    }

    // ── Task 3: new TOC fields — fallback defaults ────────────────────

    #[test]
    fn toc_fallback_defaults_new_fields() {
        let profile = RenderProfile::from_layout(&DocumentLayout::default());
        assert!(
            profile.toc_chapter_entry_bold,
            "default chapter entry should be bold"
        );
        assert!(
            profile.toc_chapter_page_bold,
            "default chapter page number should be bold"
        );
        assert_eq!(profile.toc_aftersnum_chapter, "");
        assert_eq!(profile.toc_aftersnum_section, "");
        assert_eq!(profile.toc_aftersnum_subsection, "");
        assert_eq!(profile.toc_aftersnum_subsubsection, "");
    }

    #[test]
    fn toc_entry_chapter_non_bold_when_toc_chapter_entry_bold_false() {
        let layout = DocumentLayout {
            toc_chapter_entry_bold: Some(false),
            ..DocumentLayout::default()
        };
        let document = Document {
            blocks: Vec::new(),
            layout: layout.clone(),
            toc_entries: vec![TocEntry {
                level: 1,
                number: Some("1.".to_string()),
                title: "Introduction".to_string(),
                page: Some("5".to_string()),
            }],
        };
        let profile = RenderProfile::from_layout(&layout);
        let paragraphs = generated_toc_paragraphs(&document, 0, &profile);
        assert_eq!(paragraphs.len(), 1);
        let xml = String::from_utf8(paragraphs[0].build()).expect("xml utf8");
        // When non-bold is requested, w:b w:val="false" must appear in the title run.
        assert!(
            xml.contains("w:b w:val=\"false\""),
            "expected non-bold chapter entry, xml: {xml}"
        );
    }

    #[test]
    fn toc_entry_uses_aftersnum_chapter_separator() {
        let layout = DocumentLayout {
            toc_aftersnum_chapter: Some(". ".to_string()),
            ..DocumentLayout::default()
        };
        let document = Document {
            blocks: Vec::new(),
            layout: layout.clone(),
            toc_entries: vec![TocEntry {
                level: 1,
                number: Some("1.".to_string()),
                title: "Sample chapter".to_string(),
                page: Some("3".to_string()),
            }],
        };
        let profile = RenderProfile::from_layout(&layout);
        let paragraphs = generated_toc_paragraphs(&document, 0, &profile);
        assert_eq!(paragraphs.len(), 1);
        let xml = String::from_utf8(paragraphs[0].build()).expect("xml utf8");
        // Number prefix should use aftersnum ". " (rendered as "1. ").
        assert!(
            xml.contains("1. "),
            "expected '1. ' with aftersnum separator, xml: {xml}"
        );
    }

    // ── List indent geometry tests ────────────────────────────────────────────

    #[test]
    fn list_indent_defaults_use_body_first_line_indent() {
        let mut layout = DocumentLayout::default();
        layout.body_first_line_indent_twips = Some(709);
        let profile = RenderProfile::from_layout(&layout);
        // list_left should equal body first-line indent (parindent).
        assert_eq!(profile.list_left_indent_twips, 709);
        // list_hanging = labelsep + labelwidth (both default to ~142 twips).
        assert!(
            profile.list_hanging_indent_twips > 0,
            "hanging indent should be positive"
        );
    }

    #[test]
    fn list_bullet_char_fallback_is_bullet() {
        let layout = DocumentLayout::default();
        let profile = RenderProfile::from_layout(&layout);
        assert_eq!(profile.list_bullet_char, "•");
    }

    #[test]
    fn list_bullet_char_from_layout() {
        let mut layout = DocumentLayout::default();
        layout.list_bullet_char = Some("–".to_string());
        let profile = RenderProfile::from_layout(&layout);
        assert_eq!(profile.list_bullet_char, "–");
    }

    // ── Source vspace tests ───────────────────────────────────────────────────

    #[test]
    fn source_vspace_defaults() {
        let layout = DocumentLayout::default();
        let profile = RenderProfile::from_layout(&layout);
        assert_eq!(
            profile.source_vspace_table_twips,
            DEFAULT_SOURCE_VSPACE_TABLE_TWIPS
        );
        assert_eq!(
            profile.source_vspace_figure_twips,
            DEFAULT_SOURCE_VSPACE_FIGURE_TWIPS
        );
    }

    #[test]
    fn source_vspace_from_layout() {
        let mut layout = DocumentLayout::default();
        layout.source_vspace_table_twips = Some(100);
        layout.source_vspace_figure_twips = Some(50);
        let profile = RenderProfile::from_layout(&layout);
        assert_eq!(profile.source_vspace_table_twips, 100);
        assert_eq!(profile.source_vspace_figure_twips, 50);
    }

    #[test]
    fn caption_indent_defaults_to_zero_for_figure_and_table() {
        let profile = RenderProfile::from_layout(&DocumentLayout::default());
        assert_eq!(profile.caption_indent_twips_figure, 0);
        assert_eq!(profile.caption_indent_twips_table, 0);
    }

    // ── Title page suppression tests ──────────────────────────────────────────

    #[test]
    fn title_page_suppress_defaults_false() {
        let layout = DocumentLayout::default();
        let profile = RenderProfile::from_layout(&layout);
        assert!(!profile.title_page_suppress_number);
    }

    #[test]
    fn title_page_suppress_from_layout() {
        let mut layout = DocumentLayout::default();
        layout.title_page_suppress_number = Some(true);
        let profile = RenderProfile::from_layout(&layout);
        assert!(profile.title_page_suppress_number);
    }
}
