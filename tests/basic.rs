use std::io::Read;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn tmp_output(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("ferritex_test_{name}"))
}

/// Parse the fixture, render to DOCX, then verify the output is a valid ZIP
/// that contains `word/document.xml` with at least one `<w:p` element.
#[test]
fn test_simple_docx_is_valid_zip_with_paragraphs() {
    let input = fixture("simple.tex");
    let output = tmp_output("simple.docx");

    // Parse
    let source = std::fs::read_to_string(&input)
        .unwrap_or_else(|e| panic!("cannot read fixture {input:?}: {e}"));
    let document = ferritex::parser::latex::parse_latex(&source);
    assert!(
        !document.blocks.is_empty(),
        "parser produced zero blocks from simple.tex"
    );

    // Render
    ferritex::renderer::docx::render_docx(&document, &output)
        .unwrap_or_else(|e| panic!("render_docx failed: {e}"));
    assert!(output.exists(), "output file was not created");

    // Validate: must be a readable ZIP
    let file =
        std::fs::File::open(&output).unwrap_or_else(|e| panic!("cannot open output DOCX: {e}"));
    let mut zip =
        zip::ZipArchive::new(file).unwrap_or_else(|e| panic!("output is not a valid ZIP: {e}"));

    // Must contain word/document.xml
    let mut doc_xml = zip
        .by_name("word/document.xml")
        .unwrap_or_else(|e| panic!("word/document.xml missing from DOCX: {e}"));

    let mut xml_content = String::new();
    doc_xml
        .read_to_string(&mut xml_content)
        .unwrap_or_else(|e| panic!("cannot read word/document.xml: {e}"));

    // Must contain at least one paragraph element
    assert!(
        xml_content.contains("<w:p ") || xml_content.contains("<w:p>"),
        "word/document.xml contains no <w:p> elements"
    );

    // Clean up
    let _ = std::fs::remove_file(&output);
}

/// Table fixture: must produce a DOCX with at least one `<w:tbl` element and
/// the parser must find the table block with rows and a caption.
#[test]
fn test_table_docx_contains_tbl_element() {
    use ferritex::model::Block;

    let input = fixture("with_table.tex");
    let output = tmp_output("with_table.docx");

    let source = std::fs::read_to_string(&input)
        .unwrap_or_else(|e| panic!("cannot read fixture {input:?}: {e}"));
    let document = ferritex::parser::latex::parse_latex(&source);

    // AST must contain at least one Table block.
    let table_blocks: Vec<_> = document
        .blocks
        .iter()
        .filter(|b| matches!(b, Block::Table(_)))
        .collect();
    assert!(!table_blocks.is_empty(), "no Table block found in AST");

    // The table must have a caption and at least one row.
    if let Block::Table(t) = table_blocks[0] {
        assert!(!t.caption.is_empty(), "table caption is missing");
        assert!(!t.rows.is_empty(), "table has no rows");
        assert!(!t.source.is_empty(), "table source line is missing");
    }

    // Render and validate DOCX.
    ferritex::renderer::docx::render_docx(&document, &output)
        .unwrap_or_else(|e| panic!("render_docx failed: {e}"));

    let file =
        std::fs::File::open(&output).unwrap_or_else(|e| panic!("cannot open output DOCX: {e}"));
    let mut zip =
        zip::ZipArchive::new(file).unwrap_or_else(|e| panic!("output is not a valid ZIP: {e}"));
    let mut doc_xml = zip
        .by_name("word/document.xml")
        .unwrap_or_else(|e| panic!("word/document.xml missing: {e}"));
    let mut xml_content = String::new();
    doc_xml
        .read_to_string(&mut xml_content)
        .unwrap_or_else(|e| panic!("cannot read word/document.xml: {e}"));

    assert!(
        xml_content.contains("<w:tbl"),
        "DOCX does not contain a <w:tbl> element — table was not rendered"
    );

    let _ = std::fs::remove_file(&output);
}

