//! Command-line interface argument parsing, subcommand dispatch, and pipe-safe output.

architecture_component!(Cli);

use std::ffi::OsString;
use std::io::{self, Write as _};
use std::path::PathBuf;

use clap::error::ErrorKind as ClapErrorKind;
use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::code_lint::graph::{
    FindQueryOptions, GraphOptions, OutputFormat as GraphOutputFormat, ProjectDepth,
    SliceQueryOptions, generate_graph, run_find_query, run_slice_query,
};
use crate::code_lint::runner::{LintOptions, run_code_lint};
use crate::command_lint::runner::run_command_lint;
use crate::config::Config;
use crate::diagnostic::{Diagnostic, OutputFormat, print_diagnostics};
use crate::hook::{AntigravityEvent, run_antigravity_hook};
use crate::rule_catalog::{DiscoveryArgs, code_report_footer, explain_footer};
use crate::rule_selection::{ConfigOverrides, load_command_config, load_config};

/// Exit status returned when all checks succeed or a discovery/help flag is rendered.
pub const EXIT_SUCCESS: u8 = 0;

/// Exit status returned when one or more lint violations are reported.
pub const EXIT_VIOLATIONS: u8 = 1;

/// Exit status returned on CLI usage errors, invalid configuration, or unreadable files.
pub const EXIT_ERROR: u8 = 2;

const CLI_BIN_NAME: &str = env!("CARGO_PKG_NAME");

/// Parses `args` (including `argv[0]`) and executes the `omnicoach` CLI in-process, returning the
/// process exit code ([`EXIT_SUCCESS`], [`EXIT_VIOLATIONS`], or [`EXIT_ERROR`]) without calling
/// [`std::process::exit`].
#[must_use]
pub fn run_from_args<I, T>(args: I) -> u8
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match CliArgs::try_parse_from(args) {
        Ok(parsed) => parsed,
        Err(clap_error) => return handle_clap_error(&clap_error),
    };

    match dispatch(cli) {
        Ok(code) => code,
        Err(error) => {
            let _ = writeln!(io::stderr().lock(), "Error: {error}");
            EXIT_ERROR
        }
    }
}

fn handle_clap_error(clap_error: &clap::Error) -> u8 {
    map_clap_exit_code(clap_error.kind(), clap_error.print())
}

fn map_clap_exit_code(kind: ClapErrorKind, print_result: io::Result<()>) -> u8 {
    if let Err(io_error) = print_result
        && io_error.kind() != io::ErrorKind::BrokenPipe
    {
        return EXIT_ERROR;
    }
    match kind {
        ClapErrorKind::DisplayHelp | ClapErrorKind::DisplayVersion => EXIT_SUCCESS,
        _ => EXIT_ERROR,
    }
}

#[derive(Parser, Debug)]
#[command(
    name = CLI_BIN_NAME,
    bin_name = CLI_BIN_NAME,
    version,
    about = "Static code analysis and command workflow coach"
)]
struct CliArgs {
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

    /// Hierarchy depth (`module`, `public`, or `full`); defaults to `full` for a single file and
    /// `module` for a directory or several files
    #[arg(long, value_enum)]
    depth: Option<GraphDepthArg>,

    /// Omit language signatures and emit compact bidirectional nodes (`F2` instead of `F3`)
    #[arg(long)]
    no_signatures: bool,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum GraphDepthArg {
    /// Component and module headers only (`Component -> Module`)
    Module,
    /// Component, module, and public surface declarations (`Component -> Module -> pub items`)
    Public,
    /// Full ownership clusters, private helpers, and type methods (`Component -> Module -> Owner -> items`)
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

fn dispatch(cli: CliArgs) -> anyhow::Result<u8> {
    if let Some(output) = cli.discovery.render(&cli.config_overrides) {
        write_stdout_ignoring_broken_pipe(output?.as_bytes())?;
        return Ok(EXIT_SUCCESS);
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
                    run_hook(|input| run_antigravity_hook(event, input))?;
                }
            }
            Ok(EXIT_SUCCESS)
        }
        DomainCommand::Code(args) => {
            let config = load(load_config, &cli.config_overrides)?;
            let options = build_code_lint_options(args, cli.diff, cli.diff_rev);
            run_code(&options, &config, cli.format)
        }
        DomainCommand::Command(args) => {
            let config = load(load_command_config, &cli.config_overrides)?;
            let Some(command) = args.cmd else {
                return Err(anyhow::anyhow!(
                    "missing required `--cmd` argument for `command`"
                ));
            };
            run_command_check(&command, &config, cli.format)
        }
        DomainCommand::Graph(args) => {
            let config = load(load_config, &cli.config_overrides)?;
            run_graph(args, &config)
        }
    }
}

