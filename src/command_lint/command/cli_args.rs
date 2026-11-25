//! Zero-copy declarative CLI option and positional classification over [`CommandArg`] slices.

use super::model::{CommandArg, InterceptedCommand};
use crate::diagnostic::SourceSpan;

const OPTION_TERMINATOR: &str = "--";

/// How a CLI scanner handles options that appear after the first positional operand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::command_lint) enum OptionOrder {
    /// GNU / `clap` style: options and positional operands may be freely interleaved until `--`.
    Permute,
    /// POSIX / subcommand-prefix style: the first positional operand ends option scanning,
    /// treating all remaining tokens (including `-flag` and `--`) as positional arguments.
    StopAtFirstPositional,
}

/// Compile-time declarative option layout for statically classifying a `&[CommandArg]` slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::command_lint) struct CliSpec {
    /// Whether option scanning continues past positional arguments (`Permute`) or stops at the
    /// first positional (`StopAtFirstPositional`).
    pub(in crate::command_lint) order: OptionOrder,
    /// Single-character short options that consume a required value (for example `"Rm"`).
    pub(in crate::command_lint) short_values: &'static str,
    /// Whether attached short option values strip a leading `=` (`-r=main` -> `"main"`), as in
    /// `clap`-based CLIs (`jj`, `cargo`, `uv`). POSIX tools (`sed -e=`) set this to `false`.
    pub(in crate::command_lint) short_equals_values: bool,
    /// Long option names (without leading `--`) that consume a required value (for example
    /// `&["repository", "message"]`).
    pub(in crate::command_lint) long_values: &'static [&'static str],
    /// Single-character short flags (such as `"hV"`) whose presence aborts classification (`None`).
    pub(in crate::command_lint) abort_shorts: &'static str,
    /// Long option names without leading `--` (such as `&["help", "version"]`) whose presence
    /// aborts classification (`None`).
    pub(in crate::command_lint) abort_longs: &'static [&'static str],
}

impl CliSpec {
    /// Baseline permissive `CliSpec` (`Permute`, no value-taking or aborting flags) for `const`
    /// struct-update literals (`CliSpec { short_values: "R", ..CliSpec::DEFAULT }`).
    pub(in crate::command_lint) const DEFAULT: Self = Self {
        order: OptionOrder::Permute,
        short_values: "",
        short_equals_values: false,
        long_values: &[],
        abort_shorts: "",
        abort_longs: &[],
    };

