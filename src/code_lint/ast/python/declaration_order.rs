//! Python declaration-order scope builders and field-after-method extraction.

use super::bindings::{collect_local_bound_names, parameters_shadow_name};
use super::functions::{CLS_RECEIVER, SELF_RECEIVER, is_overload_def, is_property_accessor_def};
use crate::code_lint::ast::call_clusters::{
    DeclarationScope, DeclarationScopeKind, PlacementAnchor, RelationKind, ScopeItem,
    ScopeItemKind, Visibility, collapse_whitespace, extend_references, extend_unique, push_unique,
    split_at_redefinitions,
};
use crate::code_lint::ast::tree::{AstNode, ParsedFile, span_from_ruff_range};
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_expr, walk_stmt};
use ruff_python_ast::{Decorator, Expr, Stmt, StmtClassDef, StmtFunctionDef};
use ruff_text_size::{Ranged as _, TextRange};
use std::collections::{HashMap, HashSet};

const CONSTRUCTOR_NAMES: &[&str] = &[
    "__prepare__",
    "__init_subclass__",
    "__new__",
    "__init__",
    "__post_init__",
    "__attrs_pre_init__",
    "__attrs_post_init__",
];

/// A type-annotated class-body attribute declaration (`Stmt::AnnAssign` with an identifier target)
/// appearing after at least one direct method definition in the same Python `class`.
pub struct PythonFieldAfterMethod<'a> {
    /// Name of the enclosing class.
    pub class_name: String,
    /// Identifier of the misplaced field.
    pub name: String,
    /// First direct method in the class before which the field should move.
    pub first_method: PlacementAnchor,
    /// AST node of the misplaced attribute declaration.
    pub node: AstNode<'a>,
}

/// A top-level Python statement appearing after the `if __name__ == "__main__":` guard block.
pub struct PythonStatementAfterMainGuard<'a> {
    /// AST node of the trailing statement.
    pub node: AstNode<'a>,
    /// 1-based starting line of the `if __name__ == "__main__":` guard block.
    pub guard_line: usize,
}

/// Collects all module and `class` [`DeclarationScope`]s in a Python `file`.
#[must_use]
pub fn collect_declaration_scopes(file: &ParsedFile) -> Vec<DeclarationScope<'_>> {
    struct ScopeCollector<'a> {
        file: &'a ParsedFile,
        future_annotations: bool,
        scopes: Vec<DeclarationScope<'a>>,
    }

    impl<'a> SourceOrderVisitor<'a> for ScopeCollector<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if let Stmt::ClassDef(class_def) = statement
                && let Some(scope) =
                    build_class_declaration_scope(class_def, self.future_annotations, self.file)
            {
                self.scopes.push(scope);
            }
            walk_stmt(self, statement);
        }
    }

    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let module_body = &parsed.syntax().body;
    let future_annotations = has_future_annotations(module_body);
    let scopes = build_module_declaration_scopes(module_body, future_annotations, file);
    let mut collector = ScopeCollector {
        file,
        future_annotations,
        scopes,
    };
    collector.visit_body(module_body);
    collector.scopes
}

