//! Shell command-line AST and word-expansion parser backed by `brush_parser`.

use std::path::Path;

use brush_parser::ast::{
    AndOr, AndOrList, Assignment, AssignmentName, AssignmentValue, CaseClauseCommand, Command,
    CommandPrefixOrSuffixItem, CompoundCommand, CompoundList, CompoundListItem, ForClauseCommand,
    FunctionBody, FunctionDefinition, IfClauseCommand, IoFileRedirectKind, IoFileRedirectTarget,
    IoHereDocument, IoRedirect, Pipeline, Program, RedirectList, SeparatorOperator, SimpleCommand,
    SourceLocation as _, SubshellCommand, WhileOrUntilClauseCommand, Word,
};
use brush_parser::word::{WordPiece, WordPieceWithSource};
use brush_parser::{ParserOptions, SourceSpan as BrushSpan, Token, TokenizerOptions};

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

    /// Returns `true` if `brush_parser` rejected the command line due to a shell syntax error.
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

/// Tokenizes and parses `source` into `out`, returning `true` when the script is valid shell syntax.
fn parse_script_into(
    source: &str,
    base_byte_offset: usize,
    scopes: &mut Vec<ShellScope>,
    out: &mut Vec<InterceptedCommand>,
) -> bool {
    let parser_options = ParserOptions::default();
    let tokenizer_options = TokenizerOptions::default();
    let Ok(tokens) = brush_parser::tokenize_str_with_options(source, &tokenizer_options) else {
        return false;
    };
    let Ok(program) = brush_parser::parse_tokens(&tokens, &parser_options) else {
        return false;
    };

    let char_offsets = CharToByteMap::new(source, base_byte_offset);
    let token_spans: Vec<SourceSpan> = tokens
        .iter()
        .map(|token: &Token| char_offsets.map_span(token.location()))
        .collect();

    let walker = ScriptWalker {
        char_offsets: &char_offsets,
        token_spans: &token_spans,
        parser_options: &parser_options,
    };
    walker.walk_program(&program, scopes, out);
    true
}

struct ScriptWalker<'a> {
    char_offsets: &'a CharToByteMap,
    token_spans: &'a [SourceSpan],
    parser_options: &'a ParserOptions,
}

