use crate::common::{parse_fixture, read_docx_entry, render_document_to_temp_docx};

#[test]
fn simple_docx_is_valid_zip_with_paragraphs() {
    let document = parse_fixture("simple.tex");
    assert!(
        !document.blocks.is_empty(),
        "parser produced zero blocks from simple.tex"
    );

    let output = render_document_to_temp_docx(&document, "simple");
    let xml = read_docx_entry(output.path(), "word/document.xml");

    assert!(
        xml.contains("<w:p ") || xml.contains("<w:p>"),
        "word/document.xml contains no <w:p> elements"
    );
}
