//! Zero-copy declarative CLI option and positional classification over [`CommandArg`] slices.

use std::collections::HashMap;
use std::iter::Peekable;
use std::slice::Iter;

use super::model::{CommandArg, InterceptedCommand};
use crate::diagnostic::SourceSpan;

const OPTION_TERMINATOR: &str = "--";

/// Compile-time declarative option layout active before a CLI subcommand token.
///
/// Pre-subcommand scanning always stops at the first positional operand so options after
/// `<SUBCOMMAND>` are never consumed as leading global flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::command_lint) struct PreSubcommandSpec {
    /// Single-character short options that consume a required value (for example `"R"`).
    pub(in crate::command_lint) short_values: &'static str,
    /// Long option names (without leading `--`) that consume a required value (for example
    /// `&["repository", "config"]`).
    pub(in crate::command_lint) long_values: &'static [&'static str],
    /// Single-character short flags (such as `"hV"`) whose presence aborts classification (`None`).
    pub(in crate::command_lint) abort_shorts: &'static str,
    /// Long option names without leading `--` (such as `&["help", "version"]`) whose presence
    /// aborts classification (`None`).
    pub(in crate::command_lint) abort_longs: &'static [&'static str],
}

impl PreSubcommandSpec {
    /// Baseline empty `PreSubcommandSpec` for `const` struct-update literals.
    pub(in crate::command_lint) const DEFAULT: Self = Self {
        short_values: "",
        long_values: &[],
        abort_shorts: "",
        abort_longs: &[],
    };

    /// Classifies leading options before the first positional without cloning strings, returning
    /// `None` if an `abort_shorts` or `abort_longs` flag is encountered before option scanning ends.
    #[must_use]
    pub(in crate::command_lint) fn parse<'a>(
        &self,
        arguments: &'a [CommandArg],
    ) -> Option<ParsedCli<'a>> {
        let mut positionals = Vec::new();
        let mut double_dash_positional_index = None;
        let mut iter = arguments.iter();

        while let Some(argument) = iter.next() {
            let static_prefix = argument.static_prefix();
            let full_text = argument.text.as_str();

            if !argument.is_dynamic() && full_text == OPTION_TERMINATOR {
                double_dash_positional_index = Some(positionals.len());
                positionals.extend(iter.by_ref());
                break;
            }

            if let Some(after_dashes) = static_prefix.strip_prefix(OPTION_TERMINATOR) {
                if after_dashes.is_empty()
                    || after_dashes.starts_with('=')
                    || self.parse_long_option(argument, after_dashes, &mut iter)?
                        == CliOptionOutcome::StopAsPositional
                {
                    positionals.push(argument);
                    positionals.extend(iter.by_ref());
                    break;
                }
                continue;
            }

            if let Some(short_prefix) = static_prefix.strip_prefix('-')
                && (!short_prefix.is_empty() || argument.is_dynamic())
            {
                if self.parse_short_cluster(argument, short_prefix, &mut iter)?
                    == CliOptionOutcome::StopAsPositional
                {
                    positionals.push(argument);
                    positionals.extend(iter.by_ref());
                    break;
                }
                continue;
            }

            positionals.push(argument);
            positionals.extend(iter.by_ref());
            break;
        }

        Some(ParsedCli {
            positionals,
            double_dash_positional_index,
        })
    }

    fn parse_long_option<'a>(
        &self,
        argument: &'a CommandArg,
        after_dashes: &'a str,
        iter: &mut Iter<'a, CommandArg>,
    ) -> Option<CliOptionOutcome> {
        if let Some((long_name, _)) = after_dashes.split_once('=') {
            if self.abort_longs.contains(&long_name) {
                return None;
            }
            return Some(CliOptionOutcome::Continue);
        }

        if argument.is_dynamic() {
            return Some(CliOptionOutcome::StopAsPositional);
        }

        if self.abort_longs.contains(&after_dashes) {
            return None;
        }

        if self.long_values.contains(&after_dashes) {
            skip_next_value_token(iter);
        }
        Some(CliOptionOutcome::Continue)
    }

    fn parse_short_cluster<'a>(
        &self,
        argument: &'a CommandArg,
        short_prefix: &'a str,
        iter: &mut Iter<'a, CommandArg>,
    ) -> Option<CliOptionOutcome> {
        let full_text = argument.text.as_str();
        for (rel_offset, character) in short_prefix.char_indices() {
            if self.abort_shorts.contains(character) {
                return None;
            }
            if self.short_values.contains(character) {
                let byte_after_char = 1 + rel_offset + character.len_utf8();
                if byte_after_char >= full_text.len() {
                    skip_next_value_token(iter);
                }
                return Some(CliOptionOutcome::Continue);
            }
        }
        if argument.is_dynamic() {
            return Some(CliOptionOutcome::StopAsPositional);
        }
        Some(CliOptionOutcome::Continue)
    }
}

