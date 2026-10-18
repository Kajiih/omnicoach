//! Python declaration-order scope builders and field-after-method extraction.

use super::classes::{CLS_RECEIVER, OVERLOAD_DECORATOR_PATHS, SELF_RECEIVER};
use super::scopes::{collect_local_bound_names, parameters_shadow_name};
use super::{AstNode, ParsedFile, extract_decorators_from_slice};
use crate::code_lint::ast::{
    DeclarationScope, MethodVisibility, PlacementAnchor, ScopeItem, ScopeItemKind, TypeMethod,
    TypeMethodScope, span_from_ruff_range, split_at_redefinitions,
};
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_expr, walk_stmt};
use ruff_python_ast::{Expr, Stmt, StmtClassDef, StmtFunctionDef};
use std::collections::{HashMap, HashSet};

const PYTHON_CONSTRUCTOR_NAMES: &[&str] = &[
    "__prepare__",
    "__init_subclass__",
    "__new__",
    "__init__",
    "__post_init__",
    "__attrs_pre_init__",
    "__attrs_post_init__",
];

/// Collects each Python `class` definition in `file` with its direct methods in source order.
#[must_use]
pub(in crate::code_lint::ast) fn collect_type_method_scopes(
    file: &ParsedFile,
) -> Vec<TypeMethodScope<'_>> {
    struct ClassMethodVisitor<'a> {
        file: &'a ParsedFile,
        scopes: Vec<TypeMethodScope<'a>>,
    }

    impl<'a> SourceOrderVisitor<'a> for ClassMethodVisitor<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if let Stmt::ClassDef(class_def) = statement {
                self.scopes.push(TypeMethodScope {
                    type_name: class_def.name.id.to_string(),
                    methods: collect_class_methods(class_def, self.file),
                });
            }
            walk_stmt(self, statement);
        }
    }

    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut visitor = ClassMethodVisitor {
        file,
        scopes: Vec::new(),
    };
    visitor.visit_body(&parsed.syntax().body);
    visitor.scopes
}

/// Collects the direct methods of `class_def` in source order, grouping `@overload` signatures
/// and `@<prop>.setter` / `@<prop>.deleter` accessors with their first definition.
fn collect_class_methods<'a>(
    class_def: &StmtClassDef,
    file: &'a ParsedFile,
) -> Vec<TypeMethod<'a>> {
    group_python_scope_items(&class_def.body, true, file)
        .into_iter()
        .map(|callable| TypeMethod {
            name_node: callable.name_node,
            name: callable.name,
            start_line: callable.start_line,
            visibility: callable.visibility,
            is_constructor: callable.is_constructor,
        })
        .collect()
}

/// Collects all module and `class` [`DeclarationScope`]s in a Python `file`.
#[must_use]
pub(in crate::code_lint::ast) fn collect_declaration_scopes(
    file: &ParsedFile,
) -> Vec<DeclarationScope<'_>> {
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

/// A type-annotated class or instance attribute (`name: Type` or `name: Type = value`) declared
/// after a method definition in a Python class body.
pub struct PythonFieldAfterMethod<'a> {
    /// Name of the enclosing class.
    pub class_name: String,
    /// Attribute identifier name.
    pub name: String,
    /// First method in the enclosing class body, directly above which the attribute belongs.
    pub first_method: PlacementAnchor,
    /// The full `Stmt::AnnAssign` AST node.
    pub node: AstNode<'a>,
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
    let grouped = group_python_scope_items(module_body, false, file);
    if grouped.len() < 2 {
        return Vec::new();
    }

    let segments = split_at_redefinitions(grouped, |item| item.name.as_str());
    let mut scopes = Vec::new();

    for (seg_idx, segment) in segments.iter().enumerate() {
        if segment.len() < 2 {
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
                build_python_module_scope_item(group, &item_by_name, &external_class_callees, file)
            })
            .collect();

        let first_stmt_indices: Vec<usize> =
            segment.iter().map(|item| item.first_stmt_idx).collect();
        let is_callable: Vec<bool> = segment
            .iter()
            .map(|item| matches!(item.item, PythonScopeItem::Callable(_)))
            .collect();
        let pinned = compute_python_pinned_before(
            module_body,
            seg_start..seg_end,
            &first_stmt_indices,
            &item_by_name,
            &items,
            &is_callable,
            future_annotations,
        );
        for (item, pin) in items.iter_mut().zip(pinned) {
            item.pinned_before = pin;
        }

        scopes.push(DeclarationScope { items });
    }

    scopes
}

