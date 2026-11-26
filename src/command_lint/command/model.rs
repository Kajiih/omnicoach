//! Parser-agnostic domain types representing parsed shell commands, arguments, redirections,
//! and enclosing shell scopes.

use std::path::{Path, PathBuf};

use crate::diagnostic::SourceSpan;
use crate::paths::{is_rooted_path, lexical_normalize, resolve_against_cwd};

/// Representation of a single intercepted CLI command invocation within a
/// [`crate::command_lint::command::ParsedCommand`].
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
    /// Working directory state accumulated from preceding in-scope `cd` builtins.
    pub(super) working_dir: WorkingDirState,
}

impl InterceptedCommand {
    /// Unquoted program name as written (e.g. `"jj"` or `"/usr/bin/jj"`).
    #[must_use]
    pub fn program_name(&self) -> &str {
        &self.program.text
    }

    /// Base file name of the invoked program (e.g. `"jj"` from `"/usr/bin/jj"`).
    #[must_use]
    pub fn program_base_name(&self) -> &str {
        self.program.executable_base_name()
    }

    /// Resolves this command's effective working directory from `base_dir`, applying any
    /// preceding in-scope `cd` builtins and an optional explicit `override_dir` (such as `jj -R`),
    /// normalized lexically.
    ///
    /// Returns `None` when a preceding dynamic or environment-dependent `cd` left the effective
    /// working directory statically unknown (unless `override_dir` is rooted), or when
    /// `override_dir` is empty.
    #[must_use]
    pub(crate) fn resolve_working_dir(
        &self,
        base_dir: &Path,
        override_dir: Option<&Path>,
    ) -> Option<PathBuf> {
        if let Some(target) = override_dir {
            if target.as_os_str().is_empty() {
                return None;
            }
            if is_rooted_path(target) {
                return Some(lexical_normalize(target));
            }
        }
        let offset = self.working_dir.known_offset()?;
        let after_cd = resolve_against_cwd(offset, Some(base_dir));
        Some(match override_dir {
            Some(target) => resolve_against_cwd(target, Some(&after_cd)),
            None => after_cd,
        })
    }
}

/// A single shell word (program name or CLI argument) with unquoted text and span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandArg {
    /// Unquoted text value (e.g. `feat: msg` for `"feat: msg"`).
    pub text: String,
    /// UTF-8 byte span in [`crate::command_lint::command::ParsedCommand::source`].
    pub span: SourceSpan,
    /// `true` if the word contains dynamic expansions (`$VAR`, `$(...)`, `$((...))`).
    pub has_expansion: bool,
}

impl CommandArg {
    /// Base executable file name of [`Self::text`] (e.g. `"jj"` for `"/usr/bin/jj"`).
    #[must_use]
    fn executable_base_name(&self) -> &str {
        Path::new(&self.text)
            .file_name()
            .and_then(|file_name| file_name.to_str())
            .unwrap_or(&self.text)
    }
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
    /// UTF-8 byte span of the full `NAME=VALUE` assignment token in
    /// [`crate::command_lint::command::ParsedCommand::source`].
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
    /// UTF-8 byte span of the redirection in
    /// [`crate::command_lint::command::ParsedCommand::source`].
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

/// Statically tracked working-directory offset relative to the session's initial directory, or
/// `None` when a dynamic or environment-dependent `cd` invalidated tracking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WorkingDirState(Option<PathBuf>);

impl WorkingDirState {
    fn known_offset(&self) -> Option<&Path> {
        self.0.as_deref()
    }
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
impl WorkingDirState {
    pub(super) const fn initial() -> Self {
        Self(Some(PathBuf::new()))
    }

    pub(super) const fn unknown() -> Self {
        Self(None)
    }

    pub(super) fn apply_static_cd(&mut self, target: &Path) {
        if is_rooted_path(target) {
            self.0 = Some(target.to_path_buf());
        } else if let Some(current) = &mut self.0 {
            current.push(target);
        }
    }

    pub(super) fn merge(&mut self, other: &Self) {
        if self != other {
            self.0 = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executable_base_name_strips_unix_directory_prefix() {
        let bare = CommandArg {
            text: String::from("jj"),
            span: SourceSpan::new(0, 2),
            has_expansion: false,
        };
        let qualified = CommandArg {
            text: String::from("/usr/bin/jj"),
            span: SourceSpan::new(0, 11),
            has_expansion: false,
        };
        assert_eq!(bare.executable_base_name(), "jj");
        assert_eq!(qualified.executable_base_name(), "jj");
    }
}
