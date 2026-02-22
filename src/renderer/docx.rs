use std::{fs::File, path::Path};

use docx_rs::{
    AbstractNumbering, AlignmentType, Docx, IndentLevel, Level, LevelJc, LevelText, NumberFormat,
    Numbering, NumberingId, Paragraph, Run, Start, Table as DocxTable, TableCell as DocxCell,
    TableRow as DocxRow,
};

use crate::model::{Block, Document, Figure, Inline, Table};

/// Render the intermediate [`Document`] AST to a `.docx` file at `output_path`.
///
/// DOCX structure rules (see AGENTS.md):
/// - All body paragraphs use justify alignment (`AlignmentType::Both`).
/// - Section headings use the built-in `Heading1` / `Heading2` / `Heading3` styles.
/// - Bold runs use `Run::bold()`, italic runs use `Run::italic()`.
/// - No `<w:sectPr>` is inserted inside paragraph properties.
pub fn render_docx(document: &Document, output_path: &Path) -> anyhow::Result<()> {
    let mut docx = Docx::new();

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
        }
    }

    let file = File::create(output_path)?;
    docx.build().pack(file)?;
    Ok(())
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
    let mut para = Paragraph::new().numbering(NumberingId::new(num_id), IndentLevel::new(0));
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
                    let mut para = Paragraph::new().align(AlignmentType::Both);
                    for run in inline_runs(&cell.content, false, false) {
                        para = para.add_run(run);
                    }
                    DocxCell::new().add_paragraph(para)
                })
                .collect();
            DocxRow::new(cells)
        })
        .collect();
    DocxTable::new(rows)
}

/// A centred italic paragraph used for captions.
fn caption_paragraph(inlines: &[Inline]) -> Paragraph {
    let mut para = Paragraph::new().align(AlignmentType::Center);
    for run in inline_runs(inlines, false, true) {
        para = para.add_run(run);
    }
    para
}

/// A centred plain paragraph used for source attributions.
fn source_paragraph(inlines: &[Inline]) -> Paragraph {
    let mut para = Paragraph::new().align(AlignmentType::Center);
    for run in inline_runs(inlines, false, false) {
        para = para.add_run(run);
    }
    para
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
            let mut para = Paragraph::new().style(style);
            for run in inline_runs(title, false, false) {
                para = para.add_run(run);
            }
            para
        }
        Block::Paragraph(inlines) => {
            let mut para = Paragraph::new().align(AlignmentType::Both);
            for run in inline_runs(inlines, false, false) {
                para = para.add_run(run);
            }
            para
        }
        // Table, Figure, List are handled separately in render_docx — unreachable here.
        Block::Table(_) | Block::Figure(_) | Block::List(_) => unreachable!(),
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
        }
    }
    runs
}
