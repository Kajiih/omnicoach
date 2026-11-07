//! Static analysis and command workflow coach binary.

omnicoach::architecture_component!(Bin);

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use omnicoach::code_lint::graph::{
    FindQueryOptions, GraphOptions, OutputFormat as GraphOutputFormat, ProjectDepth,
    SliceQueryOptions, generate_graph, run_find_query, run_slice_query,
};
use omnicoach::code_lint::runner::{LintOptions, run_code_lint};
use omnicoach::command_lint::runner::run_command_lint;
use omnicoach::config::Config;
use omnicoach::diagnostic::{OutputFormat, print_diagnostics};
use omnicoach::hook::{AntigravityEvent, run_antigravity_hook};
use omnicoach::rule_catalog::{DiscoveryArgs, code_report_footer, explain_footer};
use omnicoach::rule_selection::{ConfigOverrides, load_command_config, load_config};

fn main() {
    if let Err(error) = run() {
        eprintln!("Error: {error}");
        std::process::exit(2);
    }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if let Some(output) = cli.discovery.render(&cli.config_overrides) {
        print!("{}", output?);
        return Ok(());
    }

    let Some(subcommand) = cli.subcommand else {
        let config = load(load_config, &cli.config_overrides)?;
        let options = LintOptions {
            paths: vec![PathBuf::from(".")],
            diff: cli.diff,
            diff_rev: cli.diff_rev,
        };
        return run_code(&options, &config, cli.format);
    };

    match subcommand {
        DomainCommand::Hook(args) => {
            match args.agent {
                HookAgent::Antigravity { event } => {
                    run_hook(|input| run_antigravity_hook(event, input));
                }
            }
            Ok(())
        }
        DomainCommand::Code(args) => {
            let config = load(load_config, &cli.config_overrides)?;
            let options = LintOptions {
                paths: args.paths,
                diff: args.diff || cli.diff,
                diff_rev: args.diff_rev.or(cli.diff_rev),
            };
            run_code(&options, &config, cli.format)
        }
        DomainCommand::Command(args) => {
            let config = load(load_command_config, &cli.config_overrides)?;
            let Some(command) = args.cmd else {
                unreachable!("clap requires --cmd unless a discovery flag is given");
            };
            run_command(&command, &config, cli.format)
        }
        DomainCommand::Graph(args) => {
            let config = load(load_config, &cli.config_overrides)?;
            run_graph(args, &config)
        }
    }
}

/// Loads the configuration with `loader` and prints its warnings to stderr.
fn load<E: std::fmt::Display>(
    loader: impl FnOnce(&ConfigOverrides) -> Result<Config, E>,
    overrides: &ConfigOverrides,
) -> anyhow::Result<Config> {
    let config = loader(overrides)
        .map_err(|error| anyhow::anyhow!("Failed to load configuration: {error}"))?;
    for warning in &config.warnings {
        eprintln!("Warning: {warning}");
    }
    Ok(config)
}

/// Reads the hook payload from stdin and prints the response, failing open (no output) on any
/// error so a malformed payload never blocks the agent.
fn run_hook<E: std::fmt::Display>(handle: impl FnOnce(&str) -> Result<String, E>) {
    let input = match std::io::read_to_string(std::io::stdin()) {
        Ok(input) => input,
        Err(error) => {
            eprintln!("[omnicoach hook] failed to read stdin: {error}");
            return;
        }
    };
    match handle(&input) {
        Ok(output) if !output.is_empty() => println!("{output}"),
        Ok(_) => {}
        Err(error) => eprintln!("[omnicoach hook] ignoring invalid hook input: {error}"),
    }
}

