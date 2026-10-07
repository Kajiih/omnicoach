//! AST helper predicates and structural extractors for Python.

mod annotations;
mod classes;
mod format_strings;
mod functions;
mod logging;
mod scopes;
mod strings;

pub use self::annotations::{
    AnnotationTraversalDepth, CollectionKind, CollectionShape, PythonCollectionType,
    PythonReturnTypeUnion, collect_collection_types, collection_display, collection_type,
    extract_generic_type, has_unaliased_collections_abc_set_import, return_type_union,
};
pub use self::classes::{
    PythonAnnotatedAttribute, PythonBaseClass, PythonClassInfo, PythonInstanceAttributeAnnotation,
    collect_class_attributes, collect_instance_attribute_annotations, extract_classes,
};
pub use self::format_strings::{
    PythonFormatPlaceholder, PythonFormatString, PythonFormatStyle, collect_format_strings,
    extract_valid_field_root, named_format_field_roots,
};
pub(super) use self::functions::is_trait_impl_member;
pub use self::functions::{
    PythonFunctionSignature, PythonParameterInfo, PythonParameterKind, extract_function_signatures,
    find_nested_functions,
};
pub use self::logging::{PythonLoggerCall, collect_logger_calls};
pub(super) use self::scopes::collect_bindings;
pub use self::scopes::{
    PythonFunctionScope, PythonScopeFunction, PythonSiblingCall, collect_function_scopes,
};

use self::annotations::{has_final_annotation_expr, is_bare_final_annotation_expr};
use self::classes::is_in_protocol_or_abc_class;
#[cfg(test)]
use self::functions::extract_parameters;
use self::functions::{direct_function_definitions, method_receiver_name_ast};
use self::logging::extract_logger_call;
use self::scopes::parameters_shadow_name;
use self::strings::{fstring_segments_and_interpolations, static_string_text};
use crate::code_lint::ast::{
    AstNode, LiteralOccurrence, LiteralRole, LiteralValue, ParsedFile, PositionalRead,
    ScopePositionalReads, parse_float_literal, parse_integer_literal, span_from_ruff_range,
};
use crate::diagnostic::SourceSpan;
#[cfg(test)]
use ruff_python_ast::Parameters;
use ruff_python_ast::visitor::source_order::{
    SourceOrderVisitor, walk_except_handler, walk_expr, walk_stmt,
};
use ruff_python_ast::{
    Decorator, ExceptHandler, Expr, ModModule, Stmt, StmtAssert, StmtFunctionDef, StringFlags as _,
    WithItem,
};
use ruff_text_size::Ranged as _;
use std::collections::{HashMap, HashSet};

const PYTEST_RAISES: &str = "raises";
const ENVIRON_NAME: &str = "environ";

/// Finds the `Expr` node in `module` whose byte span equals `target_span`.
pub(super) fn find_expr_at_span(module: &ModModule, target_span: SourceSpan) -> Option<&Expr> {
    struct ExprFinder<'a> {
        target_span: SourceSpan,
        found: Option<&'a Expr>,
    }

    impl<'a> SourceOrderVisitor<'a> for ExprFinder<'a> {
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

/// Finds the `StmtFunctionDef` node in `module` whose definition or name span equals `target_span`.
pub(super) fn find_function_def_at_span(
    module: &ModModule,
    target_span: SourceSpan,
) -> Option<&StmtFunctionDef> {
    struct FunctionFinder<'a> {
        target_span: SourceSpan,
        found: Option<&'a StmtFunctionDef>,
    }

    impl<'a> SourceOrderVisitor<'a> for FunctionFinder<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if self.found.is_some() {
                return;
            }
            let span = span_from_ruff_range(statement.range());
            if !(span.start <= self.target_span.start && self.target_span.end <= span.end) {
                return;
            }
            if let Stmt::FunctionDef(func_def) = statement
                && (span == self.target_span
                    || span_from_ruff_range(func_def.name.range) == self.target_span)
            {
                self.found = Some(func_def);
                return;
            }
            walk_stmt(self, statement);
        }
    }

    let mut finder = FunctionFinder {
        target_span,
        found: None,
    };
    finder.visit_body(&module.body);
    finder.found
}

/// Finds the `Parameters` node in `module` matching `target_span` (either the `Parameters` span,
/// the enclosing `StmtFunctionDef` span, or the first function's parameters when `target_span`
/// spans the module).
#[cfg(test)]
pub(super) fn find_parameters_at_span(
    module: &ModModule,
    target_span: SourceSpan,
) -> Option<&Parameters> {
    struct ParamsFinder<'a> {
        target_span: SourceSpan,
        module_span: SourceSpan,
        found: Option<&'a Parameters>,
    }

    impl<'a> SourceOrderVisitor<'a> for ParamsFinder<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if self.found.is_some() {
                return;
            }
            if let Stmt::FunctionDef(func_def) = statement
                && (span_from_ruff_range(func_def.range) == self.target_span
                    || span_from_ruff_range(func_def.parameters.range) == self.target_span
                    || self.target_span == self.module_span)
            {
                self.found = Some(&func_def.parameters);
                return;
            }
            walk_stmt(self, statement);
        }

        fn visit_parameters(&mut self, parameters: &'a Parameters) {
            if self.found.is_none() && span_from_ruff_range(parameters.range) == self.target_span {
                self.found = Some(parameters);
            }
        }
    }

    let mut finder = ParamsFinder {
        target_span,
        module_span: span_from_ruff_range(module.range),
        found: None,
    };
    finder.visit_body(&module.body);
    finder.found
}

/// Finds the `StmtAssert` node in `module` whose byte span equals `target_span`.
fn find_assert_at_span(module: &ModModule, target_span: SourceSpan) -> Option<&StmtAssert> {
    struct AssertFinder<'a> {
        target_span: SourceSpan,
        found: Option<&'a StmtAssert>,
    }

    impl<'a> SourceOrderVisitor<'a> for AssertFinder<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if self.found.is_some() {
                return;
            }
            let span = span_from_ruff_range(statement.range());
            if !(span.start <= self.target_span.start && self.target_span.end <= span.end) {
                return;
            }
            if let Stmt::Assert(assert_statement) = statement
                && span == self.target_span
            {
                self.found = Some(assert_statement);
                return;
            }
            walk_stmt(self, statement);
        }
    }

    let mut finder = AssertFinder {
        target_span,
        found: None,
    };
    finder.visit_body(&module.body);
    finder.found
}

/// Returns true if `node` is a Python import binding (`import x` or `from m import y`).
#[must_use]
pub(super) fn is_import_binding(node: &AstNode<'_>) -> bool {
    struct ImportSpanChecker {
        target_span: SourceSpan,
        found: bool,
    }

    impl<'a> SourceOrderVisitor<'a> for ImportSpanChecker {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if self.found {
                return;
            }
            let span = span_from_ruff_range(statement.range());
            if !(span.start <= self.target_span.start && self.target_span.end <= span.end) {
                return;
            }
            if matches!(statement, Stmt::Import(_) | Stmt::ImportFrom(_)) {
                self.found = true;
                return;
            }
            walk_stmt(self, statement);
        }
    }

    let Some(parsed) = node.file.py_module() else {
        return false;
    };
    let mut checker = ImportSpanChecker {
        target_span: node.span(),
        found: false,
    };
    checker.visit_body(&parsed.syntax().body);
    checker.found
}

/// Returns true if `node` is the declared name of a Python `def` or `class`.
#[must_use]
pub(super) fn is_structural_definition(node: &AstNode<'_>) -> bool {
    struct StructuralDefChecker {
        target_span: SourceSpan,
        found: bool,
    }

    impl<'a> SourceOrderVisitor<'a> for StructuralDefChecker {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if self.found {
                return;
            }
            let span = span_from_ruff_range(statement.range());
            if !(span.start <= self.target_span.start && self.target_span.end <= span.end) {
                return;
            }
            match statement {
                Stmt::FunctionDef(func_def)
                    if span_from_ruff_range(func_def.name.range) == self.target_span =>
                {
                    self.found = true;
                    return;
                }
                Stmt::ClassDef(class_def)
                    if span_from_ruff_range(class_def.name.range) == self.target_span =>
                {
                    self.found = true;
                    return;
                }
                _ => {}
            }
            walk_stmt(self, statement);
        }
    }

    let Some(parsed) = node.file.py_module() else {
        return false;
    };
    let mut checker = StructuralDefChecker {
        target_span: node.span(),
        found: false,
    };
    checker.visit_body(&parsed.syntax().body);
    checker.found
}

/// Returns true if a Python `StmtFunctionDef` is a test function (`test` or `test_*`).
fn is_test_function_def(func_def: &StmtFunctionDef) -> bool {
    let func_name = func_def.name.as_str();
    func_name == "test" || func_name.starts_with("test_")
}

/// Returns true if a Python `ExprCall` is a test assertion call
/// (`self.assert*()`, `pytest.raises(...)`, `raises(...)`, `pytest.warns(...)`, `self.fail(...)`).
fn is_assertion_call_expr(call: &ruff_python_ast::ExprCall) -> bool {
    match call.func.as_ref() {
        Expr::Name(name) => name.id.as_str() == PYTEST_RAISES,
        Expr::Attribute(attr) => {
            let attr_name = attr.attr.as_str();
            if attr_name.starts_with("assert") {
                return true;
            }
            let Expr::Name(obj) = attr.value.as_ref() else {
                return false;
            };
            let obj_text = obj.id.as_str();
            (obj_text == "pytest" && (attr_name == PYTEST_RAISES || attr_name == "warns"))
                || (obj_text == "self" && attr_name == "fail")
        }
        _ => false,
    }
}

/// Represents a keyword argument (`key=value`) in a Python call or argument list.
#[derive(Clone)]
pub struct KeywordArg<'a> {
    /// The keyword parameter name.
    pub name: String,
    /// AST node for the argument identifier name.
    pub name_node: AstNode<'a>,
    /// AST node for the argument value expression.
    pub value_node: AstNode<'a>,
}

impl KeywordArg<'_> {
    /// Evaluates literal boolean arguments (`True` / `False`).
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self.value_node.text().as_ref() {
            "True" => Some(true),
            "False" => Some(false),
            _ => None,
        }
    }
}

/// Structured metadata for a Python decorator.
#[derive(Clone)]
pub struct DecoratorInfo<'a> {
    /// Full decorator AST node (including `@`).
    pub node: AstNode<'a>,
    /// Full path string (e.g. `"dataclasses.dataclass"`, `"pytest.mark.parametrize"`).
    pub path: String,
    /// Terminal identifier (e.g. `"dataclass"`, `"parametrize"`).
    pub terminal_name: String,
    /// Call node if the decorator was invoked with parentheses `@dec(...)`.
    pub call_node: Option<AstNode<'a>>,
    /// Parsed keyword arguments if invoked as a call.
    pub keyword_args: Vec<KeywordArg<'a>>,
}

impl<'a> DecoratorInfo<'a> {
    /// Looks up a keyword argument by name.
    #[must_use]
    pub fn get_arg(&self, key: &str) -> Option<&KeywordArg<'a>> {
        self.keyword_args.iter().find(|kw| kw.name == key)
    }

    /// Returns true if a keyword argument with `key` was explicitly passed.
    #[must_use]
    pub fn has_arg(&self, key: &str) -> bool {
        self.get_arg(key).is_some()
    }
}

/// Helper to resolve the dotted expression path and terminal identifier from an `Expr`.
pub(super) fn resolve_path_and_terminal_expr(expr: &Expr, source: &str) -> (String, String) {
    let span = span_from_ruff_range(expr.range());
    let path = source[span.start..span.end].to_string();
    let terminal = if let Expr::Attribute(attr) = expr {
        attr.attr.to_string()
    } else {
        path.rsplit('.').next().unwrap_or("").to_string()
    };
    (path, terminal)
}

/// Converts a slice of `ruff_python_ast::Decorator` nodes into `DecoratorInfo` values.
pub(super) fn extract_decorators_from_slice<'a>(
    decorators: &[Decorator],
    file: &'a ParsedFile,
) -> Vec<DecoratorInfo<'a>> {
    let mut out = Vec::with_capacity(decorators.len());
    for decorator in decorators {
        let (call_node, target_expr, keyword_args) = if let Expr::Call(call) = &decorator.expression
        {
            let kwargs = call
                .arguments
                .keywords
                .iter()
                .filter_map(|kw| {
                    let arg_ident = kw.arg.as_ref()?;
                    Some(KeywordArg {
                        name: arg_ident.id.to_string(),
                        name_node: AstNode::from_span(file, span_from_ruff_range(arg_ident.range)),
                        value_node: AstNode::from_span(
                            file,
                            span_from_ruff_range(kw.value.range()),
                        ),
                    })
                })
                .collect();
            (
                Some(AstNode::from_span(file, span_from_ruff_range(call.range()))),
                call.func.as_ref(),
                kwargs,
            )
        } else {
            (None, &decorator.expression, Vec::new())
        };

        let (path, terminal_name) = resolve_path_and_terminal_expr(target_expr, &file.source);
        out.push(DecoratorInfo {
            node: AstNode::from_span(file, span_from_ruff_range(decorator.range)),
            path,
            terminal_name,
            call_node,
            keyword_args,
        });
    }
    out
}

/// Extracts all decorators from a `function_definition` or `class_definition` node.
#[must_use]
fn extract_decorators<'a>(node: &AstNode<'a>) -> Vec<DecoratorInfo<'a>> {
    struct DecoratorOwnerFinder<'a> {
        target_span: SourceSpan,
        found: Option<&'a [Decorator]>,
    }

    impl<'a> SourceOrderVisitor<'a> for DecoratorOwnerFinder<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if self.found.is_some() {
                return;
            }
            let span = span_from_ruff_range(statement.range());
            if !(span.start <= self.target_span.start && self.target_span.end <= span.end) {
                return;
            }
            match statement {
                Stmt::FunctionDef(func_def)
                    if span == self.target_span
                        || span_from_ruff_range(func_def.name.range) == self.target_span =>
                {
                    self.found = Some(&func_def.decorator_list);
                    return;
                }
                Stmt::ClassDef(class_def)
                    if span == self.target_span
                        || span_from_ruff_range(class_def.name.range) == self.target_span =>
                {
                    self.found = Some(&class_def.decorator_list);
                    return;
                }
                _ => {}
            }
            walk_stmt(self, statement);
        }
    }

    let Some(parsed) = node.file.py_module() else {
        return Vec::new();
    };
    let mut finder = DecoratorOwnerFinder {
        target_span: node.span(),
        found: None,
    };
    finder.visit_body(&parsed.syntax().body);
    finder.found.map_or_else(Vec::new, |list| {
        extract_decorators_from_slice(list, node.file)
    })
}