/// Builds a [`DeclarationScope`] for direct methods of `class_def`.
fn build_class_declaration_scope<'a>(
    class_def: &StmtClassDef,
    future_annotations: bool,
    file: &'a ParsedFile,
) -> Option<DeclarationScope<'a>> {
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
                if is_valid_receiver
                    && let Some(&idx) = self.method_by_name.get(attr.attr.id.as_str())
                    && !self.callees.contains(&idx)
                {
                    self.callees.push(idx);
                }
            }
            walk_expr(self, expr);
        }
    }

    let grouped = group_python_scope_items(&class_def.body, true, file);
    if grouped.len() < 2 {
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
            let mut callees = Vec::new();
            if let PythonScopeItem::Callable(defs) = &group.item {
                for def in defs {
                    let local_names = collect_local_bound_names(def, file);
                    let has_self_param = parameters_shadow_name(&def.parameters, SELF_RECEIVER);
                    let has_cls_param = parameters_shadow_name(&def.parameters, CLS_RECEIVER);
                    let mut visitor = ClassMethodCallVisitor {
                        class_name,
                        method_by_name: &method_by_name,
                        local_names: &local_names,
                        has_self_param,
                        has_cls_param,
                        callees: &mut callees,
                    };
                    visitor.visit_body(&def.body);
                }
            }
            let body_callees = callees.clone();
            ScopeItem {
                name_node: group.name_node,
                name: group.name,
                end_line: group.end_line,
                visibility: group.visibility,
                is_constructor: group.is_constructor,
                kind: ScopeItemKind::Callable,
                callees,
                param_callees: Vec::new(),
                return_callees: Vec::new(),
                body_callees,
                pinned_before: None,
            }
        })
        .collect();

    let is_callable = vec![true; items.len()];
    let pinned = compute_python_pinned_before(
        &class_def.body,
        0..class_def.body.len(),
        &first_stmt_indices,
        &method_by_name,
        &items,
        &is_callable,
        future_annotations,
    );
    for (item, pin) in items.iter_mut().zip(pinned) {
        item.pinned_before = pin;
    }

    Some(DeclarationScope { items })
}

struct GroupedPythonScopeItem<'a, 'ast> {
    name_node: AstNode<'a>,
    name: String,
    start_line: usize,
    end_line: usize,
    visibility: MethodVisibility,
    is_constructor: bool,
    first_stmt_idx: usize,
    item: PythonScopeItem<'ast>,
}

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
            collect_python_class_module_refs(
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
fn build_python_module_scope_item<'a>(
    group: &GroupedPythonScopeItem<'a, '_>,
    item_by_name: &HashMap<String, usize>,
    external_class_callees: &HashMap<&str, Vec<usize>>,
    file: &ParsedFile,
) -> ScopeItem<'a> {
    let empty_locals = HashSet::new();
    let mut callees = Vec::new();
    let mut param_callees = Vec::new();
    let mut return_callees = Vec::new();
    let mut body_callees = Vec::new();
    let kind = match &group.item {
        PythonScopeItem::Callable(defs) => {
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
            for &idx in param_callees
                .iter()
                .chain(&return_callees)
                .chain(&body_callees)
            {
                if !callees.contains(&idx) {
                    callees.push(idx);
                }
            }
            ScopeItemKind::Callable
        }
        PythonScopeItem::Type(class_def) => {
            collect_python_class_module_refs(
                class_def,
                item_by_name,
                external_class_callees,
                &empty_locals,
                file,
                &mut callees,
            );
            body_callees.clone_from(&callees);
            ScopeItemKind::Type
        }
    };
    ScopeItem {
        name_node: group.name_node,
        name: group.name.clone(),
        end_line: group.end_line,
        visibility: group.visibility,
        is_constructor: group.is_constructor,
        kind,
        callees,
        param_callees,
        return_callees,
        body_callees,
        pinned_before: None,
    }
}

fn collect_python_class_module_refs<'map>(
    class_def: &StmtClassDef,
    item_by_name: &'map HashMap<String, usize>,
    external_class_callees: &'map HashMap<&'map str, Vec<usize>>,
    empty_locals: &HashSet<String>,
    file: &ParsedFile,
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
        } else {
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
                if let Some(&idx) = self.fn_by_name.get(ident)
                    && !self.callees.contains(&idx)
                {
                    self.callees.push(idx);
                }
                if let Some(bridged) = self.class_callees.get(ident) {
                    for &idx in bridged {
                        if !self.callees.contains(&idx) {
                            self.callees.push(idx);
                        }
                    }
                }
            }
        }
        walk_expr(self, expr);
    }
}

