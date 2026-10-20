//! Python binding extraction and lexical scope boundaries.

use super::annotations::has_final_annotation_expr;
use super::functions::{has_override_decorator, method_receiver_name_ast};
use super::tree::direct_function_definitions;
use crate::code_lint::ast::{
    AstNode, Binding, BindingKind, ImportMap, ParsedFile, push_bindings, span_from_ruff_range,
};
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
use std::collections::HashSet;

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

/// Collects all binding definitions (variables, functions, classes, instance attributes declared
/// in `__init__`, etc.) within a Python file, with what introduced each.
#[must_use]
pub(in crate::code_lint::ast) fn collect_bindings(file: &ParsedFile) -> Vec<Binding<'_>> {
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

/// Collects the file-level imports and `def` / `class` definitions of `file` (see
/// [`crate::code_lint::ast::resolve_name`]).
#[must_use]
pub(in crate::code_lint::ast) fn collect_import_map(file: &ParsedFile) -> ImportMap {
    let mut map = ImportMap::default();
    if let Some(parsed) = file.py_module() {
        collect_python_import_bindings(&parsed.syntax().body, &mut map);
    }
    map
}

/// Returns the RHS expression if `statement` assigns a value to a constant binding (a single
/// `UPPER_SNAKE_CASE` target name or a `Final`-annotated target).
pub(super) fn constant_assignment_value<'a>(
    statement: &'a Stmt,
    file: &ParsedFile,
) -> Option<&'a Expr> {
    match statement {
        Stmt::Assign(assign)
            if matches!(
                assign.targets.as_slice(),
                [Expr::Name(name)] if is_constant_name(name.id.as_str())
            ) =>
        {
            Some(&assign.value)
        }
        Stmt::AnnAssign(ann)
            if matches!(
                ann.target.as_ref(),
                Expr::Name(name) if is_constant_name(name.id.as_str())
            ) || has_final_annotation_expr(&ann.annotation, file) =>
        {
            ann.value.as_deref()
        }
        _ => None,
    }
}

/// Collects all local variable, parameter, import, and nested-definition names bound directly
/// inside `function_def` (without descending into nested `def` or `class` bodies).
pub(super) fn collect_local_bound_names(
    function_def: &StmtFunctionDef,
    file: &ParsedFile,
) -> HashSet<String> {
    struct LocalNameVisitor<'a, 'b> {
        file: &'a ParsedFile,
        names: &'b mut HashSet<String>,
    }

    impl SourceOrderVisitor<'_> for LocalNameVisitor<'_, '_> {
        fn visit_stmt(&mut self, statement: &Stmt) {
            match statement {
                Stmt::FunctionDef(inner) => {
                    self.names.insert(inner.name.id.to_string());
                    return;
                }
                Stmt::ClassDef(inner) => {
                    self.names.insert(inner.name.id.to_string());
                    return;
                }
                Stmt::Assign(assign) => {
                    for target in &assign.targets {
                        extend_expr_target_names(target, self.file, self.names);
                    }
                }
                Stmt::AnnAssign(ann) => {
                    extend_expr_target_names(&ann.target, self.file, self.names);
                }
                Stmt::For(for_statement) => {
                    extend_expr_target_names(&for_statement.target, self.file, self.names);
                }
                Stmt::Import(import_statement) => {
                    let mut nodes = Vec::new();
                    extract_from_import_aliases(&import_statement.names, self.file, &mut nodes);
                    for node in nodes {
                        self.names.insert(node.text().into_owned());
                    }
                    return;
                }
                Stmt::ImportFrom(import_from) => {
                    let mut nodes = Vec::new();
                    extract_from_import_aliases(&import_from.names, self.file, &mut nodes);
                    for node in nodes {
                        self.names.insert(node.text().into_owned());
                    }
                    return;
                }
                _ => {}
            }
            walk_stmt(self, statement);
        }

        fn visit_expr(&mut self, expr: &Expr) {
            if let Expr::Named(named) = expr {
                extend_expr_target_names(&named.target, self.file, self.names);
            }
            walk_expr(self, expr);
        }

        fn visit_comprehension(&mut self, comprehension: &Comprehension) {
            extend_expr_target_names(&comprehension.target, self.file, self.names);
            walk_comprehension(self, comprehension);
        }

        fn visit_with_item(&mut self, with_item: &WithItem) {
            if let Some(optional_vars) = &with_item.optional_vars {
                extend_expr_target_names(optional_vars, self.file, self.names);
            }
            walk_with_item(self, with_item);
        }

        fn visit_except_handler(&mut self, except_handler: &ExceptHandler) {
            let ExceptHandler::ExceptHandler(handler) = except_handler;
            if let Some(name) = &handler.name
                && name.id.as_str() != "_"
            {
                self.names.insert(name.id.to_string());
            }
            walk_except_handler(self, except_handler);
        }

        fn visit_match_case(&mut self, match_case: &MatchCase) {
            let mut nodes = Vec::new();
            extract_from_match_pattern(&match_case.pattern, self.file, &mut nodes);
            for node in nodes {
                self.names.insert(node.text().into_owned());
            }
            walk_match_case(self, match_case);
        }
    }

    let mut names = HashSet::new();
    let mut param_nodes = Vec::new();
    extract_from_parameters(&function_def.parameters, file, &mut param_nodes);
    for node in param_nodes {
        names.insert(node.text().into_owned());
    }
    let mut visitor = LocalNameVisitor {
        file,
        names: &mut names,
    };
    visitor.visit_body(&function_def.body);
    names
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