impl ScriptWalker<'_> {
    fn walk_program(
        &self,
        program: &Program,
        scopes: &mut Vec<ShellScope>,
        out: &mut Vec<InterceptedCommand>,
    ) {
        let multiple_lines = program.complete_commands.len() > 1;
        if multiple_lines {
            push_unique_scope(scopes, ShellScope::ChainedList);
        }
        for list in &program.complete_commands {
            self.walk_compound_list(list, scopes, out);
        }
        if multiple_lines {
            scopes.pop();
        }
    }

    fn walk_compound_list(
        &self,
        list: &CompoundList,
        scopes: &mut Vec<ShellScope>,
        out: &mut Vec<InterceptedCommand>,
    ) {
        let has_multiple_items = list.0.len() > 1;
        for CompoundListItem(and_or, sep) in &list.0 {
            let is_async = matches!(sep, SeparatorOperator::Async);
            let pushed_chain =
                has_multiple_items && push_unique_scope(scopes, ShellScope::ChainedList);
            let pushed_bg = is_async && push_unique_scope(scopes, ShellScope::Background);
            self.walk_and_or_list(and_or, scopes, out);
            if pushed_bg {
                scopes.pop();
            }
            if pushed_chain {
                scopes.pop();
            }
        }
    }

    fn walk_and_or_list(
        &self,
        and_or: &AndOrList,
        scopes: &mut Vec<ShellScope>,
        out: &mut Vec<InterceptedCommand>,
    ) {
        let is_chained = !and_or.additional.is_empty();
        let pushed = is_chained && push_unique_scope(scopes, ShellScope::ChainedList);
        self.walk_pipeline(&and_or.first, scopes, out);
        for item in &and_or.additional {
            let (AndOr::And(pipeline) | AndOr::Or(pipeline)) = item;
            self.walk_pipeline(pipeline, scopes, out);
        }
        if pushed {
            scopes.pop();
        }
    }

    fn walk_pipeline(
        &self,
        pipeline: &Pipeline,
        scopes: &mut Vec<ShellScope>,
        out: &mut Vec<InterceptedCommand>,
    ) {
        let is_pipeline = pipeline.seq.len() > 1;
        let pushed = is_pipeline && push_unique_scope(scopes, ShellScope::Pipeline);
        for cmd in &pipeline.seq {
            self.walk_command(cmd, scopes, out);
        }
        if pushed {
            scopes.pop();
        }
    }

    fn walk_command(
        &self,
        command: &Command,
        scopes: &mut Vec<ShellScope>,
        out: &mut Vec<InterceptedCommand>,
    ) {
        match command {
            Command::Simple(simple) => self.walk_simple_command(simple, scopes, out),
            Command::Compound(compound, redirects) => {
                self.walk_compound_command(compound, scopes, out);
                self.walk_redirect_list(redirects.as_ref(), scopes, out);
            }
            Command::Function(FunctionDefinition {
                body: FunctionBody(compound, redirects),
                ..
            }) => {
                let pushed = push_unique_scope(scopes, ShellScope::CompoundBlock);
                self.walk_compound_command(compound, scopes, out);
                self.walk_redirect_list(redirects.as_ref(), scopes, out);
                if pushed {
                    scopes.pop();
                }
            }
            Command::ExtendedTest(_, redirects) => {
                self.walk_redirect_list(redirects.as_ref(), scopes, out);
            }
        }
    }

    fn walk_compound_command(
        &self,
        compound: &CompoundCommand,
        scopes: &mut Vec<ShellScope>,
        out: &mut Vec<InterceptedCommand>,
    ) {
        match compound {
            CompoundCommand::Subshell(SubshellCommand { list, .. }) => {
                let pushed = push_unique_scope(scopes, ShellScope::Subshell);
                self.walk_compound_list(list, scopes, out);
                if pushed {
                    scopes.pop();
                }
            }
            CompoundCommand::BraceGroup(brace) => {
                let pushed = push_unique_scope(scopes, ShellScope::CompoundBlock);
                self.walk_compound_list(&brace.list, scopes, out);
                if pushed {
                    scopes.pop();
                }
            }
            CompoundCommand::ForClause(ForClauseCommand { values, body, .. }) => {
                if let Some(words) = values {
                    for word in words {
                        let _ = self.parse_word(word, scopes, out);
                    }
                }
                let pushed = push_unique_scope(scopes, ShellScope::CompoundBlock);
                self.walk_compound_list(&body.list, scopes, out);
                if pushed {
                    scopes.pop();
                }
            }
            CompoundCommand::CaseClause(CaseClauseCommand { value, cases, .. }) => {
                let _ = self.parse_word(value, scopes, out);
                let pushed = push_unique_scope(scopes, ShellScope::CompoundBlock);
                for case_item in cases {
                    for pattern in &case_item.patterns {
                        let _ = self.parse_word(pattern, scopes, out);
                    }
                    if let Some(case_commands) = &case_item.cmd {
                        self.walk_compound_list(case_commands, scopes, out);
                    }
                }
                if pushed {
                    scopes.pop();
                }
            }
            CompoundCommand::IfClause(IfClauseCommand {
                condition,
                then,
                elses,
                ..
            }) => {
                let pushed = push_unique_scope(scopes, ShellScope::CompoundBlock);
                self.walk_compound_list(condition, scopes, out);
                self.walk_compound_list(then, scopes, out);
                if let Some(else_clauses) = elses {
                    for else_clause in else_clauses {
                        if let Some(else_cond) = &else_clause.condition {
                            self.walk_compound_list(else_cond, scopes, out);
                        }
                        self.walk_compound_list(&else_clause.body, scopes, out);
                    }
                }
                if pushed {
                    scopes.pop();
                }
            }
            CompoundCommand::WhileClause(WhileOrUntilClauseCommand(cond, body, _))
            | CompoundCommand::UntilClause(WhileOrUntilClauseCommand(cond, body, _)) => {
                let pushed = push_unique_scope(scopes, ShellScope::CompoundBlock);
                self.walk_compound_list(cond, scopes, out);
                self.walk_compound_list(&body.list, scopes, out);
                if pushed {
                    scopes.pop();
                }
            }
            CompoundCommand::ArithmeticForClause(arith_for) => {
                let pushed = push_unique_scope(scopes, ShellScope::CompoundBlock);
                self.walk_compound_list(&arith_for.body.list, scopes, out);
                if pushed {
                    scopes.pop();
                }
            }
            CompoundCommand::Arithmetic(_) | CompoundCommand::Coprocess(_) => {}
        }
    }

    fn walk_redirect_list(
        &self,
        redirects: Option<&RedirectList>,
        scopes: &mut Vec<ShellScope>,
        out: &mut Vec<InterceptedCommand>,
    ) {
        if let Some(RedirectList(list)) = redirects {
            for redirect in list {
                let _ = self.parse_io_redirect(redirect, scopes, out);
            }
        }
    }

    fn walk_simple_command(
        &self,
        simple: &SimpleCommand,
        scopes: &mut Vec<ShellScope>,
        out: &mut Vec<InterceptedCommand>,
    ) {
        let mut env_assignments = Vec::new();
        let mut redirects = Vec::new();
        let mut arguments = Vec::new();
        let mut nested_commands = Vec::new();

        if let Some(prefix) = &simple.prefix {
            for item in &prefix.0 {
                self.collect_prefix_or_suffix_item(
                    item,
                    scopes,
                    &mut env_assignments,
                    &mut redirects,
                    &mut arguments,
                    &mut nested_commands,
                );
            }
        }

        let program = simple
            .word_or_name
            .as_ref()
            .map(|word| self.parse_word(word, scopes, &mut nested_commands));

        if let Some(suffix) = &simple.suffix {
            for item in &suffix.0 {
                self.collect_prefix_or_suffix_item(
                    item,
                    scopes,
                    &mut env_assignments,
                    &mut redirects,
                    &mut arguments,
                    &mut nested_commands,
                );
            }
        }

        if let Some(program) = program {
            let mut span = program.span;
            for env in &env_assignments {
                span = hull_span(span, env.span);
            }
            for arg in &arguments {
                span = hull_span(span, arg.span);
            }
            for redirect in &redirects {
                span = hull_span(span, redirect.span);
            }
            out.push(InterceptedCommand {
                span,
                program,
                arguments,
                env_assignments,
                redirects,
                enclosing_scopes: scopes.clone(),
            });
        }

        out.extend(nested_commands);
    }

    fn collect_prefix_or_suffix_item(
        &self,
        item: &CommandPrefixOrSuffixItem,
        scopes: &mut Vec<ShellScope>,
        env_assignments: &mut Vec<EnvAssignment>,
        redirects: &mut Vec<CommandRedirect>,
        arguments: &mut Vec<CommandArg>,
        nested_commands: &mut Vec<InterceptedCommand>,
    ) {
        match item {
            CommandPrefixOrSuffixItem::AssignmentWord(assignment, full_word) => {
                env_assignments.push(self.parse_assignment(
                    assignment,
                    full_word,
                    scopes,
                    nested_commands,
                ));
            }
            CommandPrefixOrSuffixItem::Word(word) => {
                arguments.push(self.parse_word(word, scopes, nested_commands));
            }
            CommandPrefixOrSuffixItem::IoRedirect(io_redirect) => {
                if let Some(redirect) = self.parse_io_redirect(io_redirect, scopes, nested_commands)
                {
                    redirects.push(redirect);
                }
            }
            CommandPrefixOrSuffixItem::ProcessSubstitution(_, subshell) => {
                let span = self.char_offsets.map_span(&subshell.loc);
                arguments.push(CommandArg {
                    text: String::new(),
                    span,
                    has_expansion: true,
                });
                let pushed = push_unique_scope(scopes, ShellScope::ProcessSubstitution);
                self.walk_compound_list(&subshell.list, scopes, nested_commands);
                if pushed {
                    scopes.pop();
                }
            }
        }
    }

    fn parse_assignment(
        &self,
        assignment: &Assignment,
        full_word: &Word,
        scopes: &mut Vec<ShellScope>,
        nested_commands: &mut Vec<InterceptedCommand>,
    ) -> EnvAssignment {
        let name = match &assignment.name {
            AssignmentName::VariableName(var) => var.clone(),
            AssignmentName::ArrayElementName(var, subscript) => format!("{var}[{subscript}]"),
        };
        let full_span = self.word_span(full_word);
        let value_offset = full_word.value.find('=').map_or(0, |index| index + 1);
        let value_span = SourceSpan::new(full_span.start + value_offset, full_span.end);
        let value = match &assignment.value {
            AssignmentValue::Scalar(val_word) => {
                self.parse_word_at_span(&val_word.value, value_span, scopes, nested_commands)
            }
            AssignmentValue::Array(elements) => {
                let mut has_expansion = false;
                for (key, element_value) in elements {
                    if let Some(key_word) = key {
                        has_expansion |= self
                            .parse_word_at_span(
                                &key_word.value,
                                value_span,
                                scopes,
                                nested_commands,
                            )
                            .has_expansion;
                    }
                    has_expansion |= self
                        .parse_word_at_span(
                            &element_value.value,
                            value_span,
                            scopes,
                            nested_commands,
                        )
                        .has_expansion;
                }
                CommandArg {
                    text: String::new(),
                    span: value_span,
                    has_expansion,
                }
            }
        };
        EnvAssignment {
            name,
            value,
            append: assignment.append,
            span: full_span,
        }
    }

    fn parse_io_redirect(
        &self,
        io_redirect: &IoRedirect,
        scopes: &mut Vec<ShellScope>,
        nested_commands: &mut Vec<InterceptedCommand>,
    ) -> Option<CommandRedirect> {
        match io_redirect {
            IoRedirect::File(raw_fd, kind, target) => {
                let fd = raw_fd.and_then(|value| u32::try_from(value).ok());
                let redirect_kind = match kind {
                    IoFileRedirectKind::Read => RedirectKind::Read,
                    IoFileRedirectKind::Write => RedirectKind::Write,
                    IoFileRedirectKind::Append => RedirectKind::Append,
                    IoFileRedirectKind::ReadAndWrite => RedirectKind::ReadWrite,
                    IoFileRedirectKind::Clobber => RedirectKind::Clobber,
                    IoFileRedirectKind::DuplicateInput => RedirectKind::DuplicateInput,
                    IoFileRedirectKind::DuplicateOutput => RedirectKind::DuplicateOutput,
                };
                let (resolved_target, target_span) = match target {
                    IoFileRedirectTarget::Filename(word) => {
                        let arg = self.parse_word(word, scopes, nested_commands);
                        let span = arg.span;
                        (RedirectTarget::File(arg), span)
                    }
                    IoFileRedirectTarget::Duplicate(word) => {
                        let arg = self.parse_word(word, scopes, nested_commands);
                        let span = arg.span;
                        let dup_target = arg
                            .text
                            .parse::<u32>()
                            .map_or(RedirectTarget::File(arg), RedirectTarget::Fd);
                        (dup_target, span)
                    }
                    IoFileRedirectTarget::ProcessSubstitution(_, subshell) => {
                        let sub_span = self.char_offsets.map_span(&subshell.loc);
                        let pushed = push_unique_scope(scopes, ShellScope::ProcessSubstitution);
                        self.walk_compound_list(&subshell.list, scopes, nested_commands);
                        if pushed {
                            scopes.pop();
                        }
                        (
                            RedirectTarget::ProcessSubstitution { span: sub_span },
                            sub_span,
                        )
                    }
                    // Synthesized by brush_parser for `|&` pipelines; not a source-level redirect.
                    IoFileRedirectTarget::Fd(_) => return None,
                };
                Some(CommandRedirect {
                    fd,
                    kind: redirect_kind,
                    target: resolved_target,
                    span: self.extend_redirect_span_left(target_span, raw_fd.is_some()),
                })
            }
            IoRedirect::OutputAndError(word, append) => {
                let arg = self.parse_word(word, scopes, nested_commands);
                let span = self.extend_redirect_span_left(arg.span, false);
                Some(CommandRedirect {
                    fd: None,
                    kind: if *append {
                        RedirectKind::AppendOutputAndError
                    } else {
                        RedirectKind::OutputAndError
                    },
                    target: RedirectTarget::File(arg),
                    span,
                })
            }
            IoRedirect::HereString(raw_fd, word) => {
                let fd = raw_fd.and_then(|value| u32::try_from(value).ok());
                let arg = self.parse_word(word, scopes, nested_commands);
                let span = self.extend_redirect_span_left(arg.span, raw_fd.is_some());
                Some(CommandRedirect {
                    fd,
                    kind: RedirectKind::HereString,
                    target: RedirectTarget::HereString(arg),
                    span,
                })
            }
            IoRedirect::HereDocument(raw_fd, IoHereDocument { here_end, doc, .. }) => {
                let fd = raw_fd.and_then(|value| u32::try_from(value).ok());
                let end_span = self.word_span(here_end);
                let header_span = self.extend_redirect_span_left(end_span, raw_fd.is_some());
                let doc_span = doc
                    .location()
                    .map(|loc| self.char_offsets.map_span(&loc))
                    .filter(|span| span.end > span.start);
                let target_span =
                    doc_span.map_or(end_span, |body_span| hull_span(end_span, body_span));
                let span =
                    doc_span.map_or(header_span, |body_span| hull_span(header_span, body_span));
                Some(CommandRedirect {
                    fd,
                    kind: RedirectKind::HereDoc,
                    target: RedirectTarget::HereDoc {
                        delimiter: here_end.value.clone(),
                        body: doc.value.clone(),
                        span: target_span,
                    },
                    span,
                })
            }
        }
    }

    /// Includes the preceding redirection operator token (and optional explicit file-descriptor
    /// token, such as `2` in `2>&1`) in a redirection's span.
    fn extend_redirect_span_left(
        &self,
        target_span: SourceSpan,
        has_explicit_fd: bool,
    ) -> SourceSpan {
        let target_token_position = self
            .token_spans
            .partition_point(|span| span.start < target_span.start);
        if target_token_position == 0 {
            return target_span;
        }
        let op_span = self.token_spans[target_token_position - 1];
        let start = if has_explicit_fd
            && target_token_position >= 2
            && self.token_spans[target_token_position - 2].end == op_span.start
        {
            self.token_spans[target_token_position - 2].start
        } else {
            op_span.start
        };
        SourceSpan::new(start.min(target_span.start), target_span.end)
    }

    fn parse_word(
        &self,
        word: &Word,
        scopes: &mut Vec<ShellScope>,
        nested_commands: &mut Vec<InterceptedCommand>,
    ) -> CommandArg {
        self.parse_word_at_span(&word.value, self.word_span(word), scopes, nested_commands)
    }

    fn parse_word_at_span(
        &self,
        raw: &str,
        span: SourceSpan,
        scopes: &mut Vec<ShellScope>,
        nested_commands: &mut Vec<InterceptedCommand>,
    ) -> CommandArg {
        let Ok(pieces) = brush_parser::word::parse(raw, self.parser_options) else {
            return CommandArg {
                text: raw.to_owned(),
                span,
                has_expansion: false,
            };
        };

        let mut unquoted = String::with_capacity(raw.len());
        let mut has_expansion = false;
        Self::collect_word_pieces(
            &pieces,
            span.start,
            raw,
            &mut unquoted,
            &mut has_expansion,
            scopes,
            nested_commands,
        );

        CommandArg {
            text: unquoted,
            span,
            has_expansion,
        }
    }

    fn collect_word_pieces(
        pieces: &[WordPieceWithSource],
        word_byte_start: usize,
        raw_word: &str,
        unquoted: &mut String,
        has_expansion: &mut bool,
        scopes: &mut Vec<ShellScope>,
        nested_commands: &mut Vec<InterceptedCommand>,
    ) {
        for piece_with_source in pieces {
            match &piece_with_source.piece {
                WordPiece::Text(text)
                | WordPiece::SingleQuotedText(text)
                | WordPiece::AnsiCQuotedText(text) => {
                    unquoted.push_str(text);
                }
                WordPiece::EscapeSequence(seq) => {
                    unquoted.push_str(seq.strip_prefix('\\').unwrap_or(seq));
                }
                WordPiece::DoubleQuotedSequence(inner)
                | WordPiece::GettextDoubleQuotedSequence(inner) => {
                    Self::collect_word_pieces(
                        inner,
                        word_byte_start,
                        raw_word,
                        unquoted,
                        has_expansion,
                        scopes,
                        nested_commands,
                    );
                }
                WordPiece::CommandSubstitution(inner_script) => {
                    *has_expansion = true;
                    if let Some(raw_slice) =
                        raw_word.get(piece_with_source.start_index..piece_with_source.end_index)
                    {
                        unquoted.push_str(raw_slice);
                    }
                    // `$(` is 2 ASCII bytes before `inner_script`.
                    let inner_base = word_byte_start + piece_with_source.start_index + 2;
                    let pushed = push_unique_scope(scopes, ShellScope::CommandSubstitution);
                    let _ = parse_script_into(inner_script, inner_base, scopes, nested_commands);
                    if pushed {
                        scopes.pop();
                    }
                }
                WordPiece::BackquotedCommandSubstitution(inner_script) => {
                    *has_expansion = true;
                    if let Some(raw_slice) =
                        raw_word.get(piece_with_source.start_index..piece_with_source.end_index)
                    {
                        unquoted.push_str(raw_slice);
                    }
                    // `` ` `` is 1 ASCII byte before `inner_script`.
                    let inner_base = word_byte_start + piece_with_source.start_index + 1;
                    let pushed = push_unique_scope(scopes, ShellScope::CommandSubstitution);
                    let _ = parse_script_into(inner_script, inner_base, scopes, nested_commands);
                    if pushed {
                        scopes.pop();
                    }
                }
                WordPiece::ParameterExpansion(_)
                | WordPiece::ArithmeticExpression(_)
                | WordPiece::TildeExpansion(_) => {
                    *has_expansion = true;
                    if let Some(raw_slice) =
                        raw_word.get(piece_with_source.start_index..piece_with_source.end_index)
                    {
                        unquoted.push_str(raw_slice);
                    }
                }
            }
        }
    }

    fn word_span(&self, word: &Word) -> SourceSpan {
        word.location().map_or_else(
            || SourceSpan::new(0, 0),
            |loc| self.char_offsets.map_span(&loc),
        )
    }
}