/// Returns true if a Python `function_definition` or `class_definition` has a decorator whose
/// terminal identifier or full path satisfies `predicate`.
#[must_use]
pub(super) fn has_decorator(node: &AstNode<'_>, predicate: impl Fn(&str) -> bool) -> bool {
    extract_decorators(node)
        .into_iter()
        .any(|dec| predicate(&dec.terminal_name) || predicate(&dec.path))
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
}

impl PythonModuleAssignment<'_> {
    /// Whether the assignment declares a constant: an `UPPER_SNAKE_CASE` name or a `Final`
    /// annotation.
    #[must_use]
    pub fn is_constant(&self) -> bool {
        if is_constant_name(&self.name) {
            return true;
        }
        let Some(annotation) = &self.annotation else {
            return false;
        };
        let Some(parsed) = annotation.file.py_module() else {
            return false;
        };
        let Some(expr) = find_expr_at_span(parsed.syntax(), annotation.span()) else {
            return false;
        };
        has_final_annotation_expr(expr, &annotation.file.source)
    }
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

/// Traverses upward from an expression to find if it is enclosed in a `WithItem`.
#[must_use]
fn find_enclosing_with_item<'a>(node: &AstNode<'a>) -> Option<AstNode<'a>> {
    struct WithItemFinder {
        target_span: SourceSpan,
        found: Option<SourceSpan>,
    }

    impl<'a> SourceOrderVisitor<'a> for WithItemFinder {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if self.found.is_some() {
                return;
            }
            let span = span_from_ruff_range(statement.range());
            if !(span.start <= self.target_span.start && self.target_span.end <= span.end) {
                return;
            }
            if let Stmt::With(with_statement) = statement {
                for item in &with_statement.items {
                    if span_from_ruff_range(item.context_expr.range()) == self.target_span {
                        self.found = Some(span_from_ruff_range(item.range()));
                        return;
                    }
                }
            }
            walk_stmt(self, statement);
        }

        fn visit_with_item(&mut self, _with_item: &'a WithItem) {}
    }

    let parsed = node.file.py_module()?;
    let mut finder = WithItemFinder {
        target_span: node.span(),
        found: None,
    };
    finder.visit_body(&parsed.syntax().body);
    finder.found.map(|span| AstNode::from_span(node.file, span))
}

/// Traverses upward from a node to find its nearest enclosing `Stmt::With`.
#[must_use]
fn find_enclosing_with_statement<'a>(node: &AstNode<'a>) -> Option<AstNode<'a>> {
    struct WithStatementFinder {
        target_span: SourceSpan,
        found: Option<SourceSpan>,
    }

    impl<'a> SourceOrderVisitor<'a> for WithStatementFinder {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            let span = span_from_ruff_range(statement.range());
            if !(span.start <= self.target_span.start && self.target_span.end <= span.end) {
                return;
            }
            if matches!(statement, Stmt::With(_)) {
                self.found = Some(span);
            }
            walk_stmt(self, statement);
        }
    }

    let parsed = node.file.py_module()?;
    let mut finder = WithStatementFinder {
        target_span: node.span(),
        found: None,
    };
    finder.visit_body(&parsed.syntax().body);
    finder.found.map(|span| AstNode::from_span(node.file, span))
}

/// Returns true if `node` is invoked as a context manager inside a Python `with` statement header.
#[must_use]
pub fn is_with_context_manager(node: &AstNode<'_>) -> bool {
    find_enclosing_with_item(node).is_some() && find_enclosing_with_statement(node).is_some()
}

/// Returns true if `node` is enclosed inside an `ExceptHandler` block within the same scope.
#[must_use]
pub fn is_inside_except_clause(node: &AstNode<'_>) -> bool {
    struct ExceptScopeFinder {
        target_span: SourceSpan,
        in_except: bool,
        matched: Option<bool>,
    }

    impl<'a> SourceOrderVisitor<'a> for ExceptScopeFinder {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if self.matched.is_some() {
                return;
            }
            let span = span_from_ruff_range(statement.range());
            if !(span.start <= self.target_span.start && self.target_span.end <= span.end) {
                return;
            }
            let prev = self.in_except;
            if matches!(statement, Stmt::FunctionDef(_) | Stmt::ClassDef(_)) {
                self.in_except = false;
            }
            walk_stmt(self, statement);
            self.in_except = prev;
        }

        fn visit_except_handler(&mut self, except_handler: &'a ExceptHandler) {
            if self.matched.is_some() {
                return;
            }
            let span = span_from_ruff_range(except_handler.range());
            if !(span.start <= self.target_span.start && self.target_span.end <= span.end) {
                return;
            }
            let prev = self.in_except;
            self.in_except = true;
            walk_except_handler(self, except_handler);
            self.in_except = prev;
        }

        fn visit_expr(&mut self, expr: &'a Expr) {
            if self.matched.is_some() {
                return;
            }
            let span = span_from_ruff_range(expr.range());
            if !(span.start <= self.target_span.start && self.target_span.end <= span.end) {
                return;
            }
            if span == self.target_span {
                self.matched = Some(self.in_except);
                return;
            }
            let prev = self.in_except;
            if matches!(expr, Expr::Lambda(_)) {
                self.in_except = false;
            }
            walk_expr(self, expr);
            self.in_except = prev;
        }
    }

    let Some(parsed) = node.file.py_module() else {
        return false;
    };
    let mut finder = ExceptScopeFinder {
        target_span: node.span(),
        in_except: false,
        matched: None,
    };
    finder.visit_body(&parsed.syntax().body);
    finder.matched.unwrap_or(false)
}

/// Returns true if `expr` is a Python `tuple` or `list` consisting solely of `>= 2` boolean literals (`True` / `False`).
fn is_boolean_literal_collection_expr(expr: &Expr) -> bool {
    let elements = match expr {
        Expr::Tuple(tuple) => &tuple.elts,
        Expr::List(list) => &list.elts,
        _ => return false,
    };
    elements.len() >= 2
        && elements
            .iter()
            .all(|element| matches!(element, Expr::BooleanLiteral(_)))
}

/// Collects all Python `Stmt::Assert` nodes in `file`.
#[must_use]
pub fn collect_assert_statements(file: &ParsedFile) -> Vec<AstNode<'_>> {
    struct AssertCollector<'a> {
        file: &'a ParsedFile,
        out: Vec<AstNode<'a>>,
    }

    impl<'a> SourceOrderVisitor<'a> for AssertCollector<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if let Stmt::Assert(assert_statement) = statement {
                self.out.push(AstNode::from_span(
                    self.file,
                    span_from_ruff_range(assert_statement.range()),
                ));
            }
            walk_stmt(self, statement);
        }
    }

    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut collector = AssertCollector {
        file,
        out: Vec::new(),
    };
    collector.visit_body(&parsed.syntax().body);
    collector.out
}

/// Returns true if a Python `assert` statement node has a top-level `and` boolean operator.
#[must_use]
pub fn has_top_level_logical_and(assert_node: &AstNode<'_>) -> bool {
    let Some(parsed) = assert_node.file.py_module() else {
        return false;
    };
    let Some(assert_statement) = find_assert_at_span(parsed.syntax(), assert_node.span()) else {
        return false;
    };
    matches!(
        assert_statement.test.as_ref(),
        Expr::BoolOp(bool_op) if bool_op.op == ruff_python_ast::BoolOp::And
    )
}

/// Returns true if a Python `assert` statement node compares against a boolean literal tuple/list.
#[must_use]
pub fn has_boolean_literal_comparison(assert_node: &AstNode<'_>) -> bool {
    let Some(parsed) = assert_node.file.py_module() else {
        return false;
    };
    let Some(assert_statement) = find_assert_at_span(parsed.syntax(), assert_node.span()) else {
        return false;
    };
    let Expr::Compare(comp) = assert_statement.test.as_ref() else {
        return false;
    };
    comp.operands.iter().any(is_boolean_literal_collection_expr)
}

/// Counts top-level assertion constructs in a Python test function body.
fn count_python_assertions_in_body(body: &[Stmt]) -> usize {
    struct AssertionCounter {
        count: usize,
    }

    impl<'a> SourceOrderVisitor<'a> for AssertionCounter {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            match statement {
                Stmt::FunctionDef(_) | Stmt::ClassDef(_) => {}
                Stmt::Assert(_) => {
                    self.count += 1;
                }
                _ => walk_stmt(self, statement),
            }
        }

        fn visit_expr(&mut self, expr: &'a Expr) {
            if let Expr::Call(call) = expr
                && is_assertion_call_expr(call)
            {
                self.count += 1;
                return;
            }
            walk_expr(self, expr);
        }
    }

    let mut counter = AssertionCounter { count: 0 };
    counter.visit_body(body);
    counter.count
}

/// Collects all outermost Python test functions along with their identifier node, name, and assertion count.
#[must_use]
pub(super) fn collect_test_function_assertion_counts(
    file: &ParsedFile,
) -> Vec<(AstNode<'_>, String, usize)> {
    struct TestAssertionCollector<'a> {
        file: &'a ParsedFile,
        out: Vec<(AstNode<'a>, String, usize)>,
    }

    impl<'a> SourceOrderVisitor<'a> for TestAssertionCollector<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if let Stmt::FunctionDef(func_def) = statement {
                if is_test_function_def(func_def) {
                    let count = count_python_assertions_in_body(&func_def.body);
                    self.out.push((
                        AstNode::from_span(self.file, span_from_ruff_range(func_def.name.range)),
                        func_def.name.to_string(),
                        count,
                    ));
                }
                return;
            }
            walk_stmt(self, statement);
        }
    }

    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut collector = TestAssertionCollector {
        file,
        out: Vec::new(),
    };
    collector.visit_body(&parsed.syntax().body);
    collector.out
}

/// A Python subscript reading the `os.environ` (or bare `environ`) mapping.
pub struct PythonEnvironSubscript<'a> {
    /// The whole subscript (`os.environ["HOST"]`).
    pub node: AstNode<'a>,
    /// The mapping expression being indexed (`os.environ` or `environ`).
    pub mapping: AstNode<'a>,
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

/// Collects all Python subscript expressions indexing into `os.environ` or `environ`.
#[must_use]
pub fn collect_environ_subscripts(file: &ParsedFile) -> Vec<PythonEnvironSubscript<'_>> {
    struct EnvironSubscriptCollector<'a> {
        file: &'a ParsedFile,
        out: Vec<PythonEnvironSubscript<'a>>,
    }

    impl<'a> SourceOrderVisitor<'a> for EnvironSubscriptCollector<'a> {
        fn visit_expr(&mut self, expr: &'a Expr) {
            if let Expr::Subscript(sub) = expr
                && is_environ_mapping_expr(&sub.value)
            {
                self.out.push(PythonEnvironSubscript {
                    node: AstNode::from_span(self.file, span_from_ruff_range(sub.range())),
                    mapping: AstNode::from_span(self.file, span_from_ruff_range(sub.value.range())),
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
        out: Vec::new(),
    };
    collector.visit_body(&parsed.syntax().body);
    collector.out
}

struct UnwrappedMultilineFinder<'a, F> {
    file: &'a ParsedFile,
    is_allowed_wrapper: F,
    docstring_expr_span: Option<SourceSpan>,
    allowed_call_depth: usize,
    out: Vec<AstNode<'a>>,
}

impl<F> UnwrappedMultilineFinder<'_, F> {
    fn record_multiline_part(&mut self, span: SourceSpan, is_triple_quoted: bool) {
        let is_docstring = self.docstring_expr_span == Some(span);
        if is_triple_quoted
            && self.file.source[span.start..span.end].contains('\n')
            && !is_docstring
            && self.allowed_call_depth == 0
        {
            self.out.push(AstNode::from_span(self.file, span));
        }
    }
}

impl<'a, F: Fn(&str, &str) -> bool> SourceOrderVisitor<'a> for UnwrappedMultilineFinder<'a, F> {
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
                let prev_depth = self.allowed_call_depth;
                self.allowed_call_depth = 0;
                self.visit_body(&func.body);
                self.allowed_call_depth = prev_depth;
            }
            Stmt::ClassDef(cls) => {
                for dec in &cls.decorator_list {
                    self.visit_decorator(dec);
                }
                if let Some(args) = &cls.arguments {
                    self.visit_arguments(args);
                }
                let prev_depth = self.allowed_call_depth;
                self.allowed_call_depth = 0;
                self.visit_body(&cls.body);
                self.allowed_call_depth = prev_depth;
            }
            Stmt::Expr(expr_statement) => {
                let prev_doc = self.docstring_expr_span;
                self.docstring_expr_span = Some(span_from_ruff_range(expr_statement.range()));
                walk_stmt(self, statement);
                self.docstring_expr_span = prev_doc;
            }
            _ => walk_stmt(self, statement),
        }
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Lambda(lambda) => {
                if let Some(params) = &lambda.parameters {
                    self.visit_parameters(params);
                }
                let prev_depth = self.allowed_call_depth;
                self.allowed_call_depth = 0;
                self.visit_expr(&lambda.body);
                self.allowed_call_depth = prev_depth;
            }
            Expr::Call(call) => {
                let (path, terminal) =
                    resolve_path_and_terminal_expr(&call.func, &self.file.source);
                let is_allowed = (self.is_allowed_wrapper)(&path, &terminal);
                if is_allowed {
                    self.allowed_call_depth += 1;
                }
                walk_expr(self, expr);
                if is_allowed {
                    self.allowed_call_depth -= 1;
                }
            }
            Expr::StringLiteral(str_lit) => {
                for part in str_lit.value.as_slice() {
                    self.record_multiline_part(
                        span_from_ruff_range(part.range()),
                        part.flags.is_triple_quoted(),
                    );
                }
            }
            Expr::FString(fstr) => {
                for part in &fstr.value {
                    match part {
                        ruff_python_ast::FStringPartRef::Literal(lit) => {
                            self.record_multiline_part(
                                span_from_ruff_range(lit.range()),
                                lit.flags.is_triple_quoted(),
                            );
                        }
                        ruff_python_ast::FStringPartRef::FString(fpart) => {
                            self.record_multiline_part(
                                span_from_ruff_range(fpart.range()),
                                fpart.flags.is_triple_quoted(),
                            );
                        }
                    }
                }
                walk_expr(self, expr);
            }
            Expr::BytesLiteral(bytes_lit) => {
                for part in bytes_lit.value.as_slice() {
                    self.record_multiline_part(
                        span_from_ruff_range(part.range()),
                        part.flags.is_triple_quoted(),
                    );
                }
            }
            _ => walk_expr(self, expr),
        }
    }
}

