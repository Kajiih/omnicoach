//! CLI option and positional argument classification over parsed [`CommandArg`] words.

use std::collections::HashMap;
use std::iter::Peekable;
use std::slice::Iter;

use super::shell::{CommandArg, InterceptedCommand};
use crate::diagnostic::SourceSpan;

macro_rules! jj_global_options_with_values {
    ($($extra:literal,)*) => {
        &[
            $($extra,)*
            "-R",
            "--repository",
            "--at-operation",
            "--at-op",
            "--color",
            "--config",
            "--config-toml",
            "--config-file",
        ]
    };
}

const JJ_GLOBAL_OPTIONS_WITH_VALUES: &[&str] = jj_global_options_with_values!();

/// CLI schema definition for Jujutsu (`jj`) commands.
pub const JJ_CLI_SCHEMA: ProgramCliSchema = ProgramCliSchema {
    program_name: "jj",
    options_with_values: jj_global_options_with_values!(
        "-r",
        "--revision",
        "--revisions",
        "-n",
        "--limit",
        "-T",
        "--template",
        "--tool",
        "--context",
        "-m",
        "--message",
    ),
};

/// Schema defining a program's command-line interface options layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgramCliSchema {
    /// The name of the binary/program (e.g. `"jj"`, `"git"`).
    pub program_name: &'static str,
    /// Option flags that consume a value (e.g. `["-R", "--repository"]`).
    pub options_with_values: &'static [&'static str],
}

/// Parsed command-line positional arguments and option flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedArgs {
    /// Positional arguments in order (e.g. `["edit", "@-"]`), excluding options and their values.
    pub positionals: Vec<String>,
    options: HashMap<String, Vec<String>>,
}

impl ParsedArgs {
    /// Parses `arguments` according to `schema`.
    #[must_use]
    pub fn parse(arguments: &[CommandArg], schema: &ProgramCliSchema) -> Self {
        ArgParser::new(arguments, schema.options_with_values).parse()
    }

    /// Returns `true` if a boolean or key-value option flag was provided.
    #[must_use]
    pub fn has_flag(&self, flag: &str) -> bool {
        self.options.contains_key(flag)
    }

    /// Returns the first value associated with `option`, if present.
    #[must_use]
    pub fn get_option(&self, option: &str) -> Option<&str> {
        self.options
            .get(option)
            .and_then(|values| values.first())
            .map(String::as_str)
    }

    /// Returns all values associated with `option` in the order they were provided.
    #[must_use]
    pub fn option_values(&self, option: &str) -> &[String] {
        self.options.get(option).map_or(&[], Vec::as_slice)
    }

    /// Returns `true` if the positional arguments start with `sequence`.
    #[must_use]
    pub fn has_subcommand_sequence(&self, sequence: &[&str]) -> bool {
        self.positionals
            .get(..sequence.len())
            .is_some_and(|prefix| prefix == sequence)
    }
}

impl InterceptedCommand {
    /// Classifies `self.arguments` into positional arguments and option key-values using `schema`.
    #[must_use]
    pub fn parse_args(&self, schema: &ProgramCliSchema) -> ParsedArgs {
        ParsedArgs::parse(&self.arguments, schema)
    }
}

/// Leading global option arguments placed before a CLI subcommand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeadingGlobalFlags {
    /// The first leading flag token (e.g. `"--no-pager"` or `"-R"`).
    pub first_flag: String,
    /// The subcommand name that followed the leading flags (e.g. `"log"` or `"test"`).
    pub subcommand: String,
    flags_span: SourceSpan,
    subcommand_span: SourceSpan,
}

impl LeadingGlobalFlags {
    /// Rewrites `command` in `source` so the subcommand token appears immediately before the
    /// leading global flags, preserving any leading program/toolchain prefix and interspersed
    /// redirections.
    #[must_use]
    pub fn reorder_subcommand_first(&self, command: &InterceptedCommand, source: &str) -> String {
        let prefix_slice = &source[command.span.start..self.flags_span.start];
        let flags_slice = &source[self.flags_span.start..self.flags_span.end];
        let between_slice = source[self.flags_span.end..self.subcommand_span.start].trim();
        let subcommand_slice = &source[self.subcommand_span.start..self.subcommand_span.end];
        let suffix_slice = &source[self.subcommand_span.end..command.span.end];

        if between_slice.is_empty() {
            format!("{prefix_slice}{subcommand_slice} {flags_slice}{suffix_slice}")
        } else {
            format!("{prefix_slice}{between_slice} {subcommand_slice} {flags_slice}{suffix_slice}")
        }
    }
}

