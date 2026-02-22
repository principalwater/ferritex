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
    log::info!(
        "Conversion not yet implemented: {} -> {}",
        cli.input.display(),
        cli.output.display()
    );
    Ok(())
}