/// Borrowed classification of positional operands produced by [`PreSubcommandSpec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::command_lint) struct ParsedCli<'a> {
    positionals: Vec<&'a CommandArg>,
    double_dash_positional_index: Option<usize>,
}

impl<'a> ParsedCli<'a> {
    /// Positional arguments in command-line order, excluding recognized options and their values.
    #[must_use]
    pub(in crate::command_lint) fn positionals(&self) -> &[&'a CommandArg] {
        &self.positionals
    }

    const fn has_double_dash_before_first_positional(&self) -> bool {
        matches!(self.double_dash_positional_index, Some(0))
    }
}

/// Schema defining a program's command-line interface options layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::command_lint) struct ProgramCliSchema {
    /// Option flags that consume a value (e.g. `["-R", "--repository"]`).
    pub(in crate::command_lint) options_with_values: &'static [&'static str],
}

/// Leading-flag layout for a `clap`-style subcommand CLI where global flags (`Arg::global(true)`)
/// are accepted both before and after `<SUBCOMMAND>`.
///
/// On a valid `clap` subcommand invocation, only global flags and standalone top-level flags are
/// accepted before `<SUBCOMMAND>`, so every non-standalone flag preceding the subcommand is a
/// global flag that can move after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::command_lint) struct SubcommandCliSchema {
    /// Binary base name matched against [`InterceptedCommand::program_base_name`].
    pub(in crate::command_lint) program_name: &'static str,
    /// Option specification active before `<SUBCOMMAND>`.
    pub(in crate::command_lint) pre_subcommand_spec: PreSubcommandSpec,
    /// Whether an initial `+toolchain` selector token (such as `cargo +nightly`) may precede flags.
    pub(in crate::command_lint) allows_plus_toolchain: bool,
}

impl SubcommandCliSchema {
    /// Finds the index of the first subcommand token at or after `start_index`, skipping leading
    /// global options defined in `self.pre_subcommand_spec`. Returns `None` on standalone abort
    /// flags, `--` before the subcommand, or no valid subcommand token.
    #[must_use]
    pub(in crate::command_lint) fn find_subcommand_index(
        &self,
        arguments: &[CommandArg],
        start_index: usize,
    ) -> Option<usize> {
        let slice = arguments.get(start_index..)?;
        let parsed = self.pre_subcommand_spec.parse(slice)?;
        if parsed.has_double_dash_before_first_positional() {
            return None;
        }
        let first_positional = *parsed.positionals().first()?;
        if !is_valid_subcommand_token(first_positional) {
            return None;
        }
        slice
            .iter()
            .position(|candidate| std::ptr::eq(candidate, first_positional))
            .map(|offset| start_index + offset)
    }
}

