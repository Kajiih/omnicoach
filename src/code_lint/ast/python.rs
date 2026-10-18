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
pub use self::classes::{
    PythonAnnotatedAttribute, PythonAttributeDefinition, PythonBaseClass, PythonClassInfo,
    PythonInheritedDocstring, PythonInstanceAttributeAnnotation, collect_class_attributes,
    collect_instance_attribute_annotations, extract_classes, inherited_method_docstring,
    is_fake_class_name, local_ancestors, local_base_classes,
};
pub use self::complexity::PythonComplexityMetric;
pub use self::declaration_order::{PythonFieldAfterMethod, collect_fields_after_methods};
pub(super) use self::declaration_order::{collect_declaration_scopes, collect_type_method_scopes};
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
pub use self::logging::{PythonLoggerCall, collect_logger_calls};
pub use self::parameter_usage::{
    ParameterCollectionCapability, ParameterUsage, summarize_parameter_usages,
};
pub(super) use self::positional_reads::collect_positional_reads;
pub use self::returns::PythonBodyResults;
pub(super) use self::returns::collect_function_returns;
pub(super) use self::scopes::collect_bindings;
pub(super) use self::strings::find_unwrapped_multiline_strings;
pub use self::test_analysis::{PythonAssert, collect_assert_statements};
pub(super) use self::test_analysis::{
    collect_multi_phase_test_functions, collect_test_function_assertion_counts,
    collect_test_logic_constructs, collect_test_setup_scopes, collect_test_to_test_calls,
};

use self::annotations::{has_final_annotation_expr, is_bare_final_annotation_expr};
use self::classes::is_protocol_or_abc_class;
use self::functions::{
    direct_function_definitions, has_override_decorator, method_receiver_name_ast,
};
use self::literals::is_constant_name;
use self::logging::extract_logger_call;
use self::strings::{TemplateParts, fstring_segments_and_interpolations, static_string_text};
use crate::code_lint::ast::{
    AstNode, EnclosingFunction, ParsedFile, QualifiedName, ResolvedName, resolve_name,
    span_from_ruff_range, with_innermost_function,
};
use crate::diagnostic::SourceSpan;
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_expr, walk_stmt};
use ruff_python_ast::{Decorator, ExceptHandler, Expr, ModModule, Stmt};
use ruff_text_size::Ranged as _;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

const ENVIRON_NAME: &str = "environ";

/// The callee of `call` if it is a `call` node, without the type arguments of a generic
/// instantiation (`list[str]()` has callee `list`).
#[must_use]
pub fn call_callee<'a>(call: &AstNode<'a>) -> Option<AstNode<'a>> {
    let Expr::Call(call_expr) = find_expr_at_span(call.file.py_module()?.syntax(), call.span())?
    else {
        return None;
    };
    let callee = if let Expr::Subscript(subscript) = call_expr.func.as_ref() {
        subscript.value.as_ref()
    } else {
        call_expr.func.as_ref()
    };
    Some(AstNode::from_span(
        call.file,
        span_from_ruff_range(callee.range()),
    ))
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

/// A single-name assignment at module level, including inside the bodies of top-level `if`,
/// `try` and `with` statements (`ALLOWED = [...]`, `PORTS: Final = {...}`, `NAME: str`).
pub struct PythonModuleAssignment<'a> {
    /// The assigned name (e.g. `"ALLOWED"` or `"_cache"`).
    pub name: String,
    /// The type annotation, if any.
    pub annotation: Option<AstNode<'a>>,
    /// The assigned value without enclosing parentheses, absent for a bare annotation.
    pub value: Option<AstNode<'a>>,
    /// True if the annotation is `Final` or `Final[T]` (qualified or not, optionally wrapped in
    /// `Annotated`).
    has_final_annotation: bool,
}

impl PythonModuleAssignment<'_> {
    /// Whether the assignment declares a constant: an `UPPER_SNAKE_CASE` name or a `Final`
    /// annotation.
    #[must_use]
    pub fn is_constant(&self) -> bool {
        is_constant_name(&self.name) || self.has_final_annotation
    }
}