    /// Classifies `arguments` without cloning strings, returning `None` if an `abort_shorts` or
    /// `abort_longs` flag is encountered before option scanning ends.
    #[must_use]
    pub(in crate::command_lint) fn parse<'a>(
        &self,
        arguments: &'a [CommandArg],
    ) -> Option<ParsedCli<'a>> {
        let mut positionals = Vec::new();
        let mut occurrences = Vec::new();
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
                if after_dashes.is_empty() || after_dashes.starts_with('=') {
                    if self.order == OptionOrder::StopAtFirstPositional {
                        positionals.push(argument);
                        positionals.extend(iter.by_ref());
                        break;
                    }
                    continue;
                }
                if self.parse_long_option(argument, after_dashes, &mut iter, &mut occurrences)?
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
                if self.parse_short_cluster(argument, short_prefix, &mut iter, &mut occurrences)?
                    == CliOptionOutcome::StopAsPositional
                {
                    positionals.push(argument);
                    positionals.extend(iter.by_ref());
                    break;
                }
                continue;
            }

            positionals.push(argument);
            if self.order == OptionOrder::StopAtFirstPositional {
                positionals.extend(iter.by_ref());
                break;
            }
        }

        Some(ParsedCli {
            positionals,
            occurrences,
            double_dash_positional_index,
        })
    }

    fn parse_long_option<'a>(
        &self,
        argument: &'a CommandArg,
        after_dashes: &'a str,
        iter: &mut std::slice::Iter<'a, CommandArg>,
        occurrences: &mut Vec<OptionOccurrence<'a>>,
    ) -> Option<CliOptionOutcome> {
        if let Some((long_name, _)) = after_dashes.split_once('=') {
            if self.abort_longs.contains(&long_name) {
                return None;
            }
            let value_offset = OPTION_TERMINATOR.len() + long_name.len() + 1;
            let value_text = &argument.text[value_offset..];
            occurrences.push(OptionOccurrence {
                flag: FlagName::Long(long_name),
                value: Some(OptValue {
                    text: value_text,
                    is_dynamic: argument.is_dynamic(),
                }),
            });
            return Some(CliOptionOutcome::Continue);
        }

        if argument.is_dynamic() {
            return Some(if self.order == OptionOrder::StopAtFirstPositional {
                CliOptionOutcome::StopAsPositional
            } else {
                CliOptionOutcome::Continue
            });
        }

        let long_name = after_dashes;
        if self.abort_longs.contains(&long_name) {
            return None;
        }

        let value = if self.long_values.contains(&long_name) {
            consume_next_value_token(iter)
        } else {
            None
        };
        occurrences.push(OptionOccurrence {
            flag: FlagName::Long(long_name),
            value,
        });
        Some(CliOptionOutcome::Continue)
    }

    fn parse_short_cluster<'a>(
        &self,
        argument: &'a CommandArg,
        short_prefix: &'a str,
        iter: &mut std::slice::Iter<'a, CommandArg>,
        occurrences: &mut Vec<OptionOccurrence<'a>>,
    ) -> Option<CliOptionOutcome> {
        let full_text = argument.text.as_str();
        let initial_len = occurrences.len();
        for (rel_offset, character) in short_prefix.char_indices() {
            if self.abort_shorts.contains(character) {
                return None;
            }
            if self.short_values.contains(character) {
                let byte_after_char = 1 + rel_offset + character.len_utf8();
                let value = if byte_after_char < full_text.len() {
                    let raw_attached = &full_text[byte_after_char..];
                    let value_text = if self.short_equals_values {
                        raw_attached.strip_prefix('=').unwrap_or(raw_attached)
                    } else {
                        raw_attached
                    };
                    Some(OptValue {
                        text: value_text,
                        is_dynamic: argument.is_dynamic(),
                    })
                } else {
                    consume_next_value_token(iter)
                };
                occurrences.push(OptionOccurrence {
                    flag: FlagName::Short(character),
                    value,
                });
                return Some(CliOptionOutcome::Continue);
            }
            occurrences.push(OptionOccurrence {
                flag: FlagName::Short(character),
                value: None,
            });
        }
        if self.order == OptionOrder::StopAtFirstPositional && argument.is_dynamic() {
            occurrences.truncate(initial_len);
            return Some(CliOptionOutcome::StopAsPositional);
        }
        Some(CliOptionOutcome::Continue)
    }
}

/// Compile-time declarative option layout active before a CLI subcommand token.
///
/// Pre-subcommand scanning always stops at the first positional operand
/// ([`OptionOrder::StopAtFirstPositional`]), making option permutation past the subcommand
/// unrepresentable by construction.
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

    const fn into_cli_spec(self) -> CliSpec {
        CliSpec {
            order: OptionOrder::StopAtFirstPositional,
            short_values: self.short_values,
            long_values: self.long_values,
            abort_shorts: self.abort_shorts,
            abort_longs: self.abort_longs,
            ..CliSpec::DEFAULT
        }
    }
}

/// Result of looking up the last CLI occurrence across one or more option flag aliases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::command_lint) enum LastOptionValue<'a> {
    /// None of the queried flag aliases appeared in the command arguments.
    Absent,
    /// The last matching flag appeared without a value operand (dangling before `--` or EOF).
    MissingValue,
    /// The last matching flag carried a value operand and its shell-expansion marker.
    Value {
        /// Unquoted option value slice.
        text: &'a str,
        /// `true` if the value contained dynamic shell expansions (`$VAR`, `$(...)`, `~`).
        is_dynamic: bool,
    },
}

