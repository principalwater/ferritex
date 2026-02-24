use crate::common::{
    expected_fixture, parse_fixture, read_docx_entry, render_document_to_temp_docx,
};
use ferritex::model::{Block, Inline};

#[test]
fn math_docx_contains_inline_and_display_math() {
    let expected_primary = expected_fixture("math_display_primary.txt");
    let expected_primary = expected_primary.trim();
    let expected_secondary = expected_fixture("math_display_secondary.txt");
    let expected_secondary = expected_secondary.trim();

    let document = parse_fixture("with_math.tex");

    let display_math_blocks: Vec<_> = document
        .blocks
        .iter()
        .filter_map(|block| {
            if let Block::DisplayMath(src) = block {
                Some(src.as_str())
            } else {
                None
            }
        })
        .collect();
    assert!(
        display_math_blocks.len() >= 2,
        "expected at least 2 DisplayMath blocks"
    );
    assert!(
        display_math_blocks
            .iter()
            .any(|src| src.contains(expected_primary)),
        "primary display math expression was not parsed"
    );
    assert!(
        display_math_blocks
            .iter()
            .any(|src| src.contains(expected_secondary)),
        "secondary display math expression was not parsed"
    );

    let has_inline_math = document.blocks.iter().any(|block| {
        if let Block::Paragraph(inlines) = block {
            inlines
                .iter()
                .any(|inline| matches!(inline, Inline::InlineMath(_)))
        } else {
            false
        }
    });
    assert!(has_inline_math, "no InlineMath inline found");

    let output = render_document_to_temp_docx(&document, "math");
    let xml = read_docx_entry(output.path(), "word/document.xml");

    assert!(
        xml.contains(expected_primary) || xml.contains("W = \\sum"),
        "primary display math body not found in document.xml"
    );
    assert!(
        xml.contains(expected_secondary) || xml.contains("C = \\sum"),
        "secondary display math body not found in document.xml"
    );
    assert!(
        xml.contains("w:jc w:val=\"center\""),
        "display math paragraph is not centered"
    );
}