/// Extracts leading global flags placed before a subcommand on `clap`-style subcommand CLIs
/// (`jj`, `cargo`, `uv`).
#[must_use]
pub fn extract_leading_global_flags(command: &InterceptedCommand) -> Option<LeadingGlobalFlags> {
    let schema = SUBCOMMAND_CLI_SCHEMAS
        .iter()
        .find(|candidate| candidate.program_name == command.program_base_name())?;
    let arguments = &command.arguments;
    let prefix_start = usize::from(
        schema.allows_plus_toolchain
            && arguments.first().is_some_and(|arg| {
                arg.text
                    .strip_prefix('+')
                    .is_some_and(|rest| !rest.is_empty())
            }),
    );
    let subcommand_index = find_subcommand_after_leading_flags(arguments, prefix_start, schema)?;
    if subcommand_index == prefix_start {
        return None;
    }

    let first_flag_arg = &arguments[prefix_start];
    let last_flag_arg = &arguments[subcommand_index - 1];
    let subcommand_arg = &arguments[subcommand_index];

    Some(LeadingGlobalFlags {
        first_flag: first_flag_arg.text.clone(),
        subcommand: subcommand_arg.text.clone(),
        flags_span: SourceSpan::new(first_flag_arg.span.start, last_flag_arg.span.end),
        subcommand_span: subcommand_arg.span,
    })
}

struct ArgParser<'a> {
    arguments_iter: Peekable<Iter<'a, CommandArg>>,
    options_with_values: &'a [&'a str],
    positionals: Vec<String>,
    options: HashMap<String, Vec<String>>,
}

impl<'a> ArgParser<'a> {
    fn new(arguments: &'a [CommandArg], options_with_values: &'a [&'a str]) -> Self {
        Self {
            arguments_iter: arguments.iter().peekable(),
            options_with_values,
            positionals: Vec::new(),
            options: HashMap::new(),
        }
    }

    fn parse(mut self) -> ParsedArgs {
        let mut options_terminated = false;

        while let Some(argument) = self.arguments_iter.next() {
            let text = argument.text.as_str();
            if options_terminated {
                self.positionals.push(argument.text.clone());
                continue;
            }

            if text == "--" {
                options_terminated = true;
                continue;
            }

            if text.starts_with("--") {
                self.parse_long_option(text);
            } else if text.starts_with('-') && text.len() > 1 {
                self.parse_short_option_cluster(text);
            } else {
                self.positionals.push(argument.text.clone());
            }
        }

        ParsedArgs {
            positionals: self.positionals,
            options: self.options,
        }
    }

    fn parse_long_option(&mut self, argument: &str) {
        if let Some((flag, option_value)) = argument.split_once('=') {
            if self.options_with_values.contains(&flag) {
                self.options
                    .entry(flag.to_string())
                    .or_default()
                    .push(option_value.to_string());
            } else {
                self.options.entry(argument.to_string()).or_default();
            }
        } else if self.options_with_values.contains(&argument)
            && self
                .arguments_iter
                .peek()
                .is_some_and(|next_arg| next_arg.text != "--")
        {
            if let Some(option_value) = self.arguments_iter.next() {
                self.options
                    .entry(argument.to_string())
                    .or_default()
                    .push(option_value.text.clone());
            }
        } else {
            self.options.entry(argument.to_string()).or_default();
        }
    }

    fn parse_short_option_cluster(&mut self, argument: &str) {
        let mut char_indices = argument.char_indices().skip(1).peekable();

        while let Some((byte_offset, character)) = char_indices.next() {
            let flag = format!("-{character}");
            let is_option_with_value = self.options_with_values.contains(&flag.as_str());

            if is_option_with_value {
                if char_indices.peek().is_some() {
                    let rest = &argument[byte_offset + character.len_utf8()..];
                    let attached_value = rest.strip_prefix('=').unwrap_or(rest).to_string();
                    self.options.entry(flag).or_default().push(attached_value);
                    break;
                }
                let next_is_value = self
                    .arguments_iter
                    .peek()
                    .is_some_and(|next_arg| next_arg.text != "--");
                if next_is_value && let Some(option_value) = self.arguments_iter.next() {
                    self.options
                        .entry(flag)
                        .or_default()
                        .push(option_value.text.clone());
                    break;
                }
            }
            self.options.entry(flag).or_default();
        }
    }
}

/// Leading-flag layout for a `clap`-style subcommand CLI where global flags (`Arg::global(true)`)
/// are accepted both before and after `<SUBCOMMAND>`.
///
/// On a valid `clap` subcommand invocation, only global flags and standalone top-level flags are
/// accepted before `<SUBCOMMAND>`, so every non-standalone flag preceding the subcommand is a
/// global flag that can move after it. Only value-consuming global options need to be listed in
/// `global_options_with_values` so their value token is not mistaken for the subcommand.
struct SubcommandCliSchema {
    program_name: &'static str,
    global_options_with_values: &'static [&'static str],
    standalone_flags: &'static [&'static str],
    allows_plus_toolchain: bool,
}

