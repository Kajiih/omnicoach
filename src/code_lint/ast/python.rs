//! AST helper predicates and structural extractors for Python.

mod annotations;
mod calls;
mod classes;
mod complexity;
mod declaration_order;
mod docstrings;
mod format_strings;
mod functions;
mod literals;
mod logging;
mod parameter_usage;
mod positional_reads;
mod returns;
mod scopes;
mod strings;
mod test_analysis;

pub use self::annotations::{
    AnnotationTraversalDepth, CollectionKind, CollectionShape, PythonCollectionType,
    PythonReturnTypeBranch, PythonReturnTypeUnion, collect_collection_types, collection_display,
    collection_type, generator_yield_type, return_type_union,
};
pub(super) use self::calls::collect_call_candidates;
pub use self::calls::{PythonEnvironSubscript, call_callee, collect_environ_subscripts};
pub use self::classes::{
    PythonAnnotatedAttribute, PythonAttributeDefinition, PythonBaseClass, PythonClassInfo,
    PythonInheritedDocstring, PythonInstanceAttributeAnnotation, collect_class_attributes,
    collect_instance_attribute_annotations, extract_classes, inherited_method_docstring,
    is_fake_class_name, local_ancestors, local_base_classes,
};
pub use self::complexity::PythonComplexityMetric;
pub use self::declaration_order::{
    PythonStatementAfterMainGuard, collect_statements_after_main_guard,
};
pub(super) use self::declaration_order::{
    collect_declaration_scopes, collect_members_after_methods,
};
pub use self::docstrings::{
    DocstringEntry, DocstringLine, DocstringSection, DocstringSectionKind, DocstringTypedLine,
    PythonDocstring, is_clause_end, is_clause_start, leading_verb, none_words,
    normalize_docstring_text, nullability_markers, parse_google_sections, without_byte_ranges,
};
pub use self::format_strings::{
    PythonFormatPlaceholder, PythonFormatString, PythonFormatStyle, collect_format_strings,
    extract_valid_field_root, named_format_field_roots,
};
pub use self::functions::{
    PythonFunctionSignature, PythonParameterInfo, PythonParameterKind, extract_function_signatures,
    find_nested_classes, find_nested_functions,
};
pub(super) use self::literals::collect_literal_occurrences;
pub use self::literals::{PythonModuleAssignment, collect_module_assignments};
pub use self::logging::{PythonLoggerCall, collect_logger_calls};
pub use self::parameter_usage::{
    ParameterCollectionCapability, ParameterUsage, summarize_parameter_usages,
};
pub(super) use self::positional_reads::collect_positional_reads;
pub(super) use self::returns::collect_function_returns;
pub use self::returns::{PythonBodyResults, collect_locally_mutated_return_functions};
pub(super) use self::scopes::collect_bindings;
pub(super) use self::strings::find_unwrapped_multiline_strings;
pub(super) use self::test_analysis::{
    collect_assertion_checks, collect_multi_phase_test_functions,
    collect_test_function_assertion_counts, collect_test_logic_constructs,
    collect_test_setup_scopes, collect_test_to_test_calls,
};

use crate::code_lint::ast::{
    AstNode, ParsedFile, QualifiedName, ResolvedName, resolve_name, span_from_ruff_range,
};
use crate::diagnostic::SourceSpan;
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_expr, walk_stmt};
use ruff_python_ast::{Decorator, Expr, ModModule, Stmt};
use ruff_text_size::Ranged as _;

/// Methods that mutate a `list`, `dict` or `set` in place.
const MUTATING_METHODS: &[&str] = &[
    "append",
    "extend",
    "insert",
    "pop",
    "remove",
    "clear",
    "sort",
    "reverse",
    "update",
    "setdefault",
    "popitem",
    "add",
    "discard",
    "intersection_update",
    "difference_update",
    "symmetric_difference_update",
];

