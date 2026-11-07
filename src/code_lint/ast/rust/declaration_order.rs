//! Rust declaration-order scope builders and associated-item placement extraction (`ra_ap_syntax`).

use super::bindings::{collect_rust_fn_local_names, collect_rust_generic_param_names};
use super::names::{SELF_KEYWORD, SELF_TYPE_KEYWORD};
use super::trivia::previous_non_trivia_token;
use crate::code_lint::ast::call_clusters::{
    DeclarationScope, DeclarationScopeKind, PlacementAnchor, RelationKind, ScopeItem,
    ScopeItemKind, Visibility, collapse_whitespace, extend_references, extend_unique, push_unique,
    split_at_redefinitions,
};
use crate::code_lint::ast::scope_facts::IntraFileReference;
use crate::code_lint::ast::tree::{AstNode, ParsedFile, span_from_rowan_range};
use ra_ap_syntax::ast::{
    self, HasGenericParams as _, HasModuleItem as _, HasName as _, HasVisibility as _,
};
use ra_ap_syntax::{AstNode as _, SyntaxKind, SyntaxNode, SyntaxToken};
use std::collections::{HashMap, HashSet};

/// An associated `type` or `const` item declared after at least one `fn` item inside a Rust
/// `impl` or `trait` block.
pub struct RustAssociatedItemAfterMethod<'a> {
    /// Name of the enclosing `impl` self-type or `trait`.
    pub container_name: String,
    /// Identifier of the misplaced `type` or `const` item.
    pub name: String,
    /// First `fn` in the same block before which the item should move.
    pub first_method: PlacementAnchor,
    /// AST node of the misplaced item's name.
    pub node: AstNode<'a>,
}

/// Collects all module and inherent `impl` [`DeclarationScope`]s in a Rust `file`.
#[must_use]
pub fn collect_declaration_scopes(file: &ParsedFile) -> Vec<DeclarationScope<'_>> {
    let Some(parsed) = file.rs_parsed() else {
        return Vec::new();
    };
    let root_items: Vec<ast::Item> = parsed.tree().items().collect();
    let mut scopes = build_rust_module_declaration_scopes(&root_items, file);
    for node in parsed.tree().syntax().descendants() {
        if file.is_in_rust_inline_test(node.text_range().start().into()) {
            continue;
        }
        if let Some(module) = ast::Module::cast(node.clone()) {
            if let Some(module_body) = module.item_list() {
                let items: Vec<ast::Item> = module_body.items().collect();
                scopes.extend(build_rust_module_declaration_scopes(&items, file));
            }
        } else if let Some(impl_item) = ast::Impl::cast(node)
            && impl_item.trait_().is_none()
            && let Some(scope) = build_rust_impl_declaration_scope(&impl_item, file)
        {
            scopes.push(scope);
        }
    }
    scopes
}

/// Builds [`DeclarationScope`]s for direct `fn`, `struct`, `enum`, `union`, and `trait` items in
/// a Rust module item list, grouping same-module `impl` blocks with their type/trait and starting
/// a new scope segment whenever an item name is redefined.
fn build_rust_module_declaration_scopes<'a>(
    items: &[ast::Item],
    file: &'a ParsedFile,
) -> Vec<DeclarationScope<'a>> {
    let RustModuleContents {
        declarations,
        impl_items,
    } = collect_rust_module_decls(items, file);
    if declarations.is_empty() {
        return Vec::new();
    }

    let segments = split_at_redefinitions(declarations, |decl| decl.name.as_str());
    let mut scopes = Vec::new();

    for (seg_idx, segment) in segments.iter().enumerate() {
        if segment.is_empty() {
            continue;
        }
        let seg_start = if seg_idx == 0 {
            0
        } else {
            segment[0].syntax_start
        };
        let seg_end = segments
            .get(seg_idx + 1)
            .map_or(usize::MAX, |next| next[0].syntax_start);

        let item_by_name: HashMap<&str, usize> = segment
            .iter()
            .enumerate()
            .map(|(idx, decl)| (decl.name.as_str(), idx))
            .collect();
        let segment_is_type: Vec<bool> = segment
            .iter()
            .map(|decl| matches!(decl.item, RustModuleItem::Type(_)))
            .collect();

        let impl_callees = collect_rust_impl_callees(
            &impl_items,
            seg_start..seg_end,
            &item_by_name,
            &segment_is_type,
            file,
        );

        let items = segment
            .iter()
            .enumerate()
            .map(|(idx, decl)| {
                let next_start = segment
                    .get(idx + 1)
                    .map_or(seg_end, |next| next.syntax_start);
                let end_line = rust_decl_end_line(decl, next_start, &impl_items, file);
                build_rust_module_scope_item(
                    decl,
                    end_line,
                    &impl_callees.attached[idx],
                    &item_by_name,
                    &segment_is_type,
                    &impl_callees.external,
                )
            })
            .collect();

        scopes.push(DeclarationScope {
            kind: DeclarationScopeKind::Module,
            items,
        });
    }

    scopes
}

/// The declarations and `impl` blocks directly inside a Rust module item list.
struct RustModuleContents<'a> {
    /// Direct `fn`, `struct`, `enum`, `union`, and `trait` declarations, in source order.
    declarations: Vec<RustModuleDecl<'a>>,
    /// Every direct `impl` block, keyed by its starting offset.
    impl_items: Vec<(usize, ast::Impl)>,
}

/// Collects the direct `fn`, `struct`, `enum`, `union`, and `trait` declarations in `items`,
/// alongside every `impl` block keyed by its starting offset.
fn collect_rust_module_decls<'a>(
    items: &[ast::Item],
    file: &'a ParsedFile,
) -> RustModuleContents<'a> {
    let mut declarations = Vec::new();
    let mut impl_items = Vec::new();
    for item in items {
        let syntax_start: usize = item.syntax().text_range().start().into();
        if file.is_in_rust_inline_test(syntax_start) {
            continue;
        }
        match item {
            ast::Item::Fn(function) => {
                if let Some(name_node) = function.name() {
                    let name = name_node.text().trim().to_owned();
                    let visibility = if function.visibility().is_some() || name == "main" {
                        Visibility::Public
                    } else {
                        Visibility::Private
                    };
                    declarations.push(RustModuleDecl {
                        name_node: AstNode::from_span(
                            file,
                            span_from_rowan_range(name_node.syntax().text_range()),
                        ),
                        name,
                        start_line: file.line_index.line(syntax_start),
                        visibility,
                        syntax_start,
                        signature: rust_item_signature(function.syntax(), &file.source),
                        item: RustModuleItem::Fn(function.clone()),
                    });
                }
            }
            ast::Item::Struct(struct_item) => {
                declarations.extend(rust_type_decl(struct_item, syntax_start, file));
            }
            ast::Item::Enum(enum_item) => {
                declarations.extend(rust_type_decl(enum_item, syntax_start, file));
            }
            ast::Item::Union(union_item) => {
                declarations.extend(rust_type_decl(union_item, syntax_start, file));
            }
            ast::Item::Trait(trait_item) => {
                declarations.extend(rust_type_decl(trait_item, syntax_start, file));
            }
            ast::Item::Impl(impl_item) => impl_items.push((syntax_start, impl_item.clone())),
            _ => {}
        }
    }
    RustModuleContents {
        declarations,
        impl_items,
    }
}

