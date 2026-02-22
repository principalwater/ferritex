use crate::{error::FerritexError, model::Document};

/// Parse LaTeX source into an intermediate `Document` AST.
#[allow(dead_code)] // TODO: implement LaTeX parser.
pub fn parse_latex(_source: &str) -> Result<Document, FerritexError> {
    Err(FerritexError::NotImplemented(
        "LaTeX parser is not implemented yet",
    ))
}
