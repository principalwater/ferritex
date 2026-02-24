use crate::common::{
    expected_fixture_lines, parse_fixture, read_docx_entry, render_document_to_temp_docx,
};
use ferritex::model::{Block, Inline};

#[test]
fn footnotes_docx_contains_references_and_part() {
    let expected_keywords = expected_fixture_lines("footnote_keywords.txt");

    let document = parse_fixture("with_footnotes.tex");

    let footnote_payloads: Vec<_> = document
        .blocks
        .iter()
        .flat_map(|block| {
            if let Block::Paragraph(inlines) = block {
                inlines
                    .iter()
                    .filter_map(|inline| {
                        if let Inline::Footnote(content) = inline {
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
        "expected at least 3 footnotes in AST"
    );
    assert!(
        footnote_payloads.iter().any(|content| content
            .iter()
            .any(|inline| matches!(inline, Inline::Italic(_)))),
        "expected inline formatting inside footnote payload"
    );

    let output = render_document_to_temp_docx(&document, "footnotes");
    let document_xml = read_docx_entry(output.path(), "word/document.xml");
    let footnotes_xml = read_docx_entry(output.path(), "word/footnotes.xml");

    assert!(
        document_xml.contains("w:footnoteReference"),
        "document.xml does not contain footnote references"
    );
    assert!(
        footnotes_xml.contains("<w:footnote "),
        "footnotes.xml does not contain footnote elements"
    );

    let has_any_expected = expected_keywords
        .iter()
        .any(|keyword| footnotes_xml.contains(keyword));
    assert!(
        has_any_expected,
        "none of expected footnote keywords were found in footnotes.xml"
    );
}
