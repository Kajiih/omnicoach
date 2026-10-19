//! Rust declaration-order scope builders and associated-item placement extraction (`ra_ap_syntax`).

use super::bindings::{collect_rust_fn_local_names, collect_rust_generic_param_names};
use super::names::{SELF_KEYWORD, SELF_TYPE_KEYWORD};
use super::trivia::previous_non_trivia_token;
use crate::code_lint::ast::{
    AstNode, DeclarationScope, DeclarationScopeKind, ParsedFile, PlacementAnchor, RelationKind,
    ScopeItem, ScopeItemKind, Visibility, collapse_whitespace, extend_references, extend_unique,
    push_unique, span_from_rowan_range, split_at_redefinitions,
};
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
pub(in crate::code_lint::ast) fn collect_declaration_scopes(
    file: &ParsedFile,
) -> Vec<DeclarationScope<'_>> {
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
        pinned_before: None,
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
    let mut stack = vec![root.clone()];
    while let Some(node) = stack.pop() {
        if node != *root && (ast::Module::can_cast(node.kind()) || ast::Use::can_cast(node.kind()))
        {
            continue;
        }
        if let Some(path) = ast::Path::cast(node.clone()) {
            let is_module_local = path
                .qualifier()
                .is_none_or(|qualifier| qualifier.syntax().text() == SELF_KEYWORD);
            if is_module_local
                && let Some(segment) = path.segment()
                && let Some(name_ref) = segment.name_ref()
            {
                let ident = name_ref.text();
                let name = ident.trim();
                if !local_names.contains(name)
                    && let Some(&idx) = item_by_name.get(name)
                    && (path.parent_path().is_none() || segment_is_type[idx])
                {
                    push_unique(out, idx);
                }
                if let Some(bridged) = impl_callees.get(name) {
                    extend_unique(out, bridged.iter().copied());
                }
            }
        } else if node.kind() == SyntaxKind::TOKEN_TREE {
            for element in node.descendants_with_tokens() {
                let Some(token) = element.into_token() else {
                    continue;
                };
                if token.kind() != SyntaxKind::IDENT {
                    continue;
                }
                let name = token.text();
                if let Some(bridged) = impl_callees.get(name) {
                    extend_unique(out, bridged.iter().copied());
                }
                if let Some(&idx) = item_by_name.get(name)
                    && !local_names.contains(name)
                {
                    let preceded_by_dot_or_colon =
                        previous_non_trivia_token(&token).is_some_and(|prev| {
                            matches!(prev.kind(), SyntaxKind::DOT | SyntaxKind::COLON2)
                        });
                    if !preceded_by_dot_or_colon {
                        push_unique(out, idx);
                    }
                }
            }
            continue;
        }
        for child in node.children() {
            stack.push(child);
        }
    }
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
    let mut stack = vec![root.clone()];
    while let Some(node) = stack.pop() {
        if ast::Impl::can_cast(node.kind()) {
            continue;
        }
        if let Some(method_call) = ast::MethodCallExpr::cast(node.clone()) {
            if method_call
                .receiver()
                .is_some_and(|receiver| receiver.syntax().text() == SELF_KEYWORD)
                && let Some(name_ref) = method_call.name_ref()
                && let Some(&idx) = method_by_name.get(name_ref.text().trim())
            {
                push_unique(out, idx);
            }
        } else if let Some(path_expr) = ast::PathExpr::cast(node.clone())
            && let Some(path) = path_expr.path()
            && let Some(qualifier) = path.qualifier()
            && qualifier.qualifier().is_none()
        {
            let qualifier_matches_self = qualifier.syntax().text() == SELF_TYPE_KEYWORD
                || qualifier.syntax().text() == type_name;
            if qualifier_matches_self
                && let Some(segment) = path.segment()
                && let Some(name_ref) = segment.name_ref()
                && let Some(&idx) = method_by_name.get(name_ref.text().trim())
            {
                push_unique(out, idx);
            }
        } else if node.kind() == SyntaxKind::TOKEN_TREE {
            for element in node.descendants_with_tokens() {
                let Some(token) = element.into_token() else {
                    continue;
                };
                if token.kind() != SyntaxKind::IDENT {
                    continue;
                }
                let Some(&idx) = method_by_name.get(token.text()) else {
                    continue;
                };
                if let Some(sep) = previous_non_trivia_token(&token)
                    && let Some(recv) = previous_non_trivia_token(&sep)
                {
                    let is_self_call = (sep.kind() == SyntaxKind::DOT
                        && recv.text() == SELF_KEYWORD
                        && is_followed_by_open_paren(&token))
                        || (sep.kind() == SyntaxKind::COLON2
                            && (recv.text() == SELF_TYPE_KEYWORD || recv.text() == type_name));
                    if is_self_call {
                        push_unique(out, idx);
                    }
                }
            }
            continue;
        }
        for child in node.children() {
            stack.push(child);
        }
    }
}

/// Returns true if the next non-trivia token after `token` is `(`. Inside a macro token tree,
/// `self.name` followed by `(` is a method call; without it, `name` is a field.
fn is_followed_by_open_paren(token: &SyntaxToken) -> bool {
    std::iter::successors(token.next_token(), SyntaxToken::next_token)
        .find(|candidate| !candidate.kind().is_trivia())
        .is_some_and(|next| next.kind() == SyntaxKind::L_PAREN)
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
                pinned_before: None,
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
}
