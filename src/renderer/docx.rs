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
    StyleType, Table as DocxTable, TableCell as DocxCell, TableRow as DocxRow, VertAlignType,
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::model::{Block, Document, DocumentLayout, Figure, Inline, Table};

const PAGE_A4_WIDTH_TWIPS: u32 = 11_906;
const PAGE_A4_HEIGHT_TWIPS: u32 = 16_838;

const DEFAULT_PAGE_MARGIN_TOP_TWIPS: i32 = 979;
const DEFAULT_PAGE_MARGIN_BOTTOM_TWIPS: i32 = 922;
const DEFAULT_PAGE_MARGIN_LEFT_TWIPS: i32 = 1_138;
const DEFAULT_PAGE_MARGIN_RIGHT_TWIPS: i32 = 288;
const DEFAULT_PAGE_MARGIN_HEADER_TWIPS: i32 = 342;
const DEFAULT_PAGE_MARGIN_FOOTER_TWIPS: i32 = 342;
const HEADING_LEFT_INDENT_TWIPS: i32 = 0;

const FONT_SIZE_BODY_HP: usize = 28;
const FONT_SIZE_TABLE_HP: usize = 24;
const FONT_SIZE_FOOTNOTE_HP: usize = 20;

const LINE_SPACING_SINGLE_TWIPS: i32 = 240;
const LINE_SPACING_DEFAULT_BODY_TWIPS: i32 = 360;

const FIRST_LINE_INDENT_TWIPS: i32 = 709;
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "bmp", "tif", "tiff", "gif", "webp"];
const EMU_PER_TWIP: u32 = 635;
const IMAGE_SAFE_SCALE_NUM: u32 = 96;
const IMAGE_SAFE_SCALE_DEN: u32 = 100;
const LIST_LEVEL_LEFT_INDENT_TWIPS: i32 = 600;
const LIST_LEVEL_HANGING_TWIPS: i32 = 240;
const LIST_NUM_ID_BASE: usize = 100;
const FOOTNOTE_MARKER_RUN_XML: &str = "<w:r><w:rPr><w:vertAlign w:val=\"superscript\" /></w:rPr><w:footnoteRef/></w:r><w:r><w:t xml:space=\"preserve\"> </w:t></w:r>";

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

    // ── Heading formatting ─────────────────────────────────────────────
    chapter_name: String,
    heading_uppercase: bool,

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
            // Heading: no chapter prefix by default, no uppercase.
            chapter_name: layout.chapter_name.clone().unwrap_or_default(),
            heading_uppercase: layout.heading_uppercase.unwrap_or(false),
            // Language: None → no explicit language tag in DOCX.
            document_language: layout.document_language.clone(),
        }
    }

    fn max_image_width_emu(&self) -> u32 {
        let text_width_twips = PAGE_A4_WIDTH_TWIPS.saturating_sub(
            self.page_margin_left_twips.max(0) as u32 + self.page_margin_right_twips.max(0) as u32,
        );
        text_width_twips * EMU_PER_TWIP * IMAGE_SAFE_SCALE_NUM / IMAGE_SAFE_SCALE_DEN
    }
}

