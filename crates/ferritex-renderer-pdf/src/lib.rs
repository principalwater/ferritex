use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use ferritex_core::model::Document;
use tectonic::latex_to_pdf;

#[derive(Debug)]
struct CurrentDirGuard {
    previous_dir: PathBuf,
}

impl CurrentDirGuard {
    fn change_to(target_dir: &Path) -> Result<Self> {
        let previous_dir = std::env::current_dir()
            .map_err(|error| anyhow!("failed to read current working directory: {error}"))?;
        std::env::set_current_dir(target_dir).map_err(|error| {
            anyhow!(
                "failed to switch current working directory to {}: {error}",
                target_dir.display()
            )
        })?;
        Ok(Self { previous_dir })
    }
}

impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        if let Err(error) = std::env::set_current_dir(&self.previous_dir) {
            log::warn!(
                "failed to restore working directory to {}: {error}",
                self.previous_dir.display()
            );
        }
    }
}

fn with_temporary_working_dir<T, F>(target_dir: &Path, action: F) -> Result<T>
where
    F: FnOnce() -> Result<T>,
{
    let _guard = CurrentDirGuard::change_to(target_dir)?;
    action()
}

/// Render a document to PDF.
///
/// The PDF backend uses `tectonic::latex_to_pdf` as the canonical runtime path
/// for parity-oriented output.
pub fn render_pdf_with_context(
    _document: &Document,
    output: &Path,
    input_context: Option<&Path>,
) -> Result<()> {
    let input_path = input_context.ok_or_else(|| {
        anyhow!("PDF rendering requires input context path to resolve LaTeX includes and assets")
    })?;
    let input_root = input_path.parent().unwrap_or_else(|| Path::new("."));
    let latex_source = std::fs::read_to_string(input_path).map_err(|error| {
        anyhow!(
            "failed to read LaTeX source from {}: {error}",
            input_path.display()
        )
    })?;

    let pdf_bytes = with_temporary_working_dir(input_root, || {
        latex_to_pdf(&latex_source).map_err(|error| {
            anyhow!(
                "tectonic::latex_to_pdf failed for {}: {error}",
                input_path.display()
            )
        })
    })?;
    if pdf_bytes.is_empty() {
        return Err(anyhow!(
            "tectonic::latex_to_pdf returned empty PDF payload for {}",
            input_path.display()
        ));
    }

    std::fs::write(output, pdf_bytes)
        .map_err(|error| anyhow!("failed to write PDF artifact {}: {error}", output.display()))
}