/// Structured metadata for a Python decorator.
#[derive(Clone)]
pub(super) struct DecoratorInfo {
    /// Full path string as written (e.g. `"dataclasses.dataclass"`, `"pytest.mark.parametrize"`).
    pub(super) path: String,
    /// Terminal identifier as written (e.g. `"dataclass"`, `"parametrize"`).
    pub(super) terminal_name: String,
    /// Canonical path resolved through [`resolve_name`], or `None` when the first segment is a
    /// module-level definition in the file.
    resolved_path: Option<String>,
    /// Keyword argument names if invoked as a call.
    keyword_args: Vec<String>,
}

impl DecoratorInfo {
    /// Returns true if a keyword argument with `key` was explicitly passed.
    #[must_use]
    pub(super) fn has_arg(&self, key: &str) -> bool {
        self.keyword_args.iter().any(|arg| arg == key)
    }

    /// Returns true if the import-resolved decorator path equals one of `candidates`,
    /// excluding decorators whose first segment is defined in the file.
    #[must_use]
    pub(super) fn matches_any(&self, candidates: &[&str]) -> bool {
        self.resolved_path
            .as_deref()
            .is_some_and(|path| candidates.contains(&path))
    }

    /// Returns true if this decorator is an attribute access (`@<receiver>.<attr>`) whose
    /// attribute name is in `attrs` (e.g. `@prop.setter`, `@func.register`).
    #[must_use]
    pub(super) fn is_attribute_named(&self, attrs: &[&str]) -> bool {
        self.path.contains('.') && attrs.contains(&self.terminal_name.as_str())
    }
}

/// Finds the `Expr` node in `module` whose byte span equals `target_span`.
pub(super) fn find_expr_at_span(module: &ModModule, target_span: SourceSpan) -> Option<&Expr> {
    struct ExprFinder<'a> {
        target_span: SourceSpan,
        found: Option<&'a Expr>,
    }

    impl<'a> SourceOrderVisitor<'a> for ExprFinder<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            let span = span_from_ruff_range(statement.range());
            if self.found.is_none()
                && span.start <= self.target_span.start
                && self.target_span.end <= span.end
            {
                walk_stmt(self, statement);
            }
        }

        fn visit_expr(&mut self, expr: &'a Expr) {
            if self.found.is_some() {
                return;
            }
            let span = span_from_ruff_range(expr.range());
            if !(span.start <= self.target_span.start && self.target_span.end <= span.end) {
                return;
            }
            if span == self.target_span {
                self.found = Some(expr);
                return;
            }
            walk_expr(self, expr);
        }
    }

    let mut finder = ExprFinder {
        target_span,
        found: None,
    };
    finder.visit_body(&module.body);
    finder.found
}

/// Converts a slice of `ruff_python_ast::Decorator` nodes into `DecoratorInfo` values.
pub(super) fn extract_decorators_from_slice(
    decorators: &[Decorator],
    file: &ParsedFile,
) -> Vec<DecoratorInfo> {
    let mut out = Vec::with_capacity(decorators.len());
    for decorator in decorators {
        let (target_expr, keyword_args) = if let Expr::Call(call) = &decorator.expression {
            let kwargs = call
                .arguments
                .keywords
                .iter()
                .filter_map(|kw| Some(kw.arg.as_ref()?.id.to_string()))
                .collect();
            (call.func.as_ref(), kwargs)
        } else {
            (&decorator.expression, Vec::new())
        };

        let QualifiedName { path, terminal } =
            resolve_path_and_terminal_expr(target_expr, &file.source);
        let resolved_path =
            resolved_path_and_terminal_expr(target_expr, file).map(|name| name.path);
        out.push(DecoratorInfo {
            path,
            terminal_name: terminal,
            resolved_path,
            keyword_args,
        });
    }
    out
}

/// Returns true if `expr` contains any call expression (`Expr::Call`).
pub(super) fn expr_contains_call(expr: &Expr) -> bool {
    struct CallDetector {
        found: bool,
    }

    impl<'a> SourceOrderVisitor<'a> for CallDetector {
        fn visit_expr(&mut self, expr: &'a Expr) {
            if self.found {
                return;
            }
            if matches!(expr, Expr::Call(_)) {
                self.found = true;
                return;
            }
            walk_expr(self, expr);
        }
    }

    let mut detector = CallDetector { found: false };
    detector.visit_expr(expr);
    detector.found
}