/// Collects the single-name module-level assignments of `file`, in source order. Attribute and
/// unpacking targets are not collected.
#[must_use]
pub fn collect_module_assignments(file: &ParsedFile) -> Vec<PythonModuleAssignment<'_>> {
    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect_module_assignments_in_stmts(&parsed.syntax().body, file, &mut out);
    out
}

/// Collects single-name module-level assignments from `stmts`, recursing into top-level
/// `if`, `try`, and `with` blocks.
fn collect_module_assignments_in_stmts<'a>(
    stmts: &[Stmt],
    file: &'a ParsedFile,
    out: &mut Vec<PythonModuleAssignment<'a>>,
) {
    for statement in stmts {
        match statement {
            Stmt::Assign(assign) => {
                if let [Expr::Name(target)] = assign.targets.as_slice() {
                    out.push(PythonModuleAssignment {
                        name: target.id.to_string(),
                        annotation: None,
                        value: Some(AstNode::from_span(
                            file,
                            span_from_ruff_range(assign.value.range()),
                        )),
                        has_final_annotation: false,
                    });
                }
            }
            Stmt::AnnAssign(ann) => {
                if let Expr::Name(target) = ann.target.as_ref() {
                    out.push(PythonModuleAssignment {
                        name: target.id.to_string(),
                        annotation: Some(AstNode::from_span(
                            file,
                            span_from_ruff_range(ann.annotation.range()),
                        )),
                        value: ann
                            .value
                            .as_ref()
                            .map(|val| AstNode::from_span(file, span_from_ruff_range(val.range()))),
                        has_final_annotation: has_final_annotation_expr(&ann.annotation, file),
                    });
                }
            }
            Stmt::If(if_statement) => {
                collect_module_assignments_in_stmts(&if_statement.body, file, out);
                for clause in &if_statement.elif_else_clauses {
                    collect_module_assignments_in_stmts(&clause.body, file, out);
                }
            }
            Stmt::Try(try_statement) => {
                collect_module_assignments_in_stmts(&try_statement.body, file, out);
                for handler in &try_statement.handlers {
                    let ExceptHandler::ExceptHandler(handler_clause) = handler;
                    collect_module_assignments_in_stmts(&handler_clause.body, file, out);
                }
                collect_module_assignments_in_stmts(&try_statement.orelse, file, out);
                collect_module_assignments_in_stmts(&try_statement.finalbody, file, out);
            }
            Stmt::With(with_statement) => {
                collect_module_assignments_in_stmts(&with_statement.body, file, out);
            }
            _ => {}
        }
    }
}

/// A top-level Python statement appearing after the `if __name__ == "__main__":` guard block.
pub struct PythonStatementAfterMainGuard<'a> {
    /// AST node of the trailing statement.
    pub node: AstNode<'a>,
    /// 1-based starting line of the `if __name__ == "__main__":` guard block.
    pub guard_line: usize,
}

/// Collects top-level module statements that appear after an `if __name__ == "__main__":`
/// guard block.
#[must_use]
pub fn collect_statements_after_main_guard(
    file: &ParsedFile,
) -> Vec<PythonStatementAfterMainGuard<'_>> {
    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let body = &parsed.syntax().body;
    let Some(guard_index) = body.iter().position(is_main_guard_statement) else {
        return Vec::new();
    };
    let guard_line =
        AstNode::from_span(file, span_from_ruff_range(body[guard_index].range())).start_line();
    body[guard_index + 1..]
        .iter()
        .filter(|statement| !is_main_guard_statement(statement))
        .map(|statement| PythonStatementAfterMainGuard {
            node: AstNode::from_span(file, span_from_ruff_range(statement.range())),
            guard_line,
        })
        .collect()
}

/// Returns true if `statement` is `if __name__ == "__main__":` or `if "__main__" == __name__:`.
fn is_main_guard_statement(statement: &Stmt) -> bool {
    let Stmt::If(if_statement) = statement else {
        return false;
    };
    let Expr::Compare(compare) = if_statement.test.as_ref() else {
        return false;
    };
    let ([ruff_python_ast::CmpOp::Eq], [left, right]) = (&*compare.ops, &*compare.operands) else {
        return false;
    };
    (is_dunder_name_expr(left) && is_dunder_main_literal(right))
        || (is_dunder_main_literal(left) && is_dunder_name_expr(right))
}