struct RustModuleDecl<'a> {
    name_node: AstNode<'a>,
    name: String,
    start_line: usize,
    visibility: Visibility,
    syntax_start: usize,
    signature: Option<String>,
    item: RustModuleItem,
}

fn rust_type_decl<'a>(
    type_item: &(impl ast::HasName + ast::HasVisibility),
    syntax_start: usize,
    file: &'a ParsedFile,
) -> Option<RustModuleDecl<'a>> {
    let name_node = type_item.name()?;
    let name = name_node.text().trim().to_owned();
    let visibility = if type_item.visibility().is_some() {
        Visibility::Public
    } else {
        Visibility::Private
    };
    Some(RustModuleDecl {
        name_node: AstNode::from_span(file, span_from_rowan_range(name_node.syntax().text_range())),
        name,
        start_line: file.line_index.line(syntax_start),
        visibility,
        syntax_start,
        signature: rust_item_signature(type_item.syntax(), &file.source),
        item: RustModuleItem::Type(type_item.syntax().clone()),
    })
}

/// Returns the 1-based ending line of `decl`, extended to cover any contiguous `impl` blocks for
/// `decl` that appear before the next declaration at `next_start`.
fn rust_decl_end_line(
    decl: &RustModuleDecl<'_>,
    next_start: usize,
    impl_items: &[(usize, ast::Impl)],
    file: &ParsedFile,
) -> usize {
    let (syntax, is_type) = match &decl.item {
        RustModuleItem::Fn(function) => (function.syntax(), false),
        RustModuleItem::Type(syntax) => (syntax, true),
    };
    let mut end_line =
        AstNode::from_span(file, span_from_rowan_range(syntax.text_range())).end_line();
    if !is_type {
        return end_line;
    }
    for (start, impl_item) in impl_items {
        if !(decl.syntax_start..next_start).contains(start) {
            continue;
        }
        let matches_decl = impl_self_type_name(impl_item, &file.source)
            .as_deref()
            .is_some_and(|name| name == decl.name)
            || impl_trait_name(impl_item)
                .as_deref()
                .is_some_and(|name| name == decl.name);
        if matches_decl {
            let impl_end =
                AstNode::from_span(file, span_from_rowan_range(impl_item.syntax().text_range()))
                    .end_line();
            end_line = end_line.max(impl_end);
        }
    }
    end_line
}

/// Builds the [`ScopeItem`] for `decl`, merging the sibling references collected from its own
/// syntax with `attached_impl_callees` from its same-module `impl` blocks.
fn build_rust_module_scope_item<'a>(
    decl: &RustModuleDecl<'a>,
    end_line: usize,
    attached_impl_callees: &[usize],
    item_by_name: &HashMap<&str, usize>,
    segment_is_type: &[bool],
    external_impl_callees: &HashMap<String, Vec<usize>>,
) -> ScopeItem<'a> {
    let mut references = Vec::new();
    let kind = match &decl.item {
        RustModuleItem::Fn(function) => {
            collect_rust_fn_module_references(
                function,
                item_by_name,
                segment_is_type,
                external_impl_callees,
                &mut references,
            );
            ScopeItemKind::Callable
        }
        RustModuleItem::Type(syntax) => {
            let local_names = collect_rust_generic_param_names(syntax);
            let mut definition_callees = Vec::new();
            collect_rust_module_item_refs(
                syntax,
                item_by_name,
                segment_is_type,
                external_impl_callees,
                &local_names,
                &mut definition_callees,
            );
            extend_references(
                &mut references,
                definition_callees,
                RelationKind::Composition,
            );
            ScopeItemKind::Type
        }
    };
    extend_references(
        &mut references,
        attached_impl_callees.iter().copied(),
        RelationKind::Behavior,
    );
    ScopeItem {
        name_node: decl.name_node,
        start_line: decl.start_line,
        end_line,
        name: decl.name.clone(),
        signature: decl.signature.clone(),
        visibility: decl.visibility,
        is_constructor: false,
        kind,
        references,
        evaluated_before: None,
    }
}

fn collect_rust_fn_module_references(
    function: &ast::Fn,
    item_by_name: &HashMap<&str, usize>,
    segment_is_type: &[bool],
    external_impl_callees: &HashMap<String, Vec<usize>>,
    references: &mut Vec<(usize, RelationKind)>,
) {
    let generic_names = collect_rust_generic_param_names(function.syntax());
    let local_names = collect_rust_fn_local_names(function);
    let mut fn_callees = Vec::new();
    let mut param_callees = Vec::new();
    let mut return_callees = Vec::new();
    let mut body_callees = Vec::new();
    collect_rust_module_item_refs(
        function.syntax(),
        item_by_name,
        segment_is_type,
        external_impl_callees,
        &local_names,
        &mut fn_callees,
    );
    if let Some(params) = function.param_list() {
        collect_rust_module_item_refs(
            params.syntax(),
            item_by_name,
            segment_is_type,
            external_impl_callees,
            &generic_names,
            &mut param_callees,
        );
    }
    if let Some(where_clause) = function.where_clause() {
        collect_rust_module_item_refs(
            where_clause.syntax(),
            item_by_name,
            segment_is_type,
            external_impl_callees,
            &generic_names,
            &mut param_callees,
        );
    }
    if let Some(ret_type) = function.ret_type() {
        collect_rust_module_item_refs(
            ret_type.syntax(),
            item_by_name,
            segment_is_type,
            external_impl_callees,
            &generic_names,
            &mut return_callees,
        );
    }
    if let Some(body) = function.body() {
        collect_rust_module_item_refs(
            body.syntax(),
            item_by_name,
            segment_is_type,
            external_impl_callees,
            &local_names,
            &mut body_callees,
        );
    }
    for target in fn_callees {
        if !param_callees.contains(&target) && !return_callees.contains(&target) {
            push_unique(&mut body_callees, target);
        }
    }
    extend_references(references, param_callees, RelationKind::Parameter);
    extend_references(references, return_callees, RelationKind::Return);
    extend_references(references, body_callees, RelationKind::Body);
}

