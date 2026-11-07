//! Structural and dependency summary of production Rust source files (`ra_ap_syntax`).

use super::attributes::attribute_terminal_name;
use super::names::{
    SELF_KEYWORD, compact_path_text, is_path_segment_token_kind, macro_call_terminal_name,
};
use super::test_analysis::has_outer_test_attribute;
use super::tree::extract_macro_arguments;
use super::trivia::is_rust_comment_kind;
use crate::code_lint::ast::imports::{ResolvedName, resolve_name};
use crate::code_lint::ast::tree::{AstNode, ParsedFile, span_from_rowan_range};
use crate::diagnostic::SourceSpan;
use ra_ap_syntax::ast::{
    self, HasAttrs as _, HasGenericArgs as _, HasModuleItem as _, HasName as _, HasVisibility as _,
};
use ra_ap_syntax::{AstNode as _, SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};
use std::collections::HashSet;

const ARCHITECTURE_COMPONENT_MACRO: &str = "architecture_component";
const DEFINE_ARCHITECTURE_MACRO: &str = "define_architecture";

/// Visibility and name of a top-level external `mod <name>;` declaration in a Rust file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalModDeclaration {
    /// Module identifier (e.g. `"rust"` in `pub mod rust;`).
    pub name: String,
    /// Full trimmed source text of the declaration (e.g. `"pub mod rust;"`).
    pub declaration_text: String,
    /// True when the declaration has no `visibility_modifier` (`mod child;`).
    pub is_private: bool,
    /// Byte span of the declaration (excluding leading attributes and doc comments).
    pub span: SourceSpan,
}

/// A referenced path in production Rust code (from a `use` tree or inline qualified path).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustPathReference {
    /// 1-indexed starting line number of the reference or enclosing `use` declaration.
    pub line: usize,
    /// Byte span of the reference or enclosing `use` declaration.
    pub span: SourceSpan,
    /// Raw syntactic path segments joined by `::` (e.g. `"crate::code_lint::ast::ParsedFile"`,
    /// `"super::AstNode"`, `"self::rust::collect_bindings"`, `"ra_ap_syntax::SyntaxNode"`).
    pub raw_path: String,
    /// Trimmed source text of the enclosing `use` declaration or inline path node for diagnostics.
    pub statement_text: String,
    /// True when this reference originates from a glob `use ...::*;` tree.
    pub is_glob: bool,
}

/// A visible `use` declaration (`pub use ...`, `pub(crate) use ...`) in production Rust code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleUseDeclaration {
    /// 1-indexed starting line number.
    pub line: usize,
    /// Byte span of the `use` declaration (excluding leading attributes and doc comments).
    pub span: SourceSpan,
    /// Full trimmed source text (e.g. `"pub use self::child::Item;"`).
    pub declaration_text: String,
    /// Expanded target paths imported by this `use` tree (e.g. `["self::child::Item"]`).
    pub target_paths: Vec<String>,
}

/// Component entry extracted from a `define_architecture! { ... }` macro invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinedComponentEntry {
    /// Component variant identifier (e.g. `"CodeLintAst"`).
    pub name: String,
    /// Directly permitted dependency component names (`=> [Diagnostic]`).
    pub depends_on: Vec<String>,
    /// External crates encapsulated by this component (`(owns_external: ["ra_ap_syntax"])`).
    pub owns_external: Vec<String>,
}