fn run_code(options: &LintOptions, config: &Config, format: OutputFormat) -> anyhow::Result<()> {
    let report = run_code_lint(options, config)?;

    if !report.diagnostics.is_empty() {
        print_diagnostics(&report.diagnostics, format)?;
        if format == OutputFormat::Plain {
            println!(
                "{}",
                code_report_footer(env!("CARGO_BIN_NAME"), &report.diagnostics)
            );
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
    config_overrides: ConfigOverrides,

    #[command(flatten)]
    discovery: DiscoveryArgs,
}

#[derive(Subcommand, Debug)]
enum DomainCommand {
    /// Inspect source files and directories
    Code(CodeArgs),
    /// Validate an intercepted shell command string
    Command(CommandArgs),
    /// [ALPHA — experimental, under active investigation & design; format may change at any time]
    /// Generate a multi-level abstraction graph for files or directories
    Graph(GraphArgs),
    /// Handle a coding agent's lifecycle hook event from stdin
    Hook(HookArgs),
}

#[derive(Args, Debug)]
struct HookArgs {
    #[command(subcommand)]
    agent: HookAgent,
}

/// Agent whose hook protocol the payload on stdin follows.
#[derive(Subcommand, Debug)]
enum HookAgent {
    /// Antigravity (`hooks.json`)
    Antigravity {
        /// Lifecycle event that invoked the hook
        #[arg(value_enum)]
        event: AntigravityEvent,
    },
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

#[derive(Args, Debug)]
#[command(args_conflicts_with_subcommands = true)]
struct GraphArgs {
    #[command(subcommand)]
    query: Option<GraphQueryCommand>,

    /// Paths to inspect
    #[arg(default_value = ".")]
    paths: Vec<PathBuf>,

    /// Hierarchy depth for project graphs (`module`, `public`, or `full`)
    #[arg(long, value_enum, default_value_t = GraphDepthArg::Full)]
    depth: GraphDepthArg,

    /// Omit language signatures and emit compact bidirectional nodes (`F2` instead of `F3`)
    #[arg(long)]
    no_signatures: bool,
}

fn run_graph(args: GraphArgs, config: &Config) -> anyhow::Result<()> {
    let output = match args.query {
        None => {
            let options = GraphOptions {
                paths: args.paths,
                depth: match args.depth {
                    GraphDepthArg::Module => ProjectDepth::ModuleOnly,
                    GraphDepthArg::Public => ProjectDepth::PublicSurface,
                    GraphDepthArg::Full => ProjectDepth::Full,
                },
                format: if args.no_signatures {
                    GraphOutputFormat::CompactBi
                } else {
                    GraphOutputFormat::AnnotatedStubs
                },
            };
            generate_graph(&options, config)?
        }
        Some(GraphQueryCommand::Find(find_args)) => run_find_query(
            &FindQueryOptions {
                pattern: find_args.pattern,
                paths: find_args.paths,
                limit: find_args.limit,
            },
            config,
        )?,
        Some(GraphQueryCommand::Slice(slice_args)) => run_slice_query(
            &SliceQueryOptions {
                symbols: slice_args.symbols,
                paths: slice_args.paths,
                hops: slice_args.hops,
                budget: slice_args.budget,
            },
            config,
        )?,
    };
    if let Err(error) = std::io::Write::write_all(&mut std::io::stdout().lock(), output.as_bytes())
        && error.kind() != std::io::ErrorKind::BrokenPipe
    {
        return Err(error.into());
    }
    Ok(())
}

#[derive(ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
enum GraphDepthArg {
    /// Component and module headers only (`Component -> Module`)
    Module,
    /// Component, module, and public surface declarations (`Component -> Module -> pub items`)
    Public,
    /// Full ownership clusters, private helpers, and type methods (`Component -> Module -> Owner -> items`)
    #[default]
    Full,
}

#[derive(Subcommand, Debug)]
enum GraphQueryCommand {
    /// Search declarations by name, path segment, or signature
    Find(FindArgs),
    /// Render a 1-hop neighborhood slice (`<-` call sites, `->` stubs, and cluster) around a symbol
    Slice(SliceArgs),
}

#[derive(Args, Debug)]
struct FindArgs {
    /// Substring or path pattern to match
    pattern: String,

    /// Paths to search
    #[arg(default_value = ".")]
    paths: Vec<PathBuf>,

    /// Maximum number of matches to return
    #[arg(long, default_value_t = 20)]
    limit: usize,
}

#[derive(Args, Debug)]
struct SliceArgs {
    /// One or more symbol names (`foo`, `Type::method`, or `module::Type::method`)
    #[arg(required = true, num_args = 1..)]
    symbols: Vec<String>,

    /// Paths to search (repeatable; defaults to `.`)
    #[arg(long = "path", value_name = "PATH")]
    paths: Vec<PathBuf>,

    /// Neighborhood expansion depth (`1` = direct callers/callees, `2..=4` = multi-hop)
    #[arg(long, default_value_t = 1)]
    hops: usize,

    /// Approximate total token budget for the slice invocation (`1 token ~= 4 bytes`)
    #[arg(long, default_value_t = 2000)]
    budget: usize,
}
