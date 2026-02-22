use std::{fs::File, path::Path};

use docx_rs::{AlignmentType, Docx, Paragraph, Run};

use crate::model::{Block, Document, Inline};

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
        let para = build_paragraph(block);
        docx = docx.add_paragraph(para);
    }

    let file = File::create(output_path)?;
    docx.build().pack(file)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Convert a single [`Block`] into a docx-rs [`Paragraph`].
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
