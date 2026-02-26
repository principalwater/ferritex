use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

use crate::build::{OutputFormat, PdfBiberMode, ToolInstallPolicy};

/// Command-line arguments for the `ferritex` binary.
#[derive(Debug, Parser)]
#[command(
    name = "ferritex",
    version,
    about = "Build LaTeX (.tex) projects into DOCX, PDF, Markdown, or combined sets",
    after_help = "Examples:\n  ferritex build --input main.tex --format docx\n  ferritex build --input main.tex --format both --output-dir out/\n  ferritex build --input main.tex --format pdf --pdf-biber-mode auto --tool-install-policy ask\n  ferritex build --input main.tex --format pdf --pdf-biber-bin-dir /opt/biber/bin --pdf-biber-mode strict\n  ferritex build --input main.tex --format md --output-dir out/\n  ferritex convert --input main.tex --output main.docx\n  ferritex tui --input main.tex"
)]
pub struct Cli {
    /// Enable verbose logging.
    #[arg(long, global = true)]
    pub verbose: bool,

    /// Compatibility mode: path to the input .tex file.
    ///
    /// Works without subcommands: `ferritex --input a.tex --output a.docx`.
    #[arg(long, short = 'i', value_name = "INPUT", global = true)]
    pub input: Option<PathBuf>,

    /// Compatibility mode: path to the output .docx file.
    #[arg(long, short = 'o', value_name = "OUTPUT", global = true)]
    pub output: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Subcommands for explicit execution mode selection.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Unified build: convert a LaTeX project to one or more output formats.
    Build {
        /// Path to the root `.tex` input file.
        #[arg(long, short = 'i', value_name = "INPUT")]
        input: PathBuf,
        /// Output format to produce.
        #[arg(long, short = 'f', value_name = "FORMAT", default_value = "docx")]
        format: CliOutputFormat,
        /// Directory for output artifacts (defaults to input file's directory).
        #[arg(long, value_name = "DIR")]
        output_dir: Option<PathBuf>,
        /// Optional directory containing a compatible `biber` binary for PDF builds.
        ///
        /// When set, ferritex prepends this directory to `PATH` only for the
        /// PDF runtime session.
        #[arg(long, value_name = "DIR")]
        pdf_biber_bin_dir: Option<PathBuf>,
        /// Bibliography tool resolution mode for PDF builds.
        ///
        /// `auto` retries with alternative `biber` candidates when a BCF
        /// mismatch is detected. `strict` fails on the first candidate.
        #[arg(long, value_name = "MODE", default_value = "auto")]
        pdf_biber_mode: CliPdfBiberMode,
        /// Policy for missing/incompatible external tools used by renderers.
        ///
        /// `ask` prompts before installing tool shims, `auto` installs
        /// automatically when possible, `never` always fails with guidance.
        #[arg(long, value_name = "POLICY", default_value = "ask")]
        tool_install_policy: CliToolInstallPolicy,
    },
    /// Convert LaTeX source to DOCX in non-interactive mode (legacy).
    Convert {
        /// Path to input `.tex`.
        #[arg(long, short = 'i', value_name = "INPUT")]
        input: PathBuf,
        /// Path to output `.docx`.
        #[arg(long, short = 'o', value_name = "OUTPUT")]
        output: PathBuf,
    },
    /// Run an interactive terminal UI (ratatui) for conversion.
    Tui {
        /// Optional prefilled input `.tex` path.
        #[arg(long, short = 'i', value_name = "INPUT")]
        input: Option<PathBuf>,
        /// Optional prefilled output `.docx` path.
        #[arg(long, short = 'o', value_name = "OUTPUT")]
        output: Option<PathBuf>,
    },
}

/// CLI-facing output format enum, mapped to [`OutputFormat`] after parsing.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CliOutputFormat {
    Docx,
    Pdf,
    Md,
    Both,
    All,
}

impl From<CliOutputFormat> for OutputFormat {
    fn from(f: CliOutputFormat) -> Self {
        match f {
            CliOutputFormat::Docx => OutputFormat::Docx,
            CliOutputFormat::Pdf => OutputFormat::Pdf,
            CliOutputFormat::Md => OutputFormat::Md,
            CliOutputFormat::Both => OutputFormat::Both,
            CliOutputFormat::All => OutputFormat::All,
        }
    }
}

/// CLI-facing bibliography tool resolution mode for PDF builds.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CliPdfBiberMode {
    Strict,
    Auto,
}

impl From<CliPdfBiberMode> for PdfBiberMode {
    fn from(value: CliPdfBiberMode) -> Self {
        match value {
            CliPdfBiberMode::Strict => PdfBiberMode::Strict,
            CliPdfBiberMode::Auto => PdfBiberMode::Auto,
        }
    }
}

/// CLI-facing policy for missing/incompatible external tool handling.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CliToolInstallPolicy {
    Ask,
    Auto,
    Never,
}

impl From<CliToolInstallPolicy> for ToolInstallPolicy {
    fn from(value: CliToolInstallPolicy) -> Self {
        match value {
            CliToolInstallPolicy::Ask => ToolInstallPolicy::Ask,
            CliToolInstallPolicy::Auto => ToolInstallPolicy::Auto,
            CliToolInstallPolicy::Never => ToolInstallPolicy::Never,
        }
    }
}

