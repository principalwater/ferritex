use std::{
    fs::File,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
};

use anyhow::{Context, anyhow};
use docx_rs::{
    AbstractNumbering, AlignmentType, BreakType, Docx, Footnote, IndentLevel, Level, LevelJc,
    LevelText, LineSpacing, LineSpacingType, NumberFormat, Numbering, NumberingId, PageMargin,
    Paragraph, Pic, Run, RunFonts, SpecialIndentType, Start, Style, StyleType, Table as DocxTable,
    TableCell as DocxCell, TableRow as DocxRow, VertAlignType,
};

use crate::model::{Block, Document, Figure, Inline, Table};

const PAGE_A4_WIDTH_TWIPS: u32 = 11_906;
const PAGE_A4_HEIGHT_TWIPS: u32 = 16_838;

const PAGE_MARGIN_TOP_TWIPS: i32 = 1_134;
const PAGE_MARGIN_BOTTOM_TWIPS: i32 = 1_134;
const PAGE_MARGIN_LEFT_TWIPS: i32 = 1_417;
const PAGE_MARGIN_RIGHT_TWIPS: i32 = 567;
const PAGE_MARGIN_HEADER_TWIPS: i32 = 720;
const PAGE_MARGIN_FOOTER_TWIPS: i32 = 720;

const FONT_SIZE_BODY_HP: usize = 28;
const FONT_SIZE_TABLE_HP: usize = 24;
const FONT_SIZE_FOOTNOTE_HP: usize = 20;

const LINE_SPACING_SINGLE_TWIPS: i32 = 240;
const LINE_SPACING_ONE_AND_HALF_TWIPS: i32 = 360;

const FIRST_LINE_INDENT_TWIPS: i32 = 709;
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "bmp", "tif", "tiff", "gif", "webp"];
const EMU_PER_TWIP: u32 = 635;
const TEXT_WIDTH_TWIPS: u32 =
    PAGE_A4_WIDTH_TWIPS - PAGE_MARGIN_LEFT_TWIPS as u32 - PAGE_MARGIN_RIGHT_TWIPS as u32;
