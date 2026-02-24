use crate::common::{parse_fixture, read_docx_entry, render_document_to_temp_docx};
use ferritex::model::Block;

#[test]
fn table_docx_contains_table_markup() {
    let document = parse_fixture("with_table.tex");

    let table_blocks: Vec<_> = document
        .blocks
        .iter()
        .filter(|block| matches!(block, Block::Table(_)))
        .collect();
    assert!(!table_blocks.is_empty(), "no Table block found in AST");

    let Block::Table(table) = table_blocks[0] else {
        panic!("expected first table block")
    };
    assert!(!table.caption.is_empty(), "table caption is missing");
    assert!(!table.rows.is_empty(), "table has no rows");
    assert!(!table.source.is_empty(), "table source line is missing");

    let output = render_document_to_temp_docx(&document, "table");
    let xml = read_docx_entry(output.path(), "word/document.xml");

    assert!(
        xml.contains("<w:tbl"),
        "DOCX does not contain a <w:tbl> element"
    );
}