/// Finds all multiline string literals in a Python file that are not docstrings
/// and not wrapped in an allowed call.
#[must_use]
pub(super) fn find_unwrapped_multiline_strings(
    file: &ParsedFile,
    is_allowed_wrapper: impl Fn(&str, &str) -> bool,
) -> Vec<AstNode<'_>> {
    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut finder = UnwrappedMultilineFinder {
        file,
        is_allowed_wrapper,
        docstring_expr_span: None,
        allowed_call_depth: 0,
        out: Vec::new(),
    };
    finder.visit_body(&parsed.syntax().body);
    finder.out
}

/// Returns the value of a Python decimal `integer` literal (not `0x1`, `1_000`, ...).
fn decimal_literal_expr(expr: &Expr, source: &str) -> Option<i64> {
    let Expr::NumberLiteral(number) = expr else {
        return None;
    };
    if !matches!(number.value, ruff_python_ast::Number::Int(_)) {
        return None;
    }
    let text = &source[number.range().start().to_usize()..number.range().end().to_usize()];
    if !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// Returns the position read by a Python `subscript` index: a decimal literal, or its negation
/// for end-relative reads (`xs[-1]`).
fn literal_position_expr(index: &Expr, source: &str) -> Option<i64> {
    if let Expr::UnaryOp(unary) = index
        && unary.op == ruff_python_ast::UnaryOp::USub
    {
        return decimal_literal_expr(&unary.operand, source).map(std::ops::Neg::neg);
    }
    decimal_literal_expr(index, source)
}

/// Returns true if `expr` contains any call expression (`Expr::Call`).
fn expr_contains_call(expr: &Expr) -> bool {
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

/// Returns true if `receiver` names a stable value: a name, attribute or subscript chain with no
/// call inside, so that identical text means the same value.
fn is_stable_receiver_expr(receiver: &Expr) -> bool {
    matches!(
        receiver,
        Expr::Name(_) | Expr::Attribute(_) | Expr::Subscript(_)
    ) && !expr_contains_call(receiver)
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

/// Extracts the terminal function/method name if `expr` is a `call` expression.
fn called_terminal_name_expr(expr: &Expr, source: &str) -> Option<String> {
    let Expr::Call(call) = expr else {
        return None;
    };
    let (_, terminal) = resolve_path_and_terminal_expr(&call.func, source);
    (!terminal.is_empty()).then_some(terminal)
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

/// Read-only methods available on `Sequence`, `Mapping`, or `Set` (`collections.abc`).
const READONLY_COLLECTION_METHODS: &[&str] = &[
    "count",
    "index",
    "get",
    "keys",
    "values",
    "items",
    "isdisjoint",
    "issubset",
    "issuperset",
    "copy",
    "__contains__",
    "__len__",
    "__iter__",
    "__getitem__",
    "__reversed__",
];

const BOOL_CONSTRUCTOR: &str = "bool";
const BUILTIN_LEN: &str = "len";

/// Builtins that iterate their collection argument(s) in a single pass and are also treated as
/// collection-sizing/iterating calls in positional-read analysis.
const ITERATING_COLLECTION_BUILTINS: &[&str] = &["enumerate", "zip", "sorted"];

/// Additional builtins that consume an `Iterable` in a single pass.
const OTHER_SINGLE_PASS_ITERABLE_BUILTINS: &[&str] = &[
    "sum",
    "min",
    "max",
    "any",
    "all",
    "list",
    "tuple",
    "set",
    "frozenset",
    "dict",
    "iter",
    "map",
    "filter",
];

/// Additional builtins that read a collection without mutating it in place or retaining a
/// mutable alias to the outer container.
const OTHER_SAFE_READONLY_BUILTINS: &[&str] = &[
    BOOL_CONSTRUCTOR,
    "repr",
    "str",
    "hash",
    "id",
    "type",
    "isinstance",
    "issubclass",
    "print",
    "range",
    "next",
];

/// Returns true if `name` is a builtin that iterates or sizes its positional arguments
/// (`len(xs)`, `enumerate(xs)`, `zip(xs, ys)`, `reversed(xs)`, `sorted(xs)`).
fn is_collection_builtin(name: &str) -> bool {
    matches!(name, BUILTIN_LEN | "reversed") || ITERATING_COLLECTION_BUILTINS.contains(&name)
}

/// Returns true if `name` is a builtin that consumes an `Iterable` in a single pass.
fn is_single_pass_iterable_builtin(name: &str) -> bool {
    ITERATING_COLLECTION_BUILTINS.contains(&name)
        || OTHER_SINGLE_PASS_ITERABLE_BUILTINS.contains(&name)
}

/// Returns true if `name` is a builtin that reads or iterates a collection without mutating it
/// in place or retaining a mutable alias to the outer container.
fn is_safe_readonly_builtin(name: &str) -> bool {
    is_single_pass_iterable_builtin(name)
        || is_collection_builtin(name)
        || OTHER_SAFE_READONLY_BUILTINS.contains(&name)
}

/// Returns true if `expr` is a read reference (`ExprContext::Load`) to `parameter_name`.
fn is_param_load(expr: &Expr, parameter_name: &str) -> bool {
    matches!(
        expr,
        Expr::Name(name)
            if name.id.as_str() == parameter_name
                && matches!(name.ctx, ruff_python_ast::ExprContext::Load)
    )
}

struct MutationOrEscapeFinder<'a> {
    parameter_name: &'a str,
    in_boolean_context: bool,
    safe_starts: HashSet<usize>,
    found_mutated_or_escaping: bool,
}

impl<'a> MutationOrEscapeFinder<'a> {
    fn mark_if_param(&mut self, expr: &Expr) {
        if is_param_load(expr, self.parameter_name) {
            self.safe_starts.insert(expr.range().start().to_usize());
        }
    }

    fn visit_in_boolean_context(&mut self, expr: &'a Expr) {
        self.mark_if_param(expr);
        let prev = self.in_boolean_context;
        self.in_boolean_context = true;
        self.visit_expr(expr);
        self.in_boolean_context = prev;
    }

    fn visit_call_expr(&mut self, call: &'a ruff_python_ast::ExprCall) {
        if let Expr::Attribute(attr) = call.func.as_ref()
            && READONLY_COLLECTION_METHODS.contains(&attr.attr.as_str())
        {
            self.mark_if_param(&attr.value);
        }
        let is_safe_builtin = matches!(
            call.func.as_ref(),
            Expr::Name(func_name)
                if is_safe_readonly_builtin(func_name.id.as_str())
        );
        let is_bool_builtin = matches!(
            call.func.as_ref(),
            Expr::Name(func_name) if func_name.id.as_str() == BOOL_CONSTRUCTOR
        );
        if is_safe_builtin {
            for arg in &call.arguments.args {
                match arg {
                    Expr::Starred(starred) => self.mark_if_param(&starred.value),
                    _ => self.mark_if_param(arg),
                }
            }
            for kw in &call.arguments.keywords {
                self.mark_if_param(&kw.value);
            }
        }
        let prev = self.in_boolean_context;
        self.in_boolean_context = false;
        self.visit_expr(&call.func);
        for arg in &call.arguments.args {
            self.in_boolean_context = is_bool_builtin;
            self.visit_expr(arg);
        }
        self.in_boolean_context = false;
        for kw in &call.arguments.keywords {
            self.visit_keyword(kw);
        }
        self.in_boolean_context = prev;
    }
}

impl<'a> SourceOrderVisitor<'a> for MutationOrEscapeFinder<'a> {
    fn visit_annotation(&mut self, _expr: &'a Expr) {}

    fn visit_stmt(&mut self, statement: &'a Stmt) {
        if self.found_mutated_or_escaping {
            return;
        }
        match statement {
            Stmt::TypeAlias(_) => {}
            Stmt::FunctionDef(func) => {
                if parameters_shadow_name(&func.parameters, self.parameter_name) {
                    // Defaults are evaluated in the enclosing scope; only the body is shadowed.
                    let prev = self.in_boolean_context;
                    self.in_boolean_context = false;
                    self.visit_parameters(&func.parameters);
                    self.in_boolean_context = prev;
                } else {
                    walk_stmt(self, statement);
                }
            }
            Stmt::For(for_statement) => {
                self.mark_if_param(&for_statement.iter);
                walk_stmt(self, statement);
            }
            Stmt::If(if_statement) => {
                self.visit_in_boolean_context(&if_statement.test);
                self.visit_body(&if_statement.body);
                for clause in &if_statement.elif_else_clauses {
                    if let Some(test) = &clause.test {
                        self.visit_in_boolean_context(test);
                    }
                    self.visit_body(&clause.body);
                }
            }
            Stmt::While(while_statement) => {
                self.visit_in_boolean_context(&while_statement.test);
                self.visit_body(&while_statement.body);
                self.visit_body(&while_statement.orelse);
            }
            Stmt::Assert(assert_statement) => {
                self.visit_in_boolean_context(&assert_statement.test);
                if let Some(message) = &assert_statement.msg {
                    self.visit_in_boolean_context(message);
                }
            }
            _ => walk_stmt(self, statement),
        }
    }

    fn visit_comprehension(&mut self, comp: &'a ruff_python_ast::Comprehension) {
        self.mark_if_param(&comp.iter);
        ruff_python_ast::visitor::source_order::walk_comprehension(self, comp);
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        if self.found_mutated_or_escaping {
            return;
        }
        match expr {
            Expr::Lambda(lambda) => {
                if lambda
                    .parameters
                    .as_deref()
                    .is_some_and(|params| parameters_shadow_name(params, self.parameter_name))
                {
                    if let Some(params) = &lambda.parameters {
                        let prev = self.in_boolean_context;
                        self.in_boolean_context = false;
                        self.visit_parameters(params);
                        self.in_boolean_context = prev;
                    }
                    return;
                }
            }
            Expr::Name(name) if name.id.as_str() == self.parameter_name => {
                if !self.safe_starts.contains(&name.range().start().to_usize()) {
                    self.found_mutated_or_escaping = true;
                }
                return;
            }
            Expr::BoolOp(bool_op) => {
                if self.in_boolean_context {
                    for val in &bool_op.values {
                        self.mark_if_param(val);
                    }
                }
                walk_expr(self, expr);
                return;
            }
            Expr::UnaryOp(unary) if unary.op == ruff_python_ast::UnaryOp::Not => {
                self.mark_if_param(&unary.operand);
                walk_expr(self, expr);
                return;
            }
            Expr::Call(call) => {
                self.visit_call_expr(call);
                return;
            }
            Expr::Subscript(sub) if matches!(sub.ctx, ruff_python_ast::ExprContext::Load) => {
                self.mark_if_param(&sub.value);
                self.mark_if_param(&sub.slice);
            }
            Expr::List(list) if matches!(list.ctx, ruff_python_ast::ExprContext::Load) => {
                for elt in &list.elts {
                    if let Expr::Starred(starred) = elt {
                        self.mark_if_param(&starred.value);
                    }
                }
            }
            Expr::Tuple(tuple) if matches!(tuple.ctx, ruff_python_ast::ExprContext::Load) => {
                for elt in &tuple.elts {
                    if let Expr::Starred(starred) = elt {
                        self.mark_if_param(&starred.value);
                    }
                }
            }
            Expr::Set(set) => {
                for elt in &set.elts {
                    if let Expr::Starred(starred) = elt {
                        self.mark_if_param(&starred.value);
                    }
                }
            }
            Expr::Dict(dict) => {
                for item in &dict.items {
                    if item.key.is_none() {
                        self.mark_if_param(&item.value);
                    }
                }
            }
            Expr::Compare(comp) => {
                for operand in &comp.operands {
                    self.mark_if_param(operand);
                }
            }
            Expr::BinOp(bin) => {
                self.mark_if_param(&bin.left);
                self.mark_if_param(&bin.right);
            }
            Expr::If(if_expr) => {
                self.visit_in_boolean_context(&if_expr.test);
                let prev = self.in_boolean_context;
                self.in_boolean_context = false;
                self.visit_expr(&if_expr.body);
                self.visit_expr(&if_expr.orelse);
                self.in_boolean_context = prev;
                return;
            }
            _ => {}
        }
        let prev = self.in_boolean_context;
        self.in_boolean_context = false;
        walk_expr(self, expr);
        self.in_boolean_context = prev;
    }
}

/// Returns true if `parameter_name` is mutated in place or escapes (aliased, returned, yielded,
/// or passed to an unknown function/method) anywhere in `func_node`'s body.
#[must_use]
pub fn is_parameter_mutated_or_escaping(func_node: &AstNode<'_>, parameter_name: &str) -> bool {
    let Some(parsed) = func_node.file.py_module() else {
        return false;
    };
    let Some(func) = find_function_def_at_span(parsed.syntax(), func_node.span()) else {
        return false;
    };
    let mut finder = MutationOrEscapeFinder {
        parameter_name,
        in_boolean_context: false,
        safe_starts: HashSet::new(),
        found_mutated_or_escaping: false,
    };
    finder.visit_body(&func.body);
    finder.found_mutated_or_escaping
}

/// Minimum read-only `collections.abc` capability required by a parameter's usages inside
/// its function body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ParameterCollectionCapability {
    /// Parameter is never referenced in the function body.
    Unused,
    /// Parameter is only iterated once at top-level depth (`Iterable` suffices).
    Iterable,
    /// Parameter uses `len(x)`, `v in x`, truthiness (`if x:`), or multi-pass iteration (`Collection` suffices).
    Collection,
    /// Parameter uses indexing/slicing, `reversed(x)`, `.index()`/`.count()`, pattern matching, or escapes (`Sequence` required).
    Sequence,
}

#[derive(Default)]
struct CapabilityTracker {
    iteration_count: usize,
    needs_collection: bool,
    needs_sequence: bool,
}

struct CapabilityVisitor<'a> {
    parameter_name: &'a str,
    loop_or_closure_depth: usize,
    in_boolean_context: bool,
    handled_starts: HashSet<usize>,
    tracker: CapabilityTracker,
}

impl<'a> CapabilityVisitor<'a> {
    fn record_iteration(&mut self, expr: &Expr) {
        if is_param_load(expr, self.parameter_name) {
            self.handled_starts.insert(expr.range().start().to_usize());
            self.tracker.iteration_count += 1;
            if self.loop_or_closure_depth > 0 {
                self.tracker.needs_collection = true;
            }
        }
    }

    fn record_collection(&mut self, expr: &Expr) {
        if is_param_load(expr, self.parameter_name) {
            self.handled_starts.insert(expr.range().start().to_usize());
            self.tracker.needs_collection = true;
        }
    }

    fn visit_in_boolean_context(&mut self, expr: &'a Expr) {
        self.record_collection(expr);
        let prev = self.in_boolean_context;
        self.in_boolean_context = true;
        self.visit_expr(expr);
        self.in_boolean_context = prev;
    }

    fn visit_generators(&mut self, generators: &'a [ruff_python_ast::Comprehension]) {
        for (idx, comp) in generators.iter().enumerate() {
            if idx == 0 {
                self.record_iteration(&comp.iter);
                self.visit_expr(&comp.iter);
                self.loop_or_closure_depth += 1;
                self.visit_expr(&comp.target);
                for if_expr in &comp.ifs {
                    self.visit_expr(if_expr);
                }
            } else {
                self.loop_or_closure_depth += 1;
                self.record_iteration(&comp.iter);
                ruff_python_ast::visitor::source_order::walk_comprehension(self, comp);
            }
            self.loop_or_closure_depth -= 1;
        }
    }

    fn visit_single_elt_comprehension(
        &mut self,
        generators: &'a [ruff_python_ast::Comprehension],
        elt: &'a Expr,
    ) {
        let prev = self.in_boolean_context;
        self.in_boolean_context = false;
        self.visit_generators(generators);
        self.loop_or_closure_depth += 1;
        self.visit_expr(elt);
        self.loop_or_closure_depth -= 1;
        self.in_boolean_context = prev;
    }

    fn visit_dict_comprehension(&mut self, comp: &'a ruff_python_ast::ExprDictComp) {
        let prev = self.in_boolean_context;
        self.in_boolean_context = false;
        self.visit_generators(&comp.generators);
        self.loop_or_closure_depth += 1;
        if let Some(key) = &comp.key {
            self.visit_expr(key);
        }
        self.visit_expr(&comp.value);
        self.loop_or_closure_depth -= 1;
        self.in_boolean_context = prev;
    }

    fn record_starred_elements(&mut self, elements: &[Expr]) {
        for elt in elements {
            if let Expr::Starred(starred) = elt {
                self.record_iteration(&starred.value);
            }
        }
    }

    fn visit_call_expr(&mut self, call: &'a ruff_python_ast::ExprCall) {
        let is_bool_builtin = matches!(
            call.func.as_ref(),
            Expr::Name(func_name) if func_name.id.as_str() == BOOL_CONSTRUCTOR
        );
        if let Expr::Name(func_name) = call.func.as_ref() {
            match func_name.id.as_str() {
                BUILTIN_LEN | BOOL_CONSTRUCTOR => {
                    for arg in &call.arguments.args {
                        self.record_collection(arg);
                    }
                }
                name if is_single_pass_iterable_builtin(name) => {
                    for arg in &call.arguments.args {
                        self.record_iteration(arg);
                    }
                }
                _ => {}
            }
        }
        let prev = self.in_boolean_context;
        self.in_boolean_context = false;
        self.visit_expr(&call.func);
        for arg in &call.arguments.args {
            self.in_boolean_context = is_bool_builtin;
            self.visit_expr(arg);
        }
        self.in_boolean_context = false;
        for kw in &call.arguments.keywords {
            self.visit_keyword(kw);
        }
        self.in_boolean_context = prev;
    }

    fn visit_compare_expr(&mut self, comp: &ruff_python_ast::ExprCompare) {
        let has_in_operator = comp.ops.iter().any(|op| {
            matches!(
                op,
                ruff_python_ast::CmpOp::In | ruff_python_ast::CmpOp::NotIn
            )
        });
        let is_identity_check = comp.ops.iter().any(|op| {
            matches!(
                op,
                ruff_python_ast::CmpOp::Is | ruff_python_ast::CmpOp::IsNot
            )
        });
        for (idx, operand) in comp.operands.iter().enumerate() {
            if is_param_load(operand, self.parameter_name) {
                let is_last = idx + 1 == comp.operands.len();
                self.handled_starts
                    .insert(operand.range().start().to_usize());
                if has_in_operator && is_last {
                    self.tracker.needs_collection = true;
                } else if !is_identity_check {
                    self.tracker.needs_sequence = true;
                }
            }
        }
    }
}

impl<'a> SourceOrderVisitor<'a> for CapabilityVisitor<'a> {
    fn visit_annotation(&mut self, _expr: &'a Expr) {}

    fn visit_stmt(&mut self, statement: &'a Stmt) {
        if self.tracker.needs_sequence {
            return;
        }
        match statement {
            Stmt::TypeAlias(_) => {}
            Stmt::FunctionDef(func) => {
                if parameters_shadow_name(&func.parameters, self.parameter_name) {
                    let prev = self.in_boolean_context;
                    self.in_boolean_context = false;
                    self.visit_parameters(&func.parameters);
                    self.in_boolean_context = prev;
                } else {
                    self.loop_or_closure_depth += 1;
                    walk_stmt(self, statement);
                    self.loop_or_closure_depth -= 1;
                }
            }
            Stmt::For(for_statement) => {
                self.record_iteration(&for_statement.iter);
                self.visit_expr(&for_statement.iter);
                self.loop_or_closure_depth += 1;
                self.visit_expr(&for_statement.target);
                self.visit_body(&for_statement.body);
                self.visit_body(&for_statement.orelse);
                self.loop_or_closure_depth -= 1;
            }
            Stmt::While(while_statement) => {
                self.loop_or_closure_depth += 1;
                self.visit_in_boolean_context(&while_statement.test);
                self.visit_body(&while_statement.body);
                self.visit_body(&while_statement.orelse);
                self.loop_or_closure_depth -= 1;
            }
            Stmt::If(if_statement) => {
                self.visit_in_boolean_context(&if_statement.test);
                self.visit_body(&if_statement.body);
                for clause in &if_statement.elif_else_clauses {
                    if let Some(test) = &clause.test {
                        self.visit_in_boolean_context(test);
                    }
                    self.visit_body(&clause.body);
                }
            }
            Stmt::Assert(assert_statement) => {
                let prev = self.in_boolean_context;
                self.in_boolean_context = true;
                self.visit_expr(&assert_statement.test);
                if let Some(message) = &assert_statement.msg {
                    self.visit_expr(message);
                }
                self.in_boolean_context = prev;
            }
            _ => walk_stmt(self, statement),
        }
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        if self.tracker.needs_sequence {
            return;
        }
        match expr {
            Expr::Lambda(lambda) => {
                let prev = self.in_boolean_context;
                self.in_boolean_context = false;
                if lambda
                    .parameters
                    .as_deref()
                    .is_some_and(|params| parameters_shadow_name(params, self.parameter_name))
                {
                    if let Some(params) = &lambda.parameters {
                        self.visit_parameters(params);
                    }
                } else {
                    self.loop_or_closure_depth += 1;
                    walk_expr(self, expr);
                    self.loop_or_closure_depth -= 1;
                }
                self.in_boolean_context = prev;
                return;
            }
            Expr::Name(name) if name.id.as_str() == self.parameter_name => {
                if !self
                    .handled_starts
                    .contains(&name.range().start().to_usize())
                {
                    self.tracker.needs_sequence = true;
                }
                return;
            }
            Expr::ListComp(comp) => {
                self.visit_single_elt_comprehension(&comp.generators, &comp.elt);
                return;
            }
            Expr::SetComp(comp) => {
                self.visit_single_elt_comprehension(&comp.generators, &comp.elt);
                return;
            }
            Expr::Generator(comp) => {
                self.visit_single_elt_comprehension(&comp.generators, &comp.elt);
                return;
            }
            Expr::DictComp(comp) => {
                self.visit_dict_comprehension(comp);
                return;
            }
            Expr::List(list) if matches!(list.ctx, ruff_python_ast::ExprContext::Load) => {
                self.record_starred_elements(&list.elts);
            }
            Expr::Tuple(tuple) if matches!(tuple.ctx, ruff_python_ast::ExprContext::Load) => {
                self.record_starred_elements(&tuple.elts);
            }
            Expr::Set(set) => {
                self.record_starred_elements(&set.elts);
            }
            Expr::Call(call) => {
                self.visit_call_expr(call);
                return;
            }
            Expr::Compare(comp) => {
                self.visit_compare_expr(comp);
            }
            Expr::UnaryOp(unary) if unary.op == ruff_python_ast::UnaryOp::Not => {
                self.record_collection(&unary.operand);
                walk_expr(self, expr);
                return;
            }
            Expr::BoolOp(bool_op) => {
                if self.in_boolean_context {
                    for val in &bool_op.values {
                        self.record_collection(val);
                    }
                }
                walk_expr(self, expr);
                return;
            }
            Expr::If(if_expr) => {
                self.visit_in_boolean_context(&if_expr.test);
                let prev = self.in_boolean_context;
                self.in_boolean_context = false;
                self.visit_expr(&if_expr.body);
                self.visit_expr(&if_expr.orelse);
                self.in_boolean_context = prev;
                return;
            }
            _ => {}
        }
        let prev = self.in_boolean_context;
        self.in_boolean_context = false;
        walk_expr(self, expr);
        self.in_boolean_context = prev;
    }
}

/// Determines the minimum read-only collection capability (`Iterable`, `Collection`, or `Sequence`)
/// required by `parameter_name` across `func_node`'s body.
#[must_use]
pub fn analyze_parameter_collection_capability(
    func_node: &AstNode<'_>,
    parameter_name: &str,
) -> ParameterCollectionCapability {
    let Some(parsed) = func_node.file.py_module() else {
        return ParameterCollectionCapability::Unused;
    };
    let Some(func) = find_function_def_at_span(parsed.syntax(), func_node.span()) else {
        return ParameterCollectionCapability::Unused;
    };
    let mut visitor = CapabilityVisitor {
        parameter_name,
        loop_or_closure_depth: 0,
        in_boolean_context: false,
        handled_starts: HashSet::new(),
        tracker: CapabilityTracker::default(),
    };
    visitor.visit_body(&func.body);

    if visitor.tracker.needs_sequence {
        ParameterCollectionCapability::Sequence
    } else if visitor.tracker.needs_collection || visitor.tracker.iteration_count > 1 {
        ParameterCollectionCapability::Collection
    } else if visitor.tracker.iteration_count == 1 {
        ParameterCollectionCapability::Iterable
    } else {
        ParameterCollectionCapability::Unused
    }
}

struct PositionalReadsCollector<'a> {
    file: &'a ParsedFile,
    current_scope: Option<ScopePositionalReads<'a>>,
    out: Vec<ScopePositionalReads<'a>>,
}

