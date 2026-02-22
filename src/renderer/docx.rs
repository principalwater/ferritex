use std::{fs::File, path::Path};

use docx_rs::{
    AlignmentType, Docx, Paragraph, Run, Table as DocxTable, TableCell as DocxCell,
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

    for block in &document.blocks {
        match block {
            Block::Section { .. } | Block::Paragraph(_) => {
                let para = build_paragraph(block);
                docx = docx.add_paragraph(para);
            }
            Block::Table(t) => {
                // Optional caption paragraph before the table.
                if !t.caption.is_empty() {
                    docx = docx.add_paragraph(caption_paragraph(&t.caption));
                }
                docx = docx.add_table(build_table(t));
                // Optional source line after the table.
                if !t.source.is_empty() {
                    docx = docx.add_paragraph(source_paragraph(&t.source));
                }
            }
            Block::Figure(f) => {
                docx = docx.add_paragraph(build_figure_paragraph(f));
            }
        }
    }

    let file = File::create(output_path)?;
    docx.build().pack(file)?;
    Ok(())
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

/// For v0.2, figures are rendered as text-only blocks:
/// - caption (italic, centred) when present
/// - source line when present
///
/// Image embedding is deferred to v1.0.
fn build_figure_paragraph(figure: &Figure) -> Paragraph {
    if !figure.caption.is_empty() {
        caption_paragraph(&figure.caption)
    } else {
        // No caption — emit an empty paragraph as a placeholder.
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
        // Table and Figure are handled separately in render_docx — unreachable here.
        Block::Table(_) | Block::Figure(_) => unreachable!(),
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