enum RustModuleItem {
    Fn(ast::Fn),
    Type(SyntaxNode),
}

struct RustImplCallees {
    attached: Vec<Vec<usize>>,
    external: HashMap<String, Vec<usize>>,
}

/// Collects sibling module item references inside `impl_items` within `offset_range`, attaching
/// them to the corresponding type/trait index in `segment` (or returning them keyed by self-type
/// name when the self-type is outside `segment`).
fn collect_rust_impl_callees(
    impl_items: &[(usize, ast::Impl)],
    offset_range: std::ops::Range<usize>,
    item_by_name: &HashMap<&str, usize>,
    segment_is_type: &[bool],
    file: &ParsedFile,
) -> RustImplCallees {
    let empty_impls = HashMap::new();
    let mut attached = vec![Vec::new(); segment_is_type.len()];
    let mut external: HashMap<String, Vec<usize>> = HashMap::new();

    for (start, impl_item) in impl_items {
        if !offset_range.contains(start) {
            continue;
        }
        let impl_generics = collect_rust_generic_param_names(impl_item.syntax());
        let mut callees = Vec::new();
        if let Some(self_type) = impl_item.self_ty() {
            collect_rust_module_item_refs(
                self_type.syntax(),
                item_by_name,
                segment_is_type,
                &empty_impls,
                &impl_generics,
                &mut callees,
            );
        }
        if let Some(trait_type) = impl_item.trait_() {
            collect_rust_module_item_refs(
                trait_type.syntax(),
                item_by_name,
                segment_is_type,
                &empty_impls,
                &impl_generics,
                &mut callees,
            );
        }
        if let Some(where_clause) = impl_item.where_clause() {
            collect_rust_module_item_refs(
                where_clause.syntax(),
                item_by_name,
                segment_is_type,
                &empty_impls,
                &impl_generics,
                &mut callees,
            );
        }
        if let Some(assoc_items) = impl_item.assoc_item_list() {
            for assoc_item in assoc_items.assoc_items() {
                if file.is_in_rust_inline_test(assoc_item.syntax().text_range().start().into()) {
                    continue;
                }
                match assoc_item {
                    ast::AssocItem::Fn(method) => {
                        let mut local_names = impl_generics.clone();
                        local_names.extend(collect_rust_fn_local_names(&method));
                        collect_rust_module_item_refs(
                            method.syntax(),
                            item_by_name,
                            segment_is_type,
                            &empty_impls,
                            &local_names,
                            &mut callees,
                        );
                    }
                    other => {
                        collect_rust_module_item_refs(
                            other.syntax(),
                            item_by_name,
                            segment_is_type,
                            &empty_impls,
                            &impl_generics,
                            &mut callees,
                        );
                    }
                }
            }
        }
        if callees.is_empty() {
            continue;
        }
        if let Some(self_name) = impl_self_type_name(impl_item, &file.source) {
            if let Some(&self_idx) = item_by_name.get(self_name.as_str()) {
                extend_unique(&mut attached[self_idx], callees.iter().copied());
            } else {
                extend_unique(
                    external.entry(self_name).or_default(),
                    callees.iter().copied(),
                );
            }
        }
        if let Some(trait_name) = impl_trait_name(impl_item)
            && let Some(&trait_idx) = item_by_name.get(trait_name.as_str())
        {
            extend_unique(&mut attached[trait_idx], callees.iter().copied());
        }
    }

    RustImplCallees { attached, external }
}

fn impl_trait_name(impl_item: &ast::Impl) -> Option<String> {
    if let ast::Type::PathType(path_type) = impl_item.trait_()?
        && let Some(path) = path_type.path()
        && let Some(segment) = path.segment()
        && let Some(name_ref) = segment.name_ref()
    {
        return Some(name_ref.text().trim().to_owned());
    }
    None
}

/// Collects indices of sibling module items in `item_by_name` referenced inside `root`
/// (including bridged calls via `impl_callees` and macro token trees).
fn collect_rust_module_item_refs(
    root: &SyntaxNode,
    item_by_name: &HashMap<&str, usize>,
    segment_is_type: &[bool],
    impl_callees: &HashMap<String, Vec<usize>>,
    local_names: &HashSet<String>,
    out: &mut Vec<usize>,
) {
    walk_rust_syntax_refs(root, false, |syntax_ref| {
        if let RustSyntaxRef::Path {
            seg0,
            seg1,
            has_extra_segments,
            ..
        } = syntax_ref
        {
            if local_names.contains(seg0) {
                return;
            }
            let is_composite = seg1.is_some() || has_extra_segments;
            if let Some(&idx) = item_by_name.get(seg0)
                && (!is_composite || segment_is_type[idx])
            {
                push_unique(out, idx);
            }
            if let Some(bridged) = impl_callees.get(seg0) {
                extend_unique(out, bridged.iter().copied());
            }
        }
    });
}

/// Builds a [`DeclarationScope`] for direct non-test methods of an inherent `impl` block.
fn build_rust_impl_declaration_scope<'a>(
    impl_item: &ast::Impl,
    file: &'a ParsedFile,
) -> Option<DeclarationScope<'a>> {
    let type_name = impl_self_type_name(impl_item, &file.source)?;
    let associated_items = impl_item.assoc_item_list()?;

    let methods = collect_rust_impl_methods(&associated_items, &type_name, file);
    if methods.is_empty() {
        return None;
    }

    let method_by_name: HashMap<String, usize> = methods
        .iter()
        .enumerate()
        .map(|(idx, (item, _))| (item.name.clone(), idx))
        .collect();

    let items = methods
        .into_iter()
        .map(|(mut item, function)| {
            let mut body_callees = Vec::new();
            if let Some(body) = function.body() {
                collect_rust_impl_method_refs(
                    body.syntax(),
                    &type_name,
                    &method_by_name,
                    &mut body_callees,
                );
            }
            extend_references(&mut item.references, body_callees, RelationKind::Body);
            item
        })
        .collect();

    let kind = DeclarationScopeKind::Impl { type_name };
    Some(DeclarationScope { kind, items })
}