impl PositionalReadsCollector<'_> {
    fn expr_text(&self, expr: &Expr) -> String {
        let range = expr.range();
        self.file.source[range.start().to_usize()..range.end().to_usize()].to_string()
    }

    fn record_exempt_receiver(&mut self, expr: &Expr) {
        let text = self.expr_text(expr);
        if let Some(scope) = &mut self.current_scope {
            scope.exempt_receivers.insert(text);
        }
    }

    fn record_subscript(&mut self, subscript: &ruff_python_ast::ExprSubscript) {
        let receiver_text = self.expr_text(&subscript.value);
        let is_load = matches!(subscript.ctx, ruff_python_ast::ExprContext::Load);
        let position = literal_position_expr(&subscript.slice, &self.file.source);
        let is_stable = is_stable_receiver_expr(&subscript.value);
        let Some(scope) = &mut self.current_scope else {
            return;
        };
        if let Some(position) = position
            && is_load
        {
            if is_stable {
                scope.reads.push(PositionalRead {
                    node: AstNode::from_span(self.file, span_from_ruff_range(subscript.range())),
                    receiver: receiver_text,
                    position,
                });
            }
        } else {
            scope.exempt_receivers.insert(receiver_text);
        }
    }

    fn record_call_collections(&mut self, call: &ruff_python_ast::ExprCall) {
        match call.func.as_ref() {
            Expr::Name(func_name) if is_collection_builtin(func_name.id.as_str()) => {
                for arg in &call.arguments.args {
                    self.record_exempt_receiver(arg);
                }
            }
            Expr::Attribute(attr) if MUTATING_METHODS.contains(&attr.attr.as_str()) => {
                self.record_exempt_receiver(&attr.value);
            }
            _ => {}
        }
    }
}

impl<'a> SourceOrderVisitor<'a> for PositionalReadsCollector<'a> {
    fn visit_stmt(&mut self, statement: &'a Stmt) {
        match statement {
            Stmt::FunctionDef(func) => {
                // Default values and annotations are evaluated in the enclosing scope.
                for dec in &func.decorator_list {
                    self.visit_decorator(dec);
                }
                self.visit_parameters(&func.parameters);
                if let Some(returns) = &func.returns {
                    self.visit_annotation(returns);
                }
                let prev_scope = self.current_scope.replace(ScopePositionalReads::default());
                self.visit_body(&func.body);
                if let Some(finished_scope) = std::mem::replace(&mut self.current_scope, prev_scope)
                {
                    self.out.push(finished_scope);
                }
            }
            Stmt::ClassDef(_) => {
                let prev_scope = self.current_scope.take();
                walk_stmt(self, statement);
                self.current_scope = prev_scope;
            }
            Stmt::For(for_statement) => {
                self.record_exempt_receiver(&for_statement.iter);
                walk_stmt(self, statement);
            }
            _ => walk_stmt(self, statement),
        }
    }

    fn visit_comprehension(&mut self, comp: &'a ruff_python_ast::Comprehension) {
        self.record_exempt_receiver(&comp.iter);
        ruff_python_ast::visitor::source_order::walk_comprehension(self, comp);
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Lambda(_) => return,
            Expr::Subscript(sub) => {
                self.record_subscript(sub);
            }
            Expr::Call(call) => {
                self.record_call_collections(call);
            }
            _ => {}
        }
        walk_expr(self, expr);
    }
}

/// Collects Python positional reads grouped by scope (see [`super::collect_positional_reads`]).
#[must_use]
pub(super) fn collect_positional_reads(file: &ParsedFile) -> Vec<ScopePositionalReads<'_>> {
    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut collector = PositionalReadsCollector {
        file,
        current_scope: Some(ScopePositionalReads::default()),
        out: Vec::new(),
    };
    collector.visit_body(&parsed.syntax().body);
    if let Some(module_scope) = collector.current_scope {
        collector.out.push(module_scope);
    }
    collector.out
}

/// Calls whose first argument is a name or type the language requires as a string
/// (`TypeVar("T")`, `cast("Node", value)`).
const TYPE_NAME_FIRST_ARGUMENT_CALLS: &[&str] = &[
    "TypeVar",
    "NewType",
    "ParamSpec",
    "TypeVarTuple",
    "NamedTuple",
    "TypedDict",
    "cast",
];