/// Result of looking up the last CLI occurrence of one or more option flag aliases.
// TODO: Isn't there a more ricr/standar way to do things like memoization?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::command_lint) enum LastOptionValue<'a> {
    /// None of the queried flag aliases appeared in the command arguments.
    Absent,
    /// A queried flag appeared without a value operand (for example, dangling before `--` or EOF).
    MissingValue,
    /// The last matching flag carried a value token and its shell-expansion marker.
    Value {
        /// Unquoted option value text.
        text: &'a str,
        /// `true` if the value token contained shell expansions (`$VAR`, `$(...)`, `~`).
        has_expansion: bool,
    },
}

/// Parsed command-line positional arguments and option flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::command_lint) struct ParsedArgs {
    /// Positional arguments in order (e.g. `["edit", "@-"]`), excluding options and their values.
    pub(in crate::command_lint) positionals: Vec<String>,
    options: HashMap<String, Vec<String>>,
    occurrences: Vec<OptionOccurrence>,
}

impl ParsedArgs {
    /// Parses `arguments` according to `schema`.
    #[must_use]
    pub(in crate::command_lint) fn parse(
        arguments: &[CommandArg],
        schema: &ProgramCliSchema,
    ) -> Self {
        ArgParser::new(arguments, schema.options_with_values).parse()
    }

    /// Returns `true` if a boolean or key-value option flag was provided.
    #[must_use]
    pub(in crate::command_lint) fn has_flag(&self, flag: &str) -> bool {
        self.options.contains_key(flag)
    }

    /// Returns the first value associated with `option`, if present.
    #[must_use]
    pub(in crate::command_lint) fn get_option(&self, option: &str) -> Option<&str> {
        self.options
            .get(option)
            .and_then(|values| values.first())
            .map(String::as_str)
    }

    /// Returns all values associated with `option` in the order they were provided.
    #[must_use]
    pub(in crate::command_lint) fn option_values(&self, option: &str) -> &[String] {
        self.options.get(option).map_or(&[], Vec::as_slice)
    }

    /// Returns the last occurrence across `flags` in command-line order, preserving missing-value
    /// and shell-expansion state.
    #[must_use]
    pub(in crate::command_lint) fn last_option_value(&self, flags: &[&str]) -> LastOptionValue<'_> {
        let Some(occurrence) = self
            .occurrences
            .iter()
            .rev()
            .find(|entry| flags.contains(&entry.flag.as_str()))
        else {
            return LastOptionValue::Absent;
        };
        occurrence
            .value
            .as_ref()
            .map_or(LastOptionValue::MissingValue, |value| {
                LastOptionValue::Value {
                    text: &value.text,
                    has_expansion: value.has_expansion,
                }
            })
    }
}

impl InterceptedCommand {
    /// Classifies `self.arguments` into positional arguments and option key-values using `schema`.
    #[must_use]
    pub(in crate::command_lint) fn parse_args(&self, schema: &ProgramCliSchema) -> ParsedArgs {
        ParsedArgs::parse(&self.arguments, schema)
    }
}

/// Leading global option arguments placed before a CLI subcommand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::command_lint) struct LeadingGlobalFlags {
    /// The first leading flag token (e.g. `"--no-pager"` or `"-R"`).
    pub(in crate::command_lint) first_flag: String,
    /// The subcommand name that followed the leading flags (e.g. `"log"` or `"test"`).
    pub(in crate::command_lint) subcommand: String,
    flags_span: SourceSpan,
    subcommand_span: SourceSpan,
}

