//! Static analysis codebase linter binary.

use omni::code_lint::{LintOptions, run_code_lint};
use omni::core::Config;
use omni::diagnostic::print_diagnostics;
use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "omni-code-lint",
    version,
    about = "Static analysis codebase linter"
)]
struct Cli {
    /// Paths to inspect
    #[arg(default_value = ".")]
    paths: Vec<String>,

    /// Output format for diagnostics (plain, json)
    #[arg(long, default_value = "plain")]
    format: String,

    /// Run linter only on files/lines changed in VCS
    #[arg(long)]
    diff: bool,

    /// Run linter comparing against a custom revision/commit (implies --diff)
    #[arg(long)]
    diff_rev: Option<String>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Error: {error}");
        std::process::exit(2);
    }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let config =
        Config::load().map_err(|error| anyhow::anyhow!("Failed to load configuration: {error}"))?;

    let options = LintOptions {
        paths: cli.paths.into_iter().map(PathBuf::from).collect(),
        diff: cli.diff,
        diff_rev: cli.diff_rev,
    };

    let all_diagnostics = run_code_lint(&options, &config)?;

    // Print diagnostics according to format
    if !all_diagnostics.is_empty() {
        print_diagnostics(&all_diagnostics, &cli.format)?;
        std::process::exit(1);
    }
    Ok(())
}
