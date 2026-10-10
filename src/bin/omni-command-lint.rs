//! Command execution safety and workflow context linter binary.

omni::architecture_component!(Bin);

use omni::command_lint::runner::run_command_lint;
use omni::diagnostic::{OutputFormat, print_diagnostics};
use omni::rule_catalog::{DiscoveryArgs, explain_footer};
use omni::rule_selection::load_config;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "omni-command-lint",
    version,
    about = "Command execution workflow safety linter"
)]
struct Cli {
    /// Output format for diagnostics
    #[arg(long, value_enum, default_value_t = OutputFormat::Plain)]
    format: OutputFormat,

    /// Shell command string to validate
    #[arg(long, required_unless_present_any = ["list_rules", "list_tags", "explain"])]
    cmd: Option<String>,

    #[command(flatten)]
    discovery: DiscoveryArgs,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if let Some(output) = cli.discovery.render() {
        match output {
            Ok(text) => print!("{text}"),
            Err(error) => {
                eprintln!("Error: {error}");
                std::process::exit(2);
            }
        }
        return Ok(());
    }
    let Some(cmd) = cli.cmd else {
        unreachable!("clap requires --cmd unless a discovery flag is given");
    };

    let config = match load_config() {
        Ok(loaded_config) => loaded_config,
        Err(error) => {
            eprintln!("Error: Failed to load configuration: {error}");
            std::process::exit(2);
        }
    };

    let all_diagnostics = run_command_lint(&cmd, &config);

    if !all_diagnostics.is_empty() {
        print_diagnostics(&all_diagnostics, cli.format)?;
        if cli.format == OutputFormat::Plain {
            println!("{}", explain_footer(env!("CARGO_BIN_NAME")));
        }
        std::process::exit(1);
    }
    Ok(())
}
