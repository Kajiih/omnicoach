//! Python binding extraction, lexical scope boundaries, and calls between sibling functions.

use super::{
    AstNode, ParsedFile, direct_function_definitions, extract_decorators_from_slice,
    method_receiver_name_ast,
};
use crate::code_lint::ast::span_from_ruff_range;
use crate::diagnostic::SourceSpan;
use ruff_python_ast::visitor::source_order::{
    SourceOrderVisitor, walk_comprehension, walk_except_handler, walk_expr, walk_match_case,
    walk_parameters, walk_stmt, walk_with_item,
};
use ruff_python_ast::{
    Alias, Comprehension, ExceptHandler, Expr, MatchCase, Parameters, Pattern, Stmt, StmtClassDef,
    StmtFunctionDef, WithItem,
};
use ruff_text_size::Ranged as _;
use std::collections::{HashMap, HashSet};

/// Recursively extracts binding identifiers from an assignment/loop target expression, stopping
/// at `Expr::Attribute` and `Expr::Subscript` so `self.ctx = 1` or `items[idx] = 1` does not
/// record `ctx` or `idx` as a local variable binding.
pub(super) fn extract_from_expr_target<'a>(
    target: &Expr,
    file: &'a ParsedFile,
    bindings: &mut Vec<AstNode<'a>>,
) {
    match target {
        Expr::Name(name) => {
            if name.id.as_str() != "_" {
                bindings.push(AstNode::from_span(file, span_from_ruff_range(name.range)));
            }
        }
        Expr::Tuple(tuple) => {
            for elt in &tuple.elts {
                extract_from_expr_target(elt, file, bindings);
            }
        }
        Expr::List(list) => {
            for elt in &list.elts {
                extract_from_expr_target(elt, file, bindings);
            }
        }
        Expr::Starred(starred) => {
            extract_from_expr_target(&starred.value, file, bindings);
        }
        _ => {}
    }
}

/// Recursively extracts binding identifiers from a `match` `Pattern`.
fn extract_from_match_pattern<'a>(
    pattern: &Pattern,
    file: &'a ParsedFile,
    bindings: &mut Vec<AstNode<'a>>,
) {
    match pattern {
        Pattern::MatchValue(_) | Pattern::MatchSingleton(_) => {}
        Pattern::MatchSequence(seq) => {
            for child in &seq.patterns {
                extract_from_match_pattern(child, file, bindings);
            }
        }
        Pattern::MatchMapping(mapping) => {
            for child in &mapping.patterns {
                extract_from_match_pattern(child, file, bindings);
            }
            if let Some(rest) = &mapping.rest
                && rest.id.as_str() != "_"
            {
                bindings.push(AstNode::from_span(file, span_from_ruff_range(rest.range)));
            }
        }
        Pattern::MatchClass(class_pat) => {
            for child in &class_pat.arguments.patterns {
                extract_from_match_pattern(child, file, bindings);
            }
            for keyword in &class_pat.arguments.keywords {
                extract_from_match_pattern(&keyword.pattern, file, bindings);
            }
        }
        Pattern::MatchStar(star) => {
            if let Some(name) = &star.name
                && name.id.as_str() != "_"
            {
                bindings.push(AstNode::from_span(file, span_from_ruff_range(name.range)));
            }
        }
        Pattern::MatchAs(as_pat) => {
            if let Some(inner) = &as_pat.pattern {
                extract_from_match_pattern(inner, file, bindings);
            }
            if let Some(name) = &as_pat.name
                && name.id.as_str() != "_"
            {
                bindings.push(AstNode::from_span(file, span_from_ruff_range(name.range)));
            }
        }
        Pattern::MatchOr(or_pat) => {
            for child in &or_pat.patterns {
                extract_from_match_pattern(child, file, bindings);
            }
        }
    }
}

/// Inserts all identifier names bound by `target` into `names`.
fn extend_expr_target_names(target: &Expr, file: &ParsedFile, names: &mut HashSet<String>) {
    let mut nodes = Vec::new();
    extract_from_expr_target(target, file, &mut nodes);
    for node in nodes {
        names.insert(node.text().into_owned());
    }
}

