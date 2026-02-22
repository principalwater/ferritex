use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Command-line arguments for the `ferritex` binary.
#[derive(Debug, Parser)]
#[command(
    name = "ferritex",
    version,
    about = "Convert LaTeX (.tex) documents to DOCX (.docx)",
    after_help = "Examples:\n  ferritex --input main.tex --output main.docx\n  ferritex convert --input main.tex --output main.docx\n  ferritex tui --input main.tex"
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
    /// Convert LaTeX source to DOCX in non-interactive mode.
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

/// Resolved application mode after CLI parsing.
#[derive(Debug)]
pub enum Mode {
    Convert {
        input: PathBuf,
        output: PathBuf,
    },
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
}