/// Collects type-annotated class-body attribute declarations (`Stmt::AnnAssign` with an
/// identifier target) that appear after at least one direct method definition in the same class.
#[must_use]
pub fn collect_fields_after_methods(file: &ParsedFile) -> Vec<PythonFieldAfterMethod<'_>> {
    struct FieldAfterMethodVisitor<'a> {
        file: &'a ParsedFile,
        out: Vec<PythonFieldAfterMethod<'a>>,
    }

    impl<'a> SourceOrderVisitor<'a> for FieldAfterMethodVisitor<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if let Stmt::ClassDef(class_def) = statement {
                let class_name = class_def.name.id.to_string();
                let mut first_method: Option<PlacementAnchor> = None;
                for child in &class_def.body {
                    match child {
                        Stmt::FunctionDef(func_def) => {
                            first_method.get_or_insert_with(|| {
                                PlacementAnchor::before(
                                    func_def.name.id.as_str(),
                                    AstNode::from_span(
                                        self.file,
                                        span_from_ruff_range(func_def.range),
                                    ),
                                )
                            });
                        }
                        Stmt::AnnAssign(ann) => {
                            if let Some(first_method) = &first_method
                                && let Expr::Name(target) = ann.target.as_ref()
                            {
                                self.out.push(PythonFieldAfterMethod {
                                    class_name: class_name.clone(),
                                    name: target.id.to_string(),
                                    first_method: first_method.clone(),
                                    node: AstNode::from_span(
                                        self.file,
                                        span_from_ruff_range(ann.range),
                                    ),
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
            walk_stmt(self, statement);
        }
    }

    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut visitor = FieldAfterMethodVisitor {
        file,
        out: Vec::new(),
    };
    visitor.visit_body(&parsed.syntax().body);
    visitor.out
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

/// Returns true if `module_body` contains `from __future__ import annotations`.
fn has_future_annotations(module_body: &[Stmt]) -> bool {
    module_body.iter().any(|stmt| {
        if let Stmt::ImportFrom(import_from) = stmt
            && import_from.module.as_deref() == Some("__future__")
        {
            import_from
                .names
                .iter()
                .any(|alias| alias.name.id.as_str() == "annotations")
        } else {
            false
        }
    })
}

/// Builds [`DeclarationScope`]s for top-level functions and classes in `module_body`, starting a
/// new scope segment whenever a non-overload, non-property declaration name is redefined.
fn build_module_declaration_scopes<'a>(
    module_body: &[Stmt],
    future_annotations: bool,
    file: &'a ParsedFile,
) -> Vec<DeclarationScope<'a>> {
    let grouped = group_scope_items(module_body, false, file);
    if grouped.is_empty() {
        return Vec::new();
    }

    let segments = split_at_redefinitions(grouped, |item| item.name.as_str());
    let mut scopes = Vec::new();

    for (seg_idx, segment) in segments.iter().enumerate() {
        if segment.is_empty() {
            continue;
        }
        let seg_start = if seg_idx == 0 {
            0
        } else {
            segment[0].first_stmt_idx
        };
        let seg_end = segments
            .get(seg_idx + 1)
            .map_or(module_body.len(), |next| next[0].first_stmt_idx);

        let item_by_name: HashMap<String, usize> = segment
            .iter()
            .enumerate()
            .map(|(idx, item)| (item.name.clone(), idx))
            .collect();
        let external_class_callees =
            collect_external_class_callees(&module_body[seg_start..seg_end], &item_by_name, file);

        let mut items: Vec<ScopeItem<'a>> = segment
            .iter()
            .map(|group| {
                build_module_scope_item(group, &item_by_name, &external_class_callees, file)
            })
            .collect();

        let first_stmt_indices: Vec<usize> =
            segment.iter().map(|item| item.first_stmt_idx).collect();
        let evaluated_before = direct_evaluated_before(
            module_body,
            seg_start..seg_end,
            &first_stmt_indices,
            &item_by_name,
            future_annotations,
        );
        for (item, evaluated) in items.iter_mut().zip(evaluated_before) {
            item.evaluated_before = evaluated;
        }

        scopes.push(DeclarationScope {
            kind: DeclarationScopeKind::Module,
            items,
        });
    }

    scopes
}

/// Builds a [`DeclarationScope`] for direct methods of `class_def`.
fn build_class_declaration_scope<'a>(
    class_def: &StmtClassDef,
    future_annotations: bool,
    file: &'a ParsedFile,
) -> Option<DeclarationScope<'a>> {
    let grouped = group_scope_items(&class_def.body, true, file);
    if grouped.is_empty() {
        return None;
    }
    let method_by_name: HashMap<String, usize> = grouped
        .iter()
        .enumerate()
        .map(|(idx, item)| (item.name.clone(), idx))
        .collect();
    let class_name = class_def.name.id.as_str();
    let first_stmt_indices: Vec<usize> = grouped.iter().map(|item| item.first_stmt_idx).collect();

    let mut items: Vec<ScopeItem<'a>> = grouped
        .into_iter()
        .map(|group| {
            let mut body_callees = Vec::new();
            if let ScopeItemAst::Callable(defs) = &group.item {
                for def in defs {
                    let local_names = collect_local_bound_names(def, file);
                    ClassMethodCallVisitor {
                        class_name,
                        method_by_name: &method_by_name,
                        local_names: &local_names,
                        has_self_param: parameters_shadow_name(&def.parameters, SELF_RECEIVER),
                        has_cls_param: parameters_shadow_name(&def.parameters, CLS_RECEIVER),
                        callees: &mut body_callees,
                    }
                    .visit_body(&def.body);
                }
            }
            let mut references = Vec::new();
            extend_references(&mut references, body_callees, RelationKind::Body);
            ScopeItem {
                name_node: group.name_node,
                name: group.name,
                start_line: group.start_line,
                end_line: group.end_line,
                signature: Some(group.signature),
                visibility: group.visibility,
                is_constructor: group.is_constructor,
                kind: ScopeItemKind::Callable,
                references,
                evaluated_before: None,
            }
        })
        .collect();

    let evaluated_before = direct_evaluated_before(
        &class_def.body,
        0..class_def.body.len(),
        &first_stmt_indices,
        &method_by_name,
        future_annotations,
    );
    for (item, evaluated) in items.iter_mut().zip(evaluated_before) {
        item.evaluated_before = evaluated;
    }

    let kind = DeclarationScopeKind::Class {
        name: class_name.to_owned(),
    };
    Some(DeclarationScope { kind, items })
}

struct ClassMethodCallVisitor<'map, 'local> {
    class_name: &'map str,
    method_by_name: &'map HashMap<String, usize>,
    local_names: &'local HashSet<String>,
    has_self_param: bool,
    has_cls_param: bool,
    callees: &'local mut Vec<usize>,
}

impl SourceOrderVisitor<'_> for ClassMethodCallVisitor<'_, '_> {
    fn visit_stmt(&mut self, statement: &Stmt) {
        if matches!(statement, Stmt::ClassDef(_)) {
            return;
        }
        walk_stmt(self, statement);
    }

    fn visit_expr(&mut self, expr: &Expr) {
        if let Expr::Attribute(attr) = expr
            && attr.ctx.is_load()
            && let Expr::Name(recv) = attr.value.as_ref()
        {
            let recv_name = recv.id.as_str();
            let is_valid_receiver = (recv_name == SELF_RECEIVER
                && (self.has_self_param || !self.local_names.contains(SELF_RECEIVER)))
                || (recv_name == CLS_RECEIVER
                    && (self.has_cls_param || !self.local_names.contains(CLS_RECEIVER)))
                || (recv_name == self.class_name && !self.local_names.contains(recv_name));
            if is_valid_receiver && let Some(&idx) = self.method_by_name.get(attr.attr.id.as_str())
            {
                push_unique(self.callees, idx);
            }
        }
        walk_expr(self, expr);
    }
}