/// Inserts all identifier names bound by `pattern` into `names`.
fn extend_match_pattern_names(pattern: &Pattern, file: &ParsedFile, names: &mut HashSet<String>) {
    let mut nodes = Vec::new();
    extract_from_match_pattern(pattern, file, &mut nodes);
    for node in nodes {
        names.insert(node.text().into_owned());
    }
}

/// Extracts parameter name bindings from `parameters` in source order.
fn extract_from_parameters<'a>(
    parameters: &Parameters,
    file: &'a ParsedFile,
    bindings: &mut Vec<AstNode<'a>>,
) {
    let mut param_idents = Vec::new();
    for pwd in parameters.posonlyargs.iter().chain(parameters.args.iter()) {
        param_idents.push(&pwd.parameter.name);
    }
    if let Some(vararg) = &parameters.vararg {
        param_idents.push(&vararg.name);
    }
    for pwd in &parameters.kwonlyargs {
        param_idents.push(&pwd.parameter.name);
    }
    if let Some(kwarg) = &parameters.kwarg {
        param_idents.push(&kwarg.name);
    }
    for ident in param_idents {
        if ident.id.as_str() != "_" {
            bindings.push(AstNode::from_span(file, span_from_ruff_range(ident.range)));
        }
    }
}

/// Extracts bindings from Python `import` or `from ... import` alias lists.
fn extract_from_import_aliases<'a>(
    aliases: &[Alias],
    file: &'a ParsedFile,
    bindings: &mut Vec<AstNode<'a>>,
) {
    for alias in aliases {
        if alias.name.as_str() == "*" {
            continue;
        }
        if let Some(asname) = &alias.asname {
            if asname.id.as_str() != "_" {
                bindings.push(AstNode::from_span(file, span_from_ruff_range(asname.range)));
            }
        } else {
            let first_segment = alias.name.as_str().split('.').next().unwrap_or("");
            if !first_segment.is_empty() && first_segment != "_" {
                let start = usize::from(alias.name.range.start());
                bindings.push(AstNode::from_span(
                    file,
                    SourceSpan {
                        start,
                        end: start + first_segment.len(),
                    },
                ));
            }
        }
    }
}

/// Extracts the attribute names that `__init__` declares on its receiver: the first
/// `self.name = …` per name, in source order. Names the class body already declares
/// (`name: int`) are skipped, so a dataclass-style `self.name = name` is not a second
/// declaration; later reassignments, other methods, and `self.name[key] = …` declare nothing.
fn extract_instance_attribute_declarations<'a>(
    class_def: &StmtClassDef,
    file: &'a ParsedFile,
    bindings: &mut Vec<AstNode<'a>>,
) {
    let Some(init) = direct_function_definitions(&class_def.body)
        .into_iter()
        .find(|func_def| func_def.name.id == "__init__")
    else {
        return;
    };
    let Some(receiver) = method_receiver_name_ast(init, false, file) else {
        return;
    };

    let mut declared = HashSet::new();
    for statement in &class_def.body {
        match statement {
            Stmt::Assign(assign) => {
                for target in &assign.targets {
                    extend_expr_target_names(target, file, &mut declared);
                }
            }
            Stmt::AnnAssign(ann) => {
                extend_expr_target_names(&ann.target, file, &mut declared);
            }
            _ => {}
        }
    }
    collect_receiver_attribute_targets(&init.body, &receiver, file, &mut declared, bindings);
}

