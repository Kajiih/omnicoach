//! Python binding extraction and lexical scope boundaries.

use super::{
    AstNode, ParsedFile, direct_function_definitions, has_override_decorator,
    method_receiver_name_ast,
};
use crate::code_lint::ast::{Binding, BindingKind, push_bindings, span_from_ruff_range};
use crate::diagnostic::SourceSpan;
use ruff_python_ast::visitor::source_order::{
    SourceOrderVisitor, walk_comprehension, walk_except_handler, walk_expr, walk_match_case,
    walk_parameters, walk_stmt, walk_with_item,
};
use ruff_python_ast::{
    Alias, Comprehension, ExceptHandler, Expr, MatchCase, Parameters, Pattern, Stmt, StmtClassDef,
    WithItem,
};
use std::collections::HashSet;

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