/// Structural and dependency summary of the production code in a Rust source file,
/// extracted in a single CST pass while skipping `#[cfg(test)]` and `#[test]` subtrees.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RustFileSummary {
    /// `(span, argument)` pairs of `architecture_component!(...)` macro invocations in production code.
    pub architecture_components: Vec<(SourceSpan, String)>,
    /// Component entries extracted from `define_architecture! { ... }` macro invocations in production code.
    pub defined_architecture: Vec<DefinedComponentEntry>,
    /// Top-level external `mod <name>;` declarations (`mod foo;`, `pub mod foo;`).
    pub external_mods: Vec<ExternalModDeclaration>,
    /// Names of top-level private inline modules (`mod foo { ... }`).
    pub private_inline_mods: Vec<String>,
    /// `(span, item_text)` of top-level production items that are neither external `mod` declarations
    /// nor `macro_rules!` definitions.
    pub non_namespace_items: Vec<(SourceSpan, String)>,
    /// `(span, item_text)` of top-level production `macro_rules!` definitions (`macro_definition`).
    pub macro_definitions: Vec<(SourceSpan, String)>,
    /// Byte spans of all `#[macro_export]` attributes in production code.
    pub macro_exports: Vec<SourceSpan>,
    /// Visible `use` declarations (`pub use`, `pub(crate) use`, etc.) excluding `pub(crate) use <local_macro>;`.
    pub visible_uses: Vec<VisibleUseDeclaration>,
    /// All module/item paths referenced in production code (`use` trees, inline qualified paths,
    /// `use`-imported path usages, and `::`-qualified paths inside macro `token_tree` arguments).
    pub referenced_paths: Vec<RustPathReference>,
}

/// Extracts a complete structural and dependency summary of the production code in `file`
/// in a single CST pass, skipping `#[cfg(test)]` and `#[test]` items.
#[must_use]
pub fn summarize_rust_file(file: &ParsedFile) -> RustFileSummary {
    let Some(parsed) = file.rs_parsed() else {
        return RustFileSummary::default();
    };
    let source_file = parsed.tree();
    let mut summary = RustFileSummary::default();
    let mut defined_macros = HashSet::new();

    summarize_top_level_items(&source_file, file, &mut defined_macros, &mut summary, false);

    for child in source_file.syntax().children() {
        summarize_rust_node(&child, file, &defined_macros, &mut summary);
    }

    summary
}

/// Extracts a body-free top-level structural summary of `file`.
///
/// Records component declarations, `define_architecture!`, external `mod` declarations, `use`
/// declarations, and top-level item headers without walking function, impl, or expression bodies.
#[must_use]
pub fn summarize_rust_module_header(file: &ParsedFile) -> RustFileSummary {
    let Some(parsed) = file.rs_parsed() else {
        return RustFileSummary::default();
    };
    let source_file = parsed.tree();
    let mut summary = RustFileSummary::default();
    let mut defined_macros = HashSet::new();

    summarize_top_level_items(&source_file, file, &mut defined_macros, &mut summary, true);

    summary
}

fn summarize_rust_node(
    node: &SyntaxNode,
    file: &ParsedFile,
    defined_macros: &HashSet<String>,
    summary: &mut RustFileSummary,
) {
    if has_outer_test_attribute(node) {
        return;
    }
    let start_offset: usize = node.text_range().start().into();

    if let Some(attribute) = ast::Attr::cast(node.clone()) {
        if attribute.kind() == ast::AttrKind::Outer
            && attribute_terminal_name(&attribute)
                .is_some_and(|terminal| terminal.text() == "macro_export")
        {
            summary
                .macro_exports
                .push(span_from_rowan_range(attribute.syntax().text_range()));
        }
        for child in node.children() {
            if child.kind() != SyntaxKind::TOKEN_TREE {
                summarize_rust_node(&child, file, defined_macros, summary);
            }
        }
        return;
    }

    if let Some(use_item) = ast::Use::cast(node.clone()) {
        for attribute in use_item.attrs() {
            summarize_rust_node(attribute.syntax(), file, defined_macros, summary);
        }
        summarize_use_item(&use_item, file, defined_macros, summary);
        return;
    }

    if let Some(macro_rules) = ast::MacroRules::cast(node.clone()) {
        for attribute in macro_rules.attrs() {
            summarize_rust_node(attribute.syntax(), file, defined_macros, summary);
        }
        return;
    }

    if let Some(macro_call) = ast::MacroCall::cast(node.clone()) {
        let terminal_name = macro_call_terminal_name(&macro_call);
        if terminal_name == ARCHITECTURE_COMPONENT_MACRO {
            record_architecture_component(&macro_call, file, &mut summary.architecture_components);
        }
        if let Some(token_tree) = macro_call.token_tree() {
            if terminal_name == DEFINE_ARCHITECTURE_MACRO {
                collect_defined_architecture(&token_tree, &mut summary.defined_architecture);
            } else if token_tree.syntax().text().contains_char(':') {
                let statement_text = item_line_and_text(file, node).text;
                collect_token_tree_paths(
                    &token_tree,
                    file,
                    &statement_text,
                    &mut summary.referenced_paths,
                );
            }
        }
    }

    if let Some(path) = ast::Path::cast(node.clone()) {
        if has_pure_qualifiers(&path) {
            summarize_qualified_path(&path, file, defined_macros, summary);
            return;
        }
        if path.qualifier().is_none()
            && path.parent_path().is_none()
            && let Some(segment) = path.segment()
            && let Some(name_ref) = segment.name_ref()
        {
            let ident = name_ref.text();
            let trimmed = ident.trim();
            if let ResolvedName::Imported(imported) = resolve_name(file, trimmed) {
                summary.referenced_paths.push(RustPathReference {
                    line: file.line_index.line(start_offset),
                    span: span_from_rowan_range(name_ref.syntax().text_range()),
                    raw_path: imported,
                    statement_text: trimmed.to_string(),
                    is_glob: false,
                });
            }
        }
    }

    for child in node.children() {
        if !matches!(
            child.kind(),
            SyntaxKind::TOKEN_TREE | SyntaxKind::VISIBILITY
        ) {
            summarize_rust_node(&child, file, defined_macros, summary);
        }
    }
}