/// Computes `pinned_before` for each scope item in `items` by walking top-to-bottom runtime
/// evaluations in `scope_body[stmt_range]` (outside `Stmt::FunctionDef` bodies) and propagating
/// constraints through callable call edges.
fn compute_python_pinned_before(
    scope_body: &[Stmt],
    stmt_range: std::ops::Range<usize>,
    first_stmt_indices: &[usize],
    item_by_name: &HashMap<String, usize>,
    items: &[ScopeItem<'_>],
    is_callable: &[bool],
    future_annotations: bool,
) -> Vec<Option<usize>> {
    let count = items.len();
    let mut pinned: Vec<Option<usize>> = vec![None; count];

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
        let mut imm_refs = Vec::new();
        let mut visitor = ImmediateRefVisitor {
            item_by_name,
            future_annotations,
            refs: &mut imm_refs,
        };
        visitor.visit_stmt(stmt);
        for target_idx in imm_refs {
            if target_idx < bound_idx {
                pinned[target_idx] =
                    Some(pinned[target_idx].map_or(bound_idx, |prev| prev.min(bound_idx)));
            }
        }
    }

    // Propagate runtime pins through callable callees: if a function `u` is evaluated before
    // `pin`, any sibling item `v < pin` reachable from `u` must also be defined before `pin`.
    for start in 0..count {
        let Some(pin) = pinned[start] else {
            continue;
        };
        if !is_callable[start] {
            continue;
        }
        let mut stack = vec![start];
        let mut visited = vec![false; count];
        while let Some(cur) = stack.pop() {
            if visited[cur] {
                continue;
            }
            visited[cur] = true;
            for &callee in &items[cur].callees {
                if callee < pin {
                    pinned[callee] = Some(pinned[callee].map_or(pin, |prev| prev.min(pin)));
                    if is_callable[callee] {
                        stack.push(callee);
                    }
                }
            }
        }
    }

    pinned
}

/// Collects the scope items a statement references when the scope body is evaluated, skipping
/// function bodies and postponed annotations.
struct ImmediateRefVisitor<'map, 'out> {
    item_by_name: &'map HashMap<String, usize>,
    future_annotations: bool,
    refs: &'out mut Vec<usize>,
}

impl SourceOrderVisitor<'_> for ImmediateRefVisitor<'_, '_> {
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
            && !self.refs.contains(&idx)
        {
            self.refs.push(idx);
        }
        walk_expr(self, expr);
    }
}

/// Collects direct functions and classes in `scope_body` in source order, grouping `@overload`
/// signatures and `@<prop>.setter` / `@<prop>.deleter` accessors with their first definition.
fn group_python_scope_items<'a, 'ast>(
    scope_body: &'ast [Stmt],
    is_class_scope: bool,
    file: &'a ParsedFile,
) -> Vec<GroupedPythonScopeItem<'a, 'ast>> {
    struct SeenCallable {
        index: usize,
        last_stmt_idx: usize,
        has_overload: bool,
        has_non_overload: bool,
    }

    let mut items: Vec<GroupedPythonScopeItem<'a, 'ast>> = Vec::new();
    let mut seen_by_name: HashMap<String, SeenCallable> = HashMap::new();

    for (stmt_idx, stmt) in scope_body.iter().enumerate() {
        match stmt {
            Stmt::FunctionDef(function_def) => {
                let name = function_def.name.id.to_string();
                let decorators = extract_decorators_from_slice(&function_def.decorator_list, file);
                let is_overload = decorators
                    .iter()
                    .any(|decorator| decorator.matches_any(OVERLOAD_DECORATOR_PATHS));
                let is_property_accessor = decorators.iter().any(|decorator| {
                    decorator.is_attribute_named(&["getter", "setter", "deleter"])
                });

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
                        if let PythonScopeItem::Callable(defs) = &mut items[seen.index].item {
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
                let visibility = python_method_visibility(&name);
                let is_constructor =
                    is_class_scope && PYTHON_CONSTRUCTOR_NAMES.contains(&name.as_str());
                let def_node = AstNode::from_span(file, span_from_ruff_range(function_def.range));
                items.push(GroupedPythonScopeItem {
                    name_node: AstNode::from_span(
                        file,
                        span_from_ruff_range(function_def.name.range),
                    ),
                    name,
                    start_line: def_node.start_line(),
                    end_line: def_node.end_line(),
                    visibility,
                    is_constructor,
                    first_stmt_idx: stmt_idx,
                    item: PythonScopeItem::Callable(vec![function_def]),
                });
            }
            Stmt::ClassDef(class_def) if !is_class_scope => {
                let name = class_def.name.id.to_string();
                seen_by_name.remove(&name);
                let visibility = python_method_visibility(&name);
                let class_node = AstNode::from_span(file, span_from_ruff_range(class_def.range));
                items.push(GroupedPythonScopeItem {
                    name_node: AstNode::from_span(file, span_from_ruff_range(class_def.name.range)),
                    name,
                    start_line: class_node.start_line(),
                    end_line: class_node.end_line(),
                    visibility,
                    is_constructor: false,
                    first_stmt_idx: stmt_idx,
                    item: PythonScopeItem::Type(class_def),
                });
            }
            _ => {}
        }
    }

    items
}

enum PythonScopeItem<'ast> {
    Callable(Vec<&'ast StmtFunctionDef>),
    Type(&'ast StmtClassDef),
}

/// Classifies the visibility tier of a Python function or method identifier: public or
/// `__dunder__` names are `Public`; single-underscore (`_name`) and name-mangled (`__name`)
/// names are `Private`.
fn python_method_visibility(name: &str) -> MethodVisibility {
    let is_dunder = name.starts_with("__") && name.ends_with("__") && name.len() > 4;
    if is_dunder || !name.starts_with('_') {
        MethodVisibility::Public
    } else {
        MethodVisibility::Private
    }
}