/// Walks `body` (without entering nested functions, classes, or lambdas) and appends the
/// attribute identifier of every `receiver.name` assignment target whose name is not yet in
/// `declared`.
fn collect_receiver_attribute_targets<'a>(
    body: &[Stmt],
    receiver: &str,
    file: &'a ParsedFile,
    declared: &mut HashSet<String>,
    bindings: &mut Vec<AstNode<'a>>,
) {
    struct ReceiverAttrVisitor<'a, 'b> {
        receiver: &'b str,
        file: &'a ParsedFile,
        declared: &'b mut HashSet<String>,
        bindings: &'b mut Vec<AstNode<'a>>,
    }

    impl SourceOrderVisitor<'_> for ReceiverAttrVisitor<'_, '_> {
        fn visit_stmt(&mut self, statement: &Stmt) {
            match statement {
                Stmt::FunctionDef(_) | Stmt::ClassDef(_) => return,
                Stmt::Assign(assign) => {
                    for target in &assign.targets {
                        push_receiver_attributes(
                            target,
                            self.receiver,
                            self.file,
                            self.declared,
                            self.bindings,
                        );
                    }
                }
                Stmt::AnnAssign(ann) => {
                    push_receiver_attributes(
                        &ann.target,
                        self.receiver,
                        self.file,
                        self.declared,
                        self.bindings,
                    );
                }
                _ => {}
            }
            walk_stmt(self, statement);
        }

        fn visit_expr(&mut self, expr: &Expr) {
            if matches!(expr, Expr::Lambda(_)) {
                return;
            }
            walk_expr(self, expr);
        }
    }

    let mut visitor = ReceiverAttrVisitor {
        receiver,
        file,
        declared,
        bindings,
    };
    visitor.visit_body(body);
}

/// Appends the `receiver.name` attribute identifiers bound by the assignment target `target`
/// (recursing through tuple, list, and starred targets) whose name is not yet in `declared`.
fn push_receiver_attributes<'a>(
    target: &Expr,
    receiver: &str,
    file: &'a ParsedFile,
    declared: &mut HashSet<String>,
    bindings: &mut Vec<AstNode<'a>>,
) {
    match target {
        Expr::Attribute(attr) => {
            if let Expr::Name(object) = attr.value.as_ref()
                && object.id.as_str() == receiver
                && declared.insert(attr.attr.to_string())
            {
                bindings.push(AstNode::from_span(
                    file,
                    span_from_ruff_range(attr.attr.range),
                ));
            }
        }
        Expr::Tuple(tuple) => {
            for elt in &tuple.elts {
                push_receiver_attributes(elt, receiver, file, declared, bindings);
            }
        }
        Expr::List(list) => {
            for elt in &list.elts {
                push_receiver_attributes(elt, receiver, file, declared, bindings);
            }
        }
        Expr::Starred(starred) => {
            push_receiver_attributes(&starred.value, receiver, file, declared, bindings);
        }
        _ => {}
    }
}

struct BindingVisitor<'a> {
    file: &'a ParsedFile,
    bindings: Vec<AstNode<'a>>,
}