/// Collects indices of sibling `impl` methods in `method_by_name` called via `self.method()`,
/// `Self::method`, or `<type_name>::method` inside `root` (stopping at nested `impl` blocks).
/// Inside macro token trees, `self.method` counts only when called (`self.method(...)`), since
/// without parentheses it names a field.
fn collect_rust_impl_method_refs(
    root: &SyntaxNode,
    type_name: &str,
    method_by_name: &HashMap<String, usize>,
    out: &mut Vec<usize>,
) {
    walk_rust_syntax_refs(root, true, |syntax_ref| match syntax_ref {
        RustSyntaxRef::SelfMethodCall { method, .. } => {
            if let Some(&idx) = method_by_name.get(method) {
                push_unique(out, idx);
            }
        }
        RustSyntaxRef::Path {
            seg0,
            seg1: Some(method),
            has_extra_segments: false,
            ..
        } if seg0 == SELF_TYPE_KEYWORD || seg0 == type_name => {
            if let Some(&idx) = method_by_name.get(method) {
                push_unique(out, idx);
            }
        }
        RustSyntaxRef::Path { .. } => {}
    });
}

/// Collects direct non-test `fn` items of a Rust inherent `impl` block along with their
/// [`ScopeItem`] metadata in source order.
fn collect_rust_impl_methods<'a>(
    associated_items: &ast::AssocItemList,
    type_name: &str,
    file: &'a ParsedFile,
) -> Vec<(ScopeItem<'a>, ast::Fn)> {
    let mut methods = Vec::new();
    for assoc_item in associated_items.assoc_items() {
        let ast::AssocItem::Fn(function) = assoc_item else {
            continue;
        };
        if file.is_in_rust_inline_test(function.syntax().text_range().start().into()) {
            continue;
        }
        let Some(name_node) = function.name() else {
            continue;
        };
        let name = name_node.text().trim().to_owned();
        let is_exported = function.visibility().is_some();
        let visibility = if is_exported {
            Visibility::Public
        } else {
            Visibility::Private
        };
        let has_self = function
            .param_list()
            .is_some_and(|params| params.self_param().is_some());
        let is_constructor = is_exported
            && !has_self
            && is_rust_constructor_name(&name)
            && returns_self_type(&function, type_name);
        let fn_node =
            AstNode::from_span(file, span_from_rowan_range(function.syntax().text_range()));
        methods.push((
            ScopeItem {
                name_node: AstNode::from_span(
                    file,
                    span_from_rowan_range(name_node.syntax().text_range()),
                ),
                start_line: fn_node.start_line(),
                end_line: fn_node.end_line(),
                name,
                signature: rust_item_signature(function.syntax(), &file.source),
                visibility,
                is_constructor,
                kind: ScopeItemKind::Callable,
                references: Vec::new(),
                evaluated_before: None,
            },
            function,
        ));
    }
    methods
}

/// Returns true if `name` is a standard Rust constructor identifier (`new`, `try_new`, `new_*`,
/// or `try_new_*`).
fn is_rust_constructor_name(name: &str) -> bool {
    matches!(name, "new" | "try_new") || name.starts_with("new_") || name.starts_with("try_new_")
}

/// Returns true if `function` has a return type that references `Self` or `type_name`.
fn returns_self_type(function: &ast::Fn, type_name: &str) -> bool {
    function.ret_type().is_some_and(|ret_type| {
        ret_type
            .syntax()
            .descendants_with_tokens()
            .filter_map(ra_ap_syntax::NodeOrToken::into_token)
            .any(|token| {
                token.kind() == SyntaxKind::SELF_TYPE_KW
                    || (token.kind() == SyntaxKind::IDENT && token.text() == type_name)
            })
    })
}

/// Renders the compact signature of a `fn`, `struct`, `enum`, or `trait` item: its header up to
/// the body, followed by the fields or variants of a type.
fn rust_item_signature(node: &SyntaxNode, source: &str) -> Option<String> {
    let header = |start: ra_ap_syntax::TextSize, end: ra_ap_syntax::TextSize| {
        let start: usize = start.into();
        let end: usize = end.into();
        collapse_whitespace(source[start..end].trim_end_matches(';'))
    };
    let item_start = node.text_range().start();
    let item_end = node.text_range().end();
    match ast::Item::cast(node.clone())? {
        ast::Item::Fn(function) => {
            let start = function
                .visibility()
                .map(|visibility| visibility.syntax().text_range().start())
                .or_else(|| {
                    function
                        .const_token()
                        .map(|token| token.text_range().start())
                })
                .or_else(|| {
                    function
                        .async_token()
                        .map(|token| token.text_range().start())
                })
                .or_else(|| {
                    function
                        .unsafe_token()
                        .map(|token| token.text_range().start())
                })
                .or_else(|| function.abi().map(|abi| abi.syntax().text_range().start()))
                .or_else(|| function.fn_token().map(|token| token.text_range().start()))
                .unwrap_or(item_start);
            let end = function
                .body()
                .map_or(item_end, |body| body.syntax().text_range().start());
            Some(header(start, end))
        }
        ast::Item::Struct(struct_item) => {
            let start = struct_item
                .visibility()
                .map(|visibility| visibility.syntax().text_range().start())
                .or_else(|| {
                    struct_item
                        .struct_token()
                        .map(|token| token.text_range().start())
                })
                .unwrap_or(item_start);
            let end = struct_item
                .field_list()
                .map_or(item_end, |fields| fields.syntax().text_range().start());
            let fields = rust_fields_summary(struct_item.field_list(), true);
            Some(format!("{}{fields}", header(start, end)))
        }
        ast::Item::Enum(enum_item) => {
            let start = enum_item
                .visibility()
                .map(|visibility| visibility.syntax().text_range().start())
                .or_else(|| {
                    enum_item
                        .enum_token()
                        .map(|token| token.text_range().start())
                })
                .unwrap_or(item_start);
            let end = enum_item
                .variant_list()
                .map_or(item_end, |variants| variants.syntax().text_range().start());
            let variants: Vec<String> = enum_item
                .variant_list()
                .into_iter()
                .flat_map(|variants| variants.variants())
                .filter_map(|variant| {
                    let name = variant.name()?.text().to_string();
                    let payload = rust_fields_summary(variant.field_list(), false);
                    Some(format!("{name}{payload}"))
                })
                .collect();
            Some(format!(
                "{} {{ {} }}",
                header(start, end),
                variants.join(", ")
            ))
        }
        ast::Item::Trait(trait_item) => {
            let start = trait_item
                .visibility()
                .map(|visibility| visibility.syntax().text_range().start())
                .or_else(|| {
                    trait_item
                        .trait_token()
                        .map(|token| token.text_range().start())
                })
                .unwrap_or(item_start);
            let end = trait_item
                .assoc_item_list()
                .map_or(item_end, |items| items.syntax().text_range().start());
            Some(header(start, end))
        }
        _ => None,
    }
}