/// Registered `clap`-based subcommand CLIs (`jj`, `cargo`, `uv`) whose global flags can be moved
/// after the subcommand. Non-`clap` subcommand CLIs such as `git` (`git -C <dir> status`,
/// `git --no-pager log`) reject top-level flags after the subcommand and must not be added here.
const SUBCOMMAND_CLI_SCHEMAS: &[SubcommandCliSchema] = &[
    SubcommandCliSchema {
        program_name: "jj",
        global_options_with_values: JJ_GLOBAL_OPTIONS_WITH_VALUES,
        standalone_flags: &["-h", "--help", "-V", "--version"],
        allows_plus_toolchain: false,
    },
    SubcommandCliSchema {
        program_name: "cargo",
        global_options_with_values: &["-C", "--color", "--config", "-Z"],
        standalone_flags: &["-h", "--help", "-V", "--version", "--list", "--explain"],
        allows_plus_toolchain: true,
    },
    SubcommandCliSchema {
        program_name: "uv",
        global_options_with_values: &[
            "-C",
            "--color",
            "--directory",
            "--project",
            "--config-file",
            "--cache-dir",
            "--python",
            "--python-preference",
            "--python-fetch",
            "--allow-insecure-host",
        ],
        standalone_flags: &["-h", "--help", "-V", "--version"],
        allows_plus_toolchain: false,
    },
];