impl<'a> SourceOrderVisitor<'a> for BindingVisitor<'a> {
    fn visit_stmt(&mut self, statement: &'a Stmt) {
        match statement {
            Stmt::Assign(assign) => {
                for target in &assign.targets {
                    extract_from_expr_target(target, self.file, &mut self.bindings);
                }
                self.visit_expr(&assign.value);
                return;
            }
            Stmt::AnnAssign(ann) => {
                extract_from_expr_target(&ann.target, self.file, &mut self.bindings);
                self.visit_annotation(&ann.annotation);
                if let Some(value) = &ann.value {
                    self.visit_expr(value);
                }
                return;
            }
            Stmt::For(for_statement) => {
                extract_from_expr_target(&for_statement.target, self.file, &mut self.bindings);
                self.visit_expr(&for_statement.iter);
                self.visit_body(&for_statement.body);
                self.visit_body(&for_statement.orelse);
                return;
            }
            Stmt::FunctionDef(func_def) => {
                self.bindings.push(AstNode::from_span(
                    self.file,
                    span_from_ruff_range(func_def.name.range),
                ));
            }
            Stmt::ClassDef(class_def) => {
                self.bindings.push(AstNode::from_span(
                    self.file,
                    span_from_ruff_range(class_def.name.range),
                ));
                extract_instance_attribute_declarations(class_def, self.file, &mut self.bindings);
            }
            Stmt::Import(import_statement) => {
                extract_from_import_aliases(&import_statement.names, self.file, &mut self.bindings);
                return;
            }
            Stmt::ImportFrom(import_from) => {
                extract_from_import_aliases(&import_from.names, self.file, &mut self.bindings);
                return;
            }
            _ => {}
        }
        walk_stmt(self, statement);
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Named(named) = expr {
            extract_from_expr_target(&named.target, self.file, &mut self.bindings);
            self.visit_expr(&named.value);
            return;
        }
        walk_expr(self, expr);
    }

    fn visit_parameters(&mut self, parameters: &'a Parameters) {
        extract_from_parameters(parameters, self.file, &mut self.bindings);
        walk_parameters(self, parameters);
    }

    fn visit_comprehension(&mut self, comprehension: &'a Comprehension) {
        extract_from_expr_target(&comprehension.target, self.file, &mut self.bindings);
        walk_comprehension(self, comprehension);
    }

    fn visit_with_item(&mut self, with_item: &'a WithItem) {
        self.visit_expr(&with_item.context_expr);
        if let Some(optional_vars) = &with_item.optional_vars {
            extract_from_expr_target(optional_vars, self.file, &mut self.bindings);
        }
    }

    fn visit_except_handler(&mut self, except_handler: &'a ExceptHandler) {
        let ExceptHandler::ExceptHandler(handler) = except_handler;
        if let Some(type_) = &handler.type_ {
            self.visit_expr(type_);
        }
        if let Some(name) = &handler.name
            && name.id.as_str() != "_"
        {
            self.bindings.push(AstNode::from_span(
                self.file,
                span_from_ruff_range(name.range),
            ));
        }
        self.visit_body(&handler.body);
    }

    fn visit_match_case(&mut self, match_case: &'a MatchCase) {
        extract_from_match_pattern(&match_case.pattern, self.file, &mut self.bindings);
        if let Some(guard) = &match_case.guard {
            self.visit_expr(guard);
        }
        self.visit_body(&match_case.body);
    }
}

/// Collects all binding definitions (variables, functions, classes, instance attributes declared
/// in `__init__`, etc.) within a Python file.
#[must_use]
pub(in crate::code_lint::ast) fn collect_bindings(file: &ParsedFile) -> Vec<AstNode<'_>> {
    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut visitor = BindingVisitor {
        file,
        bindings: Vec::new(),
    };
    visitor.visit_body(&parsed.syntax().body);
    let _ = (
        walk_with_item::<BindingVisitor<'_>>,
        walk_except_handler::<BindingVisitor<'_>>,
        walk_match_case::<BindingVisitor<'_>>,
    );
    visitor.bindings
}

/// Returns true if `parameters` declares a parameter named `parameter_name`.
pub(super) fn parameters_shadow_name(parameters: &Parameters, parameter_name: &str) -> bool {
    parameters
        .posonlyargs
        .iter()
        .chain(parameters.args.iter())
        .chain(parameters.kwonlyargs.iter())
        .any(|pwd| pwd.parameter.name.id.as_str() == parameter_name)
        || parameters
            .vararg
            .as_ref()
            .is_some_and(|vararg| vararg.name.id.as_str() == parameter_name)
        || parameters
            .kwarg
            .as_ref()
            .is_some_and(|kwarg| kwarg.name.id.as_str() == parameter_name)
}

/// A call from a function or method to a sibling in the same module or class scope: `callee(...)`
/// in a module, `self.callee(...)` (through the method's receiver) in a class.
#[derive(Clone)]
pub struct PythonSiblingCall<'a> {
    /// The `call` expression AST node.
    pub node: AstNode<'a>,
    /// Name of the called sibling.
    pub callee_name: String,
}

/// A function or method of a module or class scope. `@overload` stubs are grouped with their
/// implementation, and `@property` getters with their `@<name>.setter` / `@<name>.deleter`.
pub struct PythonScopeFunction<'a> {
    /// Function or method name.
    pub name: String,
    /// Position of the group's first definition among the scope's direct function definitions.
    pub definition_order: usize,
    /// Calls to siblings in the bodies of the group's definitions, in walk order. Names shadowed
    /// by a local binding are not sibling calls.
    pub sibling_calls: Vec<PythonSiblingCall<'a>>,
}