/// Renders a struct or variant field list as ` { name: Type, ... }` or `(Type, ...)`.
fn rust_fields_summary(fields: Option<ast::FieldList>, include_visibility: bool) -> String {
    let visibility_prefix = |visibility: Option<ast::Visibility>| {
        visibility
            .filter(|_| include_visibility)
            .map(|node| {
                format!(
                    "{} ",
                    collapse_whitespace(&node.syntax().text().to_string())
                )
            })
            .unwrap_or_default()
    };
    match fields {
        Some(ast::FieldList::RecordFieldList(record)) => {
            let fields: Vec<String> = record
                .fields()
                .filter_map(|field| {
                    let name = field.name()?.text().to_string();
                    let field_type = collapse_whitespace(&field.ty()?.syntax().text().to_string());
                    let visibility = visibility_prefix(field.visibility());
                    Some(format!("{visibility}{name}: {field_type}"))
                })
                .collect();
            format!(" {{ {} }}", fields.join(", "))
        }
        Some(ast::FieldList::TupleFieldList(tuple)) => {
            let fields: Vec<String> = tuple
                .fields()
                .filter_map(|field| {
                    let field_type = collapse_whitespace(&field.ty()?.syntax().text().to_string());
                    let visibility = visibility_prefix(field.visibility());
                    Some(format!("{visibility}{field_type}"))
                })
                .collect();
            format!("({})", fields.join(", "))
        }
        None => String::new(),
    }
}

/// Collects associated `type` and `const` items declared after `fn` items inside Rust `impl`
/// (inherent or trait) and `trait` blocks across `file`.
#[must_use]
pub fn collect_associated_items_after_methods(
    file: &ParsedFile,
) -> Vec<RustAssociatedItemAfterMethod<'_>> {
    let Some(parsed) = file.rs_parsed() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for node in parsed.tree().syntax().descendants() {
        if file.is_in_rust_inline_test(node.text_range().start().into()) {
            continue;
        }
        if let Some(impl_item) = ast::Impl::cast(node.clone()) {
            if let (Some(container_name), Some(associated_items)) = (
                impl_self_type_name(&impl_item, &file.source),
                impl_item.assoc_item_list(),
            ) {
                collect_misplaced_assoc_items(&associated_items, &container_name, file, &mut out);
            }
        } else if let Some(trait_item) = ast::Trait::cast(node)
            && let (Some(name_node), Some(associated_items)) =
                (trait_item.name(), trait_item.assoc_item_list())
        {
            collect_misplaced_assoc_items(
                &associated_items,
                name_node.text().trim(),
                file,
                &mut out,
            );
        }
    }
    out
}

/// Collects associated `type` and `const` items declared after at least one `fn` item inside a
/// single `AssocItemList`.
fn collect_misplaced_assoc_items<'a>(
    associated_items: &ast::AssocItemList,
    container_name: &str,
    file: &'a ParsedFile,
    out: &mut Vec<RustAssociatedItemAfterMethod<'a>>,
) {
    let mut first_fn: Option<PlacementAnchor> = None;
    for assoc_item in associated_items.assoc_items() {
        if file.is_in_rust_inline_test(assoc_item.syntax().text_range().start().into()) {
            continue;
        }
        let misplaced_name = match assoc_item {
            ast::AssocItem::Fn(fn_item) => {
                if first_fn.is_none()
                    && let Some(fn_name) = fn_item.name()
                {
                    first_fn = Some(PlacementAnchor::before(
                        fn_name.text().trim(),
                        AstNode::from_span(
                            file,
                            span_from_rowan_range(fn_item.syntax().text_range()),
                        ),
                    ));
                }
                None
            }
            ast::AssocItem::TypeAlias(type_alias) if first_fn.is_some() => type_alias.name(),
            ast::AssocItem::Const(const_item) if first_fn.is_some() => const_item.name(),
            _ => None,
        };
        if let (Some(name_node), Some(first_method)) = (misplaced_name, &first_fn) {
            out.push(RustAssociatedItemAfterMethod {
                container_name: container_name.to_owned(),
                name: name_node.text().trim().to_owned(),
                first_method: first_method.clone(),
                node: AstNode::from_span(
                    file,
                    span_from_rowan_range(name_node.syntax().text_range()),
                ),
            });
        }
    }
}

/// Collects [`IntraFileReference`] entries for intra-file references to `module_items` and
/// `method_items` across production items in a Rust `file`.
#[must_use]
pub fn collect_intra_file_ref_sites(
    file: &ParsedFile,
    module_items: &HashSet<&str>,
    method_items: &HashSet<String>,
) -> Vec<IntraFileReference> {
    let Some(parsed) = file.rs_parsed() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let targets = IntraRefTargets {
        module_items,
        method_items,
    };
    let empty_generics = HashSet::new();
    let mut items: Vec<ast::Item> = parsed.tree().items().collect();
    while let Some(item) = items.pop() {
        let start_offset: usize = item.syntax().text_range().start().into();
        if file.is_in_rust_inline_test(start_offset) {
            continue;
        }
        match item {
            ast::Item::Fn(function) => {
                let Some(name_node) = function.name() else {
                    continue;
                };
                let fn_name = name_node.text().trim();
                if module_items.contains(fn_name) {
                    collect_rust_fn_intra_refs(
                        &function,
                        file,
                        targets,
                        fn_name,
                        None,
                        &empty_generics,
                        &mut out,
                    );
                }
            }
            ast::Item::Struct(struct_item) => {
                collect_rust_type_item_intra_refs(
                    struct_item.name(),
                    struct_item.syntax(),
                    file,
                    targets,
                    &mut out,
                );
            }
            ast::Item::Enum(enum_item) => {
                collect_rust_type_item_intra_refs(
                    enum_item.name(),
                    enum_item.syntax(),
                    file,
                    targets,
                    &mut out,
                );
            }
            ast::Item::Union(union_item) => {
                collect_rust_type_item_intra_refs(
                    union_item.name(),
                    union_item.syntax(),
                    file,
                    targets,
                    &mut out,
                );
            }
            ast::Item::Trait(trait_item) => {
                collect_rust_type_item_intra_refs(
                    trait_item.name(),
                    trait_item.syntax(),
                    file,
                    targets,
                    &mut out,
                );
            }
            ast::Item::Impl(impl_item) => {
                collect_rust_impl_intra_refs(&impl_item, file, targets, &mut out);
            }
            ast::Item::Module(module) => {
                if let Some(body) = module.item_list() {
                    items.extend(body.items());
                }
            }
            _ => {}
        }
    }
    out
}

/// The declarations whose references an intra-file reference walk records.
#[derive(Clone, Copy)]
struct IntraRefTargets<'a> {
    /// Names of the file's module-level items.
    module_items: &'a HashSet<&'a str>,
    /// `Type::method` names of the file's methods.
    method_items: &'a HashSet<String>,
}

