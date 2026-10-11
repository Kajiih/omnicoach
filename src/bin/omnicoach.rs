//! Static analysis and command workflow coach binary.

omni::architecture_component!(Bin);

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use omni::code_lint::runner::{LintOptions, run_code_lint};
use omni::command_lint::runner::run_command_lint;
use omni::config::Config;
use omni::diagnostic::{OutputFormat, print_diagnostics};
use omni::rule_catalog::{DiscoveryArgs, explain_footer};
use omni::rule_selection::load_config;

#[derive(Parser, Debug)]
#[command(
    name = "omnicoach",
    version,
    about = "Static code analysis and command workflow coach"
)]
struct Cli {
    #[command(subcommand)]
    subcommand: Option<DomainCommand>,

    /// Output format for diagnostics
    #[arg(long, global = true, value_enum, default_value_t = OutputFormat::Plain)]
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

#[derive(Subcommand, Debug)]
enum DomainCommand {
    /// Inspect source files and directories
    Code(CodeArgs),
    /// Validate an intercepted shell command string
    Command(CommandArgs),
}

#[derive(Args, Debug)]
struct CodeArgs {
    /// Paths to inspect
    #[arg(default_value = ".")]
    paths: Vec<PathBuf>,

    /// Run linter only on files/lines changed in VCS
    #[arg(long)]
    diff: bool,

    /// Run linter comparing against a custom revision/commit (implies --diff)
    #[arg(long)]
    diff_rev: Option<String>,
}

#[derive(Args, Debug)]
struct CommandArgs {
    /// Shell command string to validate
    #[arg(long, required_unless_present_any = ["list_rules", "list_tags", "explain"])]
    cmd: Option<String>,
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

    match cli.subcommand {
        Some(DomainCommand::Code(args)) => {
            let options = LintOptions {
                paths: args.paths,
                diff: args.diff || cli.diff,
                diff_rev: args.diff_rev.or(cli.diff_rev),
            };
            run_code(&options, &config, cli.format)
        }
        None => {
            let options = LintOptions {
                paths: vec![PathBuf::from(".")],
                diff: cli.diff,
                diff_rev: cli.diff_rev,
            };
            run_code(&options, &config, cli.format)
        }
        Some(DomainCommand::Command(args)) => {
            let Some(command) = args.cmd else {
                unreachable!("clap requires --cmd unless a discovery flag is given");
            };
            run_command(&command, &config, cli.format)
        }
    }
}

fn run_code(options: &LintOptions, config: &Config, format: OutputFormat) -> anyhow::Result<()> {
    let report = run_code_lint(options, config)?;

    if !report.diagnostics.is_empty() {
        print_diagnostics(&report.diagnostics, format)?;
        if format == OutputFormat::Plain {
            println!("{}", explain_footer(env!("CARGO_BIN_NAME")));
        }
    }
    for unreadable in &report.unreadable_files {
        eprintln!(
            "Error: Failed to read file '{}': {}",
            unreadable.path.display(),
            unreadable.error
        );
    }
    if !report.unreadable_files.is_empty() {
        std::process::exit(2);
    }
    if !report.diagnostics.is_empty() {
        std::process::exit(1);
    }
    if format == OutputFormat::Plain {
        println!("All checks passed!");
    }
    Ok(())
}

fn run_command(command: &str, config: &Config, format: OutputFormat) -> anyhow::Result<()> {
    let diagnostics = run_command_lint(command, config);

    if !diagnostics.is_empty() {
        print_diagnostics(&diagnostics, format)?;
        if format == OutputFormat::Plain {
            println!("{}", explain_footer(env!("CARGO_BIN_NAME")));
        }
        std::process::exit(1);
    }
    Ok(())
}
