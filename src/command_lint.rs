//! Command linter module.
pub(crate) mod rules;
pub mod vcs;

pub use vcs::{JjCliClient, JjClient};

use crate::core::{AstNode, Config};
use crate::diagnostic::Diagnostic;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;

/// Schema defining a program's command-line interface options layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramCliSchema {
    /// The name of the binary/program (e.g. `"jj"`, `"git"`).
    pub program_name: &'static str,
    /// List of option flags that accept values (e.g. `["-R", "--repository"]`).
    pub options_with_values: &'static [&'static str],
}

/// Representation of an intercepted CLI command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterceptedCommand {
    /// The original full raw CLI input string.
    pub raw_string: String,
    /// The name of the binary/program being executed.
    pub program_name: String,
    /// The list of arguments parsed from the command string.
    pub arguments: Vec<String>,
    /// The byte offsets span of this command in the raw CLI string.
    pub span: (usize, usize),
}

impl InterceptedCommand {
    /// Parses a raw CLI input string into all individual command invocations.
    #[must_use]
    pub fn parse_all(raw_string: &str) -> Vec<Self> {
        let grep = AstGrep::new(raw_string, SupportLang::Bash);
        grep.root()
            .dfs()
            .filter(|node| node.kind() == "command")
            .filter_map(|node| Self::parse_single(&node, raw_string))
            .collect()
    }

    fn parse_single(node: &AstNode<'_>, raw_string: &str) -> Option<Self> {
        let mut children = node.children();
        let command_name_node = children.find(|child| child.kind() == "command_name")?;
        let program_name = command_name_node.text().to_string();

        let mut arguments = Vec::new();
        for child in children {
            let text = child.text().to_string();
            if let Ok(words) = shell_words::split(&text) {
                arguments.extend(words);
            } else {
                arguments.push(text);
            }
        }

        let range = node.range();
        Some(Self {
            raw_string: raw_string.to_string(),
            program_name,
            arguments,
            span: (range.start, range.end),
        })
    }

    /// Extracts positional arguments (skipping option flags and their values) and option key-values.
    #[must_use]
    pub fn parse_args(&self, schema: &ProgramCliSchema) -> ParsedArgs {
        ArgParser::new(&self.arguments, schema.options_with_values).parse()
    }

    /// Returns the base name of the program (e.g. "jj" from "/usr/bin/jj").
    #[must_use]
    pub fn program_base_name(&self) -> &str {
        std::path::Path::new(&self.program_name)
            .file_name()
            .and_then(|file_name| file_name.to_str())
            .unwrap_or(&self.program_name)
    }
}

/// Common trait for command execution workflow safety rules.
pub trait CommandRule: crate::core::Rule {
    /// Evaluates the intercepted command against this validation rule.
    #[must_use]
    fn check_command(
        &self,
        cmd: &InterceptedCommand,
        jj_client: &dyn JjClient,
        config: &Config,
    ) -> Vec<Diagnostic>;
}

struct ArgParser<'a> {
    args_iter: std::iter::Peekable<std::slice::Iter<'a, String>>,
    options_with_values: &'a [&'a str],
    positionals: Vec<String>,
    options: std::collections::HashMap<String, Vec<String>>,
}

impl<'a> ArgParser<'a> {
    fn new(arguments: &'a [String], options_with_values: &'a [&'a str]) -> Self {
        Self {
            args_iter: arguments.iter().peekable(),
            options_with_values,
            positionals: Vec::new(),
            options: std::collections::HashMap::new(),
        }
    }

    fn parse(mut self) -> ParsedArgs {
        let mut options_terminated = false;

        while let Some(arg) = self.args_iter.next() {
            if options_terminated {
                self.positionals.push(arg.clone());
                continue;
            }

            if arg == "--" {
                options_terminated = true;
                continue;
            }

            if arg.starts_with("--") {
                self.parse_long_option(arg);
            } else if arg.starts_with('-') && arg.len() > 1 {
                self.parse_short_option_cluster(arg);
            } else {
                self.positionals.push(arg.clone());
            }
        }

        ParsedArgs {
            positionals: self.positionals,
            options: self.options,
        }
    }

    fn parse_long_option(&mut self, arg: &str) {
        if let Some((flag, val)) = arg.split_once('=') {
            if self.options_with_values.contains(&flag) {
                self.options
                    .entry(flag.to_string())
                    .or_default()
                    .push(val.to_string());
            } else {
                self.options.entry(arg.to_string()).or_default();
            }
        } else if self.options_with_values.contains(&arg)
            && self
                .args_iter
                .peek()
                .is_some_and(|next_val| !next_val.starts_with('-'))
        {
            if let Some(val) = self.args_iter.next() {
                self.options
                    .entry(arg.to_string())
                    .or_default()
                    .push(val.clone());
            }
        } else {
            self.options.entry(arg.to_string()).or_default();
        }
    }

    fn parse_short_option_cluster(&mut self, arg: &str) {
        let mut char_indices = arg.char_indices().skip(1).peekable();

        while let Some((idx, c)) = char_indices.next() {
            let flag = format!("-{c}");
            let is_option_with_value = self.options_with_values.contains(&flag.as_str());

            if is_option_with_value {
                if char_indices.peek().is_some() {
                    let val = arg[idx + c.len_utf8()..].to_string();
                    self.options.entry(flag).or_default().push(val);
                    break;
                }
                let next_is_val = self
                    .args_iter
                    .peek()
                    .is_some_and(|next_val| !next_val.starts_with('-'));
                if next_is_val && let Some(val) = self.args_iter.next() {
                    self.options.entry(flag).or_default().push(val.clone());
                    break;
                }
            }
            self.options.entry(flag).or_default();
        }
    }
}

/// Represents parsed command line arguments and options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedArgs {
    /// The positional arguments (e.g. `["edit", "@-"]`).
    pub positionals: Vec<String>,
    /// Option flags and their values.
    options: std::collections::HashMap<String, Vec<String>>,
}