/// Figure fixture: must produce a valid DOCX; the AST must contain a Figure
/// block with a non-empty caption, image path, and source.
#[test]
fn test_figure_docx_is_valid() {
    use ferritex::model::Block;

    let input = fixture("with_figure.tex");
    let output = tmp_output("with_figure.docx");

    let source = std::fs::read_to_string(&input)
        .unwrap_or_else(|e| panic!("cannot read fixture {input:?}: {e}"));
    let document = ferritex::parser::latex::parse_latex(&source);

    // AST must contain at least one Figure block.
    let figure_blocks: Vec<_> = document
        .blocks
        .iter()
        .filter(|b| matches!(b, Block::Figure(_)))
        .collect();
    assert!(!figure_blocks.is_empty(), "no Figure block found in AST");

    if let Block::Figure(f) = figure_blocks[0] {
        assert_eq!(
            f.image_path.as_deref(),
            Some("images/electricity_prices.png"),
            "image path mismatch"
        );
        assert!(!f.caption.is_empty(), "figure caption is missing");
        assert!(!f.source.is_empty(), "figuresource is missing");
    }

    // Render and validate DOCX.
    ferritex::renderer::docx::render_docx(&document, &output)
        .unwrap_or_else(|e| panic!("render_docx failed: {e}"));

    let file =
        std::fs::File::open(&output).unwrap_or_else(|e| panic!("cannot open output DOCX: {e}"));
    zip::ZipArchive::new(file).unwrap_or_else(|e| panic!("output is not a valid ZIP: {e}"));

    let _ = std::fs::remove_file(&output);
}

/// List fixture: both itemize and enumerate must produce `<w:numId>` references
/// in word/document.xml and the AST must contain two List blocks.
#[test]
fn test_lists_docx_contains_numbering() {
    use ferritex::model::Block;

    let input = fixture("with_lists.tex");
    let output = tmp_output("with_lists.docx");

    let source = std::fs::read_to_string(&input)
        .unwrap_or_else(|e| panic!("cannot read fixture {input:?}: {e}"));
    let document = ferritex::parser::latex::parse_latex(&source);

    let list_blocks: Vec<_> = document
        .blocks
        .iter()
        .filter(|b| matches!(b, Block::List(_)))
        .collect();
    assert_eq!(list_blocks.len(), 2, "expected 2 list blocks");

    if let Block::List(l) = list_blocks[0] {
        assert!(!l.ordered, "first list should be unordered (itemize)");
        assert_eq!(l.items.len(), 4, "itemize should have 4 items");
    }
    if let Block::List(l) = list_blocks[1] {
        assert!(l.ordered, "second list should be ordered (enumerate)");
        assert_eq!(l.items.len(), 3, "enumerate should have 3 items");
    }

    ferritex::renderer::docx::render_docx(&document, &output)
        .unwrap_or_else(|e| panic!("render_docx failed: {e}"));

    let file =
        std::fs::File::open(&output).unwrap_or_else(|e| panic!("cannot open output DOCX: {e}"));
    let mut zip =
        zip::ZipArchive::new(file).unwrap_or_else(|e| panic!("output is not a valid ZIP: {e}"));
    let mut doc_xml = zip
        .by_name("word/document.xml")
        .unwrap_or_else(|e| panic!("word/document.xml missing: {e}"));
    let mut xml_content = String::new();
    doc_xml
        .read_to_string(&mut xml_content)
        .unwrap_or_else(|e| panic!("cannot read word/document.xml: {e}"));

    assert!(
        xml_content.contains("w:numId"),
        "DOCX does not contain numbering references — lists were not rendered"
    );

    let _ = std::fs::remove_file(&output);
}

