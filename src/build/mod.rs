//! Unified build-core orchestrator for ferritex.
//!
//! This module provides a single entry point for converting LaTeX projects
//! into one or more output formats (DOCX, PDF, Markdown). The `convert` and
//! `tui` CLI modes delegate to this core so that path resolution, artifact
//! naming, and pipeline sequencing are defined in one place.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::parser;
use crate::renderer;

/// Output format(s) requested for a build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Generate DOCX only.
    Docx,
    /// Generate PDF only (not yet implemented).
    Pdf,
    /// Generate Markdown only (not yet implemented).
    Md,
    /// Generate both DOCX and PDF.
    Both,
    /// Generate all supported formats (DOCX + PDF + Markdown).
    All,
}

impl OutputFormat {
    /// Returns `true` if the DOCX pipeline should run.
    pub fn needs_docx(self) -> bool {
        matches!(self, Self::Docx | Self::Both | Self::All)
    }

    /// Returns `true` if the PDF pipeline should run.
    pub fn needs_pdf(self) -> bool {
        matches!(self, Self::Pdf | Self::Both | Self::All)
    }

    /// Returns `true` if the Markdown pipeline should run.
    pub fn needs_md(self) -> bool {
        matches!(self, Self::Md | Self::All)
    }
}

/// Configuration for a single build invocation.
#[derive(Debug, Clone)]
pub struct BuildConfig {
    /// Path to the root `.tex` input file.
    pub input: PathBuf,
    /// Base directory for output artifacts.
    ///
    /// When an explicit output path is provided, this is its parent directory.
    /// When only a directory is given, artifact filenames are derived from the
    /// input stem.
    pub output_dir: PathBuf,
    /// Stem used for naming output files (e.g. `"main"` → `main.docx`).
    pub output_stem: String,
    /// Which format(s) to produce.
    pub format: OutputFormat,
}

/// Result of a completed build, listing the artifacts that were produced.
#[derive(Debug, Clone, Default)]
pub struct BuildResult {
    /// Path to the generated DOCX file, if any.
    pub docx: Option<PathBuf>,
    /// Path to the generated PDF file, if any.
    pub pdf: Option<PathBuf>,
    /// Path to the generated Markdown file, if any.
    pub md: Option<PathBuf>,
}

impl BuildConfig {
    /// Create a [`BuildConfig`] from explicit input/output paths (legacy `convert` mode).
    ///
    /// The output format is always DOCX when called from the legacy path.
    pub fn from_convert_paths(input: &Path, output: &Path) -> Self {
        let output_dir = output
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let output_stem = output
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "output".to_owned());

        Self {
            input: input.to_path_buf(),
            output_dir,
            output_stem,
            format: OutputFormat::Docx,
        }
    }

    /// Create a [`BuildConfig`] from the unified `build` subcommand arguments.
    pub fn from_build_args(input: &Path, output_dir: Option<&Path>, format: OutputFormat) -> Self {
        let resolved_output_dir = output_dir.map(|p| p.to_path_buf()).unwrap_or_else(|| {
            input
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf()
        });
        let output_stem = input
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "output".to_owned());

        Self {
            input: input.to_path_buf(),
            output_dir: resolved_output_dir,
            output_stem,
            format,
        }
    }

    /// Resolve the expected DOCX artifact path.
    pub fn docx_path(&self) -> PathBuf {
        self.output_dir.join(format!("{}.docx", self.output_stem))
    }

    /// Resolve the expected PDF artifact path.
    pub fn pdf_path(&self) -> PathBuf {
        self.output_dir.join(format!("{}.pdf", self.output_stem))
    }

    /// Resolve the expected Markdown artifact path.
    pub fn md_path(&self) -> PathBuf {
        self.output_dir.join(format!("{}.md", self.output_stem))
    }
}

/// Run the full build pipeline according to the given configuration.
pub fn run_build(config: &BuildConfig) -> Result<BuildResult> {
    log::info!("Reading {}", config.input.display());
    let document = parser::latex::parse_latex_file(&config.input)
        .with_context(|| format!("failed to parse {}", config.input.display()))?;
    log::debug!("Parsed {} block(s)", document.blocks.len());

    let mut result = BuildResult::default();

    if config.format.needs_docx() {
        let docx_path = config.docx_path();
        run_docx_pipeline(&document, &config.input, &docx_path)?;
        result.docx = Some(docx_path);
    }

    if config.format.needs_pdf() {
        let pdf_path = config.pdf_path();
        run_pdf_pipeline(&document, &config.input, &pdf_path)?;
        result.pdf = Some(pdf_path);
    }

    if config.format.needs_md() {
        let md_path = config.md_path();
        run_md_pipeline(&document, &config.input, &md_path)?;
        result.md = Some(md_path);
    }

    Ok(result)
}