struct GroupedScopeItem<'a, 'ast> {
    name_node: AstNode<'a>,
    name: String,
    start_line: usize,
    end_line: usize,
    signature: String,
    visibility: Visibility,
    is_constructor: bool,
    first_stmt_idx: usize,
    item: ScopeItemAst<'ast>,
}

/// Collects sibling module item indices referenced inside each class that is not itself a scope
/// item (such as a class redefined later in the module), keyed by class name.
fn collect_external_class_callees<'body>(
    statements: &'body [Stmt],
    item_by_name: &HashMap<String, usize>,
    file: &ParsedFile,
) -> HashMap<&'body str, Vec<usize>> {
    let empty_classes = HashMap::new();
    let empty_locals = HashSet::new();
    let mut external_class_callees = HashMap::new();
    for statement in statements {
        if let Stmt::ClassDef(class_def) = statement
            && !item_by_name.contains_key(class_def.name.id.as_str())
        {
            let mut callees = Vec::new();
            collect_class_module_refs(
                class_def,
                item_by_name,
                &empty_classes,
                &empty_locals,
                file,
                &mut callees,
            );
            if !callees.is_empty() {
                external_class_callees.insert(class_def.name.id.as_str(), callees);
            }
        }
    }
    external_class_callees
}

/// Builds the [`ScopeItem`] for `group` from the sibling references in its signatures, bodies,
/// or class definition.
fn build_module_scope_item<'a>(
    group: &GroupedScopeItem<'a, '_>,
    item_by_name: &HashMap<String, usize>,
    external_class_callees: &HashMap<&str, Vec<usize>>,
    file: &ParsedFile,
) -> ScopeItem<'a> {
    let empty_locals = HashSet::new();
    let mut references = Vec::new();
    let kind = match &group.item {
        ScopeItemAst::Callable(defs) => {
            let mut param_callees = Vec::new();
            let mut return_callees = Vec::new();
            let mut body_callees = Vec::new();
            for def in defs {
                let mut param_visitor = ModuleRefVisitor {
                    fn_by_name: item_by_name,
                    class_callees: external_class_callees,
                    local_names: &empty_locals,
                    callees: &mut param_callees,
                };
                for dec in &def.decorator_list {
                    param_visitor.visit_decorator(dec);
                }
                param_visitor.visit_parameters(&def.parameters);
                if let Some(returns) = &def.returns {
                    let mut ret_visitor = ModuleRefVisitor {
                        fn_by_name: item_by_name,
                        class_callees: external_class_callees,
                        local_names: &empty_locals,
                        callees: &mut return_callees,
                    };
                    ret_visitor.visit_expr(returns);
                }
                let local_names = collect_local_bound_names(def, file);
                let mut body_visitor = ModuleRefVisitor {
                    fn_by_name: item_by_name,
                    class_callees: external_class_callees,
                    local_names: &local_names,
                    callees: &mut body_callees,
                };
                body_visitor.visit_body(&def.body);
            }
            extend_references(&mut references, param_callees, RelationKind::Parameter);
            extend_references(&mut references, return_callees, RelationKind::Return);
            extend_references(&mut references, body_callees, RelationKind::Body);
            ScopeItemKind::Callable
        }
        ScopeItemAst::Type(class_def) => {
            let mut definition_callees = Vec::new();
            let mut all_callees = Vec::new();
            collect_class_definition_refs(
                class_def,
                item_by_name,
                external_class_callees,
                &empty_locals,
                &mut definition_callees,
            );
            collect_class_module_refs(
                class_def,
                item_by_name,
                external_class_callees,
                &empty_locals,
                file,
                &mut all_callees,
            );
            extend_references(
                &mut references,
                definition_callees,
                RelationKind::Composition,
            );
            extend_references(&mut references, all_callees, RelationKind::Behavior);
            ScopeItemKind::Type
        }
    };
    ScopeItem {
        name_node: group.name_node,
        name: group.name.clone(),
        start_line: group.start_line,
        end_line: group.end_line,
        signature: Some(group.signature.clone()),
        visibility: group.visibility,
        is_constructor: group.is_constructor,
        kind,
        references,
        evaluated_before: None,
    }
}