/// Returns true if `expr` is the identifier `__name__`.
fn is_dunder_name_expr(expr: &Expr) -> bool {
    matches!(expr, Expr::Name(name) if name.id.as_str() == "__name__")
}

/// Returns true if `expr` is a string literal with value `"__main__"`.
fn is_dunder_main_literal(expr: &Expr) -> bool {
    matches!(expr, Expr::StringLiteral(literal) if literal.value.to_str() == "__main__")
}

/// Saved state of an [`EnclosingFunctionTracker`] before entering a statement.
pub(super) struct EnclosingFunctionState {
    functions: Arc<[EnclosingFunction]>,
    enclosing_class: Option<String>,
}

/// Tracks the functions enclosing the statement visited by a source-order walk.
#[derive(Default)]
pub(super) struct EnclosingFunctionTracker {
    /// Number of statements enclosing the visited statement.
    depth: usize,
    /// Name of the top-level `class` statement being entered, when visiting its direct body.
    enclosing_class: Option<String>,
    /// The enclosing functions, innermost first.
    pub(super) functions: Arc<[EnclosingFunction]>,
}

impl EnclosingFunctionTracker {
    /// Enters `statement`, which encloses everything visited until the matching [`Self::exit`].
    /// Returns the outer state to pass to [`Self::exit`].
    pub(super) fn enter(&mut self, statement: &Stmt) -> EnclosingFunctionState {
        let state = EnclosingFunctionState {
            functions: Arc::clone(&self.functions),
            enclosing_class: self.enclosing_class.clone(),
        };
        match statement {
            Stmt::FunctionDef(func_def) => {
                let function = EnclosingFunction {
                    name: func_def.name.to_string(),
                    name_span: span_from_ruff_range(func_def.name.range),
                    is_top_level: self.depth == 0,
                    enclosing_class: self.enclosing_class.take(),
                };
                self.functions = with_innermost_function(function, &state.functions);
            }
            Stmt::ClassDef(class_def) => {
                self.enclosing_class = (self.depth == 0).then(|| class_def.name.to_string());
            }
            _ => {
                self.enclosing_class = None;
            }
        }
        self.depth += 1;
        state
    }

    /// Leaves the statement entered by the [`Self::enter`] call that returned `state`.
    pub(super) fn exit(&mut self, state: EnclosingFunctionState) {
        self.depth -= 1;
        self.functions = state.functions;
        self.enclosing_class = state.enclosing_class;
    }
}

/// A Python subscript reading the `os.environ` (or bare `environ`) mapping.
pub struct PythonEnvironSubscript<'a> {
    /// The whole subscript (`os.environ["HOST"]`).
    pub node: AstNode<'a>,
    /// The mapping expression being indexed (`os.environ` or `environ`).
    pub mapping: AstNode<'a>,
    /// The functions enclosing the subscript, innermost first (see
    /// [`AstCallCandidate::enclosing_functions`](crate::code_lint::ast::AstCallCandidate::enclosing_functions)).
    pub enclosing_functions: Arc<[EnclosingFunction]>,
}

/// Collects all Python subscript expressions indexing into `os.environ` or `environ`.
#[must_use]
pub fn collect_environ_subscripts(file: &ParsedFile) -> Vec<PythonEnvironSubscript<'_>> {
    struct EnvironSubscriptCollector<'a> {
        file: &'a ParsedFile,
        enclosing_functions: EnclosingFunctionTracker,
        out: Vec<PythonEnvironSubscript<'a>>,
    }

    impl<'a> SourceOrderVisitor<'a> for EnvironSubscriptCollector<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            let outer_functions = self.enclosing_functions.enter(statement);
            walk_stmt(self, statement);
            self.enclosing_functions.exit(outer_functions);
        }

        fn visit_expr(&mut self, expr: &'a Expr) {
            if let Expr::Subscript(sub) = expr
                && is_environ_mapping_expr(&sub.value)
            {
                self.out.push(PythonEnvironSubscript {
                    node: AstNode::from_span(self.file, span_from_ruff_range(sub.range())),
                    mapping: AstNode::from_span(self.file, span_from_ruff_range(sub.value.range())),
                    enclosing_functions: Arc::clone(&self.enclosing_functions.functions),
                });
            }
            walk_expr(self, expr);
        }
    }

    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut collector = EnvironSubscriptCollector {
        file,
        enclosing_functions: EnclosingFunctionTracker::default(),
        out: Vec::new(),
    };
    collector.visit_body(&parsed.syntax().body);
    collector.out
}

