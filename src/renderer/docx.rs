use std::{fs::File, path::Path};

use docx_rs::{
    AbstractNumbering, AlignmentType, Docx, Footnote, IndentLevel, Level, LevelJc, LevelText,
    LineSpacing, LineSpacingType, NumberFormat, Numbering, NumberingId, PageMargin, Paragraph, Run,
    RunFonts, SpecialIndentType, Start, Style, StyleType, Table as DocxTable,
    TableCell as DocxCell, TableRow as DocxRow,
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

/// Render the intermediate [`Document`] AST to a `.docx` file at `output_path`.
///
/// DOCX structure rules (see AGENTS.md):
/// - Document defaults are configured for GOST-like typography (A4, margins, TNR 14pt, 1.5x spacing).
/// - Body paragraphs use the `Normal` style with justify alignment and first-line indent.
/// - Section headings use `Heading1` / `Heading2` / `Heading3` styles.
/// - Bold runs use `Run::bold()`, italic runs use `Run::italic()`.
/// - No `<w:sectPr>` is inserted inside paragraph properties.
pub fn render_docx(document: &Document, output_path: &Path) -> anyhow::Result<()> {
    let mut docx = create_styled_docx();

    // Assign a stable numbering ID for each list block we encounter.
    // abstractNumId == numId for simplicity (one-to-one mapping).
    let mut next_num_id: usize = 1;

    for block in &document.blocks {
        match block {
            Block::Section { .. } | Block::Paragraph(_) => {
                let para = build_paragraph(block);
                docx = docx.add_paragraph(para);
            }
            Block::Table(t) => {
                if !t.caption.is_empty() {
                    docx = docx.add_paragraph(caption_paragraph(&t.caption));
                }
                docx = docx.add_table(build_table(t));
                if !t.source.is_empty() {
                    docx = docx.add_paragraph(source_paragraph(&t.source));
                }
            }
            Block::Figure(f) => {
                docx = docx.add_paragraph(build_figure_paragraph(f));
                if !f.source.is_empty() {
                    docx = docx.add_paragraph(source_paragraph(&f.source));
                }
            }
            Block::List(list) => {
                let num_id = next_num_id;
                next_num_id += 1;
                docx = register_numbering(docx, num_id, list.ordered);
                for item_inlines in &list.items {
                    let para = build_list_item(item_inlines, num_id);
                    docx = docx.add_paragraph(para);
                }
            }
            Block::DisplayMath(src) => {
                docx = docx.add_paragraph(build_display_math_paragraph(src));
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

/// A centred plain paragraph used for source attributions.
fn source_paragraph(inlines: &[Inline]) -> Paragraph {
    let mut para = Paragraph::new()
        .style("Normal")
        .align(AlignmentType::Center)
        .line_spacing(single_spacing())
        .indent(Some(0), None, None, None);
    for run in inline_runs(inlines, false, false) {
        para = para.add_run(run);
    }
    para
}

/// A centred italic paragraph for display-math blocks.
fn build_display_math_paragraph(src: &str) -> Paragraph {
    Paragraph::new()
        .style("Normal")
        .align(AlignmentType::Center)
        .line_spacing(single_spacing())
        .indent(Some(0), None, None, None)
        .add_run(Run::new().add_text(src).italic())
}

// ---------------------------------------------------------------------------
// Figure rendering
// ---------------------------------------------------------------------------

/// For v0.2+, figures are rendered as text-only blocks (caption + source).
/// Image embedding is deferred to v1.0.
fn build_figure_paragraph(figure: &Figure) -> Paragraph {
    if !figure.caption.is_empty() {
        caption_paragraph(&figure.caption)
    } else {
        Paragraph::new()
    }
}

// ---------------------------------------------------------------------------
// Paragraph / section rendering
// ---------------------------------------------------------------------------

/// Convert a [`Block::Section`] or [`Block::Paragraph`] into a docx-rs [`Paragraph`].
fn build_paragraph(block: &Block) -> Paragraph {
    match block {
        Block::Section { level, title } => {
            let style = heading_style(*level);
            let mut para = Paragraph::new()
                .style(style)
                .align(AlignmentType::Left)
                .line_spacing(single_spacing())
                .indent(Some(0), None, None, None);
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