/// Parses a non-complex Python `Expr::NumberLiteral` into a [`LiteralValue`].
fn number_literal_value(expr: &Expr, source: &str) -> Option<LiteralValue> {
    let Expr::NumberLiteral(number) = expr else {
        return None;
    };
    let text = &source[number.range().start().to_usize()..number.range().end().to_usize()];
    match &number.value {
        ruff_python_ast::Number::Int(_) => parse_integer_literal(text),
        ruff_python_ast::Number::Float(_) => parse_float_literal(text),
        ruff_python_ast::Number::Complex { .. } => None,
    }
}

/// Returns the `(span, LiteralValue)` of a scalar literal expression suitable as a
/// [`LiteralRole::ConstantDefinition`] RHS (`None` for composite values like lists or
/// implicitly concatenated strings `'aa' 'bb'`).
fn scalar_literal_value(expr: &Expr, source: &str) -> Option<(SourceSpan, LiteralValue)> {
    match expr {
        Expr::NumberLiteral(_) => Some((
            span_from_ruff_range(expr.range()),
            number_literal_value(expr, source)?,
        )),
        Expr::UnaryOp(unary) if unary.op == ruff_python_ast::UnaryOp::USub => {
            let negated = number_literal_value(&unary.operand, source)?.negated()?;
            Some((span_from_ruff_range(unary.range()), negated))
        }
        Expr::StringLiteral(str_lit) => {
            let [part] = str_lit.value.as_slice() else {
                return None;
            };
            Some((
                span_from_ruff_range(part.range()),
                LiteralValue::Str(part.as_str().to_string()),
            ))
        }
        Expr::BytesLiteral(bytes_lit) => {
            let [part] = bytes_lit.value.as_slice() else {
                return None;
            };
            Some((
                span_from_ruff_range(part.range()),
                LiteralValue::Bytes(String::from_utf8_lossy(part.as_slice()).into_owned()),
            ))
        }
        Expr::FString(fstr) => {
            let mut parts = fstr.value.iter();
            let (Some(ruff_python_ast::FStringPartRef::FString(fpart)), None) =
                (parts.next(), parts.next())
            else {
                return None;
            };
            if fpart
                .elements
                .iter()
                .any(ruff_python_ast::InterpolatedStringElement::is_interpolation)
            {
                return None;
            }
            let text: String = fpart
                .elements
                .iter()
                .filter_map(|elt| elt.as_literal().map(|lit| lit.value.as_ref()))
                .collect();
            Some((span_from_ruff_range(fpart.range()), LiteralValue::Str(text)))
        }
        _ => None,
    }
}

/// Returns true if `name` is spelled as a constant (`MAX_RETRIES`, `_TIMEOUT_S`).
fn is_constant_name(name: &str) -> bool {
    name.chars().any(|character| character.is_ascii_uppercase())
        && name.chars().all(|character| {
            character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
        })
}

/// Returns true if `expr` is a standalone string/bytes literal expression (docstring candidate).
fn is_standalone_string_expr(expr: &Expr) -> bool {
    match expr {
        Expr::StringLiteral(_) | Expr::BytesLiteral(_) => true,
        Expr::FString(fstr) => !fstr.value.iter().any(|part| match part {
            ruff_python_ast::FStringPartRef::Literal(_) => false,
            ruff_python_ast::FStringPartRef::FString(fpart) => fpart
                .elements
                .iter()
                .any(ruff_python_ast::InterpolatedStringElement::is_interpolation),
        }),
        _ => false,
    }
}

struct LiteralOccurrenceCollector<'a> {
    file: &'a ParsedFile,
    in_constant_scope: bool,
    out: Vec<LiteralOccurrence<'a>>,
}

impl<'a> LiteralOccurrenceCollector<'a> {
    fn record_constant_rhs(&mut self, value: &Expr) {
        if let Some((span, lit_val)) = scalar_literal_value(value, &self.file.source) {
            self.out.push(LiteralOccurrence {
                node: AstNode::from_span(self.file, span),
                value: lit_val,
                role: LiteralRole::ConstantDefinition,
            });
        }
    }

    fn visit_non_constant_expr(&mut self, expr: &'a Expr) {
        let prev = self.in_constant_scope;
        self.in_constant_scope = false;
        self.visit_expr(expr);
        self.in_constant_scope = prev;
    }

    fn visit_fstring(&mut self, fstr: &'a ruff_python_ast::ExprFString) {
        for part in &fstr.value {
            match part {
                ruff_python_ast::FStringPartRef::Literal(lit) => {
                    self.out.push(LiteralOccurrence {
                        node: AstNode::from_span(self.file, span_from_ruff_range(lit.range())),
                        value: LiteralValue::Str(lit.as_str().to_string()),
                        role: LiteralRole::Inline,
                    });
                }
                ruff_python_ast::FStringPartRef::FString(fpart) => {
                    if fpart
                        .elements
                        .iter()
                        .any(ruff_python_ast::InterpolatedStringElement::is_interpolation)
                    {
                        for elt in &fpart.elements {
                            self.visit_interpolated_string_element(elt);
                        }
                    } else {
                        let text: String = fpart
                            .elements
                            .iter()
                            .filter_map(|elt| elt.as_literal().map(|lit| lit.value.as_ref()))
                            .collect();
                        self.out.push(LiteralOccurrence {
                            node: AstNode::from_span(
                                self.file,
                                span_from_ruff_range(fpart.range()),
                            ),
                            value: LiteralValue::Str(text),
                            role: LiteralRole::Inline,
                        });
                    }
                }
            }
        }
    }
}

impl<'a> SourceOrderVisitor<'a> for LiteralOccurrenceCollector<'a> {
    fn visit_annotation(&mut self, _expr: &'a Expr) {}

    fn visit_stmt(&mut self, statement: &'a Stmt) {
        match statement {
            Stmt::TypeAlias(_) => {}
            Stmt::Expr(expr_statement) if is_standalone_string_expr(&expr_statement.value) => {}
            Stmt::Assign(assign)
                if self.in_constant_scope
                    && matches!(
                        assign.targets.as_slice(),
                        [Expr::Name(name)] if is_constant_name(name.id.as_str())
                    ) =>
            {
                self.record_constant_rhs(&assign.value);
            }
            Stmt::AnnAssign(ann)
                if self.in_constant_scope
                    && (matches!(
                        ann.target.as_ref(),
                        Expr::Name(name) if is_constant_name(name.id.as_str())
                    ) || has_final_annotation_expr(&ann.annotation, &self.file.source)) =>
            {
                if let Some(value) = &ann.value {
                    self.record_constant_rhs(value);
                }
            }
            Stmt::ClassDef(cls) => {
                let prev = self.in_constant_scope;
                self.in_constant_scope = false;
                for dec in &cls.decorator_list {
                    self.visit_decorator(dec);
                }
                if let Some(args) = &cls.arguments {
                    self.visit_arguments(args);
                }
                self.in_constant_scope = true;
                self.visit_body(&cls.body);
                self.in_constant_scope = prev;
            }
            Stmt::If(if_statement) => {
                self.visit_non_constant_expr(&if_statement.test);
                self.visit_body(&if_statement.body);
                for clause in &if_statement.elif_else_clauses {
                    if let Some(test) = &clause.test {
                        self.visit_non_constant_expr(test);
                    }
                    self.visit_body(&clause.body);
                }
            }
            Stmt::Try(try_statement) => {
                self.visit_body(&try_statement.body);
                for handler in &try_statement.handlers {
                    let ExceptHandler::ExceptHandler(except_handler) = handler;
                    if let Some(type_expr) = &except_handler.type_ {
                        self.visit_non_constant_expr(type_expr);
                    }
                    self.visit_body(&except_handler.body);
                }
                self.visit_body(&try_statement.orelse);
                self.visit_body(&try_statement.finalbody);
            }
            Stmt::With(with_statement) => {
                for item in &with_statement.items {
                    self.visit_non_constant_expr(&item.context_expr);
                    if let Some(vars) = &item.optional_vars {
                        self.visit_non_constant_expr(vars);
                    }
                }
                self.visit_body(&with_statement.body);
            }
            Stmt::FunctionDef(_) | Stmt::For(_) | Stmt::While(_) | Stmt::Match(_) => {
                let prev = self.in_constant_scope;
                self.in_constant_scope = false;
                walk_stmt(self, statement);
                self.in_constant_scope = prev;
            }
            _ => walk_stmt(self, statement),
        }
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Subscript(sub)
                if resolve_path_and_terminal_expr(&sub.value, &self.file.source).1 == "Literal" =>
            {
                return;
            }
            Expr::Call(call) => {
                let (_, terminal) = resolve_path_and_terminal_expr(&call.func, &self.file.source);
                if TYPE_NAME_FIRST_ARGUMENT_CALLS.contains(&terminal.as_str()) {
                    self.visit_expr(&call.func);
                    if call.arguments.args.is_empty() {
                        for kw in call.arguments.keywords.iter().skip(1) {
                            self.visit_keyword(kw);
                        }
                    } else {
                        for arg in call.arguments.args.iter().skip(1) {
                            self.visit_expr(arg);
                        }
                        for kw in &call.arguments.keywords {
                            self.visit_keyword(kw);
                        }
                    }
                    return;
                }
            }
            Expr::UnaryOp(unary) if unary.op == ruff_python_ast::UnaryOp::USub => {
                if let Some(val) = number_literal_value(&unary.operand, &self.file.source)
                    && let Some(negated) = val.negated()
                {
                    self.out.push(LiteralOccurrence {
                        node: AstNode::from_span(self.file, span_from_ruff_range(unary.range())),
                        value: negated,
                        role: LiteralRole::Inline,
                    });
                    return;
                }
            }
            Expr::NumberLiteral(number) => {
                if let Some(val) = number_literal_value(expr, &self.file.source) {
                    self.out.push(LiteralOccurrence {
                        node: AstNode::from_span(self.file, span_from_ruff_range(number.range())),
                        value: val,
                        role: LiteralRole::Inline,
                    });
                }
                return;
            }
            Expr::StringLiteral(str_lit) => {
                for part in str_lit.value.as_slice() {
                    self.out.push(LiteralOccurrence {
                        node: AstNode::from_span(self.file, span_from_ruff_range(part.range())),
                        value: LiteralValue::Str(part.as_str().to_string()),
                        role: LiteralRole::Inline,
                    });
                }
                return;
            }
            Expr::BytesLiteral(bytes_lit) => {
                for part in bytes_lit.value.as_slice() {
                    self.out.push(LiteralOccurrence {
                        node: AstNode::from_span(self.file, span_from_ruff_range(part.range())),
                        value: LiteralValue::Bytes(
                            String::from_utf8_lossy(part.as_slice()).into_owned(),
                        ),
                        role: LiteralRole::Inline,
                    });
                }
                return;
            }
            Expr::FString(fstr) => {
                self.visit_fstring(fstr);
                return;
            }
            _ => {}
        }
        walk_expr(self, expr);
    }
}

/// Collects Python literal occurrences (see [`super::collect_literal_occurrences`]).
#[must_use]
pub(super) fn collect_literal_occurrences(file: &ParsedFile) -> Vec<LiteralOccurrence<'_>> {
    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut collector = LiteralOccurrenceCollector {
        file,
        in_constant_scope: true,
        out: Vec::new(),
    };
    collector.visit_body(&parsed.syntax().body);
    collector.out
}

#[cfg(test)]
mod tests {
    use super::classes::is_in_protocol_or_abc_class;
    use super::functions::is_stub_function_body;
    use super::*;
    use crate::code_lint::ast::collect_call_candidates;
    use crate::diagnostic::Language;
    use LiteralRole::{ConstantDefinition, Inline};