const IMAGE_SAFE_SCALE_NUM: u32 = 92;
const IMAGE_SAFE_SCALE_DEN: u32 = 100;
const MAX_IMAGE_WIDTH_EMU: u32 =
    TEXT_WIDTH_TWIPS * EMU_PER_TWIP * IMAGE_SAFE_SCALE_NUM / IMAGE_SAFE_SCALE_DEN;

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
    let mut docx = create_styled_docx();
    let figure_base_dir = input_tex_path.and_then(Path::parent);

    // Assign a stable numbering ID for each list block we encounter.
    // abstractNumId == numId for simplicity (one-to-one mapping).
    let mut next_num_id: usize = 1;
    let mut rendered_any_block = false;

    for block in &document.blocks {
        match block {
            Block::Section { level, .. } => {
                if *level == 1 && rendered_any_block {
                    docx = docx.add_paragraph(
                        Paragraph::new().add_run(Run::new().add_break(BreakType::Page)),
                    );
                }
                let para = build_paragraph(block);
                docx = docx.add_paragraph(para);
                rendered_any_block = true;
            }
            Block::Paragraph(_) => {
                let para = build_paragraph(block);
                docx = docx.add_paragraph(para);
                rendered_any_block = true;
            }
            Block::Table(t) => {
                if !t.caption.is_empty() {
                    docx = docx.add_paragraph(caption_paragraph(&t.caption));
                }
                docx = docx.add_table(build_table(t));
                if !t.source.is_empty() {
                    docx = docx.add_paragraph(source_paragraph(&t.source));
                }
                rendered_any_block = true;
            }
            Block::Figure(f) => {
                docx = render_figure_block(docx, f, figure_base_dir);
                rendered_any_block = true;
            }
            Block::List(list) => {
                let num_id = next_num_id;
                next_num_id += 1;
                docx = register_numbering(docx, num_id, list.ordered);
                for item_inlines in &list.items {
                    let para = build_list_item(item_inlines, num_id);
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
    Ok(())
}

fn create_styled_docx() -> Docx {
    let page_margin = PageMargin::new()
        .top(PAGE_MARGIN_TOP_TWIPS)
        .bottom(PAGE_MARGIN_BOTTOM_TWIPS)
        .left(PAGE_MARGIN_LEFT_TWIPS)
        .right(PAGE_MARGIN_RIGHT_TWIPS)
        .header(PAGE_MARGIN_HEADER_TWIPS)
        .footer(PAGE_MARGIN_FOOTER_TWIPS)
        .gutter(0);
    let fonts = times_new_roman();

    let mut docx = Docx::new()
        .page_size(PAGE_A4_WIDTH_TWIPS, PAGE_A4_HEIGHT_TWIPS)
        .page_margin(page_margin)
        .default_size(FONT_SIZE_BODY_HP)
        .default_fonts(fonts.clone())
        .default_line_spacing(one_and_half_spacing());

    for style in gost_styles(fonts) {
        docx = docx.add_style(style);
    }

    docx
}

fn gost_styles(fonts: RunFonts) -> Vec<Style> {
    let mut footnote_reference = Style::new("FootnoteReference", StyleType::Character)
        .name("Footnote Reference")
        .based_on("DefaultParagraphFont")
        .fonts(fonts.clone())
        .size(FONT_SIZE_FOOTNOTE_HP);
    footnote_reference.run_property = footnote_reference
        .run_property
        .vert_align(VertAlignType::SuperScript);

    // docx-rs always emits a minimal "Normal" style by default.
    // We still append an explicit GOST-tuned "Normal" definition and inherit from it.
    vec![
        Style::new("Normal", StyleType::Paragraph)
            .name("Normal")
            .fonts(fonts.clone())
            .size(FONT_SIZE_BODY_HP)
            .align(AlignmentType::Both)
            .line_spacing(one_and_half_spacing())
            .indent(
                Some(0),
                Some(SpecialIndentType::FirstLine(FIRST_LINE_INDENT_TWIPS)),
                None,
                None,
            ),
        heading_style_definition("Heading1", "Heading 1", 0, fonts.clone()),
        heading_style_definition("Heading2", "Heading 2", 1, fonts.clone()),
        heading_style_definition("Heading3", "Heading 3", 2, fonts.clone()),
        Style::new("Caption", StyleType::Paragraph)
            .name("Caption")
            .based_on("Normal")
            .next("Normal")
            .fonts(fonts.clone())
            .size(FONT_SIZE_BODY_HP)
            .bold()
            .align(AlignmentType::Center)
            .line_spacing(single_spacing())
            .indent(Some(0), None, None, None),
        Style::new("FootnoteText", StyleType::Paragraph)
            .name("Footnote Text")
            .based_on("Normal")
            .next("FootnoteText")
            .fonts(fonts.clone())
            .size(FONT_SIZE_FOOTNOTE_HP)
            .align(AlignmentType::Both)
            .line_spacing(single_spacing())
            .indent(
                Some(0),
                Some(SpecialIndentType::FirstLine(FIRST_LINE_INDENT_TWIPS)),
                None,
                None,
            ),
        Style::new("ListParagraph", StyleType::Paragraph)
            .name("List Paragraph")
            .based_on("Normal")
            .next("Normal")
            .fonts(fonts)
            .size(FONT_SIZE_BODY_HP)
            .align(AlignmentType::Both)
            .line_spacing(one_and_half_spacing())
            .indent(Some(0), None, None, None),
        footnote_reference,
    ]
}

fn heading_style_definition(
    style_id: &str,
    style_name: &str,
    outline_level: usize,
    fonts: RunFonts,
) -> Style {
    Style::new(style_id, StyleType::Paragraph)
        .name(style_name)
        .based_on("Normal")
        .next("Normal")
        .fonts(fonts)
        .size(FONT_SIZE_BODY_HP)
        .bold()
        .align(AlignmentType::Left)
        .line_spacing(single_spacing())
        .indent(Some(0), None, None, None)
        .outline_lvl(outline_level)
}

fn times_new_roman() -> RunFonts {
    RunFonts::new()
        .ascii("Times New Roman")
        .hi_ansi("Times New Roman")
        .cs("Times New Roman")
}

fn single_spacing() -> LineSpacing {
    LineSpacing::new()
        .line_rule(LineSpacingType::Auto)
        .line(LINE_SPACING_SINGLE_TWIPS)
}

fn one_and_half_spacing() -> LineSpacing {
    LineSpacing::new()
        .line_rule(LineSpacingType::Auto)
        .line(LINE_SPACING_ONE_AND_HALF_TWIPS)
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
    .indent(Some(720), None, Some(360), None);

    let abs_num = AbstractNumbering::new(abs_id).add_level(level);
    let num = Numbering::new(num_id, abs_id);

    docx.add_abstract_numbering(abs_num).add_numbering(num)
}

/// Build a single list-item paragraph with numbering applied.
fn build_list_item(inlines: &[Inline], num_id: usize) -> Paragraph {
    let mut para = Paragraph::new()
        .style("ListParagraph")
        .align(AlignmentType::Both)
        .line_spacing(one_and_half_spacing())
        .indent(Some(0), None, None, None)
        .numbering(NumberingId::new(num_id), IndentLevel::new(0));
    for run in inline_runs(inlines, false, false) {
        para = para.add_run(run);
    }
    para
}

// ---------------------------------------------------------------------------
// Table rendering
// ---------------------------------------------------------------------------

fn build_table(table: &Table) -> DocxTable {
    let rows: Vec<DocxRow> = table
        .rows
        .iter()
        .map(|row| {
            let cells: Vec<DocxCell> = row
                .cells
                .iter()
                .map(|cell| {
                    let mut para = Paragraph::new()
                        .style("Normal")
                        .align(AlignmentType::Both)
                        .line_spacing(single_spacing())
                        .indent(Some(0), None, None, None);
                    for run in inline_runs(&cell.content, false, false) {
                        para = para.add_run(run.size(FONT_SIZE_TABLE_HP));
                    }
                    DocxCell::new().add_paragraph(para)
                })
                .collect();
            DocxRow::new(cells)
        })
        .collect();
    DocxTable::new(rows)
}

/// A centred bold paragraph used for captions.
fn caption_paragraph(inlines: &[Inline]) -> Paragraph {
    let mut para = Paragraph::new()
        .style("Caption")
        .align(AlignmentType::Center)
        .line_spacing(single_spacing())
        .indent(Some(0), None, None, None);
    for run in inline_runs(inlines, true, false) {
        para = para.add_run(run);
    }
    para
}

/// A left-aligned plain paragraph used for source attributions.
fn source_paragraph(inlines: &[Inline]) -> Paragraph {
    let mut para = Paragraph::new()
        .style("Normal")
        .align(AlignmentType::Left)
        .line_spacing(single_spacing())
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
        .style("Normal")
        .align(AlignmentType::Center)
        .line_spacing(single_spacing())
        .indent(Some(0), None, None, None)
        .add_run(Run::new().add_text(visible).italic())
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

// ---------------------------------------------------------------------------
// Figure rendering
// ---------------------------------------------------------------------------

fn render_figure_block(mut docx: Docx, figure: &Figure, base_dir: Option<&Path>) -> Docx {
    let mut embedded = false;

    if let Some(raw_path) = figure.image_path.as_deref() {
        if let Some(resolved) = resolve_figure_path(raw_path, base_dir) {
            match read_figure_pic(&resolved, figure.width_permille) {
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
        docx = docx.add_paragraph(caption_paragraph(&figure.caption));
    }

    if !figure.source.is_empty() {
        docx = docx.add_paragraph(source_paragraph(&figure.source));
    }

    docx
}

fn read_figure_pic(path: &Path, width_permille: Option<u16>) -> anyhow::Result<Pic> {
    let image = std::fs::read(path)
        .with_context(|| format!("failed to read image bytes from {}", path.display()))?;

    let pic = catch_unwind(AssertUnwindSafe(|| Pic::new(&image))).map_err(|_| {
        anyhow!(
            "docx-rs failed to decode image {} (unsupported or corrupt image format)",
            path.display()
        )
    })?;

    Ok(scale_pic_to_text_width(pic, width_permille))
}

fn scale_pic_to_text_width(pic: Pic, width_permille: Option<u16>) -> Pic {
    let (width_emu, height_emu) = pic.size;
    if width_emu == 0 {
        return pic;
    }

    let latex_target_emu = width_permille.map(|permille| {
        (TEXT_WIDTH_TWIPS as u64 * EMU_PER_TWIP as u64 * permille as u64 / 1000) as u32
    });
    let target_width_emu = latex_target_emu
        .unwrap_or(MAX_IMAGE_WIDTH_EMU)
        .min(MAX_IMAGE_WIDTH_EMU);

    if width_emu <= target_width_emu {
        return pic;
    }

    let scaled_height =
        ((height_emu as u64) * (target_width_emu as u64) / (width_emu as u64)).max(1) as u32;
    pic.size(target_width_emu, scaled_height)
}

fn figure_image_paragraph(pic: Pic) -> Paragraph {
    Paragraph::new()
        .style("Normal")
        .align(AlignmentType::Center)
        .line_spacing(single_spacing())
        .indent(Some(0), None, None, None)
        .add_run(Run::new().add_image(pic))
}

fn figure_placeholder_paragraph(path_hint: &str) -> Paragraph {
    Paragraph::new()
        .style("Normal")
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
fn build_paragraph(block: &Block) -> Paragraph {
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
                .align(AlignmentType::Left)
                .line_spacing(single_spacing())
                .indent(Some(0), None, None, None);
            if let Some(number) = number {
                let mut prefix = number.clone();
                if !title.is_empty() {
                    prefix.push(' ');
                }
                para = para.add_run(Run::new().add_text(prefix).bold());
            }
            for run in inline_runs(title, true, false) {
                para = para.add_run(run);
            }
            para
        }
        Block::Paragraph(inlines) => {
            let mut para = Paragraph::new()
                .style("Normal")
                .align(AlignmentType::Both)
                .line_spacing(one_and_half_spacing())
                .indent(
                    Some(0),
                    Some(SpecialIndentType::FirstLine(FIRST_LINE_INDENT_TWIPS)),
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
fn inline_runs(inlines: &[Inline], bold: bool, italic: bool) -> Vec<Run> {
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
                runs.extend(inline_runs(children, true, italic));
            }
            Inline::Italic(children) => {
                runs.extend(inline_runs(children, bold, true));
            }
            Inline::InlineMath(src) => {
                // Render inline math as italic text — a plain-text approximation.
                let mut run = Run::new().add_text(src.as_str()).italic();
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
                // Render \footnote{...} as a native DOCX footnote reference + footnotes.xml entry.
                let mut footnote_para = Paragraph::new()
                    .style("FootnoteText")
                    .align(AlignmentType::Both)
                    .line_spacing(single_spacing())
                    .indent(
                        Some(0),
                        Some(SpecialIndentType::FirstLine(FIRST_LINE_INDENT_TWIPS)),
                        None,
                        None,
                    );
                for run in inline_runs(content, false, false) {
                    footnote_para = footnote_para.add_run(run.size(FONT_SIZE_FOOTNOTE_HP));
                }

                let mut footnote = Footnote::new();
                footnote.add_content(footnote_para);
                runs.push(Run::new().add_footnote_reference(footnote));
            }
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

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
