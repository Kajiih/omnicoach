//! Static analysis codebase linter binary.

omni::architecture_component!(Bin);

use omni::code_lint::runner::{LintOptions, run_code_lint};
use omni::diagnostic::{OutputFormat, print_diagnostics};
use omni::rule_catalog::{DiscoveryArgs, explain_footer};
use omni::rule_selection::load_config;
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
    paths: Vec<PathBuf>,

    /// Output format for diagnostics
    #[arg(long, value_enum, default_value_t = OutputFormat::Plain)]
    format: OutputFormat,

    /// Run linter only on files/lines changed in VCS
    #[arg(long)]
    diff: bool,

    /// Run linter comparing against a custom revision/commit (implies --diff)
    #[arg(long)]
    diff_rev: Option<String>,

    #[command(flatten)]
    discovery: DiscoveryArgs,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Error: {error}");
        std::process::exit(2);
    }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if let Some(output) = cli.discovery.render() {
        print!("{}", output?);
        return Ok(());
    }

    let config =
        load_config().map_err(|error| anyhow::anyhow!("Failed to load configuration: {error}"))?;

    let options = LintOptions {
        paths: cli.paths,
        diff: cli.diff,
        diff_rev: cli.diff_rev,
    };

    let all_diagnostics = run_code_lint(&options, &config)?;

    // Print diagnostics according to format
    if !all_diagnostics.is_empty() {
        print_diagnostics(&all_diagnostics, cli.format)?;
        if cli.format == OutputFormat::Plain {
            println!("{}", explain_footer(env!("CARGO_BIN_NAME")));
        }
        std::process::exit(1);
    }
    Ok(())
}