fn find_subcommand_after_leading_flags(
    arguments: &[CommandArg],
    prefix_start: usize,
    schema: &SubcommandCliSchema,
) -> Option<usize> {
    let mut iter = arguments.iter().enumerate().skip(prefix_start);
    while let Some((index, argument)) = iter.next() {
        let text = argument.text.as_str();
        if text == "--" || text == "-" {
            return None;
        }
        let flag_name = text.split_once('=').map_or(text, |(name, _)| name);
        if schema.standalone_flags.contains(&flag_name) {
            return None;
        }

        if text.starts_with("--") {
            if schema.global_options_with_values.contains(&text) && iter.next()?.1.text == "--" {
                return None;
            }
            continue;
        }

        if text.starts_with('-') {
            match classify_short_cluster(text, schema) {
                ShortClusterStep::StandaloneExit => return None,
                ShortClusterStep::SingleToken => {}
                ShortClusterStep::ConsumesNextValue => {
                    if iter.next()?.1.text == "--" {
                        return None;
                    }
                }
            }
            continue;
        }

        return (!argument.has_expansion
            && !text.is_empty()
            && text
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'))
        .then_some(index);
    }
    None
}

enum ShortClusterStep {
    SingleToken,
    ConsumesNextValue,
    StandaloneExit,
}

fn classify_short_cluster(argument: &str, schema: &SubcommandCliSchema) -> ShortClusterStep {
    let mut chars = argument.chars().skip(1).peekable();
    while let Some(character) = chars.next() {
        let flag = format!("-{character}");
        if schema.standalone_flags.contains(&flag.as_str()) {
            return ShortClusterStep::StandaloneExit;
        }
        if schema.global_options_with_values.contains(&flag.as_str()) {
            return if chars.peek().is_some() {
                ShortClusterStep::SingleToken
            } else {
                ShortClusterStep::ConsumesNextValue
            };
        }
    }
    ShortClusterStep::SingleToken
}

#[cfg(test)]
mod tests {
    use super::super::shell::ParsedCommand;
    use super::*;

    #[test]
    fn test_parse_args_basic() {
        let cmd = InterceptedCommand::parse_all("jj edit -R . --config myconf @-").remove(0);
        let schema = ProgramCliSchema {
            program_name: "jj",
            options_with_values: &["-R", "--repository", "--config"],
        };
        let args = cmd.parse_args(&schema);
        assert_eq!(args.positionals, vec!["edit", "@-"]);
        assert_eq!(args.get_option("-R"), Some("."));
        assert_eq!(args.get_option("--config"), Some("myconf"));
        assert!(args.has_subcommand_sequence(&["edit"]));
    }

    #[test]
    fn test_parse_args_boolean_long_flag_and_stdin_dash() {
        let cmd = InterceptedCommand::parse_all("jj --no-pager edit -").remove(0);
        let schema = ProgramCliSchema {
            program_name: "jj",
            options_with_values: &["-R"],
        };
        let args = cmd.parse_args(&schema);
        assert_eq!(args.positionals, vec!["edit", "-"]);
        assert!(args.has_flag("--no-pager"));
    }

    #[test]
    fn test_parse_args_equals_syntax() {
        let cmd = InterceptedCommand::parse_all("jj edit --repository=. @-").remove(0);
        let schema = ProgramCliSchema {
            program_name: "jj",
            options_with_values: &["-R", "--repository"],
        };
        let args = cmd.parse_args(&schema);
        assert_eq!(args.positionals, vec!["edit", "@-"]);
        assert_eq!(args.get_option("--repository"), Some("."));
    }

    #[test]
    fn test_parse_args_posix_clustering() {
        let cmd = InterceptedCommand::parse_all("jj edit -am").remove(0);
        let schema = ProgramCliSchema {
            program_name: "jj",
            options_with_values: &[],
        };
        let args = cmd.parse_args(&schema);
        assert_eq!(args.positionals, vec!["edit"]);
        assert!(args.has_flag("-a"));
        assert!(args.has_flag("-m"));
    }

    #[test]
    fn test_parse_args_posix_glued_value_and_short_equals() {
        let cmd = InterceptedCommand::parse_all("jj edit -R. -r=kpqxywon").remove(0);
        let args = cmd.parse_args(&JJ_CLI_SCHEMA);
        assert_eq!(args.positionals, vec!["edit"]);
        assert_eq!(args.get_option("-R"), Some("."));
        assert_eq!(args.get_option("-r"), Some("kpqxywon"));
    }

    #[test]
    fn test_parse_args_posix_cluster_with_value() {
        let cmd = InterceptedCommand::parse_all("git commit -am msg").remove(0);
        let schema = ProgramCliSchema {
            program_name: "git",
            options_with_values: &["-m"],
        };
        let args = cmd.parse_args(&schema);
        assert_eq!(args.positionals, vec!["commit"]);
        assert!(args.has_flag("-a"));
        assert_eq!(args.get_option("-m"), Some("msg"));
    }

    #[test]
    fn test_parse_args_posix_terminator() {
        let cmd = InterceptedCommand::parse_all("git log -- -R").remove(0);
        let schema = ProgramCliSchema {
            program_name: "git",
            options_with_values: &["-R"],
        };
        let args_term = cmd.parse_args(&schema);
        assert_eq!(args_term.positionals, vec!["log", "-R"]);
        assert!(!args_term.has_flag("-R"));
        assert_eq!(args_term.get_option("-R"), None);
    }

    #[test]
    fn test_parse_args_consumes_dash_prefixed_option_values() {
        let cmd =
            InterceptedCommand::parse_all("jj describe -m -wip --message -second @-").remove(0);
        let schema = ProgramCliSchema {
            program_name: "jj",
            options_with_values: &["-m", "--message"],
        };
        let args = cmd.parse_args(&schema);
        assert_eq!(args.positionals, vec!["describe", "@-"]);
        assert_eq!(args.get_option("-m"), Some("-wip"));
        assert_eq!(args.get_option("--message"), Some("-second"));
    }

    #[test]
    fn test_option_values_returns_all_occurrences_in_order() {
        let parsed = ParsedCommand::new("jj log -r a --revision b -rc --revisions=d -r=e");
        let cmd = &parsed.commands()[0];
        let args = cmd.parse_args(&JJ_CLI_SCHEMA);
        assert_eq!(args.option_values("-r"), &["a", "c", "e"]);
        assert_eq!(args.option_values("--revision"), &["b"]);
        assert_eq!(args.option_values("--revisions"), &["d"]);
        assert_eq!(args.option_values("--limit"), &[] as &[String]);
    }

    #[rstest::rstest]
    #[case::jj_no_pager(
        "jj --no-pager log -n 5",
        Some(("--no-pager", "log", "jj log --no-pager -n 5"))
    )]
    #[case::jj_repo_with_value(
        "jj -R /tmp/ws status",
        Some(("-R", "status", "jj status -R /tmp/ws"))
    )]
    #[case::jj_glued_repo_and_config(
        "jj -R. --config ui.paginate=never diff",
        Some(("-R.", "diff", "jj diff -R. --config ui.paginate=never"))
    )]
    #[case::jj_interspersed_redirect(
        "jj --no-pager 2>/dev/null log -n 5",
        Some(("--no-pager", "log", "jj 2>/dev/null log --no-pager -n 5"))
    )]
    #[case::cargo_locked(
        "cargo --locked test --lib",
        Some(("--locked", "test", "cargo test --locked --lib"))
    )]
    #[case::cargo_with_plus_toolchain_and_flag(
        "cargo +nightly --locked test",
        Some(("--locked", "test", "cargo +nightly test --locked"))
    )]
    #[case::cargo_short_cluster_and_value(
        "cargo -vC /tmp/crate check",
        Some(("-vC", "check", "cargo check -vC /tmp/crate"))
    )]
    #[case::uv_directory_option(
        "uv --directory /tmp/app run pytest",
        Some(("--directory", "run", "uv run --directory /tmp/app pytest"))
    )]
    #[case::uv_short_directory_option(
        "uv -C /tmp/app run pytest",
        Some(("-C", "run", "uv run -C /tmp/app pytest"))
    )]
    #[case::uv_allow_insecure_host_option(
        "uv --allow-insecure-host example.com run pytest",
        Some((
            "--allow-insecure-host",
            "run",
            "uv run --allow-insecure-host example.com pytest"
        ))
    )]
    #[case::cargo_hyphenated_subcommand(
        "cargo --locked verify-project",
        Some(("--locked", "verify-project", "cargo verify-project --locked"))
    )]
    #[case::subcommand_first_allowed("jj log --no-pager -n 5", None)]
    #[case::cargo_plus_toolchain_then_subcommand_allowed("cargo +nightly test --locked", None)]
    #[case::cargo_bare_plus_ignored("cargo + --locked test", None)]
    #[case::jj_plus_arg_not_treated_as_toolchain("jj +nightly --no-pager log", None)]
    #[case::cargo_misplaced_plus_toolchain_after_flag_ignored("cargo --locked +nightly test", None)]
    #[case::git_top_level_flag_ignored("git -C /tmp/repo status", None)]
    #[case::standalone_version_allowed("jj --version", None)]
    #[case::standalone_help_before_subcommand_allowed("jj --help log", None)]
    #[case::standalone_help_with_leading_flag_allowed("jj --no-pager --help", None)]
    #[case::standalone_short_version_cluster_before_subcommand_allowed("cargo -qV test", None)]
    #[case::cargo_explain_allowed("cargo --explain E0308", None)]
    #[case::cargo_explain_equals_allowed("cargo --color=never --explain=E0308", None)]
    #[case::cargo_z_script_spaced_allowed("cargo +nightly -Z script foo.rs", None)]
    #[case::cargo_z_script_glued_allowed("cargo +nightly -Zscript foo.rs", None)]
    #[case::empty_quoted_positional_allowed("jj --no-pager ''", None)]
    #[case::flags_only_no_subcommand_allowed("jj -R /tmp/ws --no-pager", None)]
    #[case::dangling_option_value_at_end_allowed("jj -R", None)]
    #[case::dangling_long_option_before_double_dash_allowed("jj --repository -- log", None)]
    #[case::dangling_short_option_before_double_dash_allowed("jj -R -- log", None)]
    #[case::double_dash_before_positional_allowed("jj --no-pager -- log", None)]
    #[case::single_dash_positional_allowed("jj --no-pager -", None)]
    #[case::variable_expansion_after_flag_allowed("jj --no-pager $ARGS", None)]
    fn test_extract_leading_global_flags(
        #[case] script: &str,
        #[case] expected: Option<(&str, &str, &str)>,
    ) {
        let parsed = ParsedCommand::new(script);
        let cmd = &parsed.commands()[0];
        let actual = extract_leading_global_flags(cmd);
        let reordered = actual
            .as_ref()
            .map(|found| found.reorder_subcommand_first(cmd, parsed.source()));
        let actual_tuple = actual
            .as_ref()
            .zip(reordered.as_deref())
            .map(|(found, replacement)| {
                (
                    found.first_flag.as_str(),
                    found.subcommand.as_str(),
                    replacement,
                )
            });
        assert_eq!(actual_tuple, expected, "script: `{script}`");
    }
}