/// Resolved application mode after CLI parsing.
#[derive(Debug)]
pub enum Mode {
    /// Unified build pipeline.
    Build {
        input: PathBuf,
        format: OutputFormat,
        output_dir: Option<PathBuf>,
        pdf_biber_bin_dir: Option<PathBuf>,
        pdf_biber_mode: PdfBiberMode,
        tool_install_policy: ToolInstallPolicy,
    },
    /// Legacy single-file DOCX conversion.
    Convert { input: PathBuf, output: PathBuf },
    /// Interactive TUI.
    Tui {
        input: Option<PathBuf>,
        output: Option<PathBuf>,
    },
}

impl Cli {
    /// Resolve parsed arguments to one execution mode.
    pub fn resolve_mode(self) -> anyhow::Result<(bool, Mode)> {
        let verbose = self.verbose;

        let mode = match self.command {
            Some(Command::Build {
                input,
                format,
                output_dir,
                pdf_biber_bin_dir,
                pdf_biber_mode,
                tool_install_policy,
            }) => Mode::Build {
                input,
                format: format.into(),
                output_dir,
                pdf_biber_bin_dir,
                pdf_biber_mode: pdf_biber_mode.into(),
                tool_install_policy: tool_install_policy.into(),
            },
            Some(Command::Convert { input, output }) => Mode::Convert { input, output },
            Some(Command::Tui { input, output }) => Mode::Tui { input, output },
            None => match (self.input, self.output) {
                (Some(input), Some(output)) => Mode::Convert { input, output },
                (None, None) => Mode::Tui {
                    input: None,
                    output: None,
                },
                (Some(_), None) | (None, Some(_)) => {
                    anyhow::bail!(
                        "both --input and --output must be provided in non-interactive mode"
                    )
                }
            },
        };

        Ok((verbose, mode))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_resolve_mode_compat_convert() {
        let cli = Cli::parse_from(["ferritex", "--input", "a.tex", "--output", "a.docx"]);
        let (_, mode) = cli.resolve_mode().expect("mode should resolve");
        match mode {
            Mode::Convert { input, output } => {
                assert_eq!(input, PathBuf::from("a.tex"));
                assert_eq!(output, PathBuf::from("a.docx"));
            }
            other => panic!("expected convert mode, got {other:?}"),
        }
    }

    #[test]
    fn test_resolve_mode_default_tui() {
        let cli = Cli::parse_from(["ferritex"]);
        let (_, mode) = cli.resolve_mode().expect("mode should resolve");
        assert!(matches!(
            mode,
            Mode::Tui {
                input: None,
                output: None
            }
        ));
    }

    #[test]
    fn test_resolve_mode_subcommand_convert() {
        let cli = Cli::parse_from([
            "ferritex", "convert", "--input", "x.tex", "--output", "x.docx",
        ]);
        let (_, mode) = cli.resolve_mode().expect("mode should resolve");
        assert!(matches!(mode, Mode::Convert { .. }));
    }

    #[test]
    fn test_resolve_mode_requires_both_paths_for_noninteractive() {
        let cli = Cli::parse_from(["ferritex", "--input", "a.tex"]);
        assert!(cli.resolve_mode().is_err());
    }

    #[test]
    fn test_resolve_mode_build_default_format() {
        let cli = Cli::parse_from(["ferritex", "build", "--input", "main.tex"]);
        let (_, mode) = cli.resolve_mode().expect("mode should resolve");
        match mode {
            Mode::Build {
                input,
                format,
                output_dir,
                pdf_biber_bin_dir,
                pdf_biber_mode,
                tool_install_policy,
            } => {
                assert_eq!(input, PathBuf::from("main.tex"));
                assert_eq!(format, OutputFormat::Docx);
                assert!(output_dir.is_none());
                assert!(pdf_biber_bin_dir.is_none());
                assert_eq!(pdf_biber_mode, PdfBiberMode::Auto);
                assert_eq!(tool_install_policy, ToolInstallPolicy::Ask);
            }
            other => panic!("expected build mode, got {other:?}"),
        }
    }

    #[test]
    fn test_resolve_mode_build_both_with_output_dir() {
        let cli = Cli::parse_from([
            "ferritex",
            "build",
            "--input",
            "main.tex",
            "--format",
            "both",
            "--output-dir",
            "/tmp/out",
            "--pdf-biber-bin-dir",
            "/opt/biber/bin",
            "--pdf-biber-mode",
            "strict",
            "--tool-install-policy",
            "never",
        ]);
        let (_, mode) = cli.resolve_mode().expect("mode should resolve");
        match mode {
            Mode::Build {
                format,
                output_dir,
                pdf_biber_bin_dir,
                pdf_biber_mode,
                tool_install_policy,
                ..
            } => {
                assert_eq!(format, OutputFormat::Both);
                assert_eq!(output_dir, Some(PathBuf::from("/tmp/out")));
                assert_eq!(pdf_biber_bin_dir, Some(PathBuf::from("/opt/biber/bin")));
                assert_eq!(pdf_biber_mode, PdfBiberMode::Strict);
                assert_eq!(tool_install_policy, ToolInstallPolicy::Never);
            }
            other => panic!("expected build mode, got {other:?}"),
        }
    }

    #[test]
    fn test_resolve_mode_build_md_format() {
        let cli = Cli::parse_from(["ferritex", "build", "--input", "main.tex", "--format", "md"]);
        let (_, mode) = cli.resolve_mode().expect("mode should resolve");
        match mode {
            Mode::Build { format, .. } => {
                assert_eq!(format, OutputFormat::Md);
            }
            other => panic!("expected build mode, got {other:?}"),
        }
    }
}