fn build_code_lint_options(
    args: CodeArgs,
    top_diff: bool,
    top_diff_rev: Option<String>,
) -> LintOptions {
    LintOptions {
        paths: args.paths,
        diff: args.diff || top_diff,
        diff_rev: args.diff_rev.or(top_diff_rev),
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
        let _ = writeln!(io::stderr().lock(), "Warning: {warning}");
    }
    Ok(config)
}

/// Reads the hook payload from stdin and prints the response, failing open (no output) on any
/// error so a malformed payload never blocks the agent.
fn run_hook<E: std::fmt::Display>(
    handle: impl FnOnce(&str) -> Result<String, E>,
) -> io::Result<()> {
    run_hook_io(io::stdin(), io::stdout().lock(), handle)
}

fn run_hook_io<R: io::Read, W: io::Write, E: std::fmt::Display>(
    reader: R,
    stdout: W,
    handle: impl FnOnce(&str) -> Result<String, E>,
) -> io::Result<()> {
    let input = match io::read_to_string(reader) {
        Ok(input) => input,
        Err(error) => {
            let _ = writeln!(
                io::stderr().lock(),
                "[omnicoach hook] failed to read stdin: {error}"
            );
            return Ok(());
        }
    };
    match handle(&input) {
        Ok(output) if !output.is_empty() => write_line_ignoring_broken_pipe(stdout, &output)?,
        Ok(_) => {}
        Err(error) => {
            let _ = writeln!(
                io::stderr().lock(),
                "[omnicoach hook] ignoring invalid hook input: {error}"
            );
        }
    }
    Ok(())
}

fn run_code(options: &LintOptions, config: &Config, format: OutputFormat) -> anyhow::Result<u8> {
    let report = run_code_lint(options, config)?;

    if !report.diagnostics.is_empty() {
        print_diagnostics(&report.diagnostics, format)?;
        if format == OutputFormat::Plain {
            let footer = code_report_footer(CLI_BIN_NAME, &report.diagnostics);
            write_stdout_line_ignoring_broken_pipe(&footer)?;
        }
    }
    for unreadable in &report.unreadable_files {
        let _ = writeln!(
            io::stderr().lock(),
            "Error: Failed to read file '{}': {}",
            unreadable.path.display(),
            unreadable.error
        );
    }
    if !report.unreadable_files.is_empty() {
        return Ok(EXIT_ERROR);
    }
    if !report.diagnostics.is_empty() {
        return Ok(EXIT_VIOLATIONS);
    }
    if format == OutputFormat::Plain {
        write_stdout_line_ignoring_broken_pipe("All checks passed!")?;
    }
    Ok(EXIT_SUCCESS)
}

fn run_command_check(command: &str, config: &Config, format: OutputFormat) -> anyhow::Result<u8> {
    let diagnostics = run_command_lint(command, config);
    render_command_diagnostics(&diagnostics, format)
}

fn render_command_diagnostics(
    diagnostics: &[Diagnostic],
    format: OutputFormat,
) -> anyhow::Result<u8> {
    if !diagnostics.is_empty() {
        print_diagnostics(diagnostics, format)?;
        if format == OutputFormat::Plain {
            let footer = explain_footer(CLI_BIN_NAME);
            write_stdout_line_ignoring_broken_pipe(&footer)?;
        }
        return Ok(EXIT_VIOLATIONS);
    }
    Ok(EXIT_SUCCESS)
}