fn collect_class_module_refs<'map>(
    class_def: &StmtClassDef,
    item_by_name: &'map HashMap<String, usize>,
    external_class_callees: &'map HashMap<&'map str, Vec<usize>>,
    empty_locals: &HashSet<String>,
    file: &ParsedFile,
    callees: &mut Vec<usize>,
) {
    collect_class_definition_refs(
        class_def,
        item_by_name,
        external_class_callees,
        empty_locals,
        callees,
    );
    for class_statement in &class_def.body {
        if let Stmt::FunctionDef(method_def) = class_statement {
            let mut sig_visitor = ModuleRefVisitor {
                fn_by_name: item_by_name,
                class_callees: external_class_callees,
                local_names: empty_locals,
                callees,
            };
            for dec in &method_def.decorator_list {
                sig_visitor.visit_decorator(dec);
            }
            sig_visitor.visit_parameters(&method_def.parameters);
            if let Some(returns) = &method_def.returns {
                sig_visitor.visit_expr(returns);
            }
            let method_locals = collect_local_bound_names(method_def, file);
            let mut body_visitor = ModuleRefVisitor {
                fn_by_name: item_by_name,
                class_callees: external_class_callees,
                local_names: &method_locals,
                callees,
            };
            body_visitor.visit_body(&method_def.body);
        }
    }
}

