mod cli;
mod error;
mod model;
mod parser;
mod renderer;
mod tui;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Mode};
use std::path::Path;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let (verbose, mode) = cli.resolve_mode()?;
    init_logging(verbose);

    match mode {
        Mode::Convert { input, output } => convert_paths(&input, &output),
        Mode::Tui { input, output } => tui::run_tui(input, output, convert_paths),
    }
}

fn init_logging(verbose: bool) {
    let default_level = if verbose { "debug" } else { "info" };
    let env = env_logger::Env::default().default_filter_or(default_level);
    env_logger::Builder::from_env(env).init();
}

fn convert_paths(input: &Path, output: &Path) -> Result<()> {
    log::info!("Reading {}", input.display());
    let document = parser::latex::parse_latex_file(input)?;
    log::debug!("Parsed {} block(s)", document.blocks.len());

    log::info!("Writing {}", output.display());
    renderer::docx::render_docx(&document, output)?;

    log::info!("Done.");
    Ok(())
}
