//! Shell command-line AST and word-expansion parser (`brush_parser` on host and WASI targets;
//! zero-import stub on `wasm32-unknown-unknown`).

use std::path::Path;

use super::cli_args::{ParsedArgs, ProgramCliSchema};
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

    /// Classifies `self.arguments` into positional arguments and option key-values using `schema`.
    #[must_use]
    pub fn parse_args(&self, schema: &ProgramCliSchema) -> ParsedArgs {
        ParsedArgs::parse(&self.arguments, schema)
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