/// Collects sibling module item references in the class header (decorators, bases) and non-method
/// class-body statements (such as field annotations).
fn collect_class_definition_refs<'map>(
    class_def: &StmtClassDef,
    item_by_name: &'map HashMap<String, usize>,
    external_class_callees: &'map HashMap<&'map str, Vec<usize>>,
    empty_locals: &HashSet<String>,
    callees: &mut Vec<usize>,
) {
    let mut header_visitor = ModuleRefVisitor {
        fn_by_name: item_by_name,
        class_callees: external_class_callees,
        local_names: empty_locals,
        callees,
    };
    for dec in &class_def.decorator_list {
        header_visitor.visit_decorator(dec);
    }
    if let Some(args) = &class_def.arguments {
        header_visitor.visit_arguments(args);
    }
    for class_statement in &class_def.body {
        if !matches!(class_statement, Stmt::FunctionDef(_)) {
            let mut stmt_visitor = ModuleRefVisitor {
                fn_by_name: item_by_name,
                class_callees: external_class_callees,
                local_names: empty_locals,
                callees,
            };
            stmt_visitor.visit_stmt(class_statement);
        }
    }
}

struct ModuleRefVisitor<'map, 'local> {
    fn_by_name: &'map HashMap<String, usize>,
    class_callees: &'map HashMap<&'map str, Vec<usize>>,
    local_names: &'local HashSet<String>,
    callees: &'local mut Vec<usize>,
}

impl SourceOrderVisitor<'_> for ModuleRefVisitor<'_, '_> {
    fn visit_expr(&mut self, expr: &Expr) {
        if let Expr::Name(name) = expr
            && name.ctx.is_load()
        {
            let ident = name.id.as_str();
            if !self.local_names.contains(ident) {
                if let Some(&idx) = self.fn_by_name.get(ident) {
                    push_unique(self.callees, idx);
                }
                if let Some(bridged) = self.class_callees.get(ident) {
                    extend_unique(self.callees, bridged.iter().copied());
                }
            }
        }
        walk_expr(self, expr);
    }
}

/// Collects the direct `evaluated_before` bound for each scope item by walking definition-time
/// evaluations in `scope_body[stmt_range]` (outside `Stmt::FunctionDef` bodies and postponed
/// type annotations).
fn direct_evaluated_before(
    scope_body: &[Stmt],
    stmt_range: std::ops::Range<usize>,
    first_stmt_indices: &[usize],
    item_by_name: &HashMap<String, usize>,
    future_annotations: bool,
) -> Vec<Option<usize>> {
    let count = first_stmt_indices.len();
    let mut bounds: Vec<Option<usize>> = vec![None; count];

    for (stmt_idx, stmt) in scope_body
        .iter()
        .enumerate()
        .take(stmt_range.end)
        .skip(stmt_range.start)
    {
        let Some(bound_idx) = first_stmt_indices
            .iter()
            .position(|&decl_stmt_idx| decl_stmt_idx >= stmt_idx)
        else {
            break;
        };
        if bound_idx == 0 {
            continue;
        }
        let mut refs = Vec::new();
        let mut visitor = DefinitionTimeRefVisitor {
            item_by_name,
            future_annotations,
            refs: &mut refs,
        };
        visitor.visit_stmt(stmt);
        for target_idx in refs {
            if target_idx < bound_idx {
                bounds[target_idx] =
                    Some(bounds[target_idx].map_or(bound_idx, |prev| prev.min(bound_idx)));
            }
        }
    }

    bounds
}

/// Collects the scope items a statement references when the scope body is evaluated at
/// definition time, skipping function bodies and postponed annotations.
struct DefinitionTimeRefVisitor<'map, 'out> {
    item_by_name: &'map HashMap<String, usize>,
    future_annotations: bool,
    refs: &'out mut Vec<usize>,
}