fn run_graph(args: GraphArgs, config: &Config) -> anyhow::Result<u8> {
    let output = match args.query {
        None => {
            let options = GraphOptions {
                paths: args.paths,
                depth: args.depth.map(|depth| match depth {
                    GraphDepthArg::Module => ProjectDepth::ModuleOnly,
                    GraphDepthArg::Public => ProjectDepth::PublicSurface,
                    GraphDepthArg::Full => ProjectDepth::Full,
                }),
                format: if args.no_signatures {
                    GraphOutputFormat::CompactBi
                } else {
                    GraphOutputFormat::AnnotatedStubs
                },
            };
            generate_graph(&options, &config.project)?
        }
        Some(GraphQueryCommand::Find(find_args)) => run_find_query(
            &FindQueryOptions {
                pattern: find_args.pattern,
                paths: find_args.paths,
                limit: find_args.limit,
            },
            &config.project,
        )?,
        Some(GraphQueryCommand::Slice(slice_args)) => run_slice_query(
            &SliceQueryOptions {
                symbols: slice_args.symbols,
                paths: slice_args.paths,
                hops: slice_args.hops,
                budget: slice_args.budget,
            },
            &config.project,
        )?,
    };
    write_stdout_ignoring_broken_pipe(output.as_bytes())?;
    Ok(EXIT_SUCCESS)
}

fn write_stdout_line_ignoring_broken_pipe(line: &str) -> io::Result<()> {
    write_line_ignoring_broken_pipe(io::stdout().lock(), line)
}

fn write_stdout_ignoring_broken_pipe(bytes: &[u8]) -> io::Result<()> {
    write_ignoring_broken_pipe(io::stdout().lock(), bytes)
}

fn write_line_ignoring_broken_pipe<W: io::Write>(mut writer: W, line: &str) -> io::Result<()> {
    let result = writer
        .write_all(line.as_bytes())
        .and_then(|()| writer.write_all(b"\n"))
        .and_then(|()| writer.flush());
    if let Err(error) = result
        && error.kind() != io::ErrorKind::BrokenPipe
    {
        return Err(error);
    }
    Ok(())
}