/// Borrowed identifier of a short (`-c`) or long (`--config`) CLI option flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::command_lint) enum FlagName<'a> {
    /// Single-character short flag without the leading `-` (for example `'R'` for `-R`).
    Short(char),
    /// Long flag name without the leading `--` (for example `"repository"` for `--repository`).
    Long(&'a str),
}

/// Borrowed classification of positional operands and option occurrences produced by [`CliSpec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::command_lint) struct ParsedCli<'a> {
    positionals: Vec<&'a CommandArg>,
    occurrences: Vec<OptionOccurrence<'a>>,
    double_dash_positional_index: Option<usize>,
}

impl<'a> ParsedCli<'a> {
    /// Positional arguments in command-line order, excluding recognized options and their values.
    #[must_use]
    pub(in crate::command_lint) fn positionals(&self) -> &[&'a CommandArg] {
        &self.positionals
    }

    /// Returns `true` if `flag` (such as `FlagName::Short('u')` or `FlagName::Long("no-edit")`)
    /// was provided.
    #[must_use]
    pub(in crate::command_lint) fn has_flag(&self, flag: FlagName<'_>) -> bool {
        self.occurrences.iter().any(|entry| entry.flag == flag)
    }

    /// Returns `true` if any flag in `flags` was provided.
    #[must_use]
    pub(in crate::command_lint) fn has_any_flag(&self, flags: &[FlagName<'_>]) -> bool {
        self.occurrences
            .iter()
            .any(|entry| flags.contains(&entry.flag))
    }

    /// Returns the first value provided across `flags` in command-line order, if any.
    #[must_use]
    pub(in crate::command_lint) fn first_value(&self, flags: &[FlagName<'_>]) -> Option<&'a str> {
        self.occurrences
            .iter()
            .filter(|entry| flags.contains(&entry.flag))
            .find_map(|entry| entry.value.map(|val| val.text))
    }

    /// Returns all values provided across `flags` in command-line order.
    #[must_use]
    pub(in crate::command_lint) fn values(&self, flags: &[FlagName<'_>]) -> Vec<&'a str> {
        self.occurrences
            .iter()
            .filter(|entry| flags.contains(&entry.flag))
            .filter_map(|entry| entry.value.map(|val| val.text))
            .collect()
    }

    /// Returns the last occurrence across `flags` in command-line order, preserving missing-value
    /// and shell-expansion state.
    #[must_use]
    pub(in crate::command_lint) fn last_option_value(
        &self,
        flags: &[FlagName<'_>],
    ) -> LastOptionValue<'a> {
        let Some(occurrence) = self
            .occurrences
            .iter()
            .rev()
            .find(|entry| flags.contains(&entry.flag))
        else {
            return LastOptionValue::Absent;
        };
        occurrence
            .value
            .map_or(LastOptionValue::MissingValue, |val| {
                LastOptionValue::Value {
                    text: val.text,
                    is_dynamic: val.is_dynamic,
                }
            })
    }

    const fn has_double_dash_before_first_positional(&self) -> bool {
        matches!(self.double_dash_positional_index, Some(0))
    }
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
        let parsed = self.pre_subcommand_spec.into_cli_spec().parse(slice)?;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OptionOccurrence<'a> {
    flag: FlagName<'a>,
    value: Option<OptValue<'a>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CliOptionOutcome {
    Continue,
    StopAsPositional,
}

/// Borrowed value operand consumed by a CLI option, preserving shell-expansion state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OptValue<'a> {
    /// Unquoted option value slice borrowed from [`CommandArg::text`].
    text: &'a str,
    /// `true` when the value (or the attached value tail of a partially dynamic flag token)
    /// contains dynamic shell expansions.
    is_dynamic: bool,
}

fn consume_next_value_token<'a>(
    iter: &mut std::slice::Iter<'a, CommandArg>,
) -> Option<OptValue<'a>> {
    let next_is_value = iter
        .as_slice()
        .first()
        .is_some_and(|next_arg| next_arg.is_dynamic() || next_arg.text != OPTION_TERMINATOR);
    if !next_is_value {
        return None;
    }
    let next_arg = iter.next()?;
    Some(OptValue {
        text: &next_arg.text,
        is_dynamic: next_arg.is_dynamic(),
    })
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

    const TEST_CLAP_SPEC: CliSpec = CliSpec {
        short_values: "Crn",
        short_equals_values: true,
        long_values: &["directory", "config", "revision", "revisions", "limit"],
        abort_shorts: "hV",
        abort_longs: &["help", "version"],
        ..CliSpec::DEFAULT
    };

    const TEST_POSIX_STOP_SPEC: CliSpec = CliSpec {
        order: OptionOrder::StopAtFirstPositional,
        short_values: "e",
        short_equals_values: false,
        abort_longs: &["help"],
        ..CliSpec::DEFAULT
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

    fn positional_texts<'a>(parsed: &ParsedCli<'a>) -> Vec<&'a str> {
        parsed
            .positionals()
            .iter()
            .map(|arg| arg.text.as_str())
            .collect()
    }

    #[test]
    fn test_parse_permute_options_and_positionals() {
        let cmd = InterceptedCommand::parse_all("tool check -C . --config myconf target").remove(0);
        let parsed = TEST_CLAP_SPEC.parse(&cmd.arguments).unwrap();
        assert_eq!(positional_texts(&parsed), vec!["check", "target"]);
        assert_eq!(parsed.first_value(&[FlagName::Short('C')]), Some("."));
        assert_eq!(
            parsed.first_value(&[FlagName::Long("config")]),
            Some("myconf")
        );
    }

    #[test]
    fn test_parse_stop_at_first_positional_and_literal_short_equals() {
        let cmd = InterceptedCommand::parse_all("sed -e= script.sed -i --help file.txt").remove(0);
        let parsed = TEST_POSIX_STOP_SPEC.parse(&cmd.arguments).unwrap();
        assert_eq!(parsed.first_value(&[FlagName::Short('e')]), Some("="));
        assert_eq!(
            positional_texts(&parsed),
            vec!["script.sed", "-i", "--help", "file.txt"]
        );
        assert!(!parsed.has_flag(FlagName::Short('i')));
    }

    #[test]
    fn test_parse_boolean_long_flag_and_stdin_dash() {
        let cmd = InterceptedCommand::parse_all("tool --quiet check -").remove(0);
        let parsed = TEST_CLAP_SPEC.parse(&cmd.arguments).unwrap();
        assert_eq!(positional_texts(&parsed), vec!["check", "-"]);
        assert!(parsed.has_flag(FlagName::Long("quiet")));
        assert!(parsed.has_any_flag(&[FlagName::Short('q'), FlagName::Long("quiet")]));
    }

    #[test]
    fn test_parse_equals_syntax_and_glued_shorts() {
        let cmd =
            InterceptedCommand::parse_all("tool check --directory=. -C/ws -r=abc123").remove(0);
        let parsed = TEST_CLAP_SPEC.parse(&cmd.arguments).unwrap();
        assert_eq!(positional_texts(&parsed), vec!["check"]);
        assert_eq!(
            parsed.first_value(&[FlagName::Long("directory")]),
            Some(".")
        );
        assert_eq!(parsed.first_value(&[FlagName::Short('C')]), Some("/ws"));
        assert_eq!(parsed.first_value(&[FlagName::Short('r')]), Some("abc123"));
    }

    #[test]
    fn test_parse_posix_clustering_with_values() {
        let cmd = InterceptedCommand::parse_all("tool check -am -qC /ws -vC/alt -qr=abc123 target")
            .remove(0);
        let parsed = TEST_CLAP_SPEC.parse(&cmd.arguments).unwrap();
        assert_eq!(positional_texts(&parsed), vec!["check", "target"]);
        assert_eq!(
            parsed.occurrences,
            vec![
                OptionOccurrence {
                    flag: FlagName::Short('a'),
                    value: None,
                },
                OptionOccurrence {
                    flag: FlagName::Short('m'),
                    value: None,
                },
                OptionOccurrence {
                    flag: FlagName::Short('q'),
                    value: None,
                },
                OptionOccurrence {
                    flag: FlagName::Short('C'),
                    value: Some(OptValue {
                        text: "/ws",
                        is_dynamic: false,
                    }),
                },
                OptionOccurrence {
                    flag: FlagName::Short('v'),
                    value: None,
                },
                OptionOccurrence {
                    flag: FlagName::Short('C'),
                    value: Some(OptValue {
                        text: "/alt",
                        is_dynamic: false,
                    }),
                },
                OptionOccurrence {
                    flag: FlagName::Short('q'),
                    value: None,
                },
                OptionOccurrence {
                    flag: FlagName::Short('r'),
                    value: Some(OptValue {
                        text: "abc123",
                        is_dynamic: false,
                    }),
                },
            ]
        );
    }

    #[rstest::rstest]
    #[case::short_dynamic_abort_tail(r#"tool -q"$VERBOSE" status"#, &[FlagName::Short('q')])]
    #[case::short_dynamic_value_tail(r#"tool -C"$C_VAR" status"#, &[FlagName::Short('C')])]
    #[case::long_dynamic_abort_tail(r#"tool --help"$SUFFIX" status"#, &[])]
    #[case::long_dynamic_value_tail(r#"tool --directory"$SUFFIX"=x status"#, &[])]
    #[case::dynamic_double_dash(r#"tool --"$VAR" status"#, &[])]
    #[case::empty_long_name_equals("tool --=foo status", &[])]
    fn test_parse_dynamic_option_prefixes_and_malformed_double_dash(
        #[case] script: &str,
        #[case] expected_flags: &[FlagName<'_>],
    ) {
        let cmd = InterceptedCommand::parse_all(script).remove(0);
        let parsed = TEST_CLAP_SPEC.parse(&cmd.arguments).unwrap();
        let actual_flags: Vec<FlagName<'_>> =
            parsed.occurrences.iter().map(|entry| entry.flag).collect();
        assert_eq!(positional_texts(&parsed), vec!["status"]);
        assert_eq!(actual_flags, expected_flags);
    }

    #[test]
    fn test_short_dynamic_value_tail_captures_dynamic_value() {
        let cmd = InterceptedCommand::parse_all(r#"tool -C"$C_VAR" status"#).remove(0);
        let parsed = TEST_CLAP_SPEC.parse(&cmd.arguments).unwrap();
        assert_eq!(
            parsed.last_option_value(&[FlagName::Short('C')]),
            LastOptionValue::Value {
                text: "$C_VAR",
                is_dynamic: true,
            }
        );
    }

    #[test]
    fn test_first_value_respects_cli_order_across_aliases() {
        let cmd =
            InterceptedCommand::parse_all("tool --directory /first list -C /second").remove(0);
        let parsed = TEST_CLAP_SPEC.parse(&cmd.arguments).unwrap();
        assert_eq!(
            parsed.first_value(&[FlagName::Short('C'), FlagName::Long("directory")]),
            Some("/first")
        );
    }

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
    fn test_parse_posix_terminator_and_dash_prefixed_values() {
        let cmd = InterceptedCommand::parse_all("tool -r -wip --revision -second -- -R").remove(0);
        let parsed = TEST_CLAP_SPEC.parse(&cmd.arguments).unwrap();
        assert_eq!(positional_texts(&parsed), vec!["-R"]);
        assert_eq!(
            parsed.values(&[FlagName::Short('r'), FlagName::Long("revision")]),
            vec!["-wip", "-second"]
        );
        assert!(!parsed.has_flag(FlagName::Short('R')));
    }

    #[rstest::rstest]
    #[case::abort_short("-h")]
    #[case::abort_clustered_short("-qV")]
    #[case::abort_long("--help")]
    #[case::abort_long_equals("--version=1")]
    fn test_parse_returns_none_on_abort_flags(#[case] flag: &str) {
        let script = format!("tool {flag} status");
        let cmd = InterceptedCommand::parse_all(&script).remove(0);
        assert_eq!(TEST_CLAP_SPEC.parse(&cmd.arguments), None);
    }

    #[rstest::rstest]
    #[case::absent("tool list", LastOptionValue::Absent)]
    #[case::short_after_long_wins(
        "tool --directory /first list -C /second",
        LastOptionValue::Value { text: "/second", is_dynamic: false }
    )]
    #[case::long_after_short_wins(
        "tool -C /first list --directory=/second",
        LastOptionValue::Value { text: "/second", is_dynamic: false }
    )]
    #[case::separate_expanded_value_marked(
        r#"tool list -C "$WS""#,
        LastOptionValue::Value { text: "$WS", is_dynamic: true }
    )]
    #[case::attached_short_expanded_value_marked(
        r#"tool list -C"$WS""#,
        LastOptionValue::Value { text: "$WS", is_dynamic: true }
    )]
    #[case::attached_long_expanded_value_marked(
        r#"tool list --directory="$WS/sub""#,
        LastOptionValue::Value { text: "$WS/sub", is_dynamic: true }
    )]
    #[case::tilde_value_marked(
        "tool list -C ~/ws",
        LastOptionValue::Value { text: "~/ws", is_dynamic: true }
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
        let parsed = TEST_CLAP_SPEC.parse(&cmd.arguments).unwrap();
        assert_eq!(
            parsed.last_option_value(&[FlagName::Short('C'), FlagName::Long("directory")]),
            expected
        );
    }

    #[test]
    fn test_values_returns_all_occurrences_in_cli_order() {
        let parsed_cmd = ParsedCommand::new("tool list -r a --revision b -rc --revisions=d -r=e");
        let cmd = &parsed_cmd.commands()[0];
        let parsed = TEST_CLAP_SPEC.parse(&cmd.arguments).unwrap();
        assert_eq!(parsed.values(&[FlagName::Short('r')]), vec!["a", "c", "e"]);
        assert_eq!(
            parsed.values(&[
                FlagName::Short('r'),
                FlagName::Long("revision"),
                FlagName::Long("revisions"),
            ]),
            vec!["a", "b", "c", "d", "e"]
        );
        assert_eq!(
            parsed.values(&[FlagName::Long("limit")]),
            Vec::<&str>::new()
        );
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

    #[rstest::rstest]
    #[case::permute_ignores_dynamic_lone_dash(
        OptionOrder::Permute,
        r#"tool -"$FLAG" target"#,
        &["target"]
    )]
    #[case::permute_matches_dynamic_double_dash(
        OptionOrder::Permute,
        r#"tool --"$FLAG" target"#,
        &["target"]
    )]
    #[case::permute_keeps_literal_single_dash(
        OptionOrder::Permute,
        "tool - target",
        &["-", "target"]
    )]
    #[case::permute_keeps_dynamic_lone_dash_after_terminator(
        OptionOrder::Permute,
        r#"tool -- -"$FLAG" target"#,
        &["-$FLAG", "target"]
    )]
    #[case::stop_at_first_positional_keeps_dynamic_lone_dash(
        OptionOrder::StopAtFirstPositional,
        r#"tool -"$FLAG" target"#,
        &["-$FLAG", "target"]
    )]
    fn test_cli_spec_parse_dynamic_lone_dash(
        #[case] order: OptionOrder,
        #[case] script: &str,
        #[case] expected_positionals: &[&str],
    ) {
        let spec = CliSpec {
            order,
            ..CliSpec::DEFAULT
        };
        let cmd = InterceptedCommand::parse_all(script).remove(0);
        let actual: Option<Vec<&str>> = spec.parse(&cmd.arguments).map(|parsed| {
            parsed
                .positionals()
                .iter()
                .map(|arg| arg.text.as_str())
                .collect()
        });
        assert_eq!(
            actual.as_deref(),
            Some(expected_positionals),
            "script: `{script}`"
        );
    }
}
