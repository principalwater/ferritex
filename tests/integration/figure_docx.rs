use crate::common::{
    expected_fixture, parse_fixture, read_docx_entry, render_document_to_temp_docx,
};
use ferritex::model::Block;

#[test]
fn figure_docx_contains_expected_figure_data() {
    let expected_image_path = expected_fixture("figure_image_path.txt");
    let expected_image_path = expected_image_path.trim();

    let document = parse_fixture("with_figure.tex");

    let figure_blocks: Vec<_> = document
        .blocks
        .iter()
        .filter(|block| matches!(block, Block::Figure(_)))
        .collect();
    assert!(!figure_blocks.is_empty(), "no Figure block found in AST");

    let Block::Figure(figure) = figure_blocks[0] else {
        panic!("expected first figure block")
    };
    assert_eq!(
        figure.image_path.as_deref(),
        Some(expected_image_path),
        "image path mismatch"
    );
    assert!(!figure.caption.is_empty(), "figure caption is missing");
    assert!(!figure.source.is_empty(), "figure source line is missing");

    let output = render_document_to_temp_docx(&document, "figure");
    let xml = read_docx_entry(output.path(), "word/document.xml");

    assert!(
        xml.contains("Wholesale electricity price dynamics")
            || xml.contains("Source: synthetic data"),
        "figure content was not found in document.xml"
    );
}