/// The functions of one module or class scope, in definition order.
pub struct PythonFunctionScope<'a> {
    /// True for a class body, false for the module.
    pub is_class: bool,
    /// The scope's functions or methods.
    pub functions: Vec<PythonScopeFunction<'a>>,
}

/// A logical function or method in a module or class scope (grouping `@overload` stubs with
/// their implementation and `@property` getters with `@<name>.setter` / `@<name>.deleter` at
/// their first declaration position).
struct LogicalFunction<'a> {
    name: String,
    definition_order: usize,
    has_overload: bool,
    has_non_overload_definition: bool,
    parts: Vec<&'a StmtFunctionDef>,
}

/// Collects `Expr::Named` (`:=`) targets inside a comprehension or generator expression,
/// which PEP 572 binds in the enclosing function scope rather than the comprehension scope.
fn collect_walrus_bindings_in_comprehension(
    expr: &Expr,
    file: &ParsedFile,
    bindings: &mut HashSet<String>,
) {
    struct WalrusVisitor<'a, 'b> {
        file: &'a ParsedFile,
        bindings: &'b mut HashSet<String>,
    }

    impl<'a> SourceOrderVisitor<'a> for WalrusVisitor<'a, '_> {
        fn visit_stmt(&mut self, _statement: &'a Stmt) {}

        fn visit_expr(&mut self, expr: &'a Expr) {
            if matches!(expr, Expr::Lambda(_)) {
                return;
            }
            if let Expr::Named(named) = expr {
                extend_expr_target_names(&named.target, self.file, self.bindings);
            }
            walk_expr(self, expr);
        }
    }

    let mut visitor = WalrusVisitor { file, bindings };
    walk_expr(&mut visitor, expr);
}

/// Collects local variable, parameter, nested definition, and import bindings directly owned by
/// `parameters` and `body`, without descending into nested `function_definition`, `lambda`, or
/// `class_definition` bodies.
fn collect_local_scope_bindings(
    parameters: Option<&Parameters>,
    body: &[Stmt],
    excluded_parameter: Option<&str>,
    file: &ParsedFile,
) -> HashSet<String> {
    let mut bindings = HashSet::new();
    let mut globals = HashSet::new();

    if let Some(params) = parameters {
        let mut param_nodes = Vec::new();
        extract_from_parameters(params, file, &mut param_nodes);
        for node in param_nodes {
            let text = node.text();
            if excluded_parameter != Some(text.as_ref()) {
                bindings.insert(text.into_owned());
            }
        }
    }

    collect_bindings_in_stmts(body, file, &mut bindings, &mut globals);

    for global_name in globals {
        bindings.remove(&global_name);
    }
    bindings
}

struct LocalBindingVisitor<'a, 'b> {
    file: &'a ParsedFile,
    bindings: &'b mut HashSet<String>,
    globals: &'b mut HashSet<String>,
}

impl<'a> SourceOrderVisitor<'a> for LocalBindingVisitor<'a, '_> {
    fn visit_stmt(&mut self, statement: &'a Stmt) {
        match statement {
            Stmt::Assign(assign) => {
                for target in &assign.targets {
                    extend_expr_target_names(target, self.file, self.bindings);
                }
                self.visit_expr(&assign.value);
                return;
            }
            Stmt::AnnAssign(ann) => {
                extend_expr_target_names(&ann.target, self.file, self.bindings);
                if let Some(value) = &ann.value {
                    self.visit_expr(value);
                }
                return;
            }
            Stmt::AugAssign(aug) => {
                extend_expr_target_names(&aug.target, self.file, self.bindings);
                self.visit_expr(&aug.value);
                return;
            }
            Stmt::For(for_statement) => {
                extend_expr_target_names(&for_statement.target, self.file, self.bindings);
            }
            Stmt::FunctionDef(func_def) => {
                self.bindings.insert(func_def.name.id.to_string());
                return;
            }
            Stmt::ClassDef(class_def) => {
                self.bindings.insert(class_def.name.id.to_string());
                return;
            }
            Stmt::Import(import_statement) => {
                let mut imported = Vec::new();
                extract_from_import_aliases(&import_statement.names, self.file, &mut imported);
                for item in imported {
                    self.bindings.insert(item.text().into_owned());
                }
                return;
            }
            Stmt::ImportFrom(import_from) => {
                let mut imported = Vec::new();
                extract_from_import_aliases(&import_from.names, self.file, &mut imported);
                for item in imported {
                    self.bindings.insert(item.text().into_owned());
                }
                return;
            }
            Stmt::Global(global_statement) => {
                for name in &global_statement.names {
                    self.globals.insert(name.id.to_string());
                }
                return;
            }
            _ => {}
        }
        walk_stmt(self, statement);
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Lambda(_) => return,
            Expr::ListComp(_) | Expr::SetComp(_) | Expr::DictComp(_) | Expr::Generator(_) => {
                collect_walrus_bindings_in_comprehension(expr, self.file, self.bindings);
                return;
            }
            Expr::Named(named) => {
                extend_expr_target_names(&named.target, self.file, self.bindings);
            }
            _ => {}
        }
        walk_expr(self, expr);
    }

    fn visit_with_item(&mut self, with_item: &'a WithItem) {
        self.visit_expr(&with_item.context_expr);
        if let Some(optional_vars) = &with_item.optional_vars {
            extend_expr_target_names(optional_vars, self.file, self.bindings);
        }
    }

    fn visit_except_handler(&mut self, except_handler: &'a ExceptHandler) {
        let ExceptHandler::ExceptHandler(handler) = except_handler;
        if let Some(type_) = &handler.type_ {
            self.visit_expr(type_);
        }
        if let Some(name) = &handler.name
            && name.id.as_str() != "_"
        {
            self.bindings.insert(name.id.to_string());
        }
        self.visit_body(&handler.body);
    }

    fn visit_match_case(&mut self, match_case: &'a MatchCase) {
        extend_match_pattern_names(&match_case.pattern, self.file, self.bindings);
        if let Some(guard) = &match_case.guard {
            self.visit_expr(guard);
        }
        self.visit_body(&match_case.body);
    }
}

