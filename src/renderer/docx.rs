use std::path::Path;

use crate::{error::FerritexError, model::Document};

/// Render the intermediate AST into a `.docx` file.
#[allow(dead_code)] // TODO: implement DOCX renderer.
pub fn render_docx(_document: &Document, _output_path: &Path) -> Result<(), FerritexError> {
    Err(FerritexError::NotImplemented(
        "DOCX renderer is not implemented yet",
    ))
}
