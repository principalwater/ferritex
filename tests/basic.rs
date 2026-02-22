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