fn collect_rust_type_item_intra_refs(
    name_node: Option<ast::Name>,
    syntax: &SyntaxNode,
    file: &ParsedFile,
    targets: IntraRefTargets<'_>,
    out: &mut Vec<IntraFileReference>,
) {
    let Some(name_node) = name_node else {
        return;
    };
    let type_name = name_node.text().trim();
    if !targets.module_items.contains(type_name) {
        return;
    }
    let locals = collect_rust_generic_param_names(syntax);
    collect_rust_node_intra_refs(
        syntax,
        file,
        targets,
        IntraRefScope {
            source_symbol: type_name,
            enclosing_self_type: None,
            local_names: &locals,
        },
        RelationKind::Composition,
        out,
    );
}

fn collect_rust_impl_intra_refs(
    impl_item: &ast::Impl,
    file: &ParsedFile,
    targets: IntraRefTargets<'_>,
    out: &mut Vec<IntraFileReference>,
) {
    let Some(self_type_owned) = impl_self_type_name(impl_item, &file.source) else {
        return;
    };
    let self_type = self_type_owned.as_str();
    let self_type_in_module = targets.module_items.contains(self_type);
    let is_inherent = impl_item.trait_().is_none();
    let impl_generics = collect_rust_generic_param_names(impl_item.syntax());
    if self_type_in_module {
        let impl_scope = IntraRefScope {
            source_symbol: self_type,
            enclosing_self_type: Some(self_type),
            local_names: &impl_generics,
        };
        if let Some(trait_type) = impl_item.trait_() {
            collect_rust_node_intra_refs(
                trait_type.syntax(),
                file,
                targets,
                impl_scope,
                RelationKind::Body,
                out,
            );
        }
        if let Some(where_clause) = impl_item.where_clause() {
            collect_rust_node_intra_refs(
                where_clause.syntax(),
                file,
                targets,
                impl_scope,
                RelationKind::Body,
                out,
            );
        }
    }
    let Some(assoc_items) = impl_item.assoc_item_list() else {
        return;
    };
    for assoc_item in assoc_items.assoc_items() {
        if file.is_in_rust_inline_test(assoc_item.syntax().text_range().start().into()) {
            continue;
        }
        match assoc_item {
            ast::AssocItem::Fn(method) => {
                let method_source = method.name().and_then(|name_node| {
                    let candidate = format!("{self_type}::{}", name_node.text().trim());
                    (is_inherent && targets.method_items.contains(&candidate)).then_some(candidate)
                });
                let source_symbol = if let Some(ref candidate) = method_source {
                    candidate.as_str()
                } else if self_type_in_module {
                    self_type
                } else {
                    continue;
                };
                collect_rust_fn_intra_refs(
                    &method,
                    file,
                    targets,
                    source_symbol,
                    Some(self_type),
                    &impl_generics,
                    out,
                );
            }
            other if self_type_in_module => {
                collect_rust_node_intra_refs(
                    other.syntax(),
                    file,
                    targets,
                    IntraRefScope {
                        source_symbol: self_type,
                        enclosing_self_type: Some(self_type),
                        local_names: &impl_generics,
                    },
                    RelationKind::Body,
                    out,
                );
            }
            _ => {}
        }
    }
}

fn collect_rust_fn_intra_refs(
    function: &ast::Fn,
    file: &ParsedFile,
    targets: IntraRefTargets<'_>,
    source_symbol: &str,
    self_type: Option<&str>,
    outer_generics: &HashSet<String>,
    out: &mut Vec<IntraFileReference>,
) {
    let mut generic_names = outer_generics.clone();
    generic_names.extend(collect_rust_generic_param_names(function.syntax()));
    let mut local_names = generic_names.clone();
    local_names.extend(collect_rust_fn_local_names(function));
    let signature_scope = IntraRefScope {
        source_symbol,
        enclosing_self_type: self_type,
        local_names: &generic_names,
    };
    if let Some(params) = function.param_list() {
        collect_rust_node_intra_refs(
            params.syntax(),
            file,
            targets,
            signature_scope,
            RelationKind::Parameter,
            out,
        );
    }
    if let Some(where_clause) = function.where_clause() {
        collect_rust_node_intra_refs(
            where_clause.syntax(),
            file,
            targets,
            signature_scope,
            RelationKind::Parameter,
            out,
        );
    }
    if let Some(ret_type) = function.ret_type() {
        collect_rust_node_intra_refs(
            ret_type.syntax(),
            file,
            targets,
            signature_scope,
            RelationKind::Return,
            out,
        );
    }
    if let Some(body) = function.body() {
        collect_rust_node_intra_refs(
            body.syntax(),
            file,
            targets,
            IntraRefScope {
                source_symbol,
                enclosing_self_type: self_type,
                local_names: &local_names,
            },
            RelationKind::Body,
            out,
        );
    }
}

/// The names in scope where an intra-file reference walk runs.
#[derive(Clone, Copy)]
struct IntraRefScope<'a> {
    /// Enclosing declaration symbol (`"fn_name"`, `"Type"`, or `"Type::method"`).
    source_symbol: &'a str,
    /// The `impl` self type that `Self` and `self.` resolve to, if any.
    enclosing_self_type: Option<&'a str>,
    /// Local bindings and generic parameters that shadow module-level items.
    local_names: &'a HashSet<String>,
}

fn collect_rust_node_intra_refs(
    root: &SyntaxNode,
    file: &ParsedFile,
    targets: IntraRefTargets<'_>,
    scope: IntraRefScope<'_>,
    kind: RelationKind,
    out: &mut Vec<IntraFileReference>,
) {
    let IntraRefTargets {
        module_items,
        method_items,
    } = targets;
    let IntraRefScope {
        source_symbol,
        enclosing_self_type,
        local_names,
    } = scope;
    walk_rust_syntax_refs(root, true, |syntax_ref| match syntax_ref {
        RustSyntaxRef::SelfMethodCall { method, offset } => {
            if let Some(self_type) = enclosing_self_type {
                let candidate = format!("{self_type}::{method}");
                if method_items.contains(&candidate) {
                    out.push(IntraFileReference {
                        source_symbol: source_symbol.to_owned(),
                        target_symbol: candidate,
                        line: file.line_index.line(offset),
                        kind,
                    });
                }
            }
        }
        RustSyntaxRef::Path {
            seg0, seg1, offset, ..
        } => {
            let line = file.line_index.line(offset);
            if seg0 != SELF_TYPE_KEYWORD
                && !local_names.contains(seg0)
                && module_items.contains(seg0)
            {
                out.push(IntraFileReference {
                    source_symbol: source_symbol.to_owned(),
                    target_symbol: seg0.to_owned(),
                    line,
                    kind,
                });
            }
            if let Some(seg1) = seg1 {
                let resolved_seg0 = if seg0 == SELF_TYPE_KEYWORD {
                    enclosing_self_type.unwrap_or(seg0)
                } else {
                    seg0
                };
                let candidate = format!("{resolved_seg0}::{seg1}");
                if method_items.contains(&candidate) {
                    out.push(IntraFileReference {
                        source_symbol: source_symbol.to_owned(),
                        target_symbol: candidate,
                        line,
                        kind,
                    });
                }
            }
        }
    });
}