/// Returns true if `value` is Python's `os.environ` or bare `environ`.
fn is_environ_mapping_expr(value: &Expr) -> bool {
    match value {
        Expr::Name(name) => name.id.as_str() == ENVIRON_NAME,
        Expr::Attribute(attr) => {
            attr.attr.as_str() == ENVIRON_NAME
                && matches!(attr.value.as_ref(), Expr::Name(obj) if obj.id.as_str() == "os")
        }
        _ => false,
    }
}

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

/// Collects function or method names whose return values are mutated in place in `file`.
///
/// Matches direct call mutations (`fn().append(...)`, `fn()[0] = 1`) and local bindings
/// (`buf = fn(); buf.append(...)`, `(buf := fn())`). A binding only counts in the function
/// (or module) scope that assigns it. Callees are matched by name only, so a mutated
/// `obj.get()` result also exempts an unrelated function named `get`.
#[must_use]
pub fn collect_locally_mutated_return_functions(file: &ParsedFile) -> &HashSet<String> {
    file.locally_mutated_return_functions.get_or_init(|| {
        // A binding is keyed by its enclosing function (`None` at module level) and its name.
        type ScopedName = (Option<usize>, String);

        struct MutatedReturnVisitor<'a> {
            source: &'a str,
            enclosing_function: Option<usize>,
            mutated_functions: HashSet<String>,
            bindings_to_callee: HashMap<ScopedName, String>,
            mutated_identifiers: HashSet<ScopedName>,
        }

        impl MutatedReturnVisitor<'_> {
            fn record_binding(&mut self, target: &Expr, value: &Expr) {
                if let Expr::Name(name) = target
                    && let Some(callee_name) = called_terminal_name_expr(value, self.source)
                {
                    let scoped_name = (self.enclosing_function, name.id.to_string());
                    self.bindings_to_callee.insert(scoped_name, callee_name);
                }
            }

            fn record_mutated_receiver(&mut self, receiver: &Expr) {
                if let Some(callee_name) = called_terminal_name_expr(receiver, self.source) {
                    self.mutated_functions.insert(callee_name);
                } else if let Expr::Name(name) = receiver {
                    self.mutated_identifiers
                        .insert((self.enclosing_function, name.id.to_string()));
                }
            }
        }

        impl<'a> SourceOrderVisitor<'a> for MutatedReturnVisitor<'a> {
            fn visit_stmt(&mut self, statement: &'a Stmt) {
                match statement {
                    Stmt::FunctionDef(func) => {
                        for dec in &func.decorator_list {
                            self.visit_decorator(dec);
                        }
                        self.visit_parameters(&func.parameters);
                        if let Some(returns) = &func.returns {
                            self.visit_annotation(returns);
                        }
                        let prev_func = self.enclosing_function;
                        self.enclosing_function = Some(func.range().start().to_usize());
                        self.visit_body(&func.body);
                        self.enclosing_function = prev_func;
                    }
                    Stmt::Assign(assign) => {
                        if let [target] = assign.targets.as_slice() {
                            self.record_binding(target, &assign.value);
                        }
                        walk_stmt(self, statement);
                    }
                    Stmt::AnnAssign(ann) => {
                        if let Some(value) = &ann.value {
                            self.record_binding(&ann.target, value);
                        }
                        walk_stmt(self, statement);
                    }
                    Stmt::AugAssign(aug) => {
                        self.record_mutated_receiver(&aug.target);
                        walk_stmt(self, statement);
                    }
                    _ => walk_stmt(self, statement),
                }
            }

            fn visit_expr(&mut self, expr: &'a Expr) {
                if let Expr::Named(named) = expr {
                    self.record_binding(&named.target, &named.value);
                }
                if let Some(receiver) = in_place_mutated_receiver_expr(expr) {
                    self.record_mutated_receiver(receiver);
                }
                walk_expr(self, expr);
            }
        }

        let Some(parsed) = file.py_module() else {
            return HashSet::new();
        };
        let mut visitor = MutatedReturnVisitor {
            source: &file.source,
            enclosing_function: None,
            mutated_functions: HashSet::new(),
            bindings_to_callee: HashMap::new(),
            mutated_identifiers: HashSet::new(),
        };
        visitor.visit_body(&parsed.syntax().body);

        for scoped_name in &visitor.mutated_identifiers {
            if let Some(callee_name) = visitor.bindings_to_callee.get(scoped_name) {
                visitor.mutated_functions.insert(callee_name.clone());
            }
        }

        visitor.mutated_functions
    })
}

