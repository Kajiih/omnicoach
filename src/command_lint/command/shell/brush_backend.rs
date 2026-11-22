//! Shell tokenization and AST lowering backed by `brush-parser`, compiled on every target except browser `wasm32-unknown-unknown`.

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

use super::{
    CommandArg, CommandRedirect, EnvAssignment, InterceptedCommand, RedirectKind, RedirectTarget,
    ShellScope, SourceSpan, WorkingDirState,
};

/// Tokenizes and parses `source` into `out`, returning `true` when the script is valid shell syntax.
pub(super) fn parse_script_into(
    source: &str,
    base_byte_offset: usize,
    scopes: &mut Vec<ShellScope>,
    initial_cwd: &WorkingDirState,
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
        tokens: &tokens,
        char_offsets: &char_offsets,
        token_spans: &token_spans,
        parser_options: &parser_options,
        array_token_cursor: std::cell::Cell::new(0),
    };
    let mut cwd = initial_cwd.clone();
    walker.walk_program(&program, scopes, &mut cwd, out);
    true
}

struct ScriptWalker<'a> {
    tokens: &'a [Token],
    char_offsets: &'a CharToByteMap,
    token_spans: &'a [SourceSpan],
    parser_options: &'a ParserOptions,
    array_token_cursor: std::cell::Cell<usize>,
}