/// Walks `stmts` to collect local bindings and `global` declarations within a single function
/// scope, stopping at nested scope boundaries (while still capturing PEP 572 `:=` targets inside
/// comprehensions).
fn collect_bindings_in_stmts(
    stmts: &[Stmt],
    file: &ParsedFile,
    bindings: &mut HashSet<String>,
    globals: &mut HashSet<String>,
) {
    let mut visitor = LocalBindingVisitor {
        file,
        bindings,
        globals,
    };
    visitor.visit_body(stmts);
}

/// Extracts comprehension loop variable bindings (`Comprehension.target`) owned directly by
/// `generators`.
fn collect_comprehension_bindings(
    generators: &[Comprehension],
    file: &ParsedFile,
) -> HashSet<String> {
    let mut bindings = HashSet::new();
    for generator in generators {
        extend_expr_target_names(&generator.target, file, &mut bindings);
    }
    bindings
}

/// Collects direct function definitions in `scope_body` (`module` or class `body`), grouping
/// `@overload` stubs with their implementation and `@property` getters with `@<name>.setter` /
/// `@<name>.deleter` at their first declaration position.
fn collect_scope_functions<'a>(
    scope_body: &'a [Stmt],
    file: &ParsedFile,
) -> Vec<LogicalFunction<'a>> {
    let mut functions: Vec<LogicalFunction<'a>> = Vec::new();
    let mut index_by_name: HashMap<String, usize> = HashMap::new();

    for (order, function_def) in direct_function_definitions(scope_body)
        .into_iter()
        .enumerate()
    {
        let name = function_def.name.id.to_string();
        let decorators = extract_decorators_from_slice(&function_def.decorator_list, file);
        let is_overload = decorators
            .iter()
            .any(|decorator| decorator.terminal_name == "overload");
        let is_property_accessor = decorators
            .iter()
            .any(|decorator| matches!(decorator.terminal_name.as_str(), "setter" | "deleter"));

        if let Some(&existing_index) = index_by_name.get(&name) {
            let existing = &mut functions[existing_index];
            let continues_overload =
                existing.has_overload && (!existing.has_non_overload_definition || is_overload);
            if is_property_accessor || continues_overload {
                existing.has_non_overload_definition |= !is_overload;
                existing.parts.push(function_def);
                continue;
            }
        }

        let index = functions.len();
        index_by_name.insert(name.clone(), index);
        functions.push(LogicalFunction {
            name,
            definition_order: order,
            has_overload: is_overload,
            has_non_overload_definition: !is_overload,
            parts: vec![function_def],
        });
    }

    functions
}