/// Execute the DOCX conversion pipeline: render → write.
fn run_docx_pipeline(document: &crate::model::Document, input: &Path, output: &Path) -> Result<()> {
    log::info!("Writing {}", output.display());
    renderer::docx::render_docx_with_context(document, output, Some(input))
        .with_context(|| format!("failed to write {}", output.display()))?;
    Ok(())
}

/// Execute the PDF conversion pipeline: render → write.
fn run_pdf_pipeline(document: &crate::model::Document, input: &Path, output: &Path) -> Result<()> {
    log::info!("Writing {}", output.display());
    renderer::pdf::render_pdf_with_context(document, output, Some(input))
        .with_context(|| format!("failed to write {}", output.display()))?;
    Ok(())
}

/// Execute the Markdown conversion pipeline: render → write.
fn run_md_pipeline(document: &crate::model::Document, input: &Path, output: &Path) -> Result<()> {
    log::info!("Writing {}", output.display());
    renderer::md::render_md_with_context(document, output, Some(input))
        .with_context(|| format!("failed to write {}", output.display()))?;
    log::info!("Done.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- OutputFormat --------------------------------------------------------

    #[test]
    fn output_format_docx_needs() {
        assert!(OutputFormat::Docx.needs_docx());
        assert!(!OutputFormat::Docx.needs_pdf());
    }

    #[test]
    fn output_format_pdf_needs() {
        assert!(!OutputFormat::Pdf.needs_docx());
        assert!(OutputFormat::Pdf.needs_pdf());
        assert!(!OutputFormat::Pdf.needs_md());
    }

    #[test]
    fn output_format_md_needs() {
        assert!(!OutputFormat::Md.needs_docx());
        assert!(!OutputFormat::Md.needs_pdf());
        assert!(OutputFormat::Md.needs_md());
    }

    #[test]
    fn output_format_both_needs() {
        assert!(OutputFormat::Both.needs_docx());
        assert!(OutputFormat::Both.needs_pdf());
        assert!(!OutputFormat::Both.needs_md());
    }

    #[test]
    fn output_format_all_needs() {
        assert!(OutputFormat::All.needs_docx());
        assert!(OutputFormat::All.needs_pdf());
        assert!(OutputFormat::All.needs_md());
    }

    // -- BuildConfig path resolution -----------------------------------------

    #[test]
    fn from_convert_paths_resolves_dir_and_stem() {
        let cfg = BuildConfig::from_convert_paths(
            Path::new("project/main.tex"),
            Path::new("out/report.docx"),
        );
        assert_eq!(cfg.input, PathBuf::from("project/main.tex"));
        assert_eq!(cfg.output_dir, PathBuf::from("out"));
        assert_eq!(cfg.output_stem, "report");
        assert_eq!(cfg.format, OutputFormat::Docx);
    }

    #[test]
    fn from_convert_paths_bare_filename() {
        let cfg = BuildConfig::from_convert_paths(Path::new("a.tex"), Path::new("b.docx"));
        assert_eq!(cfg.output_dir, PathBuf::from(""));
        assert_eq!(cfg.output_stem, "b");
    }

    #[test]
    fn from_build_args_defaults_output_dir_to_input_parent() {
        let cfg = BuildConfig::from_build_args(Path::new("src/main.tex"), None, OutputFormat::Docx);
        assert_eq!(cfg.output_dir, PathBuf::from("src"));
        assert_eq!(cfg.output_stem, "main");
    }

    #[test]
    fn from_build_args_explicit_output_dir() {
        let cfg = BuildConfig::from_build_args(
            Path::new("main.tex"),
            Some(Path::new("/tmp/out")),
            OutputFormat::Both,
        );
        assert_eq!(cfg.output_dir, PathBuf::from("/tmp/out"));
        assert_eq!(cfg.output_stem, "main");
        assert_eq!(cfg.format, OutputFormat::Both);
    }

    // -- Artifact path helpers -----------------------------------------------

    #[test]
    fn docx_path_combines_dir_and_stem() {
        let cfg = BuildConfig::from_build_args(
            Path::new("thesis.tex"),
            Some(Path::new("/out")),
            OutputFormat::Docx,
        );
        assert_eq!(cfg.docx_path(), PathBuf::from("/out/thesis.docx"));
    }

    #[test]
    fn pdf_path_combines_dir_and_stem() {
        let cfg = BuildConfig::from_build_args(
            Path::new("thesis.tex"),
            Some(Path::new("/out")),
            OutputFormat::Pdf,
        );
        assert_eq!(cfg.pdf_path(), PathBuf::from("/out/thesis.pdf"));
    }

    #[test]
    fn md_path_combines_dir_and_stem() {
        let cfg = BuildConfig::from_build_args(
            Path::new("thesis.tex"),
            Some(Path::new("/out")),
            OutputFormat::Md,
        );
        assert_eq!(cfg.md_path(), PathBuf::from("/out/thesis.md"));
    }
}