fn write_ignoring_broken_pipe<W: io::Write>(mut writer: W, bytes: &[u8]) -> io::Result<()> {
    if let Err(error) = writer.write_all(bytes).and_then(|()| writer.flush())
        && error.kind() != io::ErrorKind::BrokenPipe
    {
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io;

    use std::path::PathBuf;

    use clap::error::ErrorKind as ClapErrorKind;

    use crate::diagnostic::{
        Diagnostic, LineColumn, OutputFormat, RuleName, SourceLocation, SourceSpan,
        ViolationMessage,
    };

    use super::{
        CliArgs, CodeArgs, EXIT_ERROR, EXIT_SUCCESS, EXIT_VIOLATIONS, build_code_lint_options,
        handle_clap_error, map_clap_exit_code, render_command_diagnostics, run_from_args,
        run_hook_io, write_ignoring_broken_pipe, write_line_ignoring_broken_pipe,
    };

    #[test]
    fn cli_args_preserves_omnicoach_bin_name_across_wrapper_argv0() {
        let error =
            clap::Parser::try_parse_from(["/site-packages/omnicoach/__main__.py", "--help"])
                .map(|_: CliArgs| ())
                .unwrap_err();

        let rendered = error.render().to_string();
        assert!(
            rendered.contains("Usage: omnicoach [OPTIONS] [COMMAND]"),
            "unexpected help output: {rendered}"
        );
    }

    #[rstest::rstest]
    #[case::version(&["omnicoach", "--version"], EXIT_SUCCESS)]
    #[case::help(&["omnicoach", "--help"], EXIT_SUCCESS)]
    #[case::list_rules(&["omnicoach", "--list-rules"], EXIT_SUCCESS)]
    #[case::list_tags(&["omnicoach", "--list-tags"], EXIT_SUCCESS)]
    #[case::explain(&["omnicoach", "--select", "single-letter-name", "--explain", "single-letter-name"], EXIT_SUCCESS)]
    #[case::explain_unknown(&["omnicoach", "--explain", "no-such-rule"], EXIT_ERROR)]
    #[case::command_clean(&["omnicoach", "--select", "command", "command", "--cmd", "cargo test"], EXIT_SUCCESS)]
    #[case::command_missing_cmd(&["omnicoach", "command"], EXIT_ERROR)]
    #[case::unknown_flag(&["omnicoach", "--unknown-flag"], EXIT_ERROR)]
    #[case::invalid_selector(&["omnicoach", "--select", "not-a-valid-selector", "command", "--cmd", "cargo test"], EXIT_ERROR)]
    fn run_from_args_handles_discovery_commands_and_errors(
        #[case] args: &[&str],
        #[case] expected: u8,
    ) {
        let (_dir, config_path) = empty_temp_config();
        let mut full_args: Vec<String> = vec![
            args[0].to_owned(),
            "--config".to_owned(),
            config_path.display().to_string(),
        ];
        full_args.extend(args[1..].iter().map(|arg| (*arg).to_owned()));

        assert_eq!(run_from_args(full_args), expected);
    }

    #[rstest::rstest]
    #[case::clean_plain("def compute_total() -> int:\n    return 0\n", &["--select", "single-letter-name", "code"], EXIT_SUCCESS)]
    #[case::clean_json("def compute_total() -> int:\n    return 0\n", &["--select", "single-letter-name", "--format", "json", "code"], EXIT_SUCCESS)]
    #[case::violation_plain("k = 1\n", &["--select", "single-letter-name", "code"], EXIT_VIOLATIONS)]
    #[case::violation_json("k = 1\n", &["--select", "single-letter-name", "--format", "json", "code"], EXIT_VIOLATIONS)]
    #[case::graph_default("def compute_total() -> int:\n    return 0\n", &["--select", "single-letter-name", "graph"], EXIT_SUCCESS)]
    #[case::graph_module("def compute_total() -> int:\n    return 0\n", &["--select", "single-letter-name", "graph", "--depth", "module"], EXIT_SUCCESS)]
    #[case::graph_public_compact("def compute_total() -> int:\n    return 0\n", &["--select", "single-letter-name", "graph", "--depth", "public", "--no-signatures"], EXIT_SUCCESS)]
    #[case::graph_full("def compute_total() -> int:\n    return 0\n", &["--select", "single-letter-name", "graph", "--depth", "full"], EXIT_SUCCESS)]
    #[case::graph_find("def compute_total() -> int:\n    return 0\n", &["--select", "single-letter-name", "graph", "find", "compute_total"], EXIT_SUCCESS)]
    #[case::graph_slice("def compute_total() -> int:\n    return 0\n", &["--select", "single-letter-name", "graph", "slice", "compute_total", "--path"], EXIT_SUCCESS)]
    fn run_from_args_reports_file_and_graph_outcomes(
        #[case] source: &str,
        #[case] prefix_args: &[&str],
        #[case] expected: u8,
    ) {
        let (dir, config_path) = empty_temp_config();
        let target = dir.path().join("sample.py");
        std::fs::write(&target, source).unwrap();
        let mut args: Vec<&str> = vec!["omnicoach", "--config", config_path.to_str().unwrap()];
        args.extend_from_slice(prefix_args);
        args.push(target.to_str().unwrap());

        assert_eq!(run_from_args(args), expected);
    }

    #[test]
    fn run_from_args_returns_error_on_missing_file() {
        let (dir, config_path) = empty_temp_config();
        let missing = dir.path().join("does_not_exist.py");

        assert_eq!(
            run_from_args([
                "omnicoach",
                "--config",
                config_path.to_str().unwrap(),
                "--select",
                "single-letter-name",
                "code",
                missing.to_str().unwrap()
            ]),
            EXIT_ERROR
        );
    }

    #[rstest::rstest]
    #[case::empty_plain(false, OutputFormat::Plain, EXIT_SUCCESS)]
    #[case::violation_plain(true, OutputFormat::Plain, EXIT_VIOLATIONS)]
    #[case::violation_json(true, OutputFormat::Json, EXIT_VIOLATIONS)]
    fn render_command_diagnostics_returns_expected_exit_code(
        #[case] has_violation: bool,
        #[case] format: OutputFormat,
        #[case] expected: u8,
    ) {
        let sample = [Diagnostic::new(
            RuleName("edit-of-described-commit"),
            ViolationMessage::new("Summary", "Rationale", "Suggestion"),
            SourceLocation::file_span(
                "VCS_Context",
                SourceSpan::new(0, 9),
                LineColumn { line: 1, column: 1 },
            ),
        )];
        let slice = &sample[..usize::from(has_violation)];

        assert_eq!(render_command_diagnostics(slice, format).unwrap(), expected);
    }

    #[rstest::rstest]
    #[case::broken_pipe_on_write(FailStage::Write, io::ErrorKind::BrokenPipe, true, "")]
    #[case::broken_pipe_on_flush(FailStage::Flush, io::ErrorKind::BrokenPipe, true, "hi")]
    #[case::permission_denied_on_write(
        FailStage::Write,
        io::ErrorKind::PermissionDenied,
        false,
        ""
    )]
    #[case::permission_denied_on_flush(
        FailStage::Flush,
        io::ErrorKind::PermissionDenied,
        false,
        "hi"
    )]
    fn write_ignoring_broken_pipe_filters_only_broken_pipe_and_flushes(
        #[case] stage: FailStage,
        #[case] kind: io::ErrorKind,
        #[case] expect_ok: bool,
        #[case] expected_written: &str,
    ) {
        let mut writer = RecordingWriter::new(stage, kind);
        let result = write_ignoring_broken_pipe(&mut writer, b"hi");

        assert_eq!(result.is_ok(), expect_ok);
        assert_eq!(String::from_utf8_lossy(&writer.buf), expected_written);
        assert!(stage == FailStage::Write || writer.flushed);
    }

    #[rstest::rstest]
    #[case::broken_pipe_on_write(FailStage::Write, io::ErrorKind::BrokenPipe, true, "")]
    #[case::broken_pipe_on_flush(FailStage::Flush, io::ErrorKind::BrokenPipe, true, "hi\n")]
    #[case::permission_denied_on_write(
        FailStage::Write,
        io::ErrorKind::PermissionDenied,
        false,
        ""
    )]
    #[case::permission_denied_on_flush(
        FailStage::Flush,
        io::ErrorKind::PermissionDenied,
        false,
        "hi\n"
    )]
    fn write_line_ignoring_broken_pipe_appends_newline_flushes_and_filters_broken_pipe(
        #[case] stage: FailStage,
        #[case] kind: io::ErrorKind,
        #[case] expect_ok: bool,
        #[case] expected_written: &str,
    ) {
        let mut writer = RecordingWriter::new(stage, kind);
        let result = write_line_ignoring_broken_pipe(&mut writer, "hi");

        assert_eq!(result.is_ok(), expect_ok);
        assert_eq!(String::from_utf8_lossy(&writer.buf), expected_written);
        assert!(stage == FailStage::Write || writer.flushed);
    }

    #[test]
    fn handle_clap_error_maps_help_and_usage_kinds() {
        let help_error = clap::Command::new("omnicoach")
            .error(clap::error::ErrorKind::DisplayHelp, "simulated help");
        let usage_error = clap::Command::new("omnicoach").error(
            clap::error::ErrorKind::UnknownArgument,
            "simulated unknown argument",
        );

        assert_eq!(handle_clap_error(&help_error), EXIT_SUCCESS);
        assert_eq!(handle_clap_error(&usage_error), EXIT_ERROR);
    }

    #[rstest::rstest]
    #[case::help_ok(ClapErrorKind::DisplayHelp, None, EXIT_SUCCESS)]
    #[case::version_ok(ClapErrorKind::DisplayVersion, None, EXIT_SUCCESS)]
    #[case::help_broken_pipe(
        ClapErrorKind::DisplayHelp,
        Some(io::ErrorKind::BrokenPipe),
        EXIT_SUCCESS
    )]
    #[case::help_io_failure(
        ClapErrorKind::DisplayHelp,
        Some(io::ErrorKind::PermissionDenied),
        EXIT_ERROR
    )]
    #[case::usage_ok(ClapErrorKind::UnknownArgument, None, EXIT_ERROR)]
    fn map_clap_exit_code_handles_broken_pipe_and_io_errors(
        #[case] kind: ClapErrorKind,
        #[case] error_kind: Option<io::ErrorKind>,
        #[case] expected: u8,
    ) {
        let print_result = error_kind.map_or(Ok(()), |io_kind| {
            Err(io::Error::new(io_kind, "simulated print error"))
        });

        assert_eq!(map_clap_exit_code(kind, print_result), expected);
    }

    #[rstest::rstest]
    #[case::neither_diff(false, false, None, None, false, None)]
    #[case::subcommand_diff_only(true, false, Some("sub"), None, true, Some("sub"))]
    #[case::toplevel_diff_only(false, true, None, Some("top"), true, Some("top"))]
    #[case::both_diffs_subcommand_rev_wins(true, true, Some("sub"), Some("top"), true, Some("sub"))]
    fn build_code_lint_options_merges_subcommand_and_toplevel_diff_flags(
        #[case] sub_diff: bool,
        #[case] top_diff: bool,
        #[case] sub_rev: Option<&str>,
        #[case] top_rev: Option<&str>,
        #[case] expected_diff: bool,
        #[case] expected_rev: Option<&str>,
    ) {
        let code_args = CodeArgs {
            paths: vec![PathBuf::from("src")],
            diff: sub_diff,
            diff_rev: sub_rev.map(str::to_owned),
        };
        let options = build_code_lint_options(code_args, top_diff, top_rev.map(str::to_owned));

        assert_eq!(options.diff, expected_diff);
        assert_eq!(options.diff_rev.as_deref(), expected_rev);
    }

    #[rstest::rstest]
    #[case::empty_response_writes_zero_bytes(Ok(String::new()), "")]
    #[case::non_empty_response_writes_line(Ok(r#"{"decision":"deny"}"#.to_owned()), "{\"decision\":\"deny\"}\n")]
    #[case::handler_error_fails_open_writing_zero_bytes(Err("bad payload"), "")]
    fn run_hook_io_writes_only_non_empty_responses_and_fails_open(
        #[case] outcome: Result<String, &'static str>,
        #[case] expected_stdout: &str,
    ) {
        let mut stdout = Vec::new();
        run_hook_io(io::Cursor::new(b"{}"), &mut stdout, |_input| outcome).unwrap();

        assert_eq!(String::from_utf8(stdout).unwrap(), expected_stdout);
    }

    fn empty_temp_config() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join(".omnicoach.toml");
        std::fs::write(&config_path, "").unwrap();
        (dir, config_path)
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum FailStage {
        Write,
        Flush,
    }

    struct RecordingWriter {
        buf: Vec<u8>,
        flushed: bool,
        stage: FailStage,
        kind: io::ErrorKind,
    }

    impl RecordingWriter {
        const fn new(stage: FailStage, kind: io::ErrorKind) -> Self {
            Self {
                buf: Vec::new(),
                flushed: false,
                stage,
                kind,
            }
        }
    }

    impl io::Write for RecordingWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.stage == FailStage::Write {
                return Err(io::Error::new(self.kind, "simulated write failure"));
            }
            self.buf.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushed = true;
            Err(io::Error::new(self.kind, "simulated flush failure"))
        }
    }
}