/// Extracts the terminal function/method name if `expr` is a `call` expression.
fn called_terminal_name_expr(expr: &Expr, source: &str) -> Option<String> {
    let Expr::Call(call) = expr else {
        return None;
    };
    let terminal = resolve_path_and_terminal_expr(&call.func, source).terminal;
    (!terminal.is_empty()).then_some(terminal)
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

    #[rstest::rstest]
    #[case::annotated_value(
        "ALLOWED: list[str] = (\"a\",)",
        &[("ALLOWED", Some("list[str]"), Some("(\"a\",)"), true)]
    )]
    #[case::parenthesized_value_with_comment(
        "ALLOWED = (\n    # Default roles\n    [\"admin\"]\n)",
        &[("ALLOWED", None, Some("[\"admin\"]"), true)]
    )]
    #[case::bare_annotation("NAME: str", &[("NAME", Some("str"), None, true)])]
    #[case::lowercase_final(
        "from typing import Final\nallowed: Final = 1",
        &[("allowed", Some("Final"), Some("1"), true)]
    )]
    #[case::lowercase_variable("cache = {}", &[("cache", None, Some("{}"), false)])]
    #[case::inside_top_level_if("if FLAG:\n    HOSTS = []", &[("HOSTS", None, Some("[]"), true)])]
    #[case::other_targets_and_scopes_not_collected(
        "config.X = 1\nA, B = 1, 2\ndef f():\n    LOCAL = 1\nclass C:\n    ATTR = 1",
        &[]
    )]
    fn test_collect_module_assignments(
        #[case] source: &str,
        #[case] expected: &[(&str, Option<&str>, Option<&str>, bool)],
    ) {
        let file = ParsedFile::new(source, Language::Python);
        let assignments = collect_module_assignments(&file);
        let collected: Vec<_> = assignments
            .iter()
            .map(|assignment| {
                (
                    assignment.name.as_str(),
                    assignment.annotation.as_ref().map(AstNode::text),
                    assignment.value.as_ref().map(AstNode::text),
                    assignment.is_constant(),
                )
            })
            .collect();
        let expected: Vec<_> = expected
            .iter()
            .map(|&(name, annotation, value, is_constant)| {
                (
                    name,
                    annotation.map(Into::into),
                    value.map(Into::into),
                    is_constant,
                )
            })
            .collect();
        assert_eq!(collected, expected);
    }

    #[rstest::rstest]
    #[case::direct_call_mutation("make().append(1)", &["make"])]
    #[case::assigned_then_mutated("def use():\n    items = make()\n    items.append(1)", &["make"])]
    #[case::walrus_then_mutated("def use():\n    if (items := make()):\n        items.append(1)", &["make"])]
    #[case::subscript_write("def use():\n    items = make()\n    items[0] = 1", &["make"])]
    #[case::read_only_use("def use():\n    items = make()\n    return len(items)", &[])]
    #[case::binding_in_other_function("def a():\n    items = make()\ndef b():\n    items.append(1)", &[])]
    fn test_collect_locally_mutated_return_functions(
        #[case] usage: &str,
        #[case] expected: &[&str],
    ) {
        let source = format!("def make():\n    return []\n{usage}");
        let file = ParsedFile::new(&source, Language::Python);
        let expected: HashSet<String> = expected.iter().map(|name| (*name).to_string()).collect();
        assert_eq!(*collect_locally_mutated_return_functions(&file), expected);
    }
}
