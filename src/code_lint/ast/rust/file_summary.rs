//! Structural and dependency summary of production Rust source files (`ra_ap_syntax`).

use super::attributes::attribute_terminal_name;
use super::names::{
    SELF_KEYWORD, compact_path_text, is_path_segment_token_kind, macro_call_terminal_name,
};
use super::tree::extract_macro_arguments;
use super::trivia::is_rust_comment_kind;
use crate::code_lint::ast::imports::{ResolvedName, resolve_name};
use crate::code_lint::ast::tree::{AstNode, ParsedFile, span_from_rowan_range};
use ra_ap_syntax::ast::{
    self, HasAttrs as _, HasGenericArgs as _, HasModuleItem as _, HasName as _, HasVisibility as _,
};
use ra_ap_syntax::{AstNode as _, SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};
use std::collections::HashSet;

/// Visibility and name of a top-level external `mod <name>;` declaration in a Rust file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalModDeclaration {
    /// Module identifier (e.g. `"rust"` in `pub mod rust;`).
    pub name: String,
    /// Full trimmed source text of the declaration (e.g. `"pub mod rust;"`).
    pub declaration_text: String,
    /// True when the declaration has no `visibility_modifier` (`mod child;`).
    pub is_private: bool,
}

/// A referenced path in production Rust code (from a `use` tree or inline qualified path).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustPathReference {
    /// 1-indexed starting line number of the reference or enclosing `use` declaration.
    pub line: usize,
    /// Raw syntactic path segments joined by `::` (e.g. `"crate::code_lint::ast::ParsedFile"`,
    /// `"super::AstNode"`, `"self::rust::collect_bindings"`, `"ra_ap_syntax::SyntaxNode"`).
    pub raw_path: String,
    /// Trimmed source text of the enclosing `use` declaration or inline path node for diagnostics.
    pub statement_text: String,
}

/// A visible `use` declaration (`pub use ...`, `pub(crate) use ...`) in production Rust code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleUseDeclaration {
    /// 1-indexed starting line number.
    pub line: usize,
    /// Full trimmed source text (e.g. `"pub use self::child::Item;"`).
    pub declaration_text: String,
    /// Expanded target paths imported by this `use` tree (e.g. `["self::child::Item"]`).
    pub target_paths: Vec<String>,
}

/// Structural and dependency summary of the production code in a Rust source file,
/// extracted in a single CST pass while skipping `#[cfg(test)]` and `#[test]` subtrees.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RustFileSummary {
    /// Arguments of all `architecture_component!(...)` macro invocations in production code.
    pub architecture_components: Vec<String>,
    /// `(component_name, declared_dependencies)` entries from `define_architecture! { ... }` macro
    /// invocations in production code.
    pub defined_architecture: Vec<(String, Vec<String>)>,
    /// Top-level external `mod <name>;` declarations (`mod foo;`, `pub mod foo;`).
    pub external_mods: Vec<ExternalModDeclaration>,
    /// `(line, item_text)` of top-level production items that are neither external `mod` declarations
    /// nor `macro_rules!` definitions.
    pub non_namespace_items: Vec<(usize, String)>,
    /// `(line, item_text)` of top-level production `macro_rules!` definitions (`macro_definition`).
    pub macro_definitions: Vec<(usize, String)>,
    /// All `#[macro_export]` attribute texts in production code.
    pub macro_exports: Vec<String>,
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

    for item in source_file.items() {
        let start_offset: usize = item.syntax().text_range().start().into();
        if file.is_in_rust_inline_test(start_offset) {
            continue;
        }
        let ItemDeclaration {
            line: item_line,
            text: declaration_text,
        } = item_line_and_text(file, item.syntax());
        match &item {
            ast::Item::Module(module) if module.item_list().is_none() => {
                if let Some(name_node) = module.name() {
                    let is_private = module.visibility().is_none();
                    summary.external_mods.push(ExternalModDeclaration {
                        name: name_node.text().trim().to_string(),
                        declaration_text,
                        is_private,
                    });
                }
            }
            ast::Item::MacroRules(macro_rules) => {
                if let Some(name_node) = macro_rules.name() {
                    defined_macros.insert(name_node.text().to_string());
                }
                summary
                    .macro_definitions
                    .push((item_line, declaration_text));
            }
            _ => {
                summary
                    .non_namespace_items
                    .push((item_line, declaration_text));
            }
        }
    }

    for child in source_file.syntax().children() {
        summarize_rust_node(&child, file, &defined_macros, &mut summary);
    }

    summary
}