struct BindingVisitor<'a> {
    file: &'a ParsedFile,
    bindings: Vec<Binding<'a>>,
}

impl<'a> SourceOrderVisitor<'a> for BindingVisitor<'a> {
    fn visit_stmt(&mut self, statement: &'a Stmt) {
        let file = self.file;
        match statement {
            Stmt::Assign(assign) => {
                for target in &assign.targets {
                    push_bindings(&mut self.bindings, BindingKind::Value, |out| {
                        extract_from_expr_target(target, file, out);
                    });
                }
                self.visit_expr(&assign.value);
                return;
            }
            Stmt::AnnAssign(ann) => {
                push_bindings(&mut self.bindings, BindingKind::Value, |out| {
                    extract_from_expr_target(&ann.target, file, out);
                });
                self.visit_annotation(&ann.annotation);
                if let Some(value) = &ann.value {
                    self.visit_expr(value);
                }
                return;
            }
            Stmt::For(for_statement) => {
                push_bindings(&mut self.bindings, BindingKind::Value, |out| {
                    extract_from_expr_target(&for_statement.target, file, out);
                });
                self.visit_expr(&for_statement.iter);
                self.visit_body(&for_statement.body);
                self.visit_body(&for_statement.orelse);
                return;
            }
            Stmt::FunctionDef(func_def) => {
                let kind = if has_override_decorator(&func_def.decorator_list, file) {
                    BindingKind::ContractMember
                } else {
                    BindingKind::StructuralDefinition
                };
                self.bindings.push(Binding {
                    node: AstNode::from_span(file, span_from_ruff_range(func_def.name.range)),
                    kind,
                });
            }
            Stmt::ClassDef(class_def) => {
                self.bindings.push(Binding {
                    node: AstNode::from_span(file, span_from_ruff_range(class_def.name.range)),
                    kind: BindingKind::StructuralDefinition,
                });
                push_bindings(&mut self.bindings, BindingKind::Value, |out| {
                    extract_instance_attribute_declarations(class_def, file, out);
                });
            }
            Stmt::Import(import_statement) => {
                push_bindings(&mut self.bindings, BindingKind::Import, |out| {
                    extract_from_import_aliases(&import_statement.names, file, out);
                });
                return;
            }
            Stmt::ImportFrom(import_from) => {
                push_bindings(&mut self.bindings, BindingKind::Import, |out| {
                    extract_from_import_aliases(&import_from.names, file, out);
                });
                return;
            }
            _ => {}
        }
        walk_stmt(self, statement);
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Named(named) = expr {
            push_bindings(&mut self.bindings, BindingKind::Value, |out| {
                extract_from_expr_target(&named.target, self.file, out);
            });
            self.visit_expr(&named.value);
            return;
        }
        walk_expr(self, expr);
    }

    fn visit_parameters(&mut self, parameters: &'a Parameters) {
        push_bindings(&mut self.bindings, BindingKind::Value, |out| {
            extract_from_parameters(parameters, self.file, out);
        });
        walk_parameters(self, parameters);
    }

    fn visit_comprehension(&mut self, comprehension: &'a Comprehension) {
        push_bindings(&mut self.bindings, BindingKind::Value, |out| {
            extract_from_expr_target(&comprehension.target, self.file, out);
        });
        walk_comprehension(self, comprehension);
    }

    fn visit_with_item(&mut self, with_item: &'a WithItem) {
        self.visit_expr(&with_item.context_expr);
        if let Some(optional_vars) = &with_item.optional_vars {
            push_bindings(&mut self.bindings, BindingKind::Value, |out| {
                extract_from_expr_target(optional_vars, self.file, out);
            });
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
            self.bindings.push(Binding {
                node: AstNode::from_span(self.file, span_from_ruff_range(name.range)),
                kind: BindingKind::Value,
            });
        }
        self.visit_body(&handler.body);
    }

    fn visit_match_case(&mut self, match_case: &'a MatchCase) {
        push_bindings(&mut self.bindings, BindingKind::Value, |out| {
            extract_from_match_pattern(&match_case.pattern, self.file, out);
        });
        if let Some(guard) = &match_case.guard {
            self.visit_expr(guard);
        }
        self.visit_body(&match_case.body);
    }
}

