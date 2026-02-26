use anyhow::Result;
use clap::Parser;
use ferritex::{
    build::{self, BuildConfig},
    cli::{Cli, Mode},
    tui,
};
use std::path::Path;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let (verbose, mode) = cli.resolve_mode()?;
    init_logging(verbose);

    match mode {
        Mode::Build {
            input,
            format,
            output_dir,
            pdf_biber_bin_dir,
            pdf_biber_mode,
            tool_install_policy,
        } => {
            let config = BuildConfig::from_build_args(
                &input,
                output_dir.as_deref(),
                format,
                pdf_biber_bin_dir.as_deref(),
                pdf_biber_mode,
                tool_install_policy,
            );
            let result = build::run_build(&config)?;
            if let Some(ref docx) = result.docx {
                log::info!("DOCX: {}", docx.display());
            }
            if let Some(ref pdf) = result.pdf {
                log::info!("PDF: {}", pdf.display());
            }
            Ok(())
        }
        Mode::Convert { input, output } => {
            let config = BuildConfig::from_convert_paths(&input, &output);
            build::run_build(&config)?;
            Ok(())
        }
        Mode::Tui { input, output } => tui::run_tui(input, output, convert_paths),
    }
}

fn init_logging(verbose: bool) {
    let default_level = if verbose { "debug" } else { "info" };
    let env = env_logger::Env::default().default_filter_or(default_level);
    env_logger::Builder::from_env(env).init();
}

/// Legacy conversion function used by TUI mode.
///
/// TUI still calls this directly because it manages its own UI loop
/// and needs a simple `(&Path, &Path) -> Result<()>` callback.
fn convert_paths(input: &Path, output: &Path) -> Result<()> {
    let config = BuildConfig::from_convert_paths(input, output);
    build::run_build(&config)?;
    Ok(())
}