fn summarize_top_level_items(
    source_file: &ast::SourceFile,
    file: &ParsedFile,
    defined_macros: &mut HashSet<String>,
    summary: &mut RustFileSummary,
    include_header_macros_and_uses: bool,
) {
    if include_header_macros_and_uses {
        for item in source_file.items() {
            if has_outer_test_attribute(item.syntax()) {
                continue;
            }
            if let ast::Item::MacroRules(macro_rules) = item
                && let Some(name_node) = macro_rules.name()
            {
                defined_macros.insert(name_node.text().to_string());
            }
        }
    }
    for item in source_file.items() {
        if has_outer_test_attribute(item.syntax()) {
            continue;
        }
        let ItemDeclaration {
            span: item_span,
            text: declaration_text,
            ..
        } = item_line_and_text(file, item.syntax());
        match &item {
            ast::Item::Module(module) if module.item_list().is_none() => {
                if let Some(name_node) = module.name() {
                    let is_private = module.visibility().is_none();
                    summary.external_mods.push(ExternalModDeclaration {
                        name: name_node.text().trim().to_string(),
                        declaration_text,
                        is_private,
                        span: item_span,
                    });
                }
            }
            ast::Item::MacroRules(macro_rules) => {
                if let Some(name_node) = macro_rules.name() {
                    defined_macros.insert(name_node.text().to_string());
                }
                summary
                    .macro_definitions
                    .push((item_span, declaration_text));
            }
            ast::Item::MacroCall(macro_call) => {
                let terminal_name = macro_call_terminal_name(macro_call);
                if terminal_name == ARCHITECTURE_COMPONENT_MACRO {
                    if include_header_macros_and_uses {
                        record_architecture_component(
                            macro_call,
                            file,
                            &mut summary.architecture_components,
                        );
                    }
                } else if terminal_name == DEFINE_ARCHITECTURE_MACRO {
                    if include_header_macros_and_uses
                        && let Some(token_tree) = macro_call.token_tree()
                    {
                        collect_defined_architecture(
                            &token_tree,
                            &mut summary.defined_architecture,
                        );
                    }
                } else {
                    summary
                        .non_namespace_items
                        .push((item_span, declaration_text));
                }
            }
            ast::Item::Use(use_item) => {
                summary
                    .non_namespace_items
                    .push((item_span, declaration_text));
                if include_header_macros_and_uses {
                    summarize_use_item(use_item, file, defined_macros, summary);
                }
            }
            ast::Item::Module(module) => {
                if module.visibility().is_none()
                    && let Some(name_node) = module.name()
                {
                    summary
                        .private_inline_mods
                        .push(name_node.text().trim().to_string());
                }
                summary
                    .non_namespace_items
                    .push((item_span, declaration_text));
            }
            _ => {
                summary
                    .non_namespace_items
                    .push((item_span, declaration_text));
            }
        }
    }
}