/// Math fixture: inline `$...$` must produce paragraph runs; display equation
/// must render as a centered paragraph in DOCX.
#[test]
fn test_math_docx_is_valid() {
    use ferritex::model::{Block, Inline};

    let input = fixture("with_math.tex");
    let output = tmp_output("with_math.docx");

    let source = std::fs::read_to_string(&input)
        .unwrap_or_else(|e| panic!("cannot read fixture {input:?}: {e}"));
    let document = ferritex::parser::latex::parse_latex(&source);

    let display_math_blocks: Vec<_> = document
        .blocks
        .iter()
        .filter_map(|b| {
            if let Block::DisplayMath(src) = b {
                Some(src)
            } else {
                None
            }
        })
        .collect();
    assert!(
        display_math_blocks.len() >= 2,
        "expected at least 2 DisplayMath blocks (equation + \\[...\\])"
    );
    assert!(
        display_math_blocks
            .iter()
            .any(|src| src.contains("\\max_{x_{ij}} W = \\sum_{i=1}^{n} x_i")),
        "equation-style display math was not parsed"
    );
    assert!(
        display_math_blocks
            .iter()
            .any(|src| src.contains("\\min_{y_j} C = \\sum_{j=1}^{m} y_j")),
        "\\[...\\]-style display math was not parsed"
    );

    let has_inline_math = document.blocks.iter().any(|b| {
        if let Block::Paragraph(inlines) = b {
            inlines.iter().any(|i| matches!(i, Inline::InlineMath(_)))
        } else {
            false
        }
    });
    assert!(has_inline_math, "no InlineMath inline found");

    ferritex::renderer::docx::render_docx(&document, &output)
        .unwrap_or_else(|e| panic!("render_docx failed: {e}"));

    let file =
        std::fs::File::open(&output).unwrap_or_else(|e| panic!("cannot open output DOCX: {e}"));
    let mut zip =
        zip::ZipArchive::new(file).unwrap_or_else(|e| panic!("output is not a valid ZIP: {e}"));

    let mut doc_xml = zip
        .by_name("word/document.xml")
        .unwrap_or_else(|e| panic!("word/document.xml missing from DOCX: {e}"));
    let mut xml_content = String::new();
    doc_xml
        .read_to_string(&mut xml_content)
        .unwrap_or_else(|e| panic!("cannot read word/document.xml: {e}"));

    assert!(
        xml_content.contains("W = \\sum_{i=1}^{n} x_i") || xml_content.contains("W = \\sum"),
        "display math body not found in document.xml"
    );
    assert!(
        xml_content.contains("C = \\sum_{j=1}^{m} y_j") || xml_content.contains("\\min_{y_j}"),
        "\\[...\\] display math body not found in document.xml"
    );
    assert!(
        xml_content.contains("w:jc w:val=\"center\""),
        "display math paragraph is not centered"
    );

    let _ = std::fs::remove_file(&output);
}

/// Footnote fixture: parser must emit footnote inlines and renderer must create
/// a DOCX with `word/footnotes.xml` and footnote references in `document.xml`.
#[test]
fn test_footnotes_docx_contains_footnotes_part() {
    use ferritex::model::{Block, Inline};

    let input = fixture("with_footnotes.tex");
    let output = tmp_output("with_footnotes.docx");

    let source = std::fs::read_to_string(&input)
        .unwrap_or_else(|e| panic!("cannot read fixture {input:?}: {e}"));
    let document = ferritex::parser::latex::parse_latex(&source);

    let footnote_payloads: Vec<_> = document
        .blocks
        .iter()
        .flat_map(|b| {
            if let Block::Paragraph(inlines) = b {
                inlines
                    .iter()
                    .filter_map(|i| {
                        if let Inline::Footnote(content) = i {
                            Some(content)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        })
        .collect();

    assert!(
        footnote_payloads.len() >= 3,
        "expected at least 3 footnotes in AST (2 explicit + 1 autocite)"
    );
    assert!(
        footnote_payloads
            .iter()
            .any(|content| content.iter().any(|i| matches!(i, Inline::Italic(_)))),
        "expected inline formatting inside footnote payload"
    );

    ferritex::renderer::docx::render_docx(&document, &output)
        .unwrap_or_else(|e| panic!("render_docx failed: {e}"));

    let file =
        std::fs::File::open(&output).unwrap_or_else(|e| panic!("cannot open output DOCX: {e}"));
    let mut zip =
        zip::ZipArchive::new(file).unwrap_or_else(|e| panic!("output is not a valid ZIP: {e}"));

    let mut doc_xml_content = String::new();
    {
        let mut doc_xml = zip
            .by_name("word/document.xml")
            .unwrap_or_else(|e| panic!("word/document.xml missing from DOCX: {e}"));
        doc_xml
            .read_to_string(&mut doc_xml_content)
            .unwrap_or_else(|e| panic!("cannot read word/document.xml: {e}"));
    }
    assert!(
        doc_xml_content.contains("w:footnoteReference"),
        "document.xml does not contain footnote references"
    );

    let mut footnotes_xml_content = String::new();
    {
        let mut footnotes_xml = zip
            .by_name("word/footnotes.xml")
            .unwrap_or_else(|e| panic!("word/footnotes.xml missing from DOCX: {e}"));
        footnotes_xml
            .read_to_string(&mut footnotes_xml_content)
            .unwrap_or_else(|e| panic!("cannot read word/footnotes.xml: {e}"));
    }
    assert!(
        footnotes_xml_content.contains("<w:footnote "),
        "footnotes.xml does not contain footnote elements"
    );
    assert!(
        footnotes_xml_content.contains("Synthetic note")
            || footnotes_xml_content.contains("Second note")
            || footnotes_xml_content.contains("DemoAutocite2026"),
        "footnote text not found in footnotes.xml"
    );

    let _ = std::fs::remove_file(&output);
}
