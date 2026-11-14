//! CLI option and positional argument classification over parsed [`CommandArg`] words.

use std::collections::HashMap;
use std::iter::Peekable;
use std::slice::Iter;

use super::shell::{CommandArg, InterceptedCommand};

/// CLI schema definition for Jujutsu (`jj`) commands.
pub const JJ_CLI_SCHEMA: ProgramCliSchema = ProgramCliSchema {
    program_name: "jj",
    options_with_values: &[
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
        "-R",
        "--repository",
        "--at-operation",
        "--at-op",
        "--color",
        "--config",
        "--config-toml",
        "--config-file",
    ],
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
}
