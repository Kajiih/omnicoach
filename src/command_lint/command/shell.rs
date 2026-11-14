//! Shell command-line AST and word-expansion parser (`brush_parser` on host and WASI targets;
//! zero-import stub on `wasm32-unknown-unknown`).

use std::path::Path;

use crate::diagnostic::{SourceLocation, SourceSpan, VCS_CONTEXT_NAME};

/// A parsed shell command line (single command, pipeline, or compound script).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCommand {
    source: String,
    has_syntax_error: bool,
    commands: Vec<InterceptedCommand>,
}

impl ParsedCommand {
    /// Parses `raw_command` into a [`ParsedCommand`] containing all simple command invocations
    /// in execution order (including commands nested in pipelines, subshells, compound blocks,
    /// and `$(...)` / `` `...` `` command substitutions).
    #[must_use]
    pub fn new(raw_command: &str) -> Self {
        let mut commands = Vec::new();
        let has_syntax_error = !parse_script_into(raw_command, 0, &mut Vec::new(), &mut commands);
        Self {
            source: raw_command.to_string(),
            has_syntax_error,
            commands,
        }
    }

    /// The full raw command-line source text.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Returns `true` if shell parsing failed (or on `wasm32-unknown-unknown`, where shell
    /// tokenization is stubbed so `commands()` is empty).
    #[must_use]
    pub const fn has_syntax_error(&self) -> bool {
        self.has_syntax_error
    }

    /// All simple command invocations extracted in execution order.
    #[must_use]
    pub fn commands(&self) -> &[InterceptedCommand] {
        &self.commands
    }

    /// Resolves a byte [`SourceSpan`] within this command line into a [`SourceLocation`].
    #[must_use]
    pub fn to_source_location(&self, span: SourceSpan) -> SourceLocation {
        SourceLocation::virtual_span(VCS_CONTEXT_NAME, &self.source, span)
    }
}

/// Representation of a single intercepted CLI command invocation within a [`ParsedCommand`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterceptedCommand {
    /// Full UTF-8 byte span of this simple command (including env assignments, args, and redirects).
    pub span: SourceSpan,
    /// Invoked program word (e.g. `jj` or `/usr/bin/jj`).
    pub program: CommandArg,
    /// Positional and flag arguments (excluding leading env assignments and I/O redirects).
    pub arguments: Vec<CommandArg>,
    /// Leading `NAME=VALUE` or `NAME+=VALUE` environment variable assignments.
    pub env_assignments: Vec<EnvAssignment>,
    /// Input/output redirections (`> file`, `2>&1`, `<<EOF`, etc.) attached to this command.
    pub redirects: Vec<CommandRedirect>,
    /// Enclosing shell scopes from outermost to innermost.
    pub enclosing_scopes: Vec<ShellScope>,
}

impl InterceptedCommand {
    /// Parses a raw CLI input string into all individual command invocations.
    #[must_use]
    pub fn parse_all(raw_string: &str) -> Vec<Self> {
        ParsedCommand::new(raw_string).commands
    }

    /// Unquoted program name as written (e.g. `"jj"` or `"/usr/bin/jj"`).
    #[must_use]
    pub fn program_name(&self) -> &str {
        &self.program.text
    }

    /// Base file name of the invoked program (e.g. `"jj"` from `"/usr/bin/jj"`).
    #[must_use]
    pub fn program_base_name(&self) -> &str {
        Path::new(&self.program.text)
            .file_name()
            .and_then(|file_name| file_name.to_str())
            .unwrap_or(&self.program.text)
    }
}

/// A single shell word (program name or CLI argument) with unquoted text and span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandArg {
    /// Unquoted text value (e.g. `feat: msg` for `"feat: msg"`).
    pub text: String,
    /// UTF-8 byte span in [`ParsedCommand::source`].
    pub span: SourceSpan,
    /// `true` if the word contains dynamic expansions (`$VAR`, `$(...)`, `$((...))`).
    pub has_expansion: bool,
}