fn sanitize_twips(value: Option<i32>, fallback: i32) -> i32 {
    match value {
        Some(v) if v > 0 => v,
        _ => fallback,
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

    for block in &document.blocks {
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
                let para = build_paragraph(block, &profile);
                docx = docx.add_paragraph(para);
                rendered_any_block = true;
            }
            Block::Paragraph(_) => {
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
                if !t.caption.is_empty() {
                    docx = docx.add_paragraph(caption_paragraph(
                        &profile.caption_label_table,
                        Some(table_number.as_str()),
                        &t.caption,
                    ));
                }
                docx = docx.add_table(build_table(t, &profile));
                if !t.source.is_empty() {
                    docx = docx.add_paragraph(source_paragraph(&t.source, &profile));
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
                docx = register_numbering(docx, num_id, list.ordered);
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
        .page_size(PAGE_A4_WIDTH_TWIPS, PAGE_A4_HEIGHT_TWIPS)
        .page_margin(page_margin)
        .default_size(profile.font_size_body_hp)
        .default_fonts(fonts.clone())
        .default_line_spacing(line_spacing(profile.body_line_spacing_twips));

    for style in gost_styles(fonts, profile) {
        docx = docx.add_style(style);
    }

    docx.header(page_number_header())
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
        Style::new("Caption", StyleType::Paragraph)
            .name("Caption")
            .based_on("BodyText")
            .next("BodyText")
            .fonts(fonts.clone())
            .size(profile.font_size_caption_hp)
            .bold()
            .align(AlignmentType::Center)
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
    Style::new(style_id, StyleType::Paragraph)
        .name(style_name)
        .based_on("Normal")
        .next("BodyText")
        .fonts(fonts)
        .size(profile.font_size_body_hp)
        .bold()
        .align(AlignmentType::Both)
        .line_spacing(line_spacing(profile.body_line_spacing_twips))
        .indent(Some(HEADING_LEFT_INDENT_TWIPS), None, None, None)
        .outline_lvl(outline_level)
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
fn register_numbering(docx: Docx, num_id: usize, ordered: bool) -> Docx {
    let abs_id = num_id - 1; // abstract IDs are 0-based by convention

    let (format, text) = if ordered {
        ("decimal", "%1.")
    } else {
        ("bullet", "•")
    };

    let level = Level::new(
        0,
        Start::new(1),
        NumberFormat::new(format),
        LevelText::new(text),
        LevelJc::new("left"),
    )
    .indent(
        Some(LIST_LEVEL_LEFT_INDENT_TWIPS),
        Some(SpecialIndentType::Hanging(LIST_LEVEL_HANGING_TWIPS)),
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
    for run in inline_runs(inlines, false, false) {
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
                    for run in inline_runs(&cell.content, false, false) {
                        para = para.add_run(run.size(profile.font_size_table_hp));
                    }
                    DocxCell::new().add_paragraph(para)
                })
                .collect();
            DocxRow::new(cells)
        })
        .collect();
    DocxTable::new(rows)
}

fn float_number(chapter_no: usize, local_no: usize) -> String {
    if chapter_no > 0 {
        format!("{chapter_no}.{local_no}")
    } else {
        local_no.to_string()
    }
}

/// A centred bold paragraph used for captions.
fn caption_paragraph(kind: &str, number: Option<&str>, inlines: &[Inline]) -> Paragraph {
    let mut para = Paragraph::new()
        .style("Caption")
        .align(AlignmentType::Center)
        .line_spacing(single_spacing())
        .indent(Some(0), None, None, None);

    let prefixed = caption_is_prefixed(kind, inlines);
    if !prefixed {
        if let Some(number) = number {
            para = para.add_run(Run::new().add_text(format!("{kind} {number}. ")).bold());
        } else if !kind.is_empty() {
            para = para.add_run(Run::new().add_text(format!("{kind}. ")).bold());
        }
    }

    for run in inline_runs(inlines, true, false) {
        para = para.add_run(run);
    }
    para
}

fn caption_is_prefixed(kind: &str, inlines: &[Inline]) -> bool {
    let mut text = String::new();
    for inline in inlines {
        match inline {
            Inline::Text(value) => text.push_str(value),
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

/// A justified plain paragraph used for source attributions.
fn source_paragraph(inlines: &[Inline], profile: &RenderProfile) -> Paragraph {
    let mut para = Paragraph::new()
        .style("BodyText")
        .align(AlignmentType::Both)
        .line_spacing(line_spacing(profile.body_line_spacing_twips))
        .indent(Some(0), None, None, None);
    for run in inline_runs(inlines, false, false) {
        para = para.add_run(run);
    }
    para
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

    if let Some(raw_path) = figure.image_path.as_deref() {
        if let Some(resolved) = resolve_figure_path(raw_path, base_dir) {
            match read_figure_pic(&resolved, figure.width_permille, max_image_width_emu) {
                Ok(pic) => {
                    docx = docx.add_paragraph(figure_image_paragraph(pic));
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
        docx = docx.add_paragraph(figure_placeholder_paragraph(fallback));
    }

    if !figure.caption.is_empty() {
        docx = docx.add_paragraph(caption_paragraph(
            &profile.caption_label_figure,
            figure_number,
            &figure.caption,
        ));
    }

    if !figure.source.is_empty() {
        docx = docx.add_paragraph(source_paragraph(&figure.source, profile));
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

fn figure_image_paragraph(pic: Pic) -> Paragraph {
    Paragraph::new()
        .style("BodyText")
        .align(AlignmentType::Center)
        .line_spacing(single_spacing())
        .indent(Some(0), None, None, None)
        .add_run(Run::new().add_image(pic))
}

fn figure_placeholder_paragraph(path_hint: &str) -> Paragraph {
    Paragraph::new()
        .style("BodyText")
        .align(AlignmentType::Center)
        .line_spacing(single_spacing())
        .indent(Some(0), None, None, None)
        .add_run(
            Run::new()
                .add_text(format!("[Figure image not embedded: {path_hint}]"))
                .italic(),
        )
}

fn resolve_figure_path(raw: &str, base_dir: Option<&Path>) -> Option<PathBuf> {
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
        for asset_dir in ["images", "figures", "img"] {
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

/// Convert a [`Block::Section`] or [`Block::Paragraph`] into a docx-rs [`Paragraph`].
fn build_paragraph(block: &Block, profile: &RenderProfile) -> Paragraph {
    match block {
        Block::Section {
            level,
            number,
            title,
            ..
        } => {
            let style = heading_style(*level);
            let mut para = Paragraph::new()
                .style(style)
                .align(AlignmentType::Both)
                .line_spacing(line_spacing(profile.body_line_spacing_twips))
                .indent(Some(HEADING_LEFT_INDENT_TWIPS), None, None, None);
            let section_title = if *level == 1 && profile.heading_uppercase {
                uppercase_inlines(title)
            } else {
                title.to_vec()
            };
            if let Some(number) = number {
                let mut prefix = if *level == 1 && !profile.chapter_name.is_empty() {
                    let chap_name = if profile.heading_uppercase {
                        profile.chapter_name.to_uppercase()
                    } else {
                        profile.chapter_name.clone()
                    };
                    format!("{} {}", chap_name, number.trim())
                } else {
                    format!("{}.", number.trim_end_matches('.'))
                };
                if !title.is_empty() {
                    prefix.push(' ');
                }
                para = para.add_run(Run::new().add_text(prefix).bold());
            }
            for run in inline_runs(&section_title, true, false) {
                para = para.add_run(run);
            }
            para
        }
        Block::Paragraph(inlines) => {
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
            for run in inline_runs(inlines, false, false) {
                para = para.add_run(run);
            }
            para
        }
        // Table, Figure, List, DisplayMath are handled separately — unreachable here.
        Block::Table(_) | Block::Figure(_) | Block::List(_) | Block::DisplayMath(_) => {
            unreachable!()
        }
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

/// Recursively convert a slice of [`Inline`] nodes into a flat list of
/// docx-rs [`Run`]s, inheriting bold/italic state from parent nodes.
///
/// `footnote_hp` controls the font size applied inside footnote paragraphs.
fn inline_runs(inlines: &[Inline], bold: bool, italic: bool) -> Vec<Run> {
    inline_runs_with_footnote_size(inlines, bold, italic, FONT_SIZE_FOOTNOTE_HP)
}

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
    use crate::model::Inline;

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

        let resolved = resolve_figure_path("part2/chart", Some(&root));
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
        let resolved = resolve_figure_path(&absolute, None);
        assert_eq!(resolved, Some(image_path.clone()));

        let _ = std::fs::remove_dir_all(&root);
    }
}