/// If `expr` mutates a collection receiver in place (`receiver.append(...)`, `receiver[k] = v`,
/// `del receiver[k]`), returns that `receiver` expression.
pub(super) fn in_place_mutated_receiver_expr(expr: &Expr) -> Option<&Expr> {
    match expr {
        Expr::Call(call) => {
            let Expr::Attribute(attr) = call.func.as_ref() else {
                return None;
            };
            if MUTATING_METHODS.contains(&attr.attr.as_str()) {
                Some(&attr.value)
            } else {
                None
            }
        }
        Expr::Subscript(sub)
            if matches!(
                sub.ctx,
                ruff_python_ast::ExprContext::Store | ruff_python_ast::ExprContext::Del
            ) =>
        {
            Some(&sub.value)
        }
        _ => None,
    }
}

/// Helper to resolve the dotted expression path and terminal identifier from an `Expr`.
pub(super) fn resolve_path_and_terminal_expr(expr: &Expr, source: &str) -> QualifiedName {
    let span = span_from_ruff_range(expr.range());
    let path = source[span.start..span.end].to_string();
    let terminal = if let Expr::Attribute(attr) = expr {
        attr.attr.to_string()
    } else {
        path.rsplit('.').next().unwrap_or("").to_string()
    };
    QualifiedName { path, terminal }
}

/// The name that `expr` names, resolved through the imports of `file`
/// (`t.List` after `import typing as t` is `typing.List` with terminal `List`), or `None`
/// if it names a definition of `file` (see [`resolve_name`]).
pub(super) fn resolved_path_and_terminal_expr(
    expr: &Expr,
    file: &ParsedFile,
) -> Option<QualifiedName> {
    let name = resolve_path_and_terminal_expr(expr, &file.source);
    match resolve_name(file, &name.path) {
        ResolvedName::Imported(resolved) => {
            let terminal = resolved
                .rsplit_once('.')
                .map_or(resolved.as_str(), |(_, terminal)| terminal)
                .to_owned();
            Some(QualifiedName {
                path: resolved,
                terminal,
            })
        }
        ResolvedName::Local => None,
        ResolvedName::Unbound => Some(name),
    }
}

/// Resolves the byte span of the header of the innermost Python statement enclosing `span`.
pub(super) fn enclosing_statement_header_span(
    file: &ParsedFile,
    span: SourceSpan,
) -> Option<SourceSpan> {
    use ruff_python_ast::token::TokenKind;

    struct StatementFinder<'a> {
        target_span: SourceSpan,
        found: Option<&'a Stmt>,
    }

    impl<'a> SourceOrderVisitor<'a> for StatementFinder<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            let statement_span = span_from_ruff_range(statement.range());
            if !(statement_span.start <= self.target_span.start
                && self.target_span.end <= statement_span.end)
            {
                return;
            }
            self.found = Some(statement);
            walk_stmt(self, statement);
        }
    }

    let parsed = file.py_module()?;
    let mut finder = StatementFinder {
        target_span: span,
        found: None,
    };
    finder.visit_body(&parsed.syntax().body);
    let statement = finder.found?;

    let statement_range = statement.range();
    let start_offset = statement_range.start().to_usize();
    let end_offset = python_statement_body_start(statement).map_or_else(
        || statement_range.end().to_usize(),
        |body_start| {
            parsed
                .tokens()
                .iter()
                .rfind(|token| {
                    token.range().start() >= statement_range.start()
                        && token.range().end() <= body_start
                        && !matches!(
                            token.kind(),
                            TokenKind::Comment
                                | TokenKind::Newline
                                | TokenKind::NonLogicalNewline
                                | TokenKind::Indent
                                | TokenKind::Dedent
                        )
                })
                .map_or(start_offset, |token| token.range().end().to_usize())
        },
    );
    Some(SourceSpan::new(start_offset, end_offset))
}

