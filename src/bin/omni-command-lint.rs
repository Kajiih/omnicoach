//! Command execution safety and workflow context linter binary.

use omni::command_lint::vcs::JjCliClient;
use omni::command_lint::InterceptedCommand;
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

    // Load configuration
    let config = match Config::load() {
        Ok(loaded_config) => loaded_config,
        Err(error) => {
            eprintln!("Error: Failed to load configuration: {error}");
            std::process::exit(2);
        }
    };

    let mut all_diagnostics = Vec::new();

    // Parse the intercepted command calls
    let commands = InterceptedCommand::parse_all(&cli.cmd);

    let jj_client = JjCliClient;

    // Evaluate guard rules in registry matching the context for each command invocation
    for cmd in &commands {
        for rule in omni::rules::COMMAND_RULES {
            if config.is_rule_enabled(*rule) {
                all_diagnostics.extend(rule.check_command(cmd, &jj_client, &config));
            }
        }
    }

    // Print diagnostics according to format
    if !all_diagnostics.is_empty() {
        print_diagnostics(&all_diagnostics, &cli.format)?;
        std::process::exit(1);
    }
    Ok(())
}