impl SourceOrderVisitor<'_> for DefinitionTimeRefVisitor<'_, '_> {
    fn visit_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::FunctionDef(func_def) => {
                for dec in &func_def.decorator_list {
                    self.visit_decorator(dec);
                }
                let eval_annotations = !self.future_annotations && func_def.type_params.is_none();
                for param in func_def
                    .parameters
                    .posonlyargs
                    .iter()
                    .chain(&func_def.parameters.args)
                    .chain(&func_def.parameters.kwonlyargs)
                {
                    if eval_annotations && let Some(ann) = &param.parameter.annotation {
                        self.visit_expr(ann);
                    }
                    if let Some(default) = &param.default {
                        self.visit_expr(default);
                    }
                }
                if eval_annotations {
                    if let Some(vararg) = &func_def.parameters.vararg
                        && let Some(ann) = &vararg.annotation
                    {
                        self.visit_expr(ann);
                    }
                    if let Some(kwarg) = &func_def.parameters.kwarg
                        && let Some(ann) = &kwarg.annotation
                    {
                        self.visit_expr(ann);
                    }
                    if let Some(returns) = &func_def.returns {
                        self.visit_expr(returns);
                    }
                }
            }
            Stmt::ClassDef(class_def) => {
                for dec in &class_def.decorator_list {
                    self.visit_decorator(dec);
                }
                if let Some(args) = &class_def.arguments {
                    self.visit_arguments(args);
                }
                self.visit_body(&class_def.body);
            }
            Stmt::AnnAssign(ann_assign) => {
                self.visit_expr(&ann_assign.target);
                if !self.future_annotations {
                    self.visit_expr(&ann_assign.annotation);
                }
                if let Some(value) = &ann_assign.value {
                    self.visit_expr(value);
                }
            }
            Stmt::TypeAlias(_) => {}
            _ => walk_stmt(self, stmt),
        }
    }

    fn visit_expr(&mut self, expr: &Expr) {
        if let Expr::Name(name) = expr
            && name.ctx.is_load()
            && let Some(&idx) = self.item_by_name.get(name.id.as_str())
        {
            push_unique(self.refs, idx);
        }
        walk_expr(self, expr);
    }
}

/// Collects direct functions and classes in `scope_body` in source order, grouping `@overload`
/// signatures and `@<prop>.setter` / `@<prop>.deleter` accessors with their first definition.
fn group_scope_items<'a, 'ast>(
    scope_body: &'ast [Stmt],
    is_class_scope: bool,
    file: &'a ParsedFile,
) -> Vec<GroupedScopeItem<'a, 'ast>> {
    struct SeenCallable {
        index: usize,
        last_stmt_idx: usize,
        has_overload: bool,
        has_non_overload: bool,
    }

    let mut items: Vec<GroupedScopeItem<'a, 'ast>> = Vec::new();
    let mut seen_by_name: HashMap<String, SeenCallable> = HashMap::new();

    for (stmt_idx, stmt) in scope_body.iter().enumerate() {
        match stmt {
            Stmt::FunctionDef(function_def) => {
                let name = function_def.name.id.to_string();
                let is_overload = is_overload_def(function_def, file);
                let is_property_accessor = is_property_accessor_def(function_def, file);

                if let Some(seen) = seen_by_name.get_mut(&name) {
                    let continues_overload =
                        is_overload || (seen.has_overload && !seen.has_non_overload);
                    if is_property_accessor || continues_overload {
                        seen.has_overload |= is_overload;
                        seen.has_non_overload |= !is_overload;
                        if stmt_idx == seen.last_stmt_idx + 1 {
                            seen.last_stmt_idx = stmt_idx;
                            items[seen.index].end_line =
                                AstNode::from_span(file, span_from_ruff_range(function_def.range))
                                    .end_line();
                        }
                        if let ScopeItemAst::Callable(defs) = &mut items[seen.index].item {
                            defs.push(function_def);
                            continue;
                        }
                    }
                }

                let index = items.len();
                seen_by_name.insert(
                    name.clone(),
                    SeenCallable {
                        index,
                        last_stmt_idx: stmt_idx,
                        has_overload: is_overload,
                        has_non_overload: !is_overload,
                    },
                );
                let visibility = visibility_of(&name);
                let is_constructor = is_class_scope && CONSTRUCTOR_NAMES.contains(&name.as_str());
                let def_node = AstNode::from_span(file, span_from_ruff_range(function_def.range));
                items.push(GroupedScopeItem {
                    name_node: AstNode::from_span(
                        file,
                        span_from_ruff_range(function_def.name.range),
                    ),
                    name,
                    start_line: def_node.start_line(),
                    end_line: def_node.end_line(),
                    signature: function_signature_text(function_def, &file.source),
                    visibility,
                    is_constructor,
                    first_stmt_idx: stmt_idx,
                    item: ScopeItemAst::Callable(vec![function_def]),
                });
            }
            Stmt::ClassDef(class_def) if !is_class_scope => {
                let name = class_def.name.id.to_string();
                seen_by_name.remove(&name);
                let visibility = visibility_of(&name);
                let class_node = AstNode::from_span(file, span_from_ruff_range(class_def.range));
                items.push(GroupedScopeItem {
                    name_node: AstNode::from_span(file, span_from_ruff_range(class_def.name.range)),
                    name,
                    start_line: class_node.start_line(),
                    end_line: class_node.end_line(),
                    signature: class_signature_text(class_def, &file.source),
                    visibility,
                    is_constructor: false,
                    first_stmt_idx: stmt_idx,
                    item: ScopeItemAst::Type(class_def),
                });
            }
            _ => {}
        }
    }

    items
}

