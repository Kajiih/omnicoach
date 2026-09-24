//! Command execution safety and workflow context linter binary.

use omni::command_lint::runner::run_command_lint;
use omni::core::Config;
use omni::diagnostic::print_diagnostics;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "omni-command-lint",
    version,
    about = "Command execution workflow safety linter"
)]
struct Cli {
    /// Format output (plain, json)
    #[arg(long, default_value = "plain")]
    format: String,

    /// Shell command string to validate
    #[arg(long)]
    cmd: String,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let config = match Config::load() {
        Ok(loaded_config) => loaded_config,
        Err(error) => {
            eprintln!("Error: Failed to load configuration: {error}");
            std::process::exit(2);
        }
    };

    let all_diagnostics = run_command_lint(&cli.cmd, &config);

    if !all_diagnostics.is_empty() {
        print_diagnostics(&all_diagnostics, &cli.format)?;
        std::process::exit(1);
    }
    Ok(())
}