/// Maps `brush_parser`'s 0-based Unicode `char` offset to 0-based UTF-8 byte offsets, shifted by
/// `base_byte_offset` when parsing nested command substitutions.
struct CharToByteMap {
    non_ascii_offsets: Option<Vec<usize>>,
    byte_len: usize,
    base_byte_offset: usize,
}

impl CharToByteMap {
    fn new(source: &str, base_byte_offset: usize) -> Self {
        let non_ascii_offsets = (!source.is_ascii()).then(|| {
            let mut offsets: Vec<usize> = source
                .char_indices()
                .map(|(byte_offset, _)| byte_offset)
                .collect();
            offsets.push(source.len());
            offsets
        });
        Self {
            non_ascii_offsets,
            byte_len: source.len(),
            base_byte_offset,
        }
    }

    fn map_span(&self, brush_span: &BrushSpan) -> SourceSpan {
        SourceSpan::new(
            self.char_to_byte(brush_span.start.index),
            self.char_to_byte(brush_span.end.index),
        )
    }

    fn char_to_byte(&self, char_offset: usize) -> usize {
        let local_offset = self.non_ascii_offsets.as_ref().map_or_else(
            || char_offset.min(self.byte_len),
            |offsets| offsets.get(char_offset).copied().unwrap_or(self.byte_len),
        );
        self.base_byte_offset + local_offset
    }
}

/// Pushes `scope` onto `scopes` if it is not already the innermost scope, returning `true` when pushed.
fn push_unique_scope(scopes: &mut Vec<ShellScope>, scope: ShellScope) -> bool {
    if scopes.last().copied() == Some(scope) {
        false
    } else {
        scopes.push(scope);
        true
    }
}

/// Smallest span enclosing both `left` and `right`.
const fn hull_span(left: SourceSpan, right: SourceSpan) -> SourceSpan {
    let start = if left.start < right.start {
        left.start
    } else {
        right.start
    };
    let end = if left.end > right.end {
        left.end
    } else {
        right.end
    };
    SourceSpan::new(start, end)
}