impl ScriptWalker<'_> {
    fn walk_program(
        &self,
        program: &Program,
        scopes: &mut Vec<ShellScope>,
        cwd: &mut WorkingDirState,
        out: &mut Vec<InterceptedCommand>,
    ) {
        let multiple_lines = program.complete_commands.len() > 1;
        let pushed = multiple_lines && push_unique_scope(scopes, ShellScope::ChainedList);
        for list in &program.complete_commands {
            self.walk_compound_list(list, scopes, cwd, out);
        }
        if pushed {
            scopes.pop();
        }
    }

    fn walk_compound_list(
        &self,
        list: &CompoundList,
        scopes: &mut Vec<ShellScope>,
        cwd: &mut WorkingDirState,
        out: &mut Vec<InterceptedCommand>,
    ) {
        let has_multiple_items = list.0.len() > 1;
        for CompoundListItem(and_or, sep) in &list.0 {
            let is_async = matches!(sep, SeparatorOperator::Async);
            let pushed_chain =
                has_multiple_items && push_unique_scope(scopes, ShellScope::ChainedList);
            let pushed_bg = is_async && push_unique_scope(scopes, ShellScope::Background);
            if is_async {
                let mut bg_cwd = cwd.clone();
                self.walk_and_or_list(and_or, scopes, &mut bg_cwd, out);
            } else {
                self.walk_and_or_list(and_or, scopes, cwd, out);
            }
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
        cwd: &mut WorkingDirState,
        out: &mut Vec<InterceptedCommand>,
    ) {
        let is_chained = !and_or.additional.is_empty();
        let pushed = is_chained && push_unique_scope(scopes, ShellScope::ChainedList);
        let mut before_step = cwd.clone();
        self.walk_pipeline(&and_or.first, scopes, cwd, out);
        for item in &and_or.additional {
            match item {
                AndOr::And(pipeline) => {
                    before_step = cwd.clone();
                    self.walk_pipeline(pipeline, scopes, cwd, out);
                }
                AndOr::Or(pipeline) => {
                    let mut or_cwd = before_step.clone();
                    self.walk_pipeline(pipeline, scopes, &mut or_cwd, out);
                    if or_cwd != before_step {
                        cwd.merge(&or_cwd);
                    }
                }
            }
        }
        if pushed {
            scopes.pop();
        }
    }

    fn walk_pipeline(
        &self,
        pipeline: &Pipeline,
        scopes: &mut Vec<ShellScope>,
        cwd: &mut WorkingDirState,
        out: &mut Vec<InterceptedCommand>,
    ) {
        let is_pipeline = pipeline.seq.len() > 1;
        let pushed = is_pipeline && push_unique_scope(scopes, ShellScope::Pipeline);
        if is_pipeline {
            for cmd in &pipeline.seq {
                let mut stage_cwd = cwd.clone();
                self.walk_command(cmd, scopes, &mut stage_cwd, out);
            }
        } else if let Some(cmd) = pipeline.seq.first() {
            if pipeline.bang {
                let mut negated_cwd = cwd.clone();
                self.walk_command(cmd, scopes, &mut negated_cwd, out);
            } else {
                self.walk_command(cmd, scopes, cwd, out);
            }
        }
        if pushed {
            scopes.pop();
        }
    }

    fn walk_command(
        &self,
        command: &Command,
        scopes: &mut Vec<ShellScope>,
        cwd: &mut WorkingDirState,
        out: &mut Vec<InterceptedCommand>,
    ) {
        match command {
            Command::Simple(simple) => self.walk_simple_command(simple, scopes, cwd, out),
            Command::Compound(compound, redirects) => {
                let redirect_cwd = cwd.clone();
                self.walk_compound_command(compound, scopes, cwd, out);
                let mut sink = NestedSink {
                    scopes,
                    cwd: &redirect_cwd,
                    out,
                };
                self.walk_redirect_list(redirects.as_ref(), &mut sink);
            }
            Command::Function(FunctionDefinition {
                body: FunctionBody(compound, redirects),
                ..
            }) => {
                let pushed = push_unique_scope(scopes, ShellScope::CompoundBlock);
                let redirect_cwd = cwd.clone();
                let mut fn_cwd = cwd.clone();
                self.walk_compound_command(compound, scopes, &mut fn_cwd, out);
                let mut sink = NestedSink {
                    scopes,
                    cwd: &redirect_cwd,
                    out,
                };
                self.walk_redirect_list(redirects.as_ref(), &mut sink);
                if pushed {
                    scopes.pop();
                }
            }
            Command::ExtendedTest(_, redirects) => {
                let mut sink = NestedSink { scopes, cwd, out };
                self.walk_redirect_list(redirects.as_ref(), &mut sink);
            }
        }
    }

    fn walk_compound_command(
        &self,
        compound: &CompoundCommand,
        scopes: &mut Vec<ShellScope>,
        cwd: &mut WorkingDirState,
        out: &mut Vec<InterceptedCommand>,
    ) {
        match compound {
            CompoundCommand::Subshell(SubshellCommand { list, .. }) => {
                let pushed = push_unique_scope(scopes, ShellScope::Subshell);
                let mut sub_cwd = cwd.clone();
                self.walk_compound_list(list, scopes, &mut sub_cwd, out);
                if pushed {
                    scopes.pop();
                }
            }
            CompoundCommand::BraceGroup(brace) => {
                let pushed = push_unique_scope(scopes, ShellScope::CompoundBlock);
                self.walk_compound_list(&brace.list, scopes, cwd, out);
                if pushed {
                    scopes.pop();
                }
            }
            CompoundCommand::ForClause(ForClauseCommand { values, body, .. }) => {
                if let Some(words) = values {
                    let mut sink = NestedSink { scopes, cwd, out };
                    for word in words {
                        let _ = self.parse_word(word, &mut sink);
                    }
                }
                let pushed = push_unique_scope(scopes, ShellScope::CompoundBlock);
                let mut loop_cwd = cwd.clone();
                self.walk_compound_list(&body.list, scopes, &mut loop_cwd, out);
                cwd.merge(&loop_cwd);
                if pushed {
                    scopes.pop();
                }
            }
            CompoundCommand::CaseClause(case_clause) => {
                self.walk_case_clause(case_clause, scopes, cwd, out);
            }
            CompoundCommand::IfClause(if_clause) => {
                self.walk_if_clause(if_clause, scopes, cwd, out);
            }
            CompoundCommand::WhileClause(WhileOrUntilClauseCommand(cond, body, _))
            | CompoundCommand::UntilClause(WhileOrUntilClauseCommand(cond, body, _)) => {
                let pushed = push_unique_scope(scopes, ShellScope::CompoundBlock);
                let mut loop_cwd = cwd.clone();
                self.walk_compound_list(cond, scopes, &mut loop_cwd, out);
                self.walk_compound_list(&body.list, scopes, &mut loop_cwd, out);
                cwd.merge(&loop_cwd);
                if pushed {
                    scopes.pop();
                }
            }
            CompoundCommand::ArithmeticForClause(arith_for) => {
                let pushed = push_unique_scope(scopes, ShellScope::CompoundBlock);
                let mut loop_cwd = cwd.clone();
                self.walk_compound_list(&arith_for.body.list, scopes, &mut loop_cwd, out);
                cwd.merge(&loop_cwd);
                if pushed {
                    scopes.pop();
                }
            }
            CompoundCommand::Coprocess(coproc) => {
                let mut coproc_cwd = cwd.clone();
                self.walk_command(&coproc.body, scopes, &mut coproc_cwd, out);
            }
            CompoundCommand::Arithmetic(_) => {}
        }
    }

    fn walk_case_clause(
        &self,
        CaseClauseCommand { value, cases, .. }: &CaseClauseCommand,
        scopes: &mut Vec<ShellScope>,
        cwd: &mut WorkingDirState,
        out: &mut Vec<InterceptedCommand>,
    ) {
        {
            let mut sink = NestedSink { scopes, cwd, out };
            let _ = self.parse_word(value, &mut sink);
        }
        let pushed = push_unique_scope(scopes, ShellScope::CompoundBlock);
        let entry_cwd = cwd.clone();
        let mut merged_cwd = entry_cwd.clone();
        for case_item in cases {
            {
                let mut sink = NestedSink {
                    scopes,
                    cwd: &entry_cwd,
                    out,
                };
                for pattern in &case_item.patterns {
                    let _ = self.parse_word(pattern, &mut sink);
                }
            }
            if let Some(case_commands) = &case_item.cmd {
                let mut arm_cwd = entry_cwd.clone();
                self.walk_compound_list(case_commands, scopes, &mut arm_cwd, out);
                merged_cwd.merge(&arm_cwd);
            }
        }
        *cwd = merged_cwd;
        if pushed {
            scopes.pop();
        }
    }

    fn walk_if_clause(
        &self,
        IfClauseCommand {
            condition,
            then,
            elses,
            ..
        }: &IfClauseCommand,
        scopes: &mut Vec<ShellScope>,
        cwd: &mut WorkingDirState,
        out: &mut Vec<InterceptedCommand>,
    ) {
        let pushed = push_unique_scope(scopes, ShellScope::CompoundBlock);
        let entry_cwd = cwd.clone();
        let mut then_cwd = entry_cwd.clone();
        self.walk_compound_list(condition, scopes, &mut then_cwd, out);
        self.walk_compound_list(then, scopes, &mut then_cwd, out);
        let mut merged_cwd = then_cwd;
        let mut has_unconditional_else = false;
        if let Some(else_clauses) = elses {
            for else_clause in else_clauses {
                let mut else_cwd = entry_cwd.clone();
                if let Some(else_cond) = &else_clause.condition {
                    self.walk_compound_list(else_cond, scopes, &mut else_cwd, out);
                } else {
                    has_unconditional_else = true;
                }
                self.walk_compound_list(&else_clause.body, scopes, &mut else_cwd, out);
                merged_cwd.merge(&else_cwd);
            }
        }
        if !has_unconditional_else {
            merged_cwd.merge(&entry_cwd);
        }
        *cwd = merged_cwd;
        if pushed {
            scopes.pop();
        }
    }

    fn walk_redirect_list(&self, redirects: Option<&RedirectList>, sink: &mut NestedSink<'_>) {
        if let Some(RedirectList(list)) = redirects {
            for redirect in list {
                let _ = self.parse_io_redirect(redirect, sink);
            }
        }
    }

    fn walk_simple_command(
        &self,
        simple: &SimpleCommand,
        scopes: &mut Vec<ShellScope>,
        cwd: &mut WorkingDirState,
        out: &mut Vec<InterceptedCommand>,
    ) {
        let mut env_assignments = Vec::new();
        let mut redirects = Vec::new();
        let mut arguments = Vec::new();
        let mut nested_commands = Vec::new();

        let program = {
            let mut sink = NestedSink {
                scopes,
                cwd,
                out: &mut nested_commands,
            };

            if let Some(prefix) = &simple.prefix {
                for item in &prefix.0 {
                    self.collect_prefix_or_suffix_item(
                        item,
                        &mut sink,
                        AssignmentSink::Env(&mut env_assignments),
                        &mut redirects,
                        &mut arguments,
                    );
                }
            }

            let program = simple
                .word_or_name
                .as_ref()
                .map(|word| self.parse_word(word, &mut sink));

            if let Some(suffix) = &simple.suffix {
                for item in &suffix.0 {
                    self.collect_prefix_or_suffix_item(
                        item,
                        &mut sink,
                        AssignmentSink::SuffixArg,
                        &mut redirects,
                        &mut arguments,
                    );
                }
            }

            program
        };

        if let Some(program) = program {
            let cd_effect = classify_cd_effect(&program, &arguments);
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
                working_dir: cwd.clone(),
            });
            match cd_effect {
                CdEffect::None => {}
                CdEffect::Static(target) => cwd.apply_static_cd(Path::new(&target)),
                CdEffect::Unknown => *cwd = WorkingDirState::Unknown,
            }
        }

        out.extend(nested_commands);
    }

    fn collect_prefix_or_suffix_item(
        &self,
        item: &CommandPrefixOrSuffixItem,
        sink: &mut NestedSink<'_>,
        assignment_sink: AssignmentSink<'_>,
        redirects: &mut Vec<CommandRedirect>,
        arguments: &mut Vec<CommandArg>,
    ) {
        match item {
            CommandPrefixOrSuffixItem::AssignmentWord(assignment, full_word) => {
                match assignment_sink {
                    AssignmentSink::Env(env_assignments) => {
                        env_assignments.push(self.parse_assignment(assignment, full_word, sink));
                    }
                    AssignmentSink::SuffixArg => match &assignment.value {
                        AssignmentValue::Scalar(_) => {
                            arguments.push(self.parse_word(full_word, sink));
                        }
                        AssignmentValue::Array(_) => {
                            let assigned = self.parse_assignment(assignment, full_word, sink);
                            arguments.push(CommandArg {
                                text: full_word.value.clone(),
                                span: assigned.span,
                                has_expansion: assigned.value.has_expansion,
                            });
                        }
                    },
                }
            }
            CommandPrefixOrSuffixItem::Word(word) => {
                arguments.push(self.parse_word(word, sink));
            }
            CommandPrefixOrSuffixItem::IoRedirect(io_redirect) => {
                if let Some(redirect) = self.parse_io_redirect(io_redirect, sink) {
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
                let pushed = push_unique_scope(sink.scopes, ShellScope::ProcessSubstitution);
                let mut proc_cwd = sink.cwd.clone();
                self.walk_compound_list(&subshell.list, sink.scopes, &mut proc_cwd, sink.out);
                if pushed {
                    sink.scopes.pop();
                }
            }
        }
    }

    fn parse_assignment(
        &self,
        assignment: &Assignment,
        full_word: &Word,
        sink: &mut NestedSink<'_>,
    ) -> EnvAssignment {
        let name = match &assignment.name {
            AssignmentName::VariableName(var) => var.clone(),
            AssignmentName::ArrayElementName(var, subscript) => format!("{var}[{subscript}]"),
        };
        let mut full_span = self.word_span(full_word);
        let after_name = full_word
            .value
            .get(name.len()..)
            .unwrap_or(&full_word.value);
        let value_offset = after_name
            .find('=')
            .map_or(0, |index| name.len() + index + 1);
        let value_span = SourceSpan::new(full_span.start + value_offset, full_span.end);
        let value = match &assignment.value {
            AssignmentValue::Scalar(val_word) => {
                let scalar_span = val_word
                    .location()
                    .map_or(value_span, |loc| self.char_offsets.map_span(&loc));
                self.parse_word_at_span(&val_word.value, scalar_span, sink)
            }
            AssignmentValue::Array(_) => {
                self.parse_array_assignment(&mut full_span, value_span, sink)
            }
        };
        EnvAssignment {
            name,
            value,
            append: assignment.append,
            span: full_span,
        }
    }

    fn parse_array_assignment(
        &self,
        full_span: &mut SourceSpan,
        fallback_value_span: SourceSpan,
        sink: &mut NestedSink<'_>,
    ) -> CommandArg {
        let mut has_expansion = false;
        let mut value_span = fallback_value_span;
        let search_start = self.array_token_cursor.get();
        let head_offset = self.tokens[search_start..].windows(2).position(|pair| {
            matches!(&pair[0], Token::Word(head, _) if head.ends_with('='))
                && matches!(&pair[1], Token::Operator(op, _) if op == "(")
        });
        if let Some(relative_head) = head_offset {
            let head_index = search_start + relative_head;
            full_span.start = self.token_spans[head_index].start;
            value_span.start = self.token_spans[head_index + 1].start;
            let mut depth = 1usize;
            for index in (head_index + 2)..self.tokens.len() {
                let token_span = self.token_spans[index];
                match &self.tokens[index] {
                    Token::Operator(op, _) if op == "(" => depth += 1,
                    Token::Operator(op, _) if op == ")" => {
                        depth -= 1;
                        if depth == 0 {
                            full_span.end = token_span.end;
                            value_span.end = token_span.end;
                            self.array_token_cursor.set(index + 1);
                            break;
                        }
                    }
                    Token::Word(word_text, _) if depth == 1 => {
                        has_expansion |= self
                            .parse_word_at_span(word_text, token_span, sink)
                            .has_expansion;
                    }
                    _ => {}
                }
            }
        }
        CommandArg {
            text: String::new(),
            span: value_span,
            has_expansion,
        }
    }

    fn parse_io_redirect(
        &self,
        io_redirect: &IoRedirect,
        sink: &mut NestedSink<'_>,
    ) -> Option<CommandRedirect> {
        match io_redirect {
            IoRedirect::File(raw_fd, kind, target) => {
                let fd = raw_fd.and_then(|value| u32::try_from(value).ok());
                self.parse_file_redirect(fd, raw_fd.is_some(), kind, target, sink)
            }
            IoRedirect::OutputAndError(word, append) => {
                let arg = self.parse_word(word, sink);
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
                let arg = self.parse_word(word, sink);
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

    fn parse_file_redirect(
        &self,
        fd: Option<u32>,
        has_explicit_fd: bool,
        kind: &IoFileRedirectKind,
        target: &IoFileRedirectTarget,
        sink: &mut NestedSink<'_>,
    ) -> Option<CommandRedirect> {
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
                let arg = self.parse_word(word, sink);
                let span = arg.span;
                (RedirectTarget::File(arg), span)
            }
            IoFileRedirectTarget::Duplicate(word) => {
                let arg = self.parse_word(word, sink);
                let span = arg.span;
                let dup_target = arg
                    .text
                    .parse::<u32>()
                    .map_or(RedirectTarget::File(arg), RedirectTarget::Fd);
                (dup_target, span)
            }
            IoFileRedirectTarget::ProcessSubstitution(_, subshell) => {
                let sub_span = self.char_offsets.map_span(&subshell.loc);
                let pushed = push_unique_scope(sink.scopes, ShellScope::ProcessSubstitution);
                let mut proc_cwd = sink.cwd.clone();
                self.walk_compound_list(&subshell.list, sink.scopes, &mut proc_cwd, sink.out);
                if pushed {
                    sink.scopes.pop();
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
            span: self.extend_redirect_span_left(target_span, has_explicit_fd),
        })
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

    fn parse_word(&self, word: &Word, sink: &mut NestedSink<'_>) -> CommandArg {
        self.parse_word_at_span(&word.value, self.word_span(word), sink)
    }

    fn parse_word_at_span(
        &self,
        raw: &str,
        span: SourceSpan,
        sink: &mut NestedSink<'_>,
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
            sink,
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
        sink: &mut NestedSink<'_>,
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
                        sink,
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
                    let pushed = push_unique_scope(sink.scopes, ShellScope::CommandSubstitution);
                    let _ = parse_script_into(
                        inner_script,
                        inner_base,
                        sink.scopes,
                        sink.cwd,
                        sink.out,
                    );
                    if pushed {
                        sink.scopes.pop();
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
                    let pushed = push_unique_scope(sink.scopes, ShellScope::CommandSubstitution);
                    let _ = parse_script_into(
                        inner_script,
                        inner_base,
                        sink.scopes,
                        sink.cwd,
                        sink.out,
                    );
                    if pushed {
                        sink.scopes.pop();
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

struct NestedSink<'a> {
    scopes: &'a mut Vec<ShellScope>,
    cwd: &'a WorkingDirState,
    out: &'a mut Vec<InterceptedCommand>,
}

enum AssignmentSink<'a> {
    Env(&'a mut Vec<EnvAssignment>),
    SuffixArg,
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

enum CdEffect {
    None,
    Static(String),
    Unknown,
}

/// Classifies the working-directory effect of a shell command (`cd`, `builtin cd`, `command cd`,
/// `pushd`, or `popd`).
///
/// Environment-dependent directory jumps (bare `cd`, `cd -`, `cd old new`, `cd "$DIR"`,
/// `cd ~/ws`, `pushd`, `popd`) return [`CdEffect::Unknown`] so later commands never inherit a
/// stale previous directory.
fn classify_cd_effect(program: &CommandArg, arguments: &[CommandArg]) -> CdEffect {
    let Some(effective) = unwrap_builtin_wrapper(program, arguments) else {
        return CdEffect::None;
    };
    let cd_args = match effective.program {
        "cd" => effective.arguments,
        "pushd" | "popd" => return CdEffect::Unknown,
        _ => return CdEffect::None,
    };

    let mut options_done = false;
    let mut target: Option<&CommandArg> = None;
    for arg in cd_args {
        if !options_done {
            if arg.text == "--" && !arg.has_expansion {
                options_done = true;
                continue;
            }
            if arg.text == "-" {
                return CdEffect::Unknown;
            }
            if !arg.has_expansion && arg.text.starts_with('-') {
                let valid_flags = arg.text[1..]
                    .chars()
                    .all(|flag| matches!(flag, 'L' | 'P' | 'e' | '@'));
                if valid_flags {
                    continue;
                }
                return CdEffect::Unknown;
            }
            options_done = true;
        }
        if target.is_some() {
            return CdEffect::Unknown;
        }
        target = Some(arg);
    }

    let Some(chosen) = target else {
        return CdEffect::Unknown;
    };
    if chosen.has_expansion || chosen.text.is_empty() || chosen.text == "-" {
        CdEffect::Unknown
    } else {
        CdEffect::Static(chosen.text.clone())
    }
}

struct EffectiveCommand<'a> {
    program: &'a str,
    arguments: &'a [CommandArg],
}

fn unwrap_builtin_wrapper<'a>(
    program: &'a CommandArg,
    arguments: &'a [CommandArg],
) -> Option<EffectiveCommand<'a>> {
    if program.has_expansion {
        return None;
    }
    if matches!(program.text.as_str(), "builtin" | "command") {
        let mut rest = arguments;
        if rest
            .first()
            .is_some_and(|arg| !arg.has_expansion && arg.text == "--")
        {
            rest = &rest[1..];
        }
        let (first, tail) = rest.split_first()?;
        (!first.has_expansion).then_some(EffectiveCommand {
            program: first.text.as_str(),
            arguments: tail,
        })
    } else {
        Some(EffectiveCommand {
            program: program.text.as_str(),
            arguments,
        })
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