/// Records the byte span of every Python decorator in `file`.
pub(super) fn record_decorator_and_attribute_spans(
    file: &ParsedFile,
    record: &mut impl FnMut(SourceSpan),
) {
    struct DecoratorVisitor<Record> {
        record: Record,
    }

    impl<'a, Record: FnMut(SourceSpan)> SourceOrderVisitor<'a> for DecoratorVisitor<Record> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            let decorators = match statement {
                Stmt::FunctionDef(function) => function.decorator_list.as_slice(),
                Stmt::ClassDef(class) => class.decorator_list.as_slice(),
                _ => &[],
            };
            for decorator in decorators {
                (self.record)(span_from_ruff_range(decorator.range()));
            }
            walk_stmt(self, statement);
        }
    }

    if let Some(parsed) = file.py_module() {
        DecoratorVisitor { record }.visit_body(&parsed.syntax().body);
    }
}

/// Returns the byte offset where the body of a Python compound `statement` begins, or `None`
/// for simple statements without a suite.
fn python_statement_body_start(statement: &Stmt) -> Option<ruff_text_size::TextSize> {
    match statement {
        Stmt::FunctionDef(node) => node.body.first().map(|first| first.range().start()),
        Stmt::ClassDef(node) => node.body.first().map(|first| first.range().start()),
        Stmt::For(node) => node.body.first().map(|first| first.range().start()),
        Stmt::While(node) => node.body.first().map(|first| first.range().start()),
        Stmt::If(node) => node.body.first().map(|first| first.range().start()),
        Stmt::With(node) => node.body.first().map(|first| first.range().start()),
        Stmt::Try(node) => node.body.first().map(|first| first.range().start()),
        Stmt::Match(node) => node.cases.first().map(|first| first.range().start()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Language;
    use ruff_python_ast::StmtFunctionDef;

    /// Returns the first top-level function definition of `file`.
    fn first_function_def(file: &ParsedFile) -> &StmtFunctionDef {
        let parsed = file.py_module().expect("source should parse");
        let Some(Stmt::FunctionDef(func_def)) = parsed.syntax().body.first() else {
            unreachable!("source should start with a function definition");
        };
        func_def
    }

    /// Extracts the decorators of the first top-level function definition in `file`.
    fn first_function_decorators(file: &ParsedFile) -> Vec<DecoratorInfo> {
        extract_decorators_from_slice(&first_function_def(file).decorator_list, file)
    }

    #[test]
    fn test_extract_decorators_count() {
        let source = indoc::indoc! {r#"
            @dataclass(frozen=True, slots=False)
            @pytest.mark.parametrize("x", [1, 2])
            @custom
            def foo():
                pass
        "#};
        let file = ParsedFile::new(source, Language::Python);
        let decorators = first_function_decorators(&file);
        assert_eq!(decorators.len(), 3);
    }

    #[test]
    fn test_extract_decorator_keyword_args() {
        let source = indoc::indoc! {r"
            @dataclass(frozen=True, slots=False)
            def foo():
                pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let decorators = first_function_decorators(&file);

        let dec0 = &decorators[0];
        assert_eq!(dec0.terminal_name, "dataclass");
        assert!(dec0.has_arg("frozen"));
        assert!(dec0.has_arg("slots"));
        assert!(!dec0.has_arg("unknown"));
    }

    #[test]
    fn test_extract_decorators_metadata_and_path() {
        let source = indoc::indoc! {r#"
            @pytest.mark.parametrize("x", [1, 2])
            @custom
            def foo():
                pass
        "#};
        let file = ParsedFile::new(source, Language::Python);
        let decorators = first_function_decorators(&file);
        let dec0 = &decorators[0];
        let dec1 = &decorators[1];

        assert_eq!(dec0.terminal_name, "parametrize");
        assert_eq!(dec0.path, "pytest.mark.parametrize");
        assert_eq!(dec1.terminal_name, "custom");
        assert_eq!(dec1.path, "custom");
    }
}