/// Leading `NAME=VALUE` or `NAME+=VALUE` environment variable assignment on a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvAssignment {
    /// Variable name (or `name[index]` for indexed array element assignments).
    pub name: String,
    /// Assigned value word.
    pub value: CommandArg,
    /// `true` when the `+=` append operator was used.
    pub append: bool,
    /// UTF-8 byte span of the full `NAME=VALUE` assignment token in [`ParsedCommand::source`].
    pub span: SourceSpan,
}

/// An I/O redirection attached to a command (`> file`, `>> file`, `2>&1`, `<<EOF`, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRedirect {
    /// Explicit file descriptor number preceding the operator (e.g. `Some(2)` in `2>&1`).
    pub fd: Option<u32>,
    /// Kind of redirection operator.
    pub kind: RedirectKind,
    /// Target of the redirection (file path, file descriptor, heredoc, herestring, or subshell).
    pub target: RedirectTarget,
    /// UTF-8 byte span of the redirection in [`ParsedCommand::source`].
    pub span: SourceSpan,
}

/// Operator kind for a [`CommandRedirect`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectKind {
    /// `< file`
    Read,
    /// `> file`
    Write,
    /// `>> file`
    Append,
    /// `>| file`
    Clobber,
    /// `<> file`
    ReadWrite,
    /// `<& fd`
    DuplicateInput,
    /// `>& fd`
    DuplicateOutput,
    /// `&> file` or `>& file`
    OutputAndError,
    /// `&>> file`
    AppendOutputAndError,
    /// `<<EOF` or `<<-EOF`
    HereDoc,
    /// `<<< word`
    HereString,
}

/// Target operand of a [`CommandRedirect`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedirectTarget {
    /// Filename or word target (`> out.txt`).
    File(CommandArg),
    /// Numeric file descriptor target (`2>&1`).
    Fd(u32),
    /// Here-document body and delimiter (`<<EOF ... EOF`).
    HereDoc {
        /// Delimiter word (`EOF`).
        delimiter: String,
        /// Unexpanded body content.
        body: String,
        /// Span of the delimiter and body.
        span: SourceSpan,
    },
    /// Here-string word (`<<< "input"`).
    HereString(CommandArg),
    /// Process substitution target (`> >(cmd)` or `< <(cmd)`).
    ProcessSubstitution {
        /// Span of the `<(...)` or `>(...)` subshell.
        span: SourceSpan,
    },
}

/// Structural shell scope enclosing an [`InterceptedCommand`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellScope {
    /// Command is part of a multi-stage pipeline (`a | b` or `a |& b`).
    Pipeline,
    /// Command is part of a sequential or conditional list (`a && b`, `a || b`, `a ; b`).
    ChainedList,
    /// Command is executed asynchronously in the background (`cmd &`).
    Background,
    /// Command is inside a `( ... )` subshell.
    Subshell,
    /// Command is inside a compound block (`{ ...; }`, `if`, `for`, `while`, `until`, `case`, or function body).
    CompoundBlock,
    /// Command is inside a `$(...)` or `` `...` `` command substitution.
    CommandSubstitution,
    /// Command is inside a `<(...)` or `>(...)` process substitution.
    ProcessSubstitution,
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use self::brush_backend::parse_script_into;