/// Context for walking a function or method body to collect sibling calls.
struct CallWalkContext<'a, 'b> {
    file: &'a ParsedFile,
    sibling_names: &'b HashSet<&'b str>,
    receiver_name: Option<&'b str>,
    is_class: bool,
}

struct BodyCallVisitor<'a, 'b, 'c> {
    context: &'b CallWalkContext<'a, 'c>,
    active_bindings: &'b HashSet<String>,
    out: &'b mut Vec<(String, AstNode<'a>)>,
}

impl BodyCallVisitor<'_, '_, '_> {
    fn visit_comprehension_expr(&mut self, expr: &Expr, generators: &[Comprehension]) {
        let mut comp_bindings = self.active_bindings.clone();
        comp_bindings.extend(collect_comprehension_bindings(
            generators,
            self.context.file,
        ));
        let mut sub = BodyCallVisitor {
            context: self.context,
            active_bindings: &comp_bindings,
            out: self.out,
        };
        walk_expr(&mut sub, expr);
    }
}

impl SourceOrderVisitor<'_> for BodyCallVisitor<'_, '_, '_> {
    fn visit_stmt(&mut self, statement: &Stmt) {
        match statement {
            Stmt::ClassDef(_) => return,
            Stmt::FunctionDef(func_def) => {
                for decorator in &func_def.decorator_list {
                    self.visit_decorator(decorator);
                }
                if let Some(type_params) = &func_def.type_params {
                    self.visit_type_params(type_params);
                }
                self.visit_parameters(&func_def.parameters);
                if let Some(returns) = &func_def.returns {
                    self.visit_annotation(returns);
                }
                let mut inner_bindings = self.active_bindings.clone();
                inner_bindings.extend(collect_local_scope_bindings(
                    Some(&func_def.parameters),
                    &func_def.body,
                    None,
                    self.context.file,
                ));
                collect_calls_in_stmts(&func_def.body, self.context, &inner_bindings, self.out);
                return;
            }
            _ => {}
        }
        walk_stmt(self, statement);
    }

    fn visit_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Lambda(lambda) => {
                if let Some(parameters) = &lambda.parameters {
                    self.visit_parameters(parameters);
                }
                let mut inner_bindings = self.active_bindings.clone();
                inner_bindings.extend(collect_local_scope_bindings(
                    lambda.parameters.as_deref(),
                    &[],
                    None,
                    self.context.file,
                ));
                let mut sub = BodyCallVisitor {
                    context: self.context,
                    active_bindings: &inner_bindings,
                    out: self.out,
                };
                sub.visit_expr(&lambda.body);
                return;
            }
            Expr::ListComp(comp) => {
                self.visit_comprehension_expr(expr, &comp.generators);
                return;
            }
            Expr::SetComp(comp) => {
                self.visit_comprehension_expr(expr, &comp.generators);
                return;
            }
            Expr::DictComp(comp) => {
                self.visit_comprehension_expr(expr, &comp.generators);
                return;
            }
            Expr::Generator(generator) => {
                self.visit_comprehension_expr(expr, &generator.generators);
                return;
            }
            Expr::Call(call) => {
                if let Some(callee_name) =
                    match_sibling_call(call, self.context, self.active_bindings)
                {
                    self.out.push((
                        callee_name,
                        AstNode::from_span(self.context.file, span_from_ruff_range(call.range())),
                    ));
                }
            }
            _ => {}
        }
        walk_expr(self, expr);
    }
}

