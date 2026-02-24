use std::path::Path;

use anyhow::Result;
use ferritex_core::model::Document;

/// Render a document to PDF.
///
/// This backend is intentionally stubbed until the v1.0 PDF implementation is added.
pub fn render_pdf_with_context(
    _document: &Document,
    _output: &Path,
    _input_context: Option<&Path>,
) -> Result<()> {
    anyhow::bail!("PDF output backend is not implemented yet")
}