    #[test]
    fn test_collect_bindings_python() {
        let source = indoc::indoc! {r"
            import os
            import os.path
            import numpy as np
            from sys import stderr as err
            from os import path
            import sys, re
            from os import * # wildcard
            c = 2
            d, e = 3, 4
            for x in range(10): pass
            [y for y in range(10)]
            def foo(b: int = 1):
                pass
            try:
                pass
            except Exception as g:
                pass
            (v := 1)
            class MyClass:
                def my_method(self):
                    pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let bindings = collect_bindings(&file);
        let names: Vec<String> = bindings
            .iter()
            .map(|node| node.text().to_string())
            .collect();
        assert_eq!(
            names,
            vec![
                "os",
                "os",
                "np",
                "err",
                "path",
                "sys",
                "re",
                "c",
                "d",
                "e",
                "x",
                "y",
                "foo",
                "b",
                "g",
                "v",
                "MyClass",
                "my_method",
                "self",
            ]
        );
    }

    #[test]
    fn test_collect_bindings_python_match_case() {
        let source = indoc::indoc! {r"
            match val:
                case Point(x, y=z):
                    pass
                case [a, b]:
                    pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let bindings = collect_bindings(&file);
        let names: Vec<String> = bindings
            .iter()
            .map(|node| node.text().to_string())
            .collect();
        assert_eq!(names, vec!["x", "z", "a", "b"]);
    }

    #[test]
    fn test_find_enclosing_with_helpers() {
        let source = indoc::indoc! {r"
            with suppress(FileNotFoundError):
                pass
            x = suppress(KeyError)
        "};
        let file = ParsedFile::new(source, Language::Python);
        let calls: Vec<_> = collect_call_candidates(&file)
            .into_iter()
            .map(|candidate| candidate.node)
            .collect();
        assert_eq!(calls.len(), 2);

        // First call is inside a with_statement
        let with_item = find_enclosing_with_item(&calls[0]);
        assert!(with_item.is_some());
        assert!(find_enclosing_with_statement(&calls[0]).is_some());

        // Second call is outside a with_statement
        assert!(find_enclosing_with_item(&calls[1]).is_none());
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
        let func = &extract_function_signatures(&file)[0].node;
        let decorators = extract_decorators(func);
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
        let func = &extract_function_signatures(&file)[0].node;
        let decorators = extract_decorators(func);

        let dec0 = &decorators[0];
        assert_eq!(dec0.terminal_name, "dataclass");
        assert_eq!(
            dec0.get_arg("frozen").and_then(KeywordArg::as_bool),
            Some(true)
        );
        assert_eq!(
            dec0.get_arg("slots").and_then(KeywordArg::as_bool),
            Some(false)
        );
        assert!(dec0.get_arg("unknown").is_none());
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
        let func = &extract_function_signatures(&file)[0].node;
        let decorators = extract_decorators(func);

        let dec0 = &decorators[0];
        assert_eq!(dec0.terminal_name, "parametrize");
        assert_eq!(dec0.path, "pytest.mark.parametrize");

        let dec1 = &decorators[1];
        assert_eq!(dec1.terminal_name, "custom");
        assert!(dec1.call_node.is_none());
    }

    #[test]
    fn test_has_decorator_predicate_matching() {
        let source = indoc::indoc! {r#"
            @pytest.mark.parametrize("x", [1, 2])
            def foo():
                pass
        "#};
        let file = ParsedFile::new(source, Language::Python);
        let func = &extract_function_signatures(&file)[0].node;

        assert!(has_decorator(func, |name| name == "parametrize"));
        assert!(has_decorator(func, |path| path == "pytest.mark.parametrize"));
        assert!(!has_decorator(func, |name| name == "override"));
    }

    #[test]
    fn test_extract_classes_and_inheritance() {
        let source = indoc::indoc! {r"
            class FakeService(abc.ABC, Protocol):
                pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let classes = extract_classes(&file);
        assert_eq!(classes.len(), 1);

        let cls = &classes[0];
        assert_eq!(cls.name, "FakeService");
        assert!(cls.inherits_from("Protocol"));
        assert!(cls.inherits_from("ABC"));
    }

    #[test]
    fn test_extract_classes_bases_metadata() {
        let source = indoc::indoc! {r"
            class FakeService(abc.ABC, Protocol):
                pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let classes = extract_classes(&file);
        let cls = &classes[0];
        assert_eq!(cls.bases.len(), 2);
        assert_eq!(cls.bases[0].name, "abc.ABC");
        assert_eq!(cls.bases[1].name, "Protocol");
    }

    #[test]
    fn test_extract_classes_dataclass() {
        let source = indoc::indoc! {r"
            @dataclass(frozen=True)
            class Config:
                pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let classes = extract_classes(&file);
        assert_eq!(classes.len(), 1);

        let cls = &classes[0];
        assert_eq!(cls.name, "Config");
        assert_eq!(cls.decorators[0].terminal_name, "dataclass");
        assert!(!cls.inherits_from("Protocol"));
    }

    #[test]
    fn test_extract_parameters_kinds() {
        let source = indoc::indoc! {r"
            def handler(self, a: int, b: str = 'hello', *, c: bool, **kwargs):
                pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let func = &extract_function_signatures(&file)[0].node;
        let params = extract_parameters(func);

        assert_eq!(params[0].kind, PythonParameterKind::Receiver);
        assert_eq!(params[1].kind, PythonParameterKind::Positional);
        assert_eq!(params[3].kind, PythonParameterKind::KeywordOnly);
        assert_eq!(params[4].kind, PythonParameterKind::VarKeyword);
    }

    #[test]
    fn test_extract_parameters_type_annotations() {
        let source = indoc::indoc! {r"
            def handler(self, a: int, b: str = 'hello', *, c: bool, **kwargs):
                pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let func = &extract_function_signatures(&file)[0].node;
        let params = extract_parameters(func);

        assert_eq!(params[1].name, "a");
        assert_eq!(params[1].type_text.as_deref(), Some("int"));
        assert_eq!(params[2].name, "b");
        assert_eq!(params[2].type_text.as_deref(), Some("str"));
    }

    /// Module-level reads form their own scope. `rule_test!` cannot cover this: repeating a
    /// fail case in one file merges both copies into the same module scope.
    #[test]
    fn test_collect_positional_reads_module_scope() {
        let source = indoc::indoc! {r"
            src, dst = sys.argv[1], sys.argv[2]

            def main(argv):
                return argv[1]
        "};
        let file = ParsedFile::new(source, Language::Python);
        let reads_by_scope: Vec<Vec<(String, i64)>> = collect_positional_reads(&file)
            .into_iter()
            .map(|scope| {
                scope
                    .reads
                    .into_iter()
                    .map(|read| (read.receiver, read.position))
                    .collect()
            })
            .collect();

        let module_reads = vec![("sys.argv".to_string(), 1), ("sys.argv".to_string(), 2)];
        assert!(
            reads_by_scope.contains(&module_reads),
            "no module scope holding exactly the top-level reads: {reads_by_scope:?}"
        );
    }

    #[rstest::rstest]
    #[case::a1_bare_concrete("def f(x: list, y: dict, z: typing.List) -> None: pass", &[&["list"][..], &["dict"][..], &["typing.List"][..]])]
    #[case::a2_pep585_generic("def f(x: list[int], y: dict[str, int], z: set[str]) -> None: pass", &[&["list"][..], &["dict"][..], &["set"][..]])]
    #[case::a3_pep484_qualified_and_unqualified("def f(x: typing.List[int], y: typing.Dict[str, int], z: typing.Set[str], w: Set[str]) -> None: pass", &[&["typing.List"][..], &["typing.Dict"][..], &["typing.Set"][..], &["Set"][..]])]
    #[case::a4_pep604_union("def f(x: list[int] | None, y: int | list[str] | set[int]) -> None: pass", &[&["list"][..], &["list", "set"][..]])]
    #[case::a5_qualified_pep604_union("def f(x: typing.List[int] | None) -> None: pass", &[&["typing.List"][..]])]
    #[case::a6_optional_and_union("def f(x: Optional[list[int]], y: Union[list[int], str, None]) -> None: pass", &[&["list"][..], &["list"][..]])]
    #[case::a7_qualified_optional_and_union("def f(x: typing.Optional[typing.Dict[str, int]], y: typing.Union[list[int], None]) -> None: pass", &[&["typing.Dict"][..], &["list"][..]])]
    #[case::a8_annotated_unwraps_first_arg("def f(x: Annotated[list[int], 'meta'], y: typing.Annotated[set[str], Doc('x')]) -> None: pass", &[&["list"][..], &["set"][..]])]
    #[case::a9_annotated_ignores_metadata("def f(x: Annotated[Sequence[int], list]) -> None: pass", &[&[][..]])]
    #[case::a10_qualifiers("def f(x: ClassVar[list[str]], y: Final[dict[str, int]], z: Required[set[str]]) -> None: pass", &[&["list"][..], &["dict"][..], &["set"][..]])]
    #[case::a11_covariant_single_and_tuple_containers("def f(a: Sequence[list[int]], b: Collection[set[str]], c: Awaitable[list[int]], d: tuple[str, dict[str, int]], e: tuple[list[int], ...]) -> None: pass", &[&["list"][..], &["set"][..], &["list"][..], &["dict"][..], &["list"][..]])]
    #[case::a12_mapping_covariant_value("def f(x: Mapping[str, list[int]]) -> None: pass", &[&["list"][..]])]
    #[case::a13_callable_covariant_return("def f(cb: Callable[[int], list[str]]) -> None: pass", &[&["list"][..]])]
    #[case::a14_callable_contravariant_param_ignored("def f(cb: Callable[[list[int]], None]) -> None: pass", &[&[][..]])]
    #[case::a15_invariant_mutable_outer_ignored("def f(x: MutableMapping[str, list[int]], y: MutableSequence[list[int]]) -> None: pass", &[&[][..], &[][..]])]
    #[case::a16_unknown_generic_ignored("def f(x: CustomBox[list[int]]) -> None: pass", &[&[][..]])]
    #[case::a17_abstract_collections_pass("def f(a: Sequence[int], b: Mapping[str, int], c: AbstractSet[str], d: collections.abc.Set[str]) -> None: pass", &[&[][..], &[][..], &[][..], &[][..]])]
    #[case::a17b_unaliased_collections_abc_set_import("from collections.abc import Set\ndef f(a: Set[str], b: typing.Set[str]) -> None: pass", &[&[][..], &["typing.Set"][..]])]
    #[case::a18_immutable_builtins_pass("def f(a: tuple[int, ...], b: frozenset[str], c: bytes, d: str) -> None: pass", &[&[][..], &[][..], &[][..], &[][..]])]
    #[case::a19_invariant_concrete_outer_not_recursed("def f(x: list[set[str]], y: dict[str, list[int]]) -> None: pass", &[&["list"][..], &["dict"][..]])]
    #[case::a20_typed_dict_qualifiers("def f(x: NotRequired[list[int]], y: ReadOnly[dict[str, int]]) -> None: pass", &[&["list"][..], &["dict"][..]])]
    #[case::a21_generator_yield_and_return_positions("def f(a: Generator[list[int], set[str], dict[str, int]], b: Coroutine[None, set[str], list[int]], c: AsyncGenerator[list[int], set[str]]) -> None: pass", &[&["list", "dict"][..], &["list"][..], &["list"][..]])]
    #[case::a22_iterator_and_abstract_set_covariant("def f(a: Iterable[list[int]], b: Iterator[dict[str, int]], c: AbstractSet[frozenset[int]]) -> None: pass", &[&["list"][..], &["dict"][..], &[][..]])]
    #[case::a23_qualified_covariant_outer("def f(x: collections.abc.Sequence[list[int]], y: typing.Mapping[str, set[str]]) -> None: pass", &[&["list"][..], &["set"][..]])]
    fn test_collect_collection_types_concrete_mutable_matrix_a(
        #[case] source: &str,
        #[case] expected_per_param: &[&[&str]],
    ) {
        let file = ParsedFile::new(source, Language::Python);
        let abc_set_imported = has_unaliased_collections_abc_set_import(&file);
        let sigs = extract_function_signatures(&file);
        assert_eq!(sigs.len(), 1);
        let actual: Vec<Vec<String>> = sigs[0]
            .parameters
            .iter()
            .map(|parameter| {
                let type_node = parameter.type_node.as_ref().expect("param should be typed");
                collect_collection_types(
                    type_node,
                    AnnotationTraversalDepth::CovariantPositions,
                    abc_set_imported,
                )
                .into_iter()
                .filter(|collection_type| collection_type.kind == CollectionKind::ConcreteMutable)
                .map(|collection_type| collection_type.path)
                .collect()
            })
            .collect();
        let expected: Vec<Vec<String>> = expected_per_param
            .iter()
            .map(|slice| {
                slice
                    .iter()
                    .map(|expected_name| (*expected_name).to_string())
                    .collect()
            })
            .collect();
        assert_eq!(actual, expected);
    }

    #[rstest::rstest]
    #[case::ellipsis_stub("def f(x: list[int]) -> None: ...", true)]
    #[case::pass_stub("def f(x: list[int]) -> None:\n    pass", true)]
    #[case::docstring_and_ellipsis(
        "def f(x: list[int]) -> None:\n    \"\"\"Doc.\"\"\"\n    ...",
        true
    )]
    #[case::raise_not_implemented_bare(
        "def f(x: list[int]) -> None:\n    raise NotImplementedError",
        true
    )]
    #[case::raise_not_implemented_call(
        "def f(x: list[int]) -> None:\n    raise NotImplementedError('todo')",
        true
    )]
    #[case::real_body("def f(x: list[int]) -> int:\n    return len(x)", false)]
    #[case::raise_from_not_implemented_cause(
        "def f(x: list[int]) -> None:\n    raise ValueError() from NotImplementedError",
        false
    )]
    fn test_is_stub_function_body(#[case] source: &str, #[case] expected: bool) {
        let file = ParsedFile::new(source, Language::Python);
        let sigs = extract_function_signatures(&file);
        assert_eq!(is_stub_function_body(&sigs[0].node), expected);
    }

    #[rstest::rstest]
    #[case::protocol_class(
        "class P(Protocol):\n    def f(self, x: list[int]) -> None: pass",
        true
    )]
    #[case::generic_protocol_class(
        "class P(typing.Protocol[T]):\n    def f(self, x: list[int]) -> None: pass",
        true
    )]
    #[case::abc_class("class A(abc.ABC):\n    def f(self, x: list[int]) -> None: pass", true)]
    #[case::abc_meta_class(
        "class A(metaclass=abc.ABCMeta):\n    def f(self, x: list[int]) -> None: pass",
        true
    )]
    #[case::regular_class("class C:\n    def f(self, x: list[int]) -> None: pass", false)]
    fn test_is_in_protocol_or_abc_class(#[case] source: &str, #[case] expected: bool) {
        let file = ParsedFile::new(source, Language::Python);
        let sigs = extract_function_signatures(&file);
        assert_eq!(is_in_protocol_or_abc_class(&sigs[0].node), expected);
    }

    /// Collects the top-level collection types of the annotation of the first parameter of the
    /// first function in `source`, as `(path, kind, shape)`.
    fn top_level_collection_types(source: &str) -> Vec<(String, CollectionKind, CollectionShape)> {
        let file = ParsedFile::new(source, Language::Python);
        let sigs = extract_function_signatures(&file);
        let type_node = sigs[0].parameters[0]
            .type_node
            .as_ref()
            .expect("param should be typed");
        collect_collection_types(
            type_node,
            AnnotationTraversalDepth::TransparentWrappersOnly,
            has_unaliased_collections_abc_set_import(&file),
        )
        .into_iter()
        .map(|collection_type| {
            (
                collection_type.path,
                collection_type.kind,
                collection_type.shape,
            )
        })
        .collect()
    }

    #[rstest::rstest]
    #[case::list(
        "def f(x: list[int]): pass",
        "list",
        CollectionKind::ConcreteMutable,
        CollectionShape::Sequence
    )]
    #[case::typing_dict(
        "def f(x: typing.Dict[str, int]): pass",
        "typing.Dict",
        CollectionKind::ConcreteMutable,
        CollectionShape::Mapping
    )]
    #[case::unqualified_set(
        "def f(x: Set[int]): pass",
        "Set",
        CollectionKind::ConcreteMutable,
        CollectionShape::Set
    )]
    #[case::deque(
        "def f(x: collections.deque[int]): pass",
        "collections.deque",
        CollectionKind::ConcreteMutable,
        CollectionShape::Sequence
    )]
    #[case::counter(
        "def f(x: collections.Counter[str]): pass",
        "collections.Counter",
        CollectionKind::ConcreteMutable,
        CollectionShape::Mapping
    )]
    #[case::mutable_sequence(
        "def f(x: MutableSequence[int]): pass",
        "MutableSequence",
        CollectionKind::AbstractMutable,
        CollectionShape::Sequence
    )]
    #[case::mutable_mapping(
        "def f(x: collections.abc.MutableMapping[str, int]): pass",
        "collections.abc.MutableMapping",
        CollectionKind::AbstractMutable,
        CollectionShape::Mapping
    )]
    #[case::mutable_set(
        "def f(x: typing.MutableSet[int] | None): pass",
        "typing.MutableSet",
        CollectionKind::AbstractMutable,
        CollectionShape::Set
    )]
    #[case::sequence(
        "def f(x: Optional[collections.abc.Sequence[int]]): pass",
        "collections.abc.Sequence",
        CollectionKind::AbstractReadOnly,
        CollectionShape::Sequence
    )]
    #[case::mapping(
        "def f(x: Mapping[str, Sequence[int]]): pass",
        "Mapping",
        CollectionKind::AbstractReadOnly,
        CollectionShape::Mapping
    )]
    #[case::abc_set(
        "def f(x: collections.abc.Set[int]): pass",
        "collections.abc.Set",
        CollectionKind::AbstractReadOnly,
        CollectionShape::Set
    )]
    #[case::imported_abc_set(
        "from collections.abc import Set\ndef f(x: Set[int]): pass",
        "Set",
        CollectionKind::AbstractReadOnly,
        CollectionShape::Set
    )]
    #[case::collection(
        "def f(x: typing.Collection[int]): pass",
        "typing.Collection",
        CollectionKind::AbstractReadOnly,
        CollectionShape::Iterable
    )]
    #[case::iterable(
        "def f(x: Iterable[int]): pass",
        "Iterable",
        CollectionKind::AbstractReadOnly,
        CollectionShape::Iterable
    )]
    #[case::tuple(
        "def f(x: tuple[int, ...]): pass",
        "tuple",
        CollectionKind::Immutable,
        CollectionShape::Sequence
    )]
    #[case::frozenset(
        "def f(x: frozenset[int]): pass",
        "frozenset",
        CollectionKind::Immutable,
        CollectionShape::Set
    )]
    fn test_collection_type_taxonomy(
        #[case] source: &str,
        #[case] path: &str,
        #[case] kind: CollectionKind,
        #[case] shape: CollectionShape,
    ) {
        assert_eq!(
            top_level_collection_types(source),
            [(path.to_string(), kind, shape)]
        );
    }

    #[rstest::rstest]
    #[case::nested_not_collected("def f(x: Sequence[MutableSequence[int]]): pass", &["Sequence"])]
    #[case::unknown_module_ignored("def f(x: mylib.MutableSequence[int]): pass", &[])]
    #[case::union_in_order("def f(x: dict[str, int] | list[int] | dict[str, str]): pass", &["dict", "list"])]
    fn test_collect_top_level_collection_types(#[case] source: &str, #[case] expected: &[&str]) {
        let paths: Vec<String> = top_level_collection_types(source)
            .into_iter()
            .map(|(path, _, _)| path)
            .collect();
        assert_eq!(paths, expected);
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
    #[case::list_display("[1]", Some(("list", CollectionShape::Sequence)))]
    #[case::set_comprehension("{x for x in y}", Some(("set", CollectionShape::Set)))]
    #[case::empty_dict_display("{}", Some(("dict", CollectionShape::Mapping)))]
    #[case::generic_constructor_call("list[str]()", Some(("list", CollectionShape::Sequence)))]
    #[case::qualified_constructor_call(
        "collections.Counter()",
        Some(("collections.Counter", CollectionShape::Mapping))
    )]
    #[case::tuple_display("(1, 2)", None)]
    #[case::unknown_call("make()", None)]
    fn test_collection_display_and_call_callee_type(
        #[case] value: &str,
        #[case] expected: Option<(&str, CollectionShape)>,
    ) {
        let file = ParsedFile::new(&format!("VALUE = {value}"), Language::Python);
        let assignments = collect_module_assignments(&file);
        let value = assignments[0].value.as_ref().expect("value");
        let built = collection_display(value)
            .or_else(|| call_callee(value).and_then(|callee| collection_type(&callee, false)));
        assert_eq!(
            built
                .as_ref()
                .map(|collection| (collection.path.as_str(), collection.shape)),
            expected
        );
    }

    #[rstest::rstest]
    #[case::unused("def f(x):\n    return 1", ParameterCollectionCapability::Unused)]
    #[case::single_for(
        "def f(x):\n    for i in x:\n        print(i)",
        ParameterCollectionCapability::Iterable
    )]
    #[case::single_comprehension(
        "def f(x):\n    return [i for i in x]",
        ParameterCollectionCapability::Iterable
    )]
    #[case::single_consuming_builtin(
        "def f(x):\n    return sum(x)",
        ParameterCollectionCapability::Iterable
    )]
    #[case::len(
        "def f(x):\n    return len(x)",
        ParameterCollectionCapability::Collection
    )]
    #[case::membership(
        "def f(x):\n    return 1 in x",
        ParameterCollectionCapability::Collection
    )]
    #[case::truthiness(
        "def f(x):\n    if x:\n        return 1",
        ParameterCollectionCapability::Collection
    )]
    #[case::negation(
        "def f(x):\n    return not x",
        ParameterCollectionCapability::Collection
    )]
    #[case::two_passes(
        "def f(x):\n    return sum(x) + max(x)",
        ParameterCollectionCapability::Collection
    )]
    #[case::iteration_inside_loop(
        "def f(x):\n    for _ in range(3):\n        for i in x:\n            print(i)",
        ParameterCollectionCapability::Collection
    )]
    #[case::index("def f(x):\n    return x[0]", ParameterCollectionCapability::Sequence)]
    #[case::reversed(
        "def f(x):\n    return list(reversed(x))",
        ParameterCollectionCapability::Sequence
    )]
    #[case::index_method(
        "def f(x):\n    return x.index(1)",
        ParameterCollectionCapability::Sequence
    )]
    #[case::pattern_matching(
        "def f(x):\n    match x:\n        case [a]:\n            return a",
        ParameterCollectionCapability::Sequence
    )]
    #[case::passed_to_unknown_function(
        "def f(x):\n    return helper(x)",
        ParameterCollectionCapability::Sequence
    )]
    #[case::shadowing_nested_def_default(
        "def f(x):\n    def g(x=x[0]):\n        return x\n    return g",
        ParameterCollectionCapability::Sequence
    )]
    fn test_analyze_parameter_collection_capability(
        #[case] source: &str,
        #[case] expected: ParameterCollectionCapability,
    ) {
        let file = ParsedFile::new(source, Language::Python);
        let sigs = extract_function_signatures(&file);
        assert_eq!(
            analyze_parameter_collection_capability(&sigs[0].node, "x"),
            expected
        );
    }

    #[rstest::rstest]
    #[case::method_mutation("def f(x):\n    x.append(1)", true)]
    #[case::subscript_write("def f(x):\n    x[0] = 1", true)]
    #[case::subscript_delete("def f(x):\n    del x[0]", true)]
    #[case::augmented_assignment("def f(x):\n    x += [1]", true)]
    #[case::alias("def f(x):\n    y = x\n    return len(y)", true)]
    #[case::stored_in_container("def f(x):\n    y = [x]\n    return len(y)", true)]
    #[case::returned("def f(x):\n    return x", true)]
    #[case::yielded("def f(x):\n    yield x", true)]
    #[case::passed_to_unknown_function("def f(x):\n    helper(x)", true)]
    #[case::passed_to_method("def f(x):\n    registry.register(x)", true)]
    #[case::mutated_in_closure("def f(x):\n    def g():\n        x.append(1)\n    g()", true)]
    #[case::shadowing_nested_def_default(
        "def f(x):\n    def g(x=x):\n        x.append(1)\n    g()",
        true
    )]
    #[case::read_only_iteration("def f(x):\n    for i in x:\n        print(i)", false)]
    #[case::read_only_len("def f(x):\n    return len(x)", false)]
    #[case::shadowed_by_nested_def(
        "def f(x):\n    def g(x):\n        x.append(1)\n    g([])",
        false
    )]
    #[case::shadowed_by_lambda(
        "def f(x):\n    g = lambda x: x.append(1)\n    return len(x)",
        false
    )]
    fn test_is_parameter_mutated_or_escaping(#[case] source: &str, #[case] expected: bool) {
        let file = ParsedFile::new(source, Language::Python);
        let sigs = extract_function_signatures(&file);
        assert_eq!(
            is_parameter_mutated_or_escaping(&sigs[0].node, "x"),
            expected
        );
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

    #[rstest::rstest]
    #[case::class_level("class C:\n    items: list[int]", &[("items", false, false)])]
    #[case::private_included("class C:\n    _items: list[int]", &[("_items", false, false)])]
    #[case::init_attribute("class C:\n    def __init__(self):\n        self.items: list[int] = []", &[("items", false, false)])]
    #[case::self_mutation("class C:\n    items: list[int]\n    def add(self):\n        self.items.append(1)", &[("items", true, false)])]
    #[case::cls_mutation("class C:\n    items: list[int]\n    @classmethod\n    def add(cls):\n        cls.items.append(1)", &[("items", true, false)])]
    #[case::class_name_mutation("class C:\n    items: list[int]\n    def add(self):\n        C.items.append(1)", &[("items", true, false)])]
    #[case::subscript_delete("class C:\n    items: list[int]\n    def pop(self):\n        del self.items[0]", &[("items", true, false)])]
    #[case::protocol_member("class P(Protocol):\n    items: list[int]", &[("items", false, true)])]
    fn test_collect_class_attributes(
        #[case] source: &str,
        #[case] expected: &[(&str, bool, bool)],
    ) {
        let file = ParsedFile::new(source, Language::Python);
        let actual: Vec<(String, bool, bool)> = collect_class_attributes(&file)
            .into_iter()
            .map(|attribute| {
                (
                    attribute.name,
                    attribute.is_mutated_in_class,
                    attribute.is_in_protocol_or_abc,
                )
            })
            .collect();
        let expected: Vec<(String, bool, bool)> = expected
            .iter()
            .map(|(name, mutated, in_protocol)| ((*name).to_string(), *mutated, *in_protocol))
            .collect();
        assert_eq!(actual, expected);
    }

    #[rstest::rstest]
    #[case::module_upper_name_defines("MAX = 30", &[("30", ConstantDefinition)])]
    #[case::class_final_lowercase_defines("class C:\n    limit: Final[int] = -5", &[("-5", ConstantDefinition)])]
    #[case::aliased_final_defines("timeout: t.Final = 30", &[("30", ConstantDefinition)])]
    #[case::class_lowercase_is_inline("class C:\n    name = 'cc'", &[("'cc'", Inline)])]
    #[case::function_level_upper_name_is_inline("def f():\n    MAX = 30", &[("30", Inline)])]
    #[case::module_if_body_defines("if WIN:\n    RETRIES = 3\nelse:\n    RETRIES = 5", &[("3", ConstantDefinition), ("5", ConstantDefinition)])]
    #[case::module_except_body_defines("try:\n    import x\nexcept ImportError:\n    LIMIT = 9", &[("9", ConstantDefinition)])]
    #[case::function_if_body_is_inline("def f():\n    if a:\n        MAX = 30", &[("30", Inline)])]
    #[case::parenthesized_constant_defines("MSG = (\n    'refused'\n)", &[("'refused'", ConstantDefinition)])]
    #[case::composite_constant_not_collected("URLS = ['u1', 'u2']\nPAIR = 'aa' 'bb'", &[])]
    #[case::negation_anchored_on_operator("f(-42, 7 - 42)", &[("-42", Inline), ("7", Inline), ("42", Inline)])]
    #[case::negative_case_pattern("match m:\n    case [-42, 'xx']:\n        pass", &[("-42", Inline), ("'xx'", Inline)])]
    #[case::signed_numbers_in_patterns_collected("match m:\n    case {-404: _} | Resp(code=-404) | -7:\n        pass", &[("-404", Inline), ("-404", Inline), ("-7", Inline)])]
    #[case::docstring_skipped("def f():\n    '''Doc.'''\n    return 'rv'", &[("'rv'", Inline)])]
    #[case::annotations_skipped("def f(a: 'T' = 'dv') -> 'R':\n    v: 'V' = 'vv'", &[("'dv'", Inline), ("'vv'", Inline)])]
    #[case::literal_type_skipped("v = Literal['y']", &[])]
    #[case::interpolated_fstring_walked("print(f\"{row['st']} and\", f'plain')", &[("'st'", Inline), ("f'plain'", Inline)])]
    #[case::imaginary_skipped("z = 2j", &[])]
    fn test_collect_literal_occurrences_python(
        #[case] source: &str,
        #[case] expected: &[(&str, LiteralRole)],
    ) {
        let file = ParsedFile::new(source, Language::Python);
        let actual: Vec<(String, LiteralRole)> = collect_literal_occurrences(&file)
            .into_iter()
            .map(|occurrence| (occurrence.node.text().into_owned(), occurrence.role))
            .collect();
        let expected: Vec<(String, LiteralRole)> = expected
            .iter()
            .map(|(text, role)| ((*text).to_string(), *role))
            .collect();
        assert_eq!(actual, expected);
    }

    #[rstest::rstest]
    fn test_collect_literal_occurrences_python_skips_type_name_argument(
        #[values(
            "TypeVar",
            "NewType",
            "ParamSpec",
            "TypeVarTuple",
            "NamedTuple",
            "TypedDict",
            "typing.cast"
        )]
        callee: &str,
    ) {
        let file = ParsedFile::new(&format!("t = {callee}('Nm', 'vv')"), Language::Python);
        let texts: Vec<String> = collect_literal_occurrences(&file)
            .into_iter()
            .map(|occurrence| occurrence.node.text().into_owned())
            .collect();
        assert_eq!(texts, vec!["'vv'"]);
    }

    #[rstest::rstest]
    #[case::quote_style_ignored("'ab'", LiteralValue::Str("ab".to_string()))]
    #[case::triple_quoted("'''ab'''", LiteralValue::Str("ab".to_string()))]
    #[case::unicode_prefix("u'ab'", LiteralValue::Str("ab".to_string()))]
    #[case::bytes("b\"ab\"", LiteralValue::Bytes("ab".to_string()))]
    #[case::raw_bytes("Rb'ab'", LiteralValue::Bytes("ab".to_string()))]
    #[case::escapes_decoded("'a\\nb'", LiteralValue::Str("a\nb".to_string()))]
    #[case::raw_backslash_kept("r'a\\nb'", LiteralValue::Str("a\\nb".to_string()))]
    #[case::hex("0x1F", LiteralValue::Int(31))]
    #[case::separators("1_000", LiteralValue::Int(1000))]
    #[case::exponent("1e3", LiteralValue::Float(1000.0_f64.to_bits()))]
    #[case::leading_dot(".5", LiteralValue::Float(0.5_f64.to_bits()))]
    #[case::trailing_dot("3.", LiteralValue::Float(3.0_f64.to_bits()))]
    #[case::negative_float("-2.5", LiteralValue::Float((-2.5_f64).to_bits()))]
    #[case::negative_zero("-0.0", LiteralValue::Float(0.0_f64.to_bits()))]
    fn test_collect_literal_occurrences_python_values(
        #[case] literal: &str,
        #[case] expected: LiteralValue,
    ) {
        let file = ParsedFile::new(&format!("value = {literal}"), Language::Python);
        let values: Vec<LiteralValue> = collect_literal_occurrences(&file)
            .into_iter()
            .map(|occurrence| occurrence.value)
            .collect();
        assert_eq!(values, vec![expected]);
    }

    #[test]
    fn test_extract_classes_ignores_comments_and_keywords_in_superclasses() {
        let source = indoc::indoc! {r"
            class FakeClient(
                # Not a base class
                metaclass=ABCMeta,
            ):
                pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let classes = extract_classes(&file);
        assert_eq!(classes.len(), 1);
        assert!(classes[0].bases.is_empty());
    }

    #[test]
    fn test_python_base_class_is_structural_marker() {
        let source = indoc::indoc! {r"
            class C(
                HttpClient,
                object,
                Generic[T],
                Protocol,
                typing_extensions.Generic[T],
                typing_extensions.Protocol,
                ABC,
                abc.ABC,
                Repository[T],
                **kwargs,
            ):
                pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let classes = extract_classes(&file);
        let summary: Vec<(&str, bool)> = classes[0]
            .bases
            .iter()
            .map(|base| (base.name.as_str(), base.is_structural_marker()))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("HttpClient", false),
                ("object", true),
                ("Generic[T]", true),
                ("Protocol", true),
                ("typing_extensions.Generic[T]", true),
                ("typing_extensions.Protocol", true),
                ("ABC", true),
                ("abc.ABC", true),
                ("Repository[T]", false),
            ]
        );
    }

    #[rstest::rstest]
    #[case::init_and_method(
        "class C:\n    def __init__(self):\n        self.x: int = 1\n    async def reset(self):\n        if True:\n            self.y: str",
        &[("C", "__init__", "x", "int"), ("C", "reset", "y", "str")]
    )]
    #[case::private_collected_unannotated_skipped(
        "class C:\n    def __init__(self):\n        self._p: int = 1\n        self.pub = 2",
        &[("C", "__init__", "_p", "int")]
    )]
    #[case::staticmethod_classmethod_and_nested_func_skipped(
        "class C:\n    @staticmethod\n    def sm(self):\n        self.a: int = 1\n    @classmethod\n    def cm(cls):\n        cls.b: int = 2\n    def run(self):\n        def inner(self):\n            self.c: int = 3",
        &[]
    )]
    fn test_collect_instance_attribute_annotations(
        #[case] source: &str,
        #[case] expected: &[(&str, &str, &str, &str)],
    ) {
        let file = ParsedFile::new(source, Language::Python);
        let actual: Vec<(String, String, String, String)> =
            collect_instance_attribute_annotations(&file)
                .into_iter()
                .map(|attribute| {
                    (
                        attribute.class_name,
                        attribute.method_name,
                        attribute.name,
                        attribute.annotation.text().into_owned(),
                    )
                })
                .collect();
        let expected: Vec<(String, String, String, String)> = expected
            .iter()
            .map(|(class_name, method_name, name, annotation)| {
                (
                    (*class_name).to_string(),
                    (*method_name).to_string(),
                    (*name).to_string(),
                    (*annotation).to_string(),
                )
            })
            .collect();
        assert_eq!(actual, expected);
    }

    #[rstest::rstest]
    #[case::bare_final(
        "class C:\n    def __init__(self):\n        self.a: Final = 1",
        (true, false)
    )]
    #[case::qualified_bare_final(
        "class C:\n    def __init__(self):\n        self.a: typing.Final = 1",
        (true, false)
    )]
    #[case::annotated_bare_final(
        "class C:\n    def __init__(self):\n        self.a: Annotated[Final, 'm'] = 1",
        (true, false)
    )]
    #[case::parameterized_final(
        "class C:\n    def __init__(self):\n        self.a: Final[int] = 1",
        (false, false)
    )]
    #[case::dataclass(
        "@dataclass\nclass C:\n    def __post_init__(self):\n        self.a: int = 1",
        (false, true)
    )]
    #[case::pydantic_model(
        "class C(BaseModel):\n    def model_post_init(self, context):\n        self.a: int = 1",
        (false, true)
    )]
    fn test_instance_attribute_annotation_facts(
        #[case] source: &str,
        #[case] expected: (bool, bool),
    ) {
        let file = ParsedFile::new(source, Language::Python);
        let actual: Vec<(bool, bool)> = collect_instance_attribute_annotations(&file)
            .iter()
            .map(|attribute| {
                (
                    attribute.is_bare_final(),
                    attribute.is_in_field_synthesizing_class,
                )
            })
            .collect();
        assert_eq!(actual, [expected]);
    }

    #[rstest::rstest]
    #[case::simple_union("def f() -> str | int: pass", false, &["str", "int"])]
    #[case::optional_shorthand("def f() -> Optional[list[int]]: pass", true, &["list[int]"])]
    #[case::pep604_none("def f() -> Sequence[str] | None: pass", true, &["Sequence[str]"])]
    #[case::none_first("def f() -> None | set[int]: pass", true, &["set[int]"])]
    #[case::multiple_branches_with_none("def f() -> list[int] | set[str] | None: pass", true, &["list[int]", "set[str]"])]
    #[case::awaitable_coroutine_envelopes(
        "def f() -> Awaitable[Coroutine[Any, Any, Mapping[str, int] | None]]: pass",
        true,
        &["Mapping[str, int]"]
    )]
    #[case::annotated_wrapper("def f() -> Annotated[tuple[int, ...] | None, 'meta']: pass", true, &["tuple[int, ...]"])]
    fn test_return_type_union(
        #[case] source: &str,
        #[case] expected_has_none: bool,
        #[case] expected_branches: &[&str],
    ) {
        let file = ParsedFile::new(source, Language::Python);
        let signatures = extract_function_signatures(&file);
        let return_type_node = signatures[0]
            .return_type_node
            .as_ref()
            .expect("function should have return annotation");
        let union = return_type_union(return_type_node);
        assert_eq!(union.has_none, expected_has_none);
        let branches: Vec<_> = union.branches.iter().map(AstNode::text).collect();
        assert_eq!(branches, expected_branches);
    }

    #[test]
    fn test_extract_generic_type() {
        let file = ParsedFile::new("def f() -> tuple[int, ...]: pass", Language::Python);
        let signatures = extract_function_signatures(&file);
        let return_type_node = signatures[0].return_type_node.as_ref().unwrap();
        let (base, args) = extract_generic_type(return_type_node).expect("generic type");
        assert_eq!(base.text(), "tuple");
        assert_eq!(args.len(), 2);
        assert_eq!(args[0].text(), "int");
        assert_eq!(args[1].text(), "...");
    }

    #[rstest::rstest]
    #[case::simple("Order {order_id} filled", Some(&["order_id"][..]))]
    #[case::compound_attribute("Order {order.id} filled", Some(&["order"][..]))]
    #[case::compound_subscript("Order {order[id]} filled", Some(&["order"][..]))]
    #[case::conversion_and_spec("Order {order_id!r} {amount:.2f}", Some(&["order_id", "amount"][..]))]
    #[case::positional_empty_and_numbered("Order {} and {0} and {0.id} and {1[key]}", Some(&[][..]))]
    #[case::escaped_double_braces("Literal {{order_id}} value {}", Some(&[][..]))]
    #[case::triple_braces_captures_inner("Literal {{{order_id}}}", Some(&["order_id"][..]))]
    #[case::nested_format_spec("Value {:>{width}}", Some(&["width"][..]))]
    #[case::non_identifier_braces("Payload {\"order_id\": 1} and {a, b}", Some(&[][..]))]
    #[case::unclosed_opening_brace("Malformed {order_id in input", None)]
    #[case::unmatched_closing_brace("Malformed {order_id} stray }", None)]
    fn test_named_format_field_roots(#[case] message: &str, #[case] expected: Option<&[&str]>) {
        let expected = expected.map(|roots| roots.iter().map(|root| (*root).to_owned()).collect());
        assert_eq!(named_format_field_roots(message), expected);
    }

    #[test]
    fn test_collect_logger_calls_reads_callee_message_and_arguments() {
        let source = indoc::indoc! {r#"
            logger.info("Order {order_id} filled", order_id)
            self.log.error("Peer " "{peer.id} failed", **extra)
            logging.log(20, f"User {user_id}", user_id=1)
            print("not a logger", value)
        "#};
        let file = ParsedFile::new(source, Language::Python);
        let calls: Vec<_> = collect_logger_calls(&file)
            .into_iter()
            .map(|call| {
                let mut keyword_names: Vec<String> = call.keyword_names.into_iter().collect();
                keyword_names.sort();
                (
                    call.callee,
                    call.message,
                    call.has_trailing_positional_args,
                    call.has_keyword_splat,
                    keyword_names,
                )
            })
            .collect();
        assert_eq!(
            calls,
            vec![
                (
                    "logger.info".to_owned(),
                    Some("Order {order_id} filled".to_owned()),
                    true,
                    false,
                    vec![],
                ),
                (
                    "self.log.error".to_owned(),
                    Some("Peer {peer.id} failed".to_owned()),
                    false,
                    true,
                    vec![],
                ),
                (
                    "logging.log".to_owned(),
                    None,
                    false,
                    false,
                    vec!["user_id".to_owned()]
                ),
            ]
        );
    }

    #[test]
    fn test_collect_function_scopes_sibling_calls() {
        let source = indoc::indoc! {r"
            def orchestrate(x: int) -> int:
                first = step_one(x)
                second = step_two(first)
                return second + is_even(second)

            def step_one(x: int) -> int:
                return x + 1

            def step_two(x: int) -> int:
                return x * 2

            def is_even(n: int) -> bool:
                return True if n == 0 else is_odd(n - 1)

            def is_odd(n: int) -> bool:
                return False if n == 0 else is_even(n - 1)

            class Greeter:
                def greet(self) -> str:
                    return self.name() + format_name(self)

                def name(self) -> str:
                    return 'x'
        "};
        let file = ParsedFile::new(source, Language::Python);
        let scopes = collect_function_scopes(&file);
        let summary: Vec<(bool, &str, usize, Vec<String>)> = scopes
            .iter()
            .flat_map(|scope| {
                scope.functions.iter().map(|function| {
                    (
                        scope.is_class,
                        function.name.as_str(),
                        function.definition_order,
                        function
                            .sibling_calls
                            .iter()
                            .map(|call| format!("{} at {}", call.callee_name, call.node.text()))
                            .collect(),
                    )
                })
            })
            .collect();
        let calls = |texts: &[&str]| -> Vec<String> {
            texts.iter().map(|text| (*text).to_string()).collect()
        };
        assert_eq!(
            summary,
            vec![
                (
                    false,
                    "orchestrate",
                    0,
                    calls(&[
                        "step_one at step_one(x)",
                        "step_two at step_two(first)",
                        "is_even at is_even(second)",
                    ])
                ),
                (false, "step_one", 1, calls(&[])),
                (false, "step_two", 2, calls(&[])),
                (false, "is_even", 3, calls(&["is_odd at is_odd(n - 1)"])),
                (false, "is_odd", 4, calls(&["is_even at is_even(n - 1)"])),
                (true, "greet", 0, calls(&["name at self.name()"])),
                (true, "name", 1, calls(&[])),
            ]
        );
    }

    /// `(style, preceding_text, literals, [(field, conversion, format_spec)])`.
    type FormatStringSummary = (
        PythonFormatStyle,
        String,
        Vec<String>,
        Vec<(String, Option<String>, Option<String>)>,
    );

    fn summarize_format_strings(python_code: &str) -> Vec<FormatStringSummary> {
        let file = ParsedFile::new(python_code, Language::Python);
        collect_format_strings(&file)
            .into_iter()
            .map(|format_string| {
                let placeholders = format_string
                    .placeholders
                    .into_iter()
                    .map(|placeholder| {
                        (
                            placeholder.field,
                            placeholder.conversion,
                            placeholder.format_spec,
                        )
                    })
                    .collect();
                (
                    format_string.style,
                    format_string.preceding_text,
                    format_string.literals,
                    placeholders,
                )
            })
            .collect()
    }

    fn owned(texts: &[&str]) -> Vec<String> {
        texts.iter().map(|text| (*text).to_owned()).collect()
    }

    fn field(
        name: &str,
        conversion: Option<&str>,
        format_spec: Option<&str>,
    ) -> (String, Option<String>, Option<String>) {
        (
            name.to_owned(),
            conversion.map(str::to_owned),
            format_spec.map(str::to_owned),
        )
    }

    #[test]
    fn test_collect_format_strings_splits_fstrings() {
        assert_eq!(
            summarize_format_strings(r#"message = f"Copied '{ source !r:>8}' {count=}""#),
            vec![(
                PythonFormatStyle::FString,
                String::new(),
                owned(&["Copied '", "' ", ""]),
                vec![
                    field("source", Some("r"), Some(">8")),
                    field("count", None, None)
                ],
            )]
        );
    }

    #[test]
    fn test_collect_format_strings_splits_str_format_templates() {
        assert_eq!(
            summarize_format_strings(
                r#"message = "Copied '{}' to '{target.name!s}' {{raw}}".format(a, target=b)"#
            ),
            vec![(
                PythonFormatStyle::StrFormat,
                String::new(),
                owned(&["Copied '", "' to '", "' {{raw}}"]),
                vec![field("", None, None), field("target.name", Some("s"), None)],
            )]
        );
    }

    #[rstest::rstest]
    #[case::percent_operator(r#"message = "Got %s, %(key)5.2f and 100%%" % values"#)]
    #[case::logger_with_arguments(r#"logger.info("Got %s, %(key)5.2f and 100%%", values)"#)]
    fn test_collect_format_strings_splits_printf_templates(#[case] python_code: &str) {
        assert_eq!(
            summarize_format_strings(python_code),
            vec![(
                PythonFormatStyle::Printf,
                String::new(),
                owned(&["Got ", ", ", " and 100%%"]),
                vec![
                    field("", Some("s"), None),
                    field("key", Some("f"), Some("5.2"))
                ],
            )]
        );
    }

    #[test]
    fn test_collect_format_strings_reads_concatenated_prefix() {
        let summaries = summarize_format_strings("message = (\"Load \" f\"'{name}'\")");
        let (style, preceding_text, ..) = &summaries[0];
        assert_eq!(
            (summaries.len(), *style, preceding_text.as_str()),
            (1, PythonFormatStyle::FString, "Load ")
        );
    }

    #[test]
    fn test_collect_format_strings_skips_unformatted_raw_and_byte_strings() {
        let source = indoc::indoc! {r#"
            plain = "Invalid '{name}' or '%s'"
            raw = r"Invalid '{name}'".format(name=value)
            byte = b"Invalid '%s'" % value
            no_arguments = logger.info("Literal '%s'")
        "#};
        assert_eq!(summarize_format_strings(source), vec![]);
    }
}