impl ParsedArgs {
    /// Checks if a boolean or key-value option flag was provided.
    #[must_use]
    pub fn has_flag(&self, flag: &str) -> bool {
        self.options.contains_key(flag)
    }

    /// Gets the first value associated with an option flag, if present.
    #[must_use]
    pub fn get_option(&self, option: &str) -> Option<&str> {
        self.options
            .get(option)
            .and_then(|values| values.first())
            .map(String::as_str)
    }

    /// Gets all values associated with an option flag.
    #[must_use]
    pub fn get_all_options(&self, option: &str) -> &[String] {
        self.options.get(option).map_or(&[], Vec::as_slice)
    }

    /// Verifies if the sequence of positional arguments starts with the given subcommands.
    #[must_use]
    pub fn has_subcommand_sequence(&self, sequence: &[&str]) -> bool {
        if self.positionals.len() < sequence.len() {
            return false;
        }
        self.positionals
            .iter()
            .zip(sequence)
            .all(|(arg, expected)| arg == expected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_command() {
        let cmds = InterceptedCommand::parse_all("jj edit @-");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].program_name, "jj");
        assert_eq!(cmds[0].arguments, vec!["edit", "@-"]);
        assert_eq!(cmds[0].span, (0, 10));
    }

    #[test]
    fn test_parse_compound_commands() {
        let cmds = InterceptedCommand::parse_all("jj edit @- && jj describe");
        assert_eq!(cmds.len(), 2);
        assert_eq!(
            (
                cmds[0].program_name.as_str(),
                &cmds[0].arguments[..],
                cmds[0].span
            ),
            ("jj", &["edit".to_string(), "@-".to_string()][..], (0, 10))
        );
        assert_eq!(
            (
                cmds[1].program_name.as_str(),
                &cmds[1].arguments[..],
                cmds[1].span
            ),
            ("jj", &["describe".to_string()][..], (14, 25))
        );
    }

    #[test]
    fn test_parse_nested_subshell() {
        let cmds = InterceptedCommand::parse_all("(cd dir && jj edit @-)");
        assert_eq!(cmds.len(), 2);
        assert_eq!(
            (
                cmds[0].program_name.as_str(),
                &cmds[0].arguments[..],
                cmds[0].span
            ),
            ("cd", &["dir".to_string()][..], (1, 7))
        );
        assert_eq!(
            (cmds[1].program_name.as_str(), &cmds[1].arguments[..]),
            ("jj", &["edit".to_string(), "@-".to_string()][..])
        );
    }

    #[test]
    fn test_program_base_name() {
        let cmd = InterceptedCommand::parse_all("/usr/local/bin/jj edit").remove(0);
        assert_eq!(cmd.program_base_name(), "jj");

        let cmd_rel = InterceptedCommand::parse_all("./bin/git status").remove(0);
        assert_eq!(cmd_rel.program_base_name(), "git");

        let cmd_simple = InterceptedCommand::parse_all("jj edit").remove(0);
        assert_eq!(cmd_simple.program_base_name(), "jj");
    }

    #[test]
    fn test_parse_args_basic() {
        let cmd = InterceptedCommand::parse_all("jj edit -R . --config myconf @-").remove(0);
        let schema = ProgramCliSchema {
            program_name: "jj",
            options_with_values: &["-R", "--repository", "--config"],
        };
        let args = cmd.parse_args(&schema);
        assert_eq!(args.positionals, vec!["edit", "@-"]);
        assert_eq!(
            (args.get_option("-R"), args.get_option("--config")),
            (Some("."), Some("myconf"))
        );
        assert!(args.has_subcommand_sequence(&["edit"]));
        assert!(!args.has_subcommand_sequence(&["describe"]));
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
    fn test_parse_args_posix_glued_value() {
        let cmd = InterceptedCommand::parse_all("jj edit -R. @-").remove(0);
        let schema = ProgramCliSchema {
            program_name: "jj",
            options_with_values: &["-R"],
        };
        let args = cmd.parse_args(&schema);
        assert_eq!(args.positionals, vec!["edit", "@-"]);
        assert_eq!(args.get_option("-R"), Some("."));
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
}