enum ScopeItemAst<'ast> {
    Callable(Vec<&'ast StmtFunctionDef>),
    Type(&'ast StmtClassDef),
}

/// Classifies the visibility tier of a Python declaration identifier: public or `__dunder__`
/// names are `Public`; single-underscore (`_name`) and name-mangled (`__name`) names are
/// `Private`.
fn visibility_of(name: &str) -> Visibility {
    let is_dunder = name.starts_with("__") && name.ends_with("__") && name.len() > 4;
    if is_dunder || !name.starts_with('_') {
        Visibility::Public
    } else {
        Visibility::Private
    }
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

/// Renders the compact signature of a `def`: its header without the trailing colon, prefixed by a
/// `@classmethod`, `@staticmethod`, or `@property` decorator.
fn function_signature_text(function_def: &StmtFunctionDef, source: &str) -> String {
    let decorators: String = function_def
        .decorator_list
        .iter()
        .filter_map(|decorator| {
            let text = collapse_whitespace(&source[decorator.expression.range()]);
            matches!(text.as_str(), "classmethod" | "staticmethod" | "property")
                .then(|| format!("@{text} "))
        })
        .collect();
    let header = header_text(
        function_def.range(),
        &function_def.decorator_list,
        &function_def.body,
        source,
    );
    format!("{decorators}{header}")
}

/// Renders the compact signature of a `class`: its header followed by its public annotated or
/// constructor-assigned attributes.
fn class_signature_text(class_def: &StmtClassDef, source: &str) -> String {
    let header = header_text(
        class_def.range(),
        &class_def.decorator_list,
        &class_def.body,
        source,
    );
    let attributes = public_attributes(&class_def.body, source);
    if attributes.is_empty() {
        header
    } else {
        format!("{header} {{ {} }}", attributes.join(", "))
    }
}

/// The header of a `def` or `class` spanning `range`: the text between its decorators and its
/// body, joined into one line without comments or the trailing colon.
fn header_text(range: TextRange, decorators: &[Decorator], body: &[Stmt], source: &str) -> String {
    let start = decorators
        .last()
        .map_or_else(|| range.start(), |decorator| decorator.range().end());
    let end = body
        .first()
        .map_or_else(|| range.end(), |statement| statement.range().start());
    let lines: Vec<&str> = source[TextRange::new(start, end)]
        .lines()
        .map(|line| line.split('#').next().unwrap_or(line).trim())
        .filter(|line| !line.is_empty())
        .collect();
    collapse_whitespace(lines.join(" ").trim_end_matches(':'))
}

/// Public class attributes: annotated class-body names and `self.` / `cls.` attributes assigned
/// in `__init__` or `__new__`, in first-seen order.
fn public_attributes(body: &[Stmt], source: &str) -> Vec<String> {
    let mut attributes = Vec::new();
    let mut seen = HashSet::new();
    for statement in body {
        match statement {
            Stmt::AnnAssign(annotated) => {
                if let Expr::Name(name) = &*annotated.target
                    && !name.id.starts_with('_')
                    && seen.insert(name.id.to_string())
                {
                    let annotation = collapse_whitespace(&source[annotated.annotation.range()]);
                    attributes.push(format!("{}: {annotation}", name.id));
                }
            }
            Stmt::FunctionDef(function_def)
                if matches!(function_def.name.as_str(), "__init__" | "__new__") =>
            {
                for inner in &function_def.body {
                    let Stmt::Assign(assign) = inner else {
                        continue;
                    };
                    for target in &assign.targets {
                        if let Expr::Attribute(attribute) = target
                            && let Expr::Name(receiver) = &*attribute.value
                            && matches!(receiver.id.as_str(), SELF_RECEIVER | CLS_RECEIVER)
                            && !attribute.attr.starts_with('_')
                            && seen.insert(attribute.attr.to_string())
                        {
                            attributes.push(attribute.attr.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    attributes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Language;

    #[test]
    fn test_collect_class_declaration_scopes_python() {
        use Visibility::{Private, Public};

        let source = indoc::indoc! {r"
            class Service:
                @overload
                def __init__(self, port: int) -> None: ...
                @overload
                def __init__(self, port: str) -> None: ...
                def __init__(self, port: int | str) -> None:
                    pass

                @property
                def _token(self) -> str:
                    return 'x'

                @_token.setter
                def _token(self, value: str) -> None:
                    pass

                def execute(self) -> None:
                    pass

                def __repr__(self) -> str:
                    return 'Service'

                def __secret(self) -> None:
                    pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let scopes = collect_declaration_scopes(&file);
        let summary: Vec<_> = scopes
            .iter()
            .filter_map(|scope| {
                let DeclarationScopeKind::Class { name } = &scope.kind else {
                    return None;
                };
                let methods: Vec<_> = scope
                    .items
                    .iter()
                    .map(|method| {
                        (
                            method.name.as_str(),
                            method.visibility,
                            method.is_constructor,
                        )
                    })
                    .collect();
                Some((name.as_str(), methods))
            })
            .collect();
        assert_eq!(
            summary,
            vec![(
                "Service",
                vec![
                    ("__init__", Public, true),
                    ("_token", Private, false),
                    ("execute", Public, false),
                    ("__repr__", Public, false),
                    ("__secret", Private, false),
                ],
            )]
        );
    }

    #[test]
    fn test_collect_fields_after_methods_and_statements_after_main_guard() {
        let source = indoc::indoc! {r#"
            class Config:
                host: str

                def reset(self) -> None:
                    pass

                __repr__ = reset
                retries: int = 3

            if __name__ == "__main__":
                pass

            EXTRA = 1
        "#};
        let file = ParsedFile::new(source, Language::Python);
        let fields: Vec<(String, String, String, PlacementAnchor)> =
            collect_fields_after_methods(&file)
                .into_iter()
                .map(|field| {
                    (
                        field.class_name,
                        field.name,
                        field.node.text().into_owned(),
                        field.first_method,
                    )
                })
                .collect();
        let trailing: Vec<(String, usize)> = collect_statements_after_main_guard(&file)
            .into_iter()
            .map(|item| (item.node.text().into_owned(), item.guard_line))
            .collect();
        assert_eq!(
            fields,
            vec![(
                "Config".to_owned(),
                "retries".to_owned(),
                "retries: int = 3".to_owned(),
                PlacementAnchor {
                    name: "reset".to_owned(),
                    line: 4,
                },
            )]
        );
        assert_eq!(trailing, vec![("EXTRA = 1".to_owned(), 10)]);
    }

    #[test]
    fn test_field_after_method_placement_anchor() {
        let field_file = ParsedFile::new(
            indoc::indoc! {r"
                class Config:
                    @classmethod
                    def from_env(cls) -> 'Config':
                        return cls()

                    retries: int = 3
            "},
            Language::Python,
        );
        let fields = collect_fields_after_methods(&field_file);
        assert_eq!(fields.len(), 1);
        assert_eq!(
            fields[0].first_method,
            PlacementAnchor {
                name: "from_env".to_string(),
                line: 2,
            }
        );
    }

    #[test]
    fn test_statement_after_main_guard_line() {
        let guard_file = ParsedFile::new(
            indoc::indoc! {r#"
                DEFAULT_PORT = 8080

                if __name__ == "__main__":
                    print(DEFAULT_PORT)

                EXTRA = 1
            "#},
            Language::Python,
        );
        let after_guard = collect_statements_after_main_guard(&guard_file);
        assert_eq!(after_guard.len(), 1);
        assert_eq!(after_guard[0].guard_line, 3);
    }
}