/// Returns true if `name` is spelled as a constant (`MAX_RETRIES`, `_TIMEOUT_S`).
fn is_constant_name(name: &str) -> bool {
    name.chars().any(|character| character.is_ascii_uppercase())
        && name.chars().all(|character| {
            character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
        })
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

/// Records the imports and definitions of a Python module body or top-level block `body`.
fn collect_python_import_bindings(body: &[Stmt], map: &mut ImportMap) {
    for statement in body {
        match statement {
            Stmt::Import(import) => {
                for alias in &import.names {
                    let module = alias.name.as_str();
                    if let Some(asname) = &alias.asname {
                        map.imports.insert(asname.to_string(), module.to_owned());
                    } else {
                        let package = module.split('.').next().unwrap_or(module);
                        map.imports.insert(package.to_owned(), package.to_owned());
                    }
                }
            }
            Stmt::ImportFrom(import_from) => {
                let dots = ".".repeat(import_from.level as usize);
                let module = import_from
                    .module
                    .as_ref()
                    .map_or_else(|| dots.clone(), |module| format!("{dots}{module}"));
                for alias in &import_from.names {
                    let name = alias.name.as_str();
                    if name == "*" {
                        continue;
                    }
                    let path = if module.ends_with('.') {
                        format!("{module}{name}")
                    } else {
                        format!("{module}.{name}")
                    };
                    let bound = alias.asname.as_ref().unwrap_or(&alias.name);
                    map.imports.insert(bound.to_string(), path);
                }
            }
            Stmt::FunctionDef(function) => {
                map.locals.insert(function.name.to_string());
            }
            Stmt::ClassDef(class) => {
                map.locals.insert(class.name.to_string());
            }
            Stmt::If(if_statement) => {
                collect_python_import_bindings(&if_statement.body, map);
                for clause in &if_statement.elif_else_clauses {
                    collect_python_import_bindings(&clause.body, map);
                }
            }
            Stmt::Try(try_statement) => {
                collect_python_import_bindings(&try_statement.body, map);
                for ExceptHandler::ExceptHandler(handler) in &try_statement.handlers {
                    collect_python_import_bindings(&handler.body, map);
                }
                collect_python_import_bindings(&try_statement.orelse, map);
                collect_python_import_bindings(&try_statement.finalbody, map);
            }
            Stmt::With(with_statement) => {
                collect_python_import_bindings(&with_statement.body, map);
            }
            _ => {}
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

/// Inserts all identifier names bound by `target` into `names`.
fn extend_expr_target_names(target: &Expr, file: &ParsedFile, names: &mut HashSet<String>) {
    let mut nodes = Vec::new();
    extract_from_expr_target(target, file, &mut nodes);
    for node in nodes {
        names.insert(node.text().into_owned());
    }
}

/// Recursively extracts binding identifiers from an assignment/loop target expression, stopping
/// at `Expr::Attribute` and `Expr::Subscript` so `self.ctx = 1` or `items[idx] = 1` does not
/// record `ctx` or `idx` as a local variable binding.
fn extract_from_expr_target<'a>(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Language;

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
            .map(|binding| binding.node.text().to_string())
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
            .map(|binding| binding.node.text().to_string())
            .collect();
        assert_eq!(names, vec!["x", "z", "a", "b"]);
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
}