fn record_architecture_component(
    macro_call: &ast::MacroCall,
    file: &ParsedFile,
    architecture_components: &mut Vec<(SourceSpan, String)>,
) {
    let macro_span = span_from_rowan_range(macro_call.syntax().text_range());
    let macro_ast_node = AstNode::from_span(file, macro_span);
    let component_argument: String = extract_macro_arguments(&macro_ast_node)
        .into_iter()
        .map(|argument| argument.text().into_owned())
        .collect();
    architecture_components.push((macro_span, component_argument));
}

fn summarize_use_item(
    use_item: &ast::Use,
    file: &ParsedFile,
    defined_macros: &HashSet<String>,
    summary: &mut RustFileSummary,
) {
    let ItemDeclaration {
        line,
        span,
        text: declaration_text,
    } = item_line_and_text(file, use_item.syntax());
    let mut expanded_targets = Vec::new();
    if let Some(use_tree) = use_item.use_tree() {
        expand_use_tree(&use_tree, "", &mut expanded_targets);
    }
    for (raw_path, is_glob) in &expanded_targets {
        summary.referenced_paths.push(RustPathReference {
            line,
            span,
            raw_path: raw_path.clone(),
            statement_text: declaration_text.clone(),
            is_glob: *is_glob,
        });
    }
    let target_paths: Vec<String> = expanded_targets.into_iter().map(|(path, _)| path).collect();
    if let Some(visibility) = use_item.visibility() {
        let is_own_macro_path = visibility.syntax().text().to_string().trim() == "pub(crate)"
            && matches!(
                target_paths.as_slice(),
                [target] if defined_macros.contains(target.as_str())
            );
        if !is_own_macro_path {
            summary.visible_uses.push(VisibleUseDeclaration {
                line,
                span,
                declaration_text,
                target_paths,
            });
        }
    }
}

fn expand_use_tree(use_tree: &ast::UseTree, prefix: &str, out: &mut Vec<(String, bool)>) {
    if let Some(sub_trees) = use_tree.use_tree_list() {
        let next_prefix = use_tree.path().map_or_else(
            || prefix.to_string(),
            |path| join_use_prefix(prefix, &path.syntax().text().to_string()),
        );
        for child_tree in sub_trees.use_trees() {
            expand_use_tree(&child_tree, &next_prefix, out);
        }
        return;
    }
    if use_tree.star_token().is_some() {
        if let Some(path) = use_tree.path() {
            out.push((
                join_use_prefix(prefix, &path.syntax().text().to_string()),
                true,
            ));
        } else if !prefix.is_empty() {
            out.push((prefix.to_string(), true));
        }
        return;
    }
    if let Some(path) = use_tree.path() {
        let path_text = path.syntax().text().to_string();
        if compact_path_text(&path_text) != "_" {
            out.push((join_use_prefix(prefix, &path_text), false));
        }
    }
}

fn join_use_prefix(prefix: &str, segment: &str) -> String {
    let clean = compact_path_text(segment);
    if prefix.is_empty() {
        clean
    } else if clean == SELF_KEYWORD {
        prefix.to_string()
    } else if let Some(rest) = clean.strip_prefix("self::") {
        format!("{prefix}::{rest}")
    } else {
        format!("{prefix}::{clean}")
    }
}

fn summarize_qualified_path(
    path: &ast::Path,
    file: &ParsedFile,
    defined_macros: &HashSet<String>,
    summary: &mut RustFileSummary,
) {
    let parent_is_pure = path
        .syntax()
        .parent()
        .and_then(ast::Path::cast)
        .is_some_and(|parent_path| has_pure_qualifiers(&parent_path));
    if !parent_is_pure
        && let Some(segment) = path.segment()
        && let Some(name_ref) = segment.name_ref()
    {
        let start_offset: usize = path.syntax().text_range().start().into();
        let end_offset: usize = name_ref.syntax().text_range().end().into();
        let slice_text = file.source[start_offset..end_offset].trim();
        let compact = compact_path_text(slice_text);
        let raw_path = match resolve_name(file, &compact) {
            ResolvedName::Imported(imported) => imported,
            ResolvedName::Local | ResolvedName::Unbound => compact,
        };
        summary.referenced_paths.push(RustPathReference {
            line: file.line_index.line(start_offset),
            span: SourceSpan::new(start_offset, end_offset),
            raw_path,
            statement_text: slice_text.to_string(),
            is_glob: false,
        });
        if let Some(generic_args) = segment.generic_arg_list() {
            summarize_rust_node(generic_args.syntax(), file, defined_macros, summary);
        }
        if let Some(paren_args) = segment.parenthesized_arg_list() {
            summarize_rust_node(paren_args.syntax(), file, defined_macros, summary);
        }
        if let Some(ret_type) = segment.ret_type() {
            summarize_rust_node(ret_type.syntax(), file, defined_macros, summary);
        }
    }
}

