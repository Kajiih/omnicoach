//! Shell tokenization and AST lowering backed by `brush-parser`, compiled on every target except browser `wasm32-unknown-unknown`.

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
    ShellScope, SourceSpan,
};

/// Tokenizes and parses `source` into `out`, returning `true` when the script is valid shell syntax.
pub(super) fn parse_script_into(
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
        tokens: &tokens,
        char_offsets: &char_offsets,
        token_spans: &token_spans,
        parser_options: &parser_options,
        array_token_cursor: std::cell::Cell::new(0),
    };
    walker.walk_program(&program, scopes, out);
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
        out: &mut Vec<InterceptedCommand>,
    ) {
        let multiple_lines = program.complete_commands.len() > 1;
        let pushed = multiple_lines && push_unique_scope(scopes, ShellScope::ChainedList);
        for list in &program.complete_commands {
            self.walk_compound_list(list, scopes, out);
        }
        if pushed {
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
            CompoundCommand::Coprocess(coproc) => {
                self.walk_command(&coproc.body, scopes, out);
            }
            CompoundCommand::Arithmetic(_) => {}
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
                    AssignmentSink::Env(&mut env_assignments),
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
                    AssignmentSink::SuffixArg,
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
        assignment_sink: AssignmentSink<'_>,
        redirects: &mut Vec<CommandRedirect>,
        arguments: &mut Vec<CommandArg>,
        nested_commands: &mut Vec<InterceptedCommand>,
    ) {
        match item {
            CommandPrefixOrSuffixItem::AssignmentWord(assignment, full_word) => {
                match assignment_sink {
                    AssignmentSink::Env(env_assignments) => {
                        env_assignments.push(self.parse_assignment(
                            assignment,
                            full_word,
                            scopes,
                            nested_commands,
                        ));
                    }
                    AssignmentSink::SuffixArg => match &assignment.value {
                        AssignmentValue::Scalar(_) => {
                            arguments.push(self.parse_word(full_word, scopes, nested_commands));
                        }
                        AssignmentValue::Array(_) => {
                            let assigned = self.parse_assignment(
                                assignment,
                                full_word,
                                scopes,
                                nested_commands,
                            );
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
                self.parse_word_at_span(&val_word.value, scalar_span, scopes, nested_commands)
            }
            AssignmentValue::Array(_) => {
                self.parse_array_assignment(&mut full_span, value_span, scopes, nested_commands)
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
        scopes: &mut Vec<ShellScope>,
        nested_commands: &mut Vec<InterceptedCommand>,
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
                            .parse_word_at_span(word_text, token_span, scopes, nested_commands)
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