/// Recursively walks `stmts` inside a function/method body, tracking scoped local bindings and
/// recording calls to sibling functions/methods in `out`.
fn collect_calls_in_stmts<'a>(
    stmts: &[Stmt],
    context: &CallWalkContext<'a, '_>,
    active_bindings: &HashSet<String>,
    out: &mut Vec<(String, AstNode<'a>)>,
) {
    let mut visitor = BodyCallVisitor {
        context,
        active_bindings,
        out,
    };
    visitor.visit_body(stmts);
}

/// Checks whether `call` invokes a sibling function or method in `context.sibling_names`.
fn match_sibling_call(
    call: &ruff_python_ast::ExprCall,
    context: &CallWalkContext<'_, '_>,
    active_bindings: &HashSet<String>,
) -> Option<String> {
    if context.is_class {
        let receiver = context.receiver_name?;
        if active_bindings.contains(receiver) {
            return None;
        }
        let Expr::Attribute(function) = call.func.as_ref() else {
            return None;
        };
        let Expr::Name(object) = function.value.as_ref() else {
            return None;
        };
        let attribute_name = function.attr.as_str();
        if object.id.as_str() == receiver && context.sibling_names.contains(attribute_name) {
            return Some(attribute_name.to_owned());
        }
        None
    } else {
        let Expr::Name(function) = call.func.as_ref() else {
            return None;
        };
        let name = function.id.as_str();
        if context.sibling_names.contains(name) && !active_bindings.contains(name) {
            Some(name.to_owned())
        } else {
            None
        }
    }
}

/// Collects the sibling calls of each function in a single scope.
fn function_scope<'a>(
    functions: Vec<LogicalFunction<'a>>,
    is_class: bool,
    file: &'a ParsedFile,
) -> PythonFunctionScope<'a> {
    let sibling_names: HashSet<&str> = functions
        .iter()
        .map(|function| function.name.as_str())
        .collect();

    let mut sibling_calls_by_function = Vec::with_capacity(functions.len());
    for function in &functions {
        let mut sibling_calls = Vec::new();
        for function_def in &function.parts {
            let receiver = if is_class {
                method_receiver_name_ast(function_def, true, file)
            } else {
                None
            };
            let initial_bindings = collect_local_scope_bindings(
                Some(&function_def.parameters),
                &function_def.body,
                receiver.as_deref(),
                file,
            );
            let context = CallWalkContext {
                file,
                sibling_names: &sibling_names,
                receiver_name: receiver.as_deref(),
                is_class,
            };
            collect_calls_in_stmts(
                &function_def.body,
                &context,
                &initial_bindings,
                &mut sibling_calls,
            );
        }
        sibling_calls_by_function.push(sibling_calls);
    }

    let functions = functions
        .into_iter()
        .zip(sibling_calls_by_function)
        .map(|(function, sibling_calls)| PythonScopeFunction {
            name: function.name,
            definition_order: function.definition_order,
            sibling_calls: sibling_calls
                .into_iter()
                .map(|(callee_name, node)| PythonSiblingCall { node, callee_name })
                .collect(),
        })
        .collect();
    PythonFunctionScope {
        is_class,
        functions,
    }
}

/// Collects the module scope, then every class scope in source order, with each function's
/// calls to its siblings.
#[must_use]
pub fn collect_function_scopes(file: &ParsedFile) -> Vec<PythonFunctionScope<'_>> {
    struct ScopeVisitor<'a> {
        file: &'a ParsedFile,
        scopes: Vec<PythonFunctionScope<'a>>,
    }

    impl<'a> SourceOrderVisitor<'a> for ScopeVisitor<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if let Stmt::ClassDef(class_def) = statement {
                self.scopes.push(function_scope(
                    collect_scope_functions(&class_def.body, self.file),
                    true,
                    self.file,
                ));
            }
            walk_stmt(self, statement);
        }
    }

    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let module_body = &parsed.syntax().body;
    let mut visitor = ScopeVisitor {
        file,
        scopes: vec![function_scope(
            collect_scope_functions(module_body, file),
            false,
            file,
        )],
    };
    visitor.visit_body(module_body);
    visitor.scopes
}
