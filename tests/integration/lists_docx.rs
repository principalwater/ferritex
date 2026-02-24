use crate::common::{parse_fixture, read_docx_entry, render_document_to_temp_docx};
use ferritex::model::Block;

#[test]
fn lists_docx_contains_numbering_references() {
    let document = parse_fixture("with_lists.tex");

    let list_blocks: Vec<_> = document
        .blocks
        .iter()
        .filter(|block| matches!(block, Block::List(_)))
        .collect();
    assert_eq!(list_blocks.len(), 2, "expected 2 list blocks");

    let Block::List(first) = list_blocks[0] else {
        panic!("expected first list block")
    };
    assert!(!first.ordered, "first list should be unordered (itemize)");
    assert_eq!(first.items.len(), 4, "itemize should have 4 items");

    let Block::List(second) = list_blocks[1] else {
        panic!("expected second list block")
    };
    assert!(second.ordered, "second list should be ordered (enumerate)");
    assert_eq!(second.items.len(), 3, "enumerate should have 3 items");

    let output = render_document_to_temp_docx(&document, "lists");
    let xml = read_docx_entry(output.path(), "word/document.xml");

    assert!(
        xml.contains("w:numId"),
        "DOCX does not contain numbering references"
    );
}