/// Stub shell parser on browser `wasm32-unknown-unknown` builds where `brush-parser` is omitted
/// so `omnicoach_playground.wasm` links with zero `wasm-bindgen` host imports.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
const fn parse_script_into(
    _source: &str,
    _base_byte_offset: usize,
    _scopes: &mut Vec<ShellScope>,
    _out: &mut Vec<InterceptedCommand>,
) -> bool {
    false
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
mod brush_backend;

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Eq)]
    struct CommandSummary<'a> {
        program: &'a str,
        args: Vec<&'a str>,
        span: SourceSpan,
        scopes: &'a [ShellScope],
    }

    fn summarize(cmd: &InterceptedCommand) -> CommandSummary<'_> {
        CommandSummary {
            program: cmd.program_name(),
            args: cmd.arguments.iter().map(|arg| arg.text.as_str()).collect(),
            span: cmd.span,
            scopes: &cmd.enclosing_scopes,
        }
    }

    #[test]
    fn test_parse_single_command() {
        let parsed = ParsedCommand::new("jj edit @-");
        let summaries: Vec<_> = parsed.commands().iter().map(summarize).collect();
        assert!(!parsed.has_syntax_error());
        assert_eq!(
            summaries,
            vec![CommandSummary {
                program: "jj",
                args: vec!["edit", "@-"],
                span: SourceSpan::new(0, 10),
                scopes: &[],
            }]
        );
    }

    #[test]
    fn test_parse_compound_commands() {
        let cmds = InterceptedCommand::parse_all("jj edit @- && jj describe");
        let summaries: Vec<_> = cmds.iter().map(summarize).collect();
        assert_eq!(
            summaries,
            vec![
                CommandSummary {
                    program: "jj",
                    args: vec!["edit", "@-"],
                    span: SourceSpan::new(0, 10),
                    scopes: &[ShellScope::ChainedList],
                },
                CommandSummary {
                    program: "jj",
                    args: vec!["describe"],
                    span: SourceSpan::new(14, 25),
                    scopes: &[ShellScope::ChainedList],
                },
            ]
        );
    }

    #[test]
    fn test_parse_nested_subshell() {
        let cmds = InterceptedCommand::parse_all("(cd dir && jj edit @-)");
        let summaries: Vec<_> = cmds.iter().map(summarize).collect();
        assert_eq!(
            summaries,
            vec![
                CommandSummary {
                    program: "cd",
                    args: vec!["dir"],
                    span: SourceSpan::new(1, 7),
                    scopes: &[ShellScope::Subshell, ShellScope::ChainedList],
                },
                CommandSummary {
                    program: "jj",
                    args: vec!["edit", "@-"],
                    span: SourceSpan::new(11, 21),
                    scopes: &[ShellScope::Subshell, ShellScope::ChainedList],
                },
            ]
        );
    }

    #[test]
    fn test_redirects_excluded_from_arguments_and_included_in_span() {
        let cmds = InterceptedCommand::parse_all("jj edit > out.txt 2>&1");
        let cmd = &cmds[0];
        let redirects: Vec<_> = cmd
            .redirects
            .iter()
            .map(|redirect| (redirect.fd, redirect.kind, redirect.span))
            .collect();
        assert_eq!(
            summarize(cmd),
            CommandSummary {
                program: "jj",
                args: vec!["edit"],
                span: SourceSpan::new(0, 22),
                scopes: &[],
            }
        );
        assert_eq!(
            redirects,
            vec![
                (None, RedirectKind::Write, SourceSpan::new(8, 17)),
                (
                    Some(2),
                    RedirectKind::DuplicateOutput,
                    SourceSpan::new(18, 22)
                ),
            ]
        );
    }

    #[test]
    fn test_env_assignments_separated_from_arguments() {
        let cmds = InterceptedCommand::parse_all("JJ_USER=alice jj edit @-");
        let cmd = &cmds[0];
        let envs: Vec<_> = cmd
            .env_assignments
            .iter()
            .map(|env| {
                (
                    env.name.as_str(),
                    env.value.text.as_str(),
                    env.value.span,
                    env.span,
                )
            })
            .collect();
        assert_eq!(
            summarize(cmd),
            CommandSummary {
                program: "jj",
                args: vec!["edit", "@-"],
                span: SourceSpan::new(0, 24),
                scopes: &[],
            }
        );
        assert_eq!(
            envs,
            vec![(
                "JJ_USER",
                "alice",
                SourceSpan::new(8, 13),
                SourceSpan::new(0, 13)
            )]
        );
    }

    #[test]
    fn test_command_substitution_inside_double_quotes_preserves_outer_byte_span() {
        let raw = r#"echo "result: $(jj edit d123)""#;
        let cmds = InterceptedCommand::parse_all(raw);
        let inner = &cmds[1];
        assert_eq!(
            summarize(inner),
            CommandSummary {
                program: "jj",
                args: vec!["edit", "d123"],
                span: SourceSpan::new(16, 28),
                scopes: &[ShellScope::CommandSubstitution],
            }
        );
        assert_eq!(&raw[inner.span.start..inner.span.end], "jj edit d123");
    }

    #[test]
    fn test_non_ascii_prefix_maps_to_utf8_byte_offsets() {
        let raw = r#"echo "héllo" && jj edit d123"#;
        let cmds = InterceptedCommand::parse_all(raw);
        assert_eq!(cmds.len(), 2);
        assert_eq!(&raw[cmds[1].span.start..cmds[1].span.end], "jj edit d123");
    }

    #[test]
    fn test_syntax_error_reported_on_unterminated_quote() {
        let parsed = ParsedCommand::new(r#"jj edit "unterminated"#);
        assert!(parsed.has_syntax_error());
    }

    #[rstest::rstest]
    #[case::for_loop(
        "for rev in a b; do jj edit d123; done",
        "jj edit d123",
        ShellScope::CompoundBlock
    )]
    #[case::if_else(
        "if true; then jj status; else jj edit d123; fi",
        "jj edit d123",
        ShellScope::CompoundBlock
    )]
    #[case::while_loop(
        "while false; do jj edit d123; done",
        "jj edit d123",
        ShellScope::CompoundBlock
    )]
    #[case::case_clause(
        "case $x in a) jj edit d123 ;; esac",
        "jj edit d123",
        ShellScope::CompoundBlock
    )]
    #[case::brace_group("{ jj edit d123; }", "jj edit d123", ShellScope::CompoundBlock)]
    #[case::function_def(
        "run_it() { jj edit d123; }",
        "jj edit d123",
        ShellScope::CompoundBlock
    )]
    #[case::backtick_substitution(
        "echo `jj edit d123`",
        "jj edit d123",
        ShellScope::CommandSubstitution
    )]
    #[case::env_assignment_substitution(
        "JJ_USER=$(jj edit d123) jj status",
        "jj edit d123",
        ShellScope::CommandSubstitution
    )]
    #[case::process_substitution(
        "diff <(jj edit d123) out.txt",
        "jj edit d123",
        ShellScope::ProcessSubstitution
    )]
    #[case::background_job("jj edit d123 &", "jj edit d123", ShellScope::Background)]
    fn test_compound_and_nested_scopes(
        #[case] script: &str,
        #[case] expected_slice: &str,
        #[case] expected_scope: ShellScope,
    ) {
        let parsed = ParsedCommand::new(script);
        let matched = parsed
            .commands()
            .iter()
            .find(|cmd| cmd.program_name() == "jj" && cmd.arguments[0].text == "edit")
            .expect("expected `jj edit` command to be extracted");
        assert_eq!(
            &parsed.source()[matched.span.start..matched.span.end],
            expected_slice
        );
        assert!(matched.enclosing_scopes.contains(&expected_scope));
    }

    #[rstest::rstest]
    #[case::write_without_space("jj log>out.txt", RedirectKind::Write, ">out.txt")]
    #[case::append("jj log >> out.txt", RedirectKind::Append, ">> out.txt")]
    #[case::clobber("jj log >| out.txt", RedirectKind::Clobber, ">| out.txt")]
    #[case::output_and_error("jj log &> out.txt", RedirectKind::OutputAndError, "&> out.txt")]
    #[case::append_output_and_error(
        "jj log &>> out.txt",
        RedirectKind::AppendOutputAndError,
        "&>> out.txt"
    )]
    #[case::read_input("cat < in.txt", RedirectKind::Read, "< in.txt")]
    #[case::here_string("cat <<< 'hello'", RedirectKind::HereString, "<<< 'hello'")]
    #[case::duplicate_fd("jj log 2>&1", RedirectKind::DuplicateOutput, "2>&1")]
    fn test_redirect_kinds_and_spans(
        #[case] script: &str,
        #[case] expected_kind: RedirectKind,
        #[case] expected_redirect_slice: &str,
    ) {
        let parsed = ParsedCommand::new(script);
        let cmd = &parsed.commands()[0];
        let redirect = &cmd.redirects[0];
        assert_eq!(redirect.kind, expected_kind);
        assert_eq!(
            &parsed.source()[redirect.span.start..redirect.span.end],
            expected_redirect_slice
        );
    }

    #[test]
    fn test_heredoc_and_word_escapes() {
        let script = indoc::indoc! {r"
            cat <<EOF
            hello
            EOF
            echo a\ b $HOME"};
        let parsed = ParsedCommand::new(script);
        assert_eq!(
            parsed.commands()[0].redirects[0].kind,
            RedirectKind::HereDoc
        );
        assert_eq!(
            parsed.commands()[0].redirects[0].span,
            SourceSpan::new(4, 20)
        );
        assert_eq!(parsed.commands()[1].arguments[0].text, "a b");
        assert!(parsed.commands()[1].arguments[1].has_expansion);
    }

    #[test]
    fn test_array_and_subscript_assignment_command_substitution_spans() {
        let script = "ARR=($(jj status) $(jj edit d123)) map[k=1]=$(jj log)";
        let parsed = ParsedCommand::new(script);
        let slices: Vec<&str> = parsed
            .commands()
            .iter()
            .map(|cmd| &parsed.source()[cmd.span.start..cmd.span.end])
            .collect();
        assert_eq!(slices, vec!["jj status", "jj edit d123", "jj log"]);
    }

    #[test]
    fn test_suffix_assignment_words_stay_in_arguments() {
        let cmds = InterceptedCommand::parse_all(r#"jj --config FOO="bar" edit @-"#);
        assert_eq!(cmds[0].env_assignments, []);
        assert_eq!(
            summarize(&cmds[0]).args,
            vec!["--config", "FOO=bar", "edit", "@-"]
        );
    }

    #[rstest::rstest]
    #[case::utf8_and_keyed_elements(
        r#"(true) && ARR=(     "ééé" [m]=$(jj edit d123)) ARR=($(jj status))"#,
        &["jj edit d123", "jj status"]
    )]
    #[case::after_heredoc(
        indoc::indoc! {r#"
            cat <<EOF
            body
            EOF
            ARR=(     "ééé" $(jj edit d123))"#},
        &["jj edit d123"]
    )]
    fn test_array_assignment_with_subshell_utf8_and_heredoc_extracts_exact_spans(
        #[case] script: &str,
        #[case] expected: &[&str],
    ) {
        let parsed = ParsedCommand::new(script);
        let slices: Vec<&str> = parsed
            .commands()
            .iter()
            .filter(|cmd| cmd.program_name() == "jj")
            .map(|cmd| &parsed.source()[cmd.span.start..cmd.span.end])
            .collect();
        assert!(!parsed.has_syntax_error(), "syntax error in: {script}");
        assert_eq!(slices, expected);
    }

    #[test]
    fn test_coprocess_and_multiline_substitution_scopes() {
        let script = "coproc jj edit d123; echo \"$(true\nfalse)\" && jj status";
        let parsed = ParsedCommand::new(script);
        assert_eq!(parsed.commands()[0].program_name(), "jj");
        assert_eq!(
            parsed.commands().last().unwrap().enclosing_scopes,
            vec![ShellScope::ChainedList]
        );
    }
}