fn has_pure_qualifiers(path: &ast::Path) -> bool {
    let Some(qualifier) = path.qualifier() else {
        return false;
    };
    let mut current = Some(qualifier);
    while let Some(qual_path) = current {
        let Some(segment) = qual_path.segment() else {
            return false;
        };
        if !is_simple_path_segment(&segment) {
            return false;
        }
        current = qual_path.qualifier();
    }
    path.segment()
        .is_some_and(|segment| segment.name_ref().is_some() && segment.type_anchor().is_none())
}

fn is_simple_path_segment(segment: &ast::PathSegment) -> bool {
    segment.name_ref().is_some()
        && segment.type_anchor().is_none()
        && segment.generic_arg_list().is_none()
        && segment.parenthesized_arg_list().is_none()
        && segment.ret_type().is_none()
}

/// The declaration of an item, after its leading outer attributes and doc comments.
struct ItemDeclaration {
    /// 1-indexed starting line number.
    line: usize,
    /// Byte span after leading outer attributes and doc comments.
    span: SourceSpan,
    /// Trimmed source text.
    text: String,
}

/// Returns the declaration of `node`, starting after any leading outer attributes, doc comments,
/// and whitespace.
fn item_line_and_text(file: &ParsedFile, node: &SyntaxNode) -> ItemDeclaration {
    let start_offset: usize = node
        .children_with_tokens()
        .find(|element| {
            let kind = element.kind();
            kind != SyntaxKind::ATTR
                && kind != SyntaxKind::WHITESPACE
                && !is_rust_comment_kind(kind)
        })
        .map_or_else(
            || node.text_range().start().into(),
            |element| element.text_range().start().into(),
        );
    let end_offset: usize = node.text_range().end().into();
    let line = file.line_index.line(start_offset);
    let span = SourceSpan::new(start_offset, end_offset);
    let text = file.span_text(span).trim().to_string();
    ItemDeclaration { line, span, text }
}

fn collect_defined_architecture(token_tree: &ast::TokenTree, out: &mut Vec<DefinedComponentEntry>) {
    let mut last_ident: Option<String> = None;
    for element in token_tree.syntax().children_with_tokens() {
        match element {
            SyntaxElement::Token(token) => match token.kind() {
                SyntaxKind::IDENT => last_ident = Some(token.text().to_string()),
                SyntaxKind::POUND => last_ident = None,
                _ => {}
            },
            SyntaxElement::Node(node) if node.kind() == SyntaxKind::TOKEN_TREE => {
                let first_kind = node.first_token().map(|tok| tok.kind());
                if first_kind == Some(SyntaxKind::L_BRACK)
                    && let Some(name) = last_ident.take()
                {
                    let depends_on = node
                        .children_with_tokens()
                        .filter_map(SyntaxElement::into_token)
                        .filter(|tok| tok.kind() == SyntaxKind::IDENT)
                        .map(|tok| tok.text().to_string())
                        .collect();
                    out.push(DefinedComponentEntry {
                        name,
                        depends_on,
                        owns_external: Vec::new(),
                    });
                } else if first_kind == Some(SyntaxKind::L_PAREN)
                    && node
                        .descendants_with_tokens()
                        .filter_map(SyntaxElement::into_token)
                        .find(|tok| tok.kind() == SyntaxKind::IDENT)
                        .is_some_and(|tok| tok.text() == "owns_external")
                    && let Some(last_entry) = out.last_mut()
                {
                    last_entry.owns_external = extract_string_literals_in_subtree(&node);
                }
            }
            SyntaxElement::Node(_) => {}
        }
    }
}

