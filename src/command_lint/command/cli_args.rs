//! CLI option and positional argument classification over parsed [`CommandArg`] words.

use std::collections::HashMap;
use std::iter::Peekable;
use std::slice::Iter;

use super::shell::CommandArg;

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
    /// Positional arguments with their original [`CommandArg`] source spans.
    pub positional_args: Vec<CommandArg>,
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

    /// Returns `true` if the positional arguments start with `sequence`.
    #[must_use]
    pub fn has_subcommand_sequence(&self, sequence: &[&str]) -> bool {
        self.positionals.len() >= sequence.len()
            && self
                .positionals
                .iter()
                .zip(sequence)
                .all(|(argument, expected)| argument == expected)
    }
}

struct ArgParser<'a> {
    arguments_iter: Peekable<Iter<'a, CommandArg>>,
    options_with_values: &'a [&'a str],
    positionals: Vec<String>,
    positional_args: Vec<CommandArg>,
    options: HashMap<String, Vec<String>>,
}

impl<'a> ArgParser<'a> {
    fn new(arguments: &'a [CommandArg], options_with_values: &'a [&'a str]) -> Self {
        Self {
            arguments_iter: arguments.iter().peekable(),
            options_with_values,
            positionals: Vec::new(),
            positional_args: Vec::new(),
            options: HashMap::new(),
        }
    }

    fn parse(mut self) -> ParsedArgs {
        let mut options_terminated = false;

        while let Some(argument) = self.arguments_iter.next() {
            let text = argument.text.as_str();
            if options_terminated {
                self.positionals.push(argument.text.clone());
                self.positional_args.push(argument.clone());
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
                self.positional_args.push(argument.clone());
            }
        }

        ParsedArgs {
            positionals: self.positionals,
            positional_args: self.positional_args,
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
                .is_some_and(|next_arg| !next_arg.text.starts_with('-'))
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
                    let attached_value = argument[byte_offset + character.len_utf8()..].to_string();
                    self.options.entry(flag).or_default().push(attached_value);
                    break;
                }
                let next_is_value = self
                    .arguments_iter
                    .peek()
                    .is_some_and(|next_arg| !next_arg.text.starts_with('-'));
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
