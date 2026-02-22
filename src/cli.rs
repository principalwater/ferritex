use std::path::PathBuf;

use clap::Parser;

/// Command-line arguments for the `ferritex` binary.
#[derive(Debug, Parser)]
#[command(
    name = "ferritex",
    version,
    about = "Convert LaTeX (.tex) documents to DOCX (.docx)"
)]
pub struct Cli {
    /// Path to the input .tex file.
    #[arg(long, short = 'i', value_name = "INPUT")]
    pub input: PathBuf,

    /// Path to the output .docx file.
    #[arg(long, short = 'o', value_name = "OUTPUT")]
    pub output: PathBuf,

    /// Enable verbose logging.
    #[arg(long)]
    pub verbose: bool,
}