enum RustSyntaxRef<'a> {
    SelfMethodCall {
        method: &'a str,
        offset: usize,
    },
    Path {
        seg0: &'a str,
        seg1: Option<&'a str>,
        has_extra_segments: bool,
        offset: usize,
    },
}

fn walk_rust_syntax_refs(
    root: &SyntaxNode,
    stop_at_impl: bool,
    mut on_ref: impl FnMut(RustSyntaxRef<'_>),
) {
    let mut stack = vec![root.clone()];
    while let Some(node) = stack.pop() {
        if node != *root
            && (ast::Module::can_cast(node.kind())
                || ast::Use::can_cast(node.kind())
                || (stop_at_impl && ast::Impl::can_cast(node.kind())))
        {
            continue;
        }
        if let Some(method_call) = ast::MethodCallExpr::cast(node.clone()) {
            if method_call
                .receiver()
                .is_some_and(|receiver| receiver.syntax().text() == SELF_KEYWORD)
                && let Some(name_ref) = method_call.name_ref()
            {
                on_ref(RustSyntaxRef::SelfMethodCall {
                    method: name_ref.text().trim(),
                    offset: name_ref.syntax().text_range().start().into(),
                });
            }
        } else if let Some(path) = ast::Path::cast(node.clone())
            && path.parent_path().is_none()
        {
            let segments = rust_path_segments(&path);
            let offset = path.syntax().text_range().start().into();
            match segments.as_slice() {
                [seg0] => on_ref(RustSyntaxRef::Path {
                    seg0: seg0.as_str(),
                    seg1: None,
                    has_extra_segments: false,
                    offset,
                }),
                [seg0, seg1, rest @ ..] => on_ref(RustSyntaxRef::Path {
                    seg0: seg0.as_str(),
                    seg1: Some(seg1.as_str()),
                    has_extra_segments: !rest.is_empty(),
                    offset,
                }),
                [] => {}
            }
        } else if node.kind() == SyntaxKind::TOKEN_TREE {
            walk_token_tree_syntax_refs(&node, &mut on_ref);
            continue;
        }
        for child in node.children() {
            stack.push(child);
        }
    }
}

fn walk_token_tree_syntax_refs(node: &SyntaxNode, on_ref: &mut impl FnMut(RustSyntaxRef<'_>)) {
    for element in node.descendants_with_tokens() {
        let Some(token) = element.into_token() else {
            continue;
        };
        if token.kind() != SyntaxKind::IDENT {
            continue;
        }
        let name = token.text().trim();
        let offset = token.text_range().start().into();
        if let Some(sep) = previous_non_trivia_token(&token)
            && sep.kind() == SyntaxKind::DOT
            && let Some(recv) = previous_non_trivia_token(&sep)
            && recv.text() == SELF_KEYWORD
            && !is_preceded_by_dot_or_double_colon(&recv)
            && is_followed_by_open_paren(&token)
        {
            on_ref(RustSyntaxRef::SelfMethodCall {
                method: name,
                offset,
            });
            continue;
        }
        if let Some(recv) = token_before_double_colon(&token)
            && matches!(recv.kind(), SyntaxKind::IDENT | SyntaxKind::SELF_TYPE_KW)
        {
            let rtext = recv.text().trim();
            let recv_is_local = !is_preceded_by_dot_or_double_colon(&recv)
                || token_before_double_colon(&recv).is_some_and(|qual| {
                    qual.text().trim() == SELF_KEYWORD && !is_preceded_by_dot_or_double_colon(&qual)
                });
            if recv_is_local && rtext != SELF_KEYWORD {
                on_ref(RustSyntaxRef::Path {
                    seg0: rtext,
                    seg1: Some(name),
                    has_extra_segments: false,
                    offset: recv.text_range().start().into(),
                });
                continue;
            }
        }
        let is_self_qualified = token_before_double_colon(&token).is_some_and(|qual| {
            qual.text().trim() == SELF_KEYWORD && !is_preceded_by_dot_or_double_colon(&qual)
        });
        if (!is_preceded_by_dot_or_double_colon(&token) || is_self_qualified)
            && !is_followed_by_single_colon(&token)
        {
            on_ref(RustSyntaxRef::Path {
                seg0: name,
                seg1: None,
                has_extra_segments: false,
                offset,
            });
        }
    }
}

fn rust_path_segments(path: &ast::Path) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = Some(path.clone());
    while let Some(cursor) = current {
        let Some(seg) = cursor.segment() else {
            return Vec::new();
        };
        if seg.type_anchor().is_some() {
            return Vec::new();
        }
        let Some(name_ref) = seg.name_ref() else {
            return Vec::new();
        };
        segments.push(name_ref.text().trim().to_owned());
        current = cursor.qualifier();
    }
    segments.reverse();
    if segments.first().is_some_and(|first| first == SELF_KEYWORD) {
        segments.remove(0);
    }
    segments
}

/// Returns the non-trivia token immediately preceding a `::` separator before `token`, if any.
fn token_before_double_colon(token: &SyntaxToken) -> Option<SyntaxToken> {
    let sep = previous_non_trivia_token(token)?;
    if sep.kind() == SyntaxKind::COLON2 {
        return previous_non_trivia_token(&sep);
    }
    if sep.kind() == SyntaxKind::COLON {
        let first_colon = previous_non_trivia_token(&sep)?;
        if first_colon.kind() == SyntaxKind::COLON
            && first_colon.text_range().end() == sep.text_range().start()
        {
            return previous_non_trivia_token(&first_colon);
        }
    }
    None
}

/// Returns true if `token` is immediately preceded (ignoring trivia) by `.` or `::`.
fn is_preceded_by_dot_or_double_colon(token: &SyntaxToken) -> bool {
    let Some(prev) = previous_non_trivia_token(token) else {
        return false;
    };
    match prev.kind() {
        SyntaxKind::DOT | SyntaxKind::COLON2 => true,
        SyntaxKind::COLON => previous_non_trivia_token(&prev).is_some_and(|first| {
            first.kind() == SyntaxKind::COLON
                && first.text_range().end() == prev.text_range().start()
        }),
        _ => false,
    }
}

/// Returns true if `token` is followed (ignoring trivia) by a single `:` (such as a struct field
/// label `Foo { field: val }`) rather than `::`.
fn is_followed_by_single_colon(token: &SyntaxToken) -> bool {
    let Some(next) = std::iter::successors(token.next_token(), SyntaxToken::next_token)
        .find(|candidate| !candidate.kind().is_trivia())
    else {
        return false;
    };
    if next.kind() != SyntaxKind::COLON {
        return false;
    }
    next.next_token()
        .is_none_or(|after| after.kind() != SyntaxKind::COLON)
}

/// Extracts the terminal type name of `impl_item.self_ty()` (or its trimmed source text when
/// the self type is not a path type).
fn impl_self_type_name(impl_item: &ast::Impl, source: &str) -> Option<String> {
    let self_type = impl_item.self_ty()?;
    if let ast::Type::PathType(path_type) = &self_type
        && let Some(path) = path_type.path()
        && let Some(segment) = path.segment()
        && let Some(name_ref) = segment.name_ref()
    {
        return Some(name_ref.text().trim().to_owned());
    }
    let span = span_from_rowan_range(self_type.syntax().text_range());
    Some(source[span.start..span.end].trim().to_owned())
}

/// Returns true if the next non-trivia token after `token` is `(`. Inside a macro token tree,
/// `self.name` followed by `(` is a method call; without it, `name` is a field.
fn is_followed_by_open_paren(token: &SyntaxToken) -> bool {
    std::iter::successors(token.next_token(), SyntaxToken::next_token)
        .find(|candidate| !candidate.kind().is_trivia())
        .is_some_and(|next| next.kind() == SyntaxKind::L_PAREN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_impl_declaration_scopes_rust() {
        use Visibility::{Private, Public};

        let source = indoc::indoc! {r"
            impl<'a> Client<'a> {
                pub fn new() -> Self { todo!() }
                pub(crate) fn try_new_from_env() -> Self { todo!() }
                pub fn new_session(&self) -> Self { todo!() }
                fn new_internal() -> Self { todo!() }
            }

            impl Default for Client<'_> {
                fn default() -> Self { todo!() }
            }
        "};
        let file = ParsedFile::rust(source);
        let scopes = collect_declaration_scopes(&file);
        let summary: Vec<_> = scopes
            .iter()
            .filter_map(|scope| {
                let DeclarationScopeKind::Impl { type_name } = &scope.kind else {
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
                Some((type_name.as_str(), methods))
            })
            .collect();
        assert_eq!(
            summary,
            vec![(
                "Client",
                vec![
                    ("new", Public, true),
                    ("try_new_from_env", Public, true),
                    ("new_session", Public, false),
                    ("new_internal", Private, false),
                ],
            )]
        );
    }

    #[test]
    fn test_collect_associated_items_after_methods_rust() {
        let source = indoc::indoc! {r"
            trait Codec {
                type Input;
                fn decode(&self);
                type Output;
                const VERSION: u32 = 1;
            }

            impl Codec for JsonCodec {
                type Input = String;
                fn decode(&self) {}
                type Output = Vec<u8>;
            }
        "};
        let file = ParsedFile::rust(source);
        let items: Vec<(String, String, String, PlacementAnchor)> =
            collect_associated_items_after_methods(&file)
                .into_iter()
                .map(|item| {
                    (
                        item.container_name,
                        item.name,
                        item.node.text().into_owned(),
                        item.first_method,
                    )
                })
                .collect();
        assert_eq!(
            items,
            vec![
                (
                    "Codec".to_owned(),
                    "Output".to_owned(),
                    "Output".to_owned(),
                    PlacementAnchor {
                        name: "decode".to_owned(),
                        line: 3,
                    },
                ),
                (
                    "Codec".to_owned(),
                    "VERSION".to_owned(),
                    "VERSION".to_owned(),
                    PlacementAnchor {
                        name: "decode".to_owned(),
                        line: 3,
                    },
                ),
                (
                    "JsonCodec".to_owned(),
                    "Output".to_owned(),
                    "Output".to_owned(),
                    PlacementAnchor {
                        name: "decode".to_owned(),
                        line: 10,
                    },
                ),
            ]
        );
    }

    #[test]
    fn test_associated_item_after_method_placement_anchor() {
        let associated_file = ParsedFile::rust(indoc::indoc! {r"
            pub trait Decoder {
                /// Decodes an input frame.
                fn decode(&self, input: &str) -> Self::Output;

                type Output;
            }
        "});
        let associated_items = collect_associated_items_after_methods(&associated_file);
        assert_eq!(associated_items.len(), 1);
        assert_eq!(
            associated_items[0].first_method,
            PlacementAnchor {
                name: "decode".to_string(),
                line: 2,
            }
        );
    }

    #[test]
    fn test_rust_intra_refs_resolve_macro_colon_paths_and_shadow_impl_generics() {
        let file = ParsedFile::rust(indoc::indoc! {r"
            pub struct Item;
            pub struct Engine;

            pub fn helper() -> usize {
                1
            }

            impl Engine {
                pub fn step() -> usize {
                    2
                }
            }

            impl<Item> Wrapper<Item> {
                pub fn run(&self, value: Item) -> usize {
                    assert_eq!(Engine::step(), OtherMod::helper());
                    let _ = vec![Record { helper: 1 }];
                    0
                }
            }

            pub struct Wrapper<T>(T);
        "});
        let scopes = crate::code_lint::ast::collect_scope_facts(&file);
        let refs = crate::code_lint::ast::collect_intra_file_references(&file, &scopes);
        let from_run: Vec<&str> = refs
            .iter()
            .filter(|edge| edge.source_symbol == "Wrapper::run")
            .map(|edge| edge.target_symbol.as_str())
            .collect();
        assert_eq!(from_run, vec!["Engine", "Engine::step"]);
    }

    #[test]
    fn test_generic_parameter_shadowing_an_external_impl_type_does_not_reach_its_impl_callees() {
        let file = ParsedFile::rust(indoc::indoc! {r"
            impl Marker {
                fn new() -> Self {
                    make()
                }
            }

            fn make() -> usize {
                1
            }

            pub fn build<Marker: Default>() -> Marker {
                Marker::default()
            }
        "});
        let module_scope = crate::code_lint::ast::collect_scope_facts(&file).remove(0);
        let index_of = |name: &str| {
            module_scope
                .items
                .iter()
                .position(|item| item.name == name)
                .expect("item exists")
        };
        let (build, make) = (index_of("build"), index_of("make"));

        assert!(
            !module_scope
                .relations
                .iter()
                .any(|relation| relation.source == build && relation.target == make)
        );
    }
}