fn extract_string_literals_in_subtree(node: &SyntaxNode) -> Vec<String> {
    node.descendants_with_tokens()
        .filter_map(SyntaxElement::into_token)
        .filter(|tok| tok.kind() == SyntaxKind::STRING)
        .map(|tok| tok.text().trim_matches('"').to_string())
        .collect()
}

fn collect_token_tree_paths(
    token_tree: &ast::TokenTree,
    file: &ParsedFile,
    statement_text: &str,
    out: &mut Vec<RustPathReference>,
) {
    let elements: Vec<SyntaxElement> = token_tree
        .syntax()
        .children_with_tokens()
        .filter(
            |element| !matches!(element, SyntaxElement::Token(token) if token.kind().is_trivia()),
        )
        .collect();
    let mut index = 0;
    while index < elements.len() {
        if let SyntaxElement::Node(child_node) = &elements[index]
            && let Some(nested_tree) = ast::TokenTree::cast(child_node.clone())
        {
            collect_token_tree_paths(&nested_tree, file, statement_text, out);
            index += 1;
            continue;
        }
        if let Some(first_token) = token_segment_at(&elements, index)
            && let Some(after_colons) = consume_double_colon(&elements, index + 1)
            && token_segment_at(&elements, after_colons).is_some()
        {
            let start_offset: usize = first_token.text_range().start().into();
            let line = file.line_index.line(start_offset);
            let mut end_offset: usize = first_token.text_range().end().into();
            let mut segments = vec![first_token.text().trim().to_string()];
            let mut cursor = index + 1;
            while let Some(next_segment_idx) = consume_double_colon(&elements, cursor)
                && let Some(segment_token) = token_segment_at(&elements, next_segment_idx)
            {
                end_offset = segment_token.text_range().end().into();
                segments.push(segment_token.text().trim().to_string());
                cursor = next_segment_idx + 1;
            }
            let joined = segments.join("::");
            let raw_path = match resolve_name(file, &joined) {
                ResolvedName::Imported(imported) => imported,
                ResolvedName::Local | ResolvedName::Unbound => joined,
            };
            out.push(RustPathReference {
                line,
                span: SourceSpan::new(start_offset, end_offset),
                raw_path,
                statement_text: statement_text.to_string(),
                is_glob: false,
            });
            index = cursor;
            continue;
        }
        if let Some(SyntaxElement::Token(token)) = elements.get(index)
            && token.kind() == SyntaxKind::IDENT
        {
            let preceded_by_sep = (index > 0
                && matches!(
                    elements.get(index - 1),
                    Some(SyntaxElement::Token(prev))
                        if matches!(prev.kind(), SyntaxKind::DOT | SyntaxKind::COLON2)
                ))
                || (index >= 2 && consume_double_colon(&elements, index - 2) == Some(index));
            let followed_by_single_colon = matches!(
                elements.get(index + 1),
                Some(SyntaxElement::Token(next)) if next.kind() == SyntaxKind::COLON
            ) && !matches!(
                elements.get(index + 2),
                Some(SyntaxElement::Token(next2)) if next2.kind() == SyntaxKind::COLON
            );
            if !preceded_by_sep && !followed_by_single_colon {
                let ident = token.text().trim();
                if let ResolvedName::Imported(imported) = resolve_name(file, ident) {
                    out.push(RustPathReference {
                        line: file.line_index.line(token.text_range().start().into()),
                        span: span_from_rowan_range(token.text_range()),
                        raw_path: imported,
                        statement_text: statement_text.to_string(),
                        is_glob: false,
                    });
                }
            }
        }
        index += 1;
    }
}

fn token_segment_at(elements: &[SyntaxElement], index: usize) -> Option<&SyntaxToken> {
    match elements.get(index)? {
        SyntaxElement::Token(token) if is_path_segment_token_kind(token.kind()) => Some(token),
        _ => None,
    }
}

