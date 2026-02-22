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