fn summarize_rust_node(
    node: &SyntaxNode,
    file: &ParsedFile,
    defined_macros: &HashSet<String>,
    summary: &mut RustFileSummary,
) {
    let start_offset: usize = node.text_range().start().into();
    if file.is_in_rust_inline_test(start_offset) {
        return;
    }

    if let Some(attribute) = ast::Attr::cast(node.clone()) {
        if attribute.kind() == ast::AttrKind::Outer
            && attribute_terminal_name(&attribute)
                .is_some_and(|terminal| terminal.text() == "macro_export")
        {
            summary.macro_exports.push("#[macro_export]".to_string());
        }
        for child in node.children() {
            if child.kind() != SyntaxKind::TOKEN_TREE {
                summarize_rust_node(&child, file, defined_macros, summary);
            }
        }
        return;
    }

    if let Some(use_item) = ast::Use::cast(node.clone()) {
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
        let macro_ast_node = AstNode::from_span(
            file,
            span_from_rowan_range(macro_call.syntax().text_range()),
        );
        let terminal_name = macro_call_terminal_name(&macro_call);
        if terminal_name == "architecture_component" {
            let component_argument: String = extract_macro_arguments(&macro_ast_node)
                .into_iter()
                .map(|argument| argument.text().into_owned())
                .collect();
            summary.architecture_components.push(component_argument);
        }
        if let Some(token_tree) = macro_call.token_tree() {
            if terminal_name == "define_architecture" {
                collect_defined_architecture(&token_tree, &mut summary.defined_architecture);
            }
            let statement_text = item_line_and_text(file, node).text;
            collect_token_tree_paths(
                &token_tree,
                file,
                &statement_text,
                &mut summary.referenced_paths,
            );
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
                    raw_path: imported,
                    statement_text: trimmed.to_string(),
                });
            }
        }
    }

    for child in node.children() {
        if !matches!(child.kind(), SyntaxKind::TOKEN_TREE | SyntaxKind::VISIBILITY) {
            summarize_rust_node(&child, file, defined_macros, summary);
        }
    }
}

fn summarize_use_item(
    use_item: &ast::Use,
    file: &ParsedFile,
    defined_macros: &HashSet<String>,
    summary: &mut RustFileSummary,
) {
    for attribute in use_item.attrs() {
        summarize_rust_node(attribute.syntax(), file, defined_macros, summary);
    }
    let ItemDeclaration {
        line,
        text: declaration_text,
    } = item_line_and_text(file, use_item.syntax());
    let mut target_paths = Vec::new();
    if let Some(use_tree) = use_item.use_tree() {
        expand_use_tree(&use_tree, "", &mut target_paths);
    }
    for raw_path in &target_paths {
        summary.referenced_paths.push(RustPathReference {
            line,
            raw_path: raw_path.clone(),
            statement_text: declaration_text.clone(),
        });
    }
    if let Some(visibility) = use_item.visibility() {
        let is_own_macro_path = visibility.syntax().text().to_string().trim() == "pub(crate)"
            && matches!(
                target_paths.as_slice(),
                [target] if defined_macros.contains(target.as_str())
            );
        if !is_own_macro_path {
            summary.visible_uses.push(VisibleUseDeclaration {
                line,
                declaration_text,
                target_paths,
            });
        }
    }
}

fn expand_use_tree(use_tree: &ast::UseTree, prefix: &str, out: &mut Vec<String>) {
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
            out.push(join_use_prefix(prefix, &path.syntax().text().to_string()));
        } else if !prefix.is_empty() {
            out.push(prefix.to_string());
        }
        return;
    }
    if let Some(path) = use_tree.path() {
        let path_text = path.syntax().text().to_string();
        if compact_path_text(&path_text) != "_" {
            out.push(join_use_prefix(prefix, &path_text));
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
            raw_path,
            statement_text: slice_text.to_string(),
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
    let text = file.source[start_offset..end_offset].trim().to_string();
    ItemDeclaration { line, text }
}

fn collect_defined_architecture(token_tree: &ast::TokenTree, out: &mut Vec<(String, Vec<String>)>) {
    let mut last_ident: Option<String> = None;
    for element in token_tree.syntax().children_with_tokens() {
        match element {
            SyntaxElement::Token(token) => match token.kind() {
                SyntaxKind::IDENT => last_ident = Some(token.text().to_string()),
                SyntaxKind::POUND => last_ident = None,
                _ => {}
            },
            SyntaxElement::Node(node) if node.kind() == SyntaxKind::TOKEN_TREE => {
                let starts_with_bracket = node
                    .first_token()
                    .is_some_and(|tok| tok.kind() == SyntaxKind::L_BRACK);
                if starts_with_bracket && let Some(name) = last_ident.take() {
                    let declared_dependencies = node
                        .children_with_tokens()
                        .filter_map(SyntaxElement::into_token)
                        .filter(|tok| tok.kind() == SyntaxKind::IDENT)
                        .map(|tok| tok.text().to_string())
                        .collect();
                    out.push((name, declared_dependencies));
                }
            }
            SyntaxElement::Node(_) => {}
        }
    }
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
            let line = file
                .line_index
                .line(first_token.text_range().start().into());
            let mut segments = vec![first_token.text().trim().to_string()];
            let mut cursor = index + 1;
            while let Some(next_segment_idx) = consume_double_colon(&elements, cursor)
                && let Some(segment_token) = token_segment_at(&elements, next_segment_idx)
            {
                segments.push(segment_token.text().trim().to_string());
                cursor = next_segment_idx + 1;
            }
            out.push(RustPathReference {
                line,
                raw_path: segments.join("::"),
                statement_text: statement_text.to_string(),
            });
            index = cursor;
            continue;
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
                assert!(super::detail::check(input));
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
            summary.architecture_components,
            vec!["CodeLintAst".to_string()]
        );
        assert_eq!(
            summary.visible_uses,
            vec![VisibleUseDeclaration {
                line: 11,
                declaration_text: "pub use self::detail::Exported;".to_string(),
                target_paths: vec!["self::detail::Exported".to_string()],
            }]
        );
        assert_eq!(
            summary.external_mods,
            vec![
                ExternalModDeclaration {
                    name: "detail".to_string(),
                    declaration_text: "mod detail;".to_string(),
                    is_private: true,
                },
                ExternalModDeclaration {
                    name: "public_child".to_string(),
                    declaration_text: "pub mod public_child;".to_string(),
                    is_private: false,
                },
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
            ]
        );
    }
}