impl LeadingGlobalFlags {
    /// Rewrites `command` in `source` so the subcommand token appears immediately before the
    /// leading global flags, preserving any leading program/toolchain prefix and interspersed
    /// redirections.
    #[must_use]
    pub(in crate::command_lint) fn reorder_subcommand_first(
        &self,
        command: &InterceptedCommand,
        source: &str,
    ) -> String {
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
/// matched against `schemas`.
#[must_use]
pub(in crate::command_lint) fn extract_leading_global_flags(
    command: &InterceptedCommand,
    schemas: &[SubcommandCliSchema],
) -> Option<LeadingGlobalFlags> {
    let schema = schemas
        .iter()
        .find(|candidate| candidate.program_name == command.program_base_name())?;
    let arguments = &command.arguments;
    let prefix_start = usize::from(
        schema.allows_plus_toolchain
            && arguments.first().is_some_and(|arg| {
                !arg.is_dynamic()
                    && arg
                        .text
                        .strip_prefix('+')
                        .is_some_and(|rest| !rest.is_empty())
            }),
    );
    let subcommand_index = schema.find_subcommand_index(arguments, prefix_start)?;
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
    occurrences: Vec<OptionOccurrence>,
}

impl<'a> ArgParser<'a> {
    fn new(arguments: &'a [CommandArg], options_with_values: &'a [&'a str]) -> Self {
        Self {
            arguments_iter: arguments.iter().peekable(),
            options_with_values,
            positionals: Vec::new(),
            options: HashMap::new(),
            occurrences: Vec::new(),
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
                self.parse_long_option(argument);
            } else if text.starts_with('-') && text.len() > 1 {
                self.parse_short_option_cluster(argument);
            } else {
                self.positionals.push(argument.text.clone());
            }
        }

        ParsedArgs {
            positionals: self.positionals,
            options: self.options,
            occurrences: self.occurrences,
        }
    }

    fn parse_long_option(&mut self, argument: &CommandArg) {
        let text = argument.text.as_str();
        if let Some((flag, option_value)) = text.split_once('=') {
            if self.options_with_values.contains(&flag) {
                self.record_option_value(
                    flag.to_string(),
                    option_value.to_string(),
                    argument.has_expansion,
                );
            } else {
                self.record_flag(text.to_string());
            }
        } else if self.options_with_values.contains(&text)
            && self
                .arguments_iter
                .peek()
                .is_some_and(|next_arg| next_arg.text != "--")
        {
            if let Some(option_value) = self.arguments_iter.next() {
                self.record_option_value(
                    text.to_string(),
                    option_value.text.clone(),
                    option_value.has_expansion,
                );
            }
        } else {
            self.record_flag(text.to_string());
        }
    }

    fn parse_short_option_cluster(&mut self, argument: &CommandArg) {
        let text = argument.text.as_str();
        let mut char_indices = text.char_indices().skip(1).peekable();

        while let Some((byte_offset, character)) = char_indices.next() {
            let flag = format!("-{character}");
            let is_option_with_value = self.options_with_values.contains(&flag.as_str());

            if is_option_with_value {
                if char_indices.peek().is_some() {
                    let rest = &text[byte_offset + character.len_utf8()..];
                    let attached_value = rest.strip_prefix('=').unwrap_or(rest).to_string();
                    self.record_option_value(flag, attached_value, argument.has_expansion);
                    break;
                }
                let next_is_value = self
                    .arguments_iter
                    .peek()
                    .is_some_and(|next_arg| next_arg.text != "--");
                if next_is_value && let Some(option_value) = self.arguments_iter.next() {
                    self.record_option_value(
                        flag,
                        option_value.text.clone(),
                        option_value.has_expansion,
                    );
                    break;
                }
            }
            self.record_flag(flag);
        }
    }

    fn record_flag(&mut self, flag: String) {
        self.options.entry(flag.clone()).or_default();
        self.occurrences
            .push(OptionOccurrence { flag, value: None });
    }

    fn record_option_value(&mut self, flag: String, text: String, has_expansion: bool) {
        self.options
            .entry(flag.clone())
            .or_default()
            .push(text.clone());
        self.occurrences.push(OptionOccurrence {
            flag,
            value: Some(OptionValue {
                text,
                has_expansion,
            }),
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OptionOccurrence {
    flag: String,
    value: Option<OptionValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OptionValue {
    text: String,
    has_expansion: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CliOptionOutcome {
    Continue,
    StopAsPositional,
}

fn skip_next_value_token(iter: &mut Iter<'_, CommandArg>) {
    let next_is_value = iter
        .as_slice()
        .first()
        .is_some_and(|next_arg| next_arg.is_dynamic() || next_arg.text != OPTION_TERMINATOR);
    if next_is_value {
        let _ = iter.next();
    }
}

fn is_valid_subcommand_token(argument: &CommandArg) -> bool {
    let text = argument.text.as_str();
    !argument.is_dynamic()
        && !text.is_empty()
        && !text.starts_with('-')
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

#[cfg(test)]
mod tests {
    use super::super::ParsedCommand;
    use super::*;

    const TEST_PROGRAM_SCHEMA: ProgramCliSchema = ProgramCliSchema {
        options_with_values: &[
            "-C",
            "--directory",
            "--config",
            "-r",
            "--revision",
            "--revisions",
            "-n",
            "--limit",
        ],
    };

    const TEST_SUBCOMMAND_SCHEMAS: &[SubcommandCliSchema] = &[
        SubcommandCliSchema {
            program_name: "tool",
            pre_subcommand_spec: PreSubcommandSpec {
                short_values: "C",
                long_values: &["directory", "config"],
                abort_shorts: "hV",
                abort_longs: &["help", "version"],
            },
            allows_plus_toolchain: false,
        },
        SubcommandCliSchema {
            program_name: "cargo",
            pre_subcommand_spec: PreSubcommandSpec {
                short_values: "CZ",
                long_values: &["color", "config"],
                abort_shorts: "hV",
                abort_longs: &["help", "version", "list", "explain"],
            },
            allows_plus_toolchain: true,
        },
    ];

    #[test]
    fn test_find_subcommand_index_stops_before_subcommand_help_flag() {
        let schema = SubcommandCliSchema {
            program_name: "tool",
            pre_subcommand_spec: PreSubcommandSpec {
                abort_longs: &["help"],
                ..PreSubcommandSpec::DEFAULT
            },
            allows_plus_toolchain: false,
        };
        let cmd = InterceptedCommand::parse_all("tool log --help").remove(0);
        assert_eq!(schema.find_subcommand_index(&cmd.arguments, 0), Some(0));
    }

    #[test]
    fn test_parse_args_basic() {
        let cmd = InterceptedCommand::parse_all("tool check -C . --config myconf target").remove(0);
        let schema = ProgramCliSchema {
            options_with_values: &["-C", "--directory", "--config"],
        };
        let args = cmd.parse_args(&schema);
        assert_eq!(args.positionals, vec!["check", "target"]);
        assert_eq!(args.get_option("-C"), Some("."));
        assert_eq!(args.get_option("--config"), Some("myconf"));
    }

    #[test]
    fn test_parse_args_boolean_long_flag_and_stdin_dash() {
        let cmd = InterceptedCommand::parse_all("tool --quiet check -").remove(0);
        let schema = ProgramCliSchema {
            options_with_values: &["-C"],
        };
        let args = cmd.parse_args(&schema);
        assert_eq!(args.positionals, vec!["check", "-"]);
        assert!(args.has_flag("--quiet"));
    }

    #[test]
    fn test_parse_args_equals_syntax() {
        let cmd = InterceptedCommand::parse_all("tool check --directory=. target").remove(0);
        let schema = ProgramCliSchema {
            options_with_values: &["-C", "--directory"],
        };
        let args = cmd.parse_args(&schema);
        assert_eq!(args.positionals, vec!["check", "target"]);
        assert_eq!(args.get_option("--directory"), Some("."));
    }

    #[test]
    fn test_parse_args_posix_clustering() {
        let cmd = InterceptedCommand::parse_all("tool check -am").remove(0);
        let schema = ProgramCliSchema {
            options_with_values: &[],
        };
        let args = cmd.parse_args(&schema);
        assert_eq!(args.positionals, vec!["check"]);
        assert!(args.has_flag("-a"));
        assert!(args.has_flag("-m"));
    }

    #[test]
    fn test_parse_args_posix_glued_value_and_short_equals() {
        let cmd = InterceptedCommand::parse_all("tool check -C. -r=abc123").remove(0);
        let args = cmd.parse_args(&TEST_PROGRAM_SCHEMA);
        assert_eq!(args.positionals, vec!["check"]);
        assert_eq!(args.get_option("-C"), Some("."));
        assert_eq!(args.get_option("-r"), Some("abc123"));
    }

    #[test]
    fn test_parse_args_posix_cluster_with_value() {
        let cmd = InterceptedCommand::parse_all("git commit -am msg").remove(0);
        let schema = ProgramCliSchema {
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
            InterceptedCommand::parse_all("tool note -m -wip --message -second target").remove(0);
        let schema = ProgramCliSchema {
            options_with_values: &["-m", "--message"],
        };
        let args = cmd.parse_args(&schema);
        assert_eq!(args.positionals, vec!["note", "target"]);
        assert_eq!(args.get_option("-m"), Some("-wip"));
        assert_eq!(args.get_option("--message"), Some("-second"));
    }

    #[rstest::rstest]
    #[case::absent("tool list", LastOptionValue::Absent)]
    #[case::short_after_long_wins(
        "tool --directory /first list -C /second",
        LastOptionValue::Value { text: "/second", has_expansion: false }
    )]
    #[case::long_after_short_wins(
        "tool -C /first list --directory=/second",
        LastOptionValue::Value { text: "/second", has_expansion: false }
    )]
    #[case::expanded_value_marked(
        r#"tool list -C "$WS""#,
        LastOptionValue::Value { text: "$WS", has_expansion: true }
    )]
    #[case::tilde_value_marked(
        "tool list -C ~/ws",
        LastOptionValue::Value { text: "~/ws", has_expansion: true }
    )]
    #[case::dangling_flag_before_terminator(
        "tool list -C -- target",
        LastOptionValue::MissingValue
    )]
    fn test_last_option_value_tracks_cli_order_and_expansion(
        #[case] script: &str,
        #[case] expected: LastOptionValue<'_>,
    ) {
        let cmd = InterceptedCommand::parse_all(script).remove(0);
        let args = cmd.parse_args(&TEST_PROGRAM_SCHEMA);
        assert_eq!(args.last_option_value(&["-C", "--directory"]), expected);
    }