/// Checks if `elements[index..]` begins with `::` (either `COLON2` or two adjacent `COLON` tokens),
/// returning the index immediately after `::`.
fn consume_double_colon(elements: &[SyntaxElement], index: usize) -> Option<usize> {
    match elements.get(index)? {
        SyntaxElement::Token(token) if token.kind() == SyntaxKind::COLON2 => Some(index + 1),
        SyntaxElement::Token(first) if first.kind() == SyntaxKind::COLON => {
            if let Some(SyntaxElement::Token(second)) = elements.get(index + 1)
                && second.kind() == SyntaxKind::COLON
            {
                Some(index + 2)
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_summarize_rust_file_extracts_production_structure_and_paths() {
        let source = indoc::indoc! {r#"
            architecture_component!(CodeLintAst);

            mod detail;
            pub mod public_child;

            macro_rules! local_mac {
                () => {};
            }
            pub(crate) use local_mac;

            pub use self::detail::Exported;
            use crate::core::{self, Config as Cfg, tags::*};

            #[cfg(test)]
            fn ignored_test_helper() {
                use crate::forbidden::in_test;
                let _ = super::ignored::call();
            }

            const RAW_FIXTURE: &str = r"
            #[cfg(test)]
            mod fake_tests {}
            ";

            pub(in crate::code_lint) fn run(input: super::ParentType) {
                let _ = crate::a::b::Foo::<crate::c::d::Bar>::baz();
                assert!(super::detail::check(Wrapper { Cfg: Cfg, input }));
            }
        "#};
        let file = ParsedFile::rust(source);
        let summary = summarize_rust_file(&file);
        let raw_paths: Vec<&str> = summary
            .referenced_paths
            .iter()
            .map(|reference| reference.raw_path.as_str())
            .collect();

        assert_eq!(
            summary
                .architecture_components
                .iter()
                .map(|(_, name)| name.as_str())
                .collect::<Vec<_>>(),
            vec!["CodeLintAst"]
        );
        assert_eq!(
            summary
                .visible_uses
                .iter()
                .map(|decl| (
                    decl.line,
                    decl.declaration_text.as_str(),
                    decl.target_paths.as_slice()
                ))
                .collect::<Vec<_>>(),
            vec![(
                11,
                "pub use self::detail::Exported;",
                &["self::detail::Exported".to_string()][..]
            )]
        );
        assert_eq!(
            summary
                .external_mods
                .iter()
                .map(|decl| (
                    decl.name.as_str(),
                    decl.declaration_text.as_str(),
                    decl.is_private
                ))
                .collect::<Vec<_>>(),
            vec![
                ("detail", "mod detail;", true),
                ("public_child", "pub mod public_child;", false),
            ]
        );
        assert_eq!(
            raw_paths,
            vec![
                "local_mac",
                "self::detail::Exported",
                "crate::core",
                "crate::core::Config",
                "crate::core::tags",
                "super::ParentType",
                "crate::a::b::Foo",
                "crate::c::d::Bar",
                "super::detail::check",
                "crate::core::Config",
            ]
        );
    }

    #[test]
    fn test_summarize_rust_file_marks_glob_use_trees_with_is_glob() {
        let source = indoc::indoc! {r"
            use crate::core::tags::*;
            use crate::core::Config;
        "};
        let file = ParsedFile::rust(source);
        let summary = summarize_rust_file(&file);
        let flags: Vec<(&str, bool)> = summary
            .referenced_paths
            .iter()
            .map(|reference| (reference.raw_path.as_str(), reference.is_glob))
            .collect();
        assert_eq!(
            flags,
            vec![("crate::core::tags", true), ("crate::core::Config", false)]
        );
    }

    #[test]
    fn test_summarize_rust_module_header_skips_function_body_paths() {
        let source = indoc::indoc! {"
            architecture_component!(CodeLintAst);
            pub mod child;
            use crate::core::Config;

            pub fn execute() {
                let _ = crate::body_only::Call::run();
            }
        "};
        let file = ParsedFile::rust(source);
        let header = summarize_rust_module_header(&file);
        let raw_paths: Vec<&str> = header
            .referenced_paths
            .iter()
            .map(|reference| reference.raw_path.as_str())
            .collect();

        assert_eq!(
            header
                .architecture_components
                .iter()
                .map(|(_, name)| name.as_str())
                .collect::<Vec<_>>(),
            vec!["CodeLintAst"]
        );
        assert_eq!(header.external_mods.len(), 1);
        assert_eq!(raw_paths, vec!["crate::core::Config"]);
    }
}
