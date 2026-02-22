mod cli;
mod error;
mod model;
mod parser;
mod renderer;

use anyhow::Result;
use clap::Parser;
use cli::Cli;

fn main() -> Result<()> {
    let cli = Cli::parse();
    init_logging(cli.verbose);
    convert(&cli)
}

fn init_logging(verbose: bool) {
    let default_level = if verbose { "debug" } else { "info" };
    let env = env_logger::Env::default().default_filter_or(default_level);
    env_logger::Builder::from_env(env).init();
}

fn convert(cli: &Cli) -> Result<()> {
    log::info!("Reading {}", cli.input.display());
    let document = parser::latex::parse_latex_file(&cli.input)?;
    log::debug!("Parsed {} block(s)", document.blocks.len());

    log::info!("Writing {}", cli.output.display());
    renderer::docx::render_docx(&document, &cli.output)?;

    log::info!("Done.");
    Ok(())
}
