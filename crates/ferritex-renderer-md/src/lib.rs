use std::path::Path;

use anyhow::Result;
use ferritex_core::model::Document;

/// Render a document to Markdown.
///
/// This backend is intentionally stubbed until Markdown mapping rules are finalized.
pub fn render_md_with_context(
    _document: &Document,
    _output: &Path,
    _input_context: Option<&Path>,
) -> Result<()> {
    anyhow::bail!("Markdown output backend is not implemented yet")
}