    #[test]
    fn test_option_values_returns_all_occurrences_in_order() {
        let parsed = ParsedCommand::new("tool list -r a --revision b -rc --revisions=d -r=e");
        let cmd = &parsed.commands()[0];
        let args = cmd.parse_args(&TEST_PROGRAM_SCHEMA);
        assert_eq!(args.option_values("-r"), &["a", "c", "e"]);
        assert_eq!(args.option_values("--revision"), &["b"]);
        assert_eq!(args.option_values("--revisions"), &["d"]);
        assert_eq!(args.option_values("--limit"), &[] as &[String]);
    }

    #[rstest::rstest]
    #[case::leading_boolean_flag(
        "tool --quiet list -n 5",
        Some(("--quiet", "list", "tool list --quiet -n 5"))
    )]
    #[case::short_option_with_value(
        "tool -C /tmp/ws status",
        Some(("-C", "status", "tool status -C /tmp/ws"))
    )]
    #[case::glued_short_option_and_config(
        "tool -C. --config ui.color=never diff",
        Some(("-C.", "diff", "tool diff -C. --config ui.color=never"))
    )]
    #[case::interspersed_redirect(
        "tool --quiet 2>/dev/null list -n 5",
        Some(("--quiet", "list", "tool 2>/dev/null list --quiet -n 5"))
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
    #[case::long_option_with_value(
        "tool --directory /tmp/app run target",
        Some(("--directory", "run", "tool run --directory /tmp/app target"))
    )]
    #[case::cargo_hyphenated_subcommand(
        "cargo --locked verify-project",
        Some(("--locked", "verify-project", "cargo verify-project --locked"))
    )]
    #[case::subcommand_first_allowed("tool list --quiet -n 5", None)]
    #[case::cargo_plus_toolchain_then_subcommand_allowed("cargo +nightly test --locked", None)]
    #[case::cargo_dynamic_plus_toolchain_ignored("cargo +$TC --locked test", None)]
    #[case::malformed_equals_before_subcommand_ignored("tool --=foo list", None)]
    #[case::dynamic_long_flag_before_subcommand_ignored(r#"tool --directory"$SUFFIX" list"#, None)]
    #[case::dynamic_short_flag_before_subcommand_ignored(r#"tool -q"$VAR" list"#, None)]
    #[case::dynamic_lone_dash_flag_before_subcommand_ignored(r#"tool -"$FLAG" list"#, None)]
    #[case::dynamic_double_dash_before_subcommand_ignored(r#"tool --"$FLAG" list"#, None)]
    #[case::cargo_bare_plus_ignored("cargo + --locked test", None)]
    #[case::non_toolchain_plus_arg_not_treated_as_toolchain("tool +nightly --quiet list", None)]
    #[case::cargo_misplaced_plus_toolchain_after_flag_ignored("cargo --locked +nightly test", None)]
    #[case::git_top_level_flag_ignored("git -C /tmp/repo status", None)]
    #[case::standalone_version_allowed("tool --version", None)]
    #[case::standalone_help_before_subcommand_allowed("tool --help list", None)]
    #[case::standalone_help_with_leading_flag_allowed("tool --quiet --help", None)]
    #[case::standalone_short_version_cluster_before_subcommand_allowed("cargo -qV test", None)]
    #[case::cargo_explain_allowed("cargo --explain E0308", None)]
    #[case::cargo_explain_equals_allowed("cargo --color=never --explain=E0308", None)]
    #[case::cargo_z_script_spaced_allowed("cargo +nightly -Z script foo.rs", None)]
    #[case::cargo_z_script_glued_allowed("cargo +nightly -Zscript foo.rs", None)]
    #[case::empty_quoted_positional_allowed("tool --quiet ''", None)]
    #[case::flags_only_no_subcommand_allowed("tool -C /tmp/ws --quiet", None)]
    #[case::dangling_option_value_at_end_allowed("tool -C", None)]
    #[case::dangling_long_option_before_double_dash_allowed("tool --directory -- list", None)]
    #[case::dangling_short_option_before_double_dash_allowed("tool -C -- list", None)]
    #[case::double_dash_before_positional_allowed("tool --quiet -- list", None)]
    #[case::single_dash_positional_allowed("tool --quiet -", None)]
    #[case::variable_expansion_after_flag_allowed("tool --quiet $ARGS", None)]
    fn test_extract_leading_global_flags(
        #[case] script: &str,
        #[case] expected: Option<(&str, &str, &str)>,
    ) {
        let parsed = ParsedCommand::new(script);
        let cmd = &parsed.commands()[0];
        let actual = extract_leading_global_flags(cmd, TEST_SUBCOMMAND_SCHEMAS);
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

    #[test]
    fn test_pre_subcommand_spec_keeps_dynamic_lone_dash_as_positional() {
        let cmd = InterceptedCommand::parse_all(r#"tool -"$FLAG" target"#).remove(0);
        let actual: Option<Vec<&str>> =
            PreSubcommandSpec::DEFAULT
                .parse(&cmd.arguments)
                .map(|parsed| {
                    parsed
                        .positionals()
                        .iter()
                        .map(|arg| arg.text.as_str())
                        .collect()
                });
        assert_eq!(actual.as_deref(), Some(&["-$FLAG", "target"][..]));
    }
}
