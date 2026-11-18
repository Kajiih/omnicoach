//! Unrestricted `pub` declaration and consumer-identifier extraction for Rust workspace export
//! liveness checks.

use super::test_analysis::{collect_inline_test_ranges, has_outer_test_attribute};
use super::trivia::{is_rust_comment_kind, is_rust_doc_comment};
use crate::code_lint::ast::tree::{ParsedFile, span_from_rowan_range};
use crate::diagnostic::SourceSpan;
use ra_ap_syntax::ast::{self, HasModuleItem as _, HasName as _, HasVisibility as _};
use ra_ap_syntax::{AstNode as _, SyntaxElement, SyntaxKind, SyntaxNode};
use std::ops::Range;

/// An unrestricted `pub` item, inherent-`impl` associated item, or `pub use ... as Alias` rename
/// declared in non-test production Rust code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustPubDeclaration {
    /// Unqualified exported identifier (`"SourceSpan"`, `"new"`, `"PublicAlias"`).
    pub name: String,
    /// Enclosing inherent `impl` type identifier when this item is an associated `pub` item.
    pub enclosing_type: Option<String>,
    /// Byte span of the declared identifier token in its defining file.
    pub name_span: SourceSpan,
}

/// Collects unrestricted `pub` declarations in non-test production code of `file`.
///
/// Includes top-level and inline `pub mod` items, `pub use ... as` aliases, inherent `impl`
/// associated items, and `pub` items inside `macro_rules!` transcribers.
#[must_use]
pub fn collect_unrestricted_pub_declarations(file: &ParsedFile) -> Vec<RustPubDeclaration> {
    let Some(parsed) = file.rs_parsed() else {
        return Vec::new();
    };
    let mut declarations = Vec::new();
    for item in parsed.tree().items() {
        if has_outer_test_attribute(item.syntax()) {
            continue;
        }
        collect_pub_declarations_in_item(&item, &mut declarations);
    }
    declarations
}

fn collect_pub_declarations_in_item(item: &ast::Item, out: &mut Vec<RustPubDeclaration>) {
    match item {
        ast::Item::Fn(node) => push_named_pub_decl(node.visibility(), node.name(), None, out),
        ast::Item::Struct(node) => push_named_pub_decl(node.visibility(), node.name(), None, out),
        ast::Item::Enum(node) => push_named_pub_decl(node.visibility(), node.name(), None, out),
        ast::Item::Union(node) => push_named_pub_decl(node.visibility(), node.name(), None, out),
        ast::Item::Trait(node) => push_named_pub_decl(node.visibility(), node.name(), None, out),
        ast::Item::TypeAlias(node) => {
            push_named_pub_decl(node.visibility(), node.name(), None, out);
        }
        ast::Item::Const(node) => push_named_pub_decl(node.visibility(), node.name(), None, out),
        ast::Item::Static(node) => push_named_pub_decl(node.visibility(), node.name(), None, out),
        ast::Item::Module(module_item) => {
            if is_unrestricted_pub(module_item.visibility())
                && let Some(module_body) = module_item.item_list()
            {
                for nested in module_body.items() {
                    if !has_outer_test_attribute(nested.syntax()) {
                        collect_pub_declarations_in_item(&nested, out);
                    }
                }
            }
        }
        ast::Item::Use(use_item) => {
            if is_unrestricted_pub(use_item.visibility())
                && let Some(use_tree) = use_item.use_tree()
            {
                collect_pub_use_aliases(&use_tree, out);
            }
        }
        ast::Item::Impl(impl_def) if impl_def.trait_().is_none() => {
            let enclosing_type = impl_self_type_ident(impl_def);
            let Some(associated_items) = impl_def.assoc_item_list() else {
                return;
            };
            for assoc in associated_items.assoc_items() {
                if has_outer_test_attribute(assoc.syntax()) {
                    continue;
                }
                match assoc {
                    ast::AssocItem::Fn(node) => push_named_pub_decl(
                        node.visibility(),
                        node.name(),
                        enclosing_type.clone(),
                        out,
                    ),
                    ast::AssocItem::Const(node) => push_named_pub_decl(
                        node.visibility(),
                        node.name(),
                        enclosing_type.clone(),
                        out,
                    ),
                    ast::AssocItem::TypeAlias(node) => push_named_pub_decl(
                        node.visibility(),
                        node.name(),
                        enclosing_type.clone(),
                        out,
                    ),
                    ast::AssocItem::MacroCall(_) => {}
                }
            }
        }
        ast::Item::MacroRules(macro_rules) => {
            collect_macro_rules_pub_declarations(macro_rules, out);
        }
        _ => {}
    }
}

fn push_named_pub_decl(
    visibility: Option<ast::Visibility>,
    name: Option<ast::Name>,
    enclosing_type: Option<String>,
    out: &mut Vec<RustPubDeclaration>,
) {
    if !is_unrestricted_pub(visibility) {
        return;
    }
    let Some(name_node) = name else {
        return;
    };
    out.push(RustPubDeclaration {
        name: name_node.text().to_string(),
        enclosing_type,
        name_span: span_from_rowan_range(name_node.syntax().text_range()),
    });
}

fn collect_pub_use_aliases(use_tree: &ast::UseTree, out: &mut Vec<RustPubDeclaration>) {
    if let Some(rename) = use_tree.rename()
        && let Some(alias_name) = rename.name()
    {
        out.push(RustPubDeclaration {
            name: alias_name.text().to_string(),
            enclosing_type: None,
            name_span: span_from_rowan_range(alias_name.syntax().text_range()),
        });
    }
    if let Some(nested_trees) = use_tree.use_tree_list() {
        for child in nested_trees.use_trees() {
            collect_pub_use_aliases(&child, out);
        }
    }
}

fn is_unrestricted_pub(visibility: Option<ast::Visibility>) -> bool {
    visibility.is_some_and(|modifier| {
        modifier.pub_token().is_some()
            && !modifier
                .syntax()
                .descendants_with_tokens()
                .any(|elem| elem.kind() == SyntaxKind::L_PAREN)
    })
}

fn impl_self_type_ident(impl_def: &ast::Impl) -> Option<String> {
    type_terminal_ident(impl_def.self_ty()?)
}

fn type_terminal_ident(target_type: ast::Type) -> Option<String> {
    match target_type {
        ast::Type::PathType(path_type) => path_type
            .path()?
            .segment()?
            .name_ref()
            .map(|name| name.text().to_owned()),
        ast::Type::ParenType(paren) => type_terminal_ident(paren.ty()?),
        ast::Type::RefType(reference) => type_terminal_ident(reference.ty()?),
        _ => None,
    }
}

fn collect_macro_rules_pub_declarations(
    macro_rules: &ast::MacroRules,
    out: &mut Vec<RustPubDeclaration>,
) {
    let Some(body_tree) = macro_rules.token_tree() else {
        return;
    };
    let mut saw_arrow = false;
    for element in body_tree.syntax().children_with_tokens() {
        match element {
            SyntaxElement::Token(token)
                if matches!(token.kind(), SyntaxKind::FAT_ARROW | SyntaxKind::R_ANGLE) =>
            {
                saw_arrow = true;
            }
            SyntaxElement::Node(node) if node.kind() == SyntaxKind::TOKEN_TREE && saw_arrow => {
                collect_pub_declarations_in_transcriber(&node, out);
                saw_arrow = false;
            }
            SyntaxElement::Token(token) if token.kind() == SyntaxKind::SEMICOLON => {
                saw_arrow = false;
            }
            _ => {}
        }
    }
}

fn collect_pub_declarations_in_transcriber(
    transcriber: &SyntaxNode,
    out: &mut Vec<RustPubDeclaration>,
) {
    let mut tokens = transcriber
        .descendants_with_tokens()
        .filter_map(SyntaxElement::into_token)
        .filter(|token| !token.kind().is_trivia())
        .peekable();
    while let Some(token) = tokens.next() {
        if token.kind() != SyntaxKind::PUB_KW {
            continue;
        }
        if tokens
            .peek()
            .is_some_and(|next| next.kind() == SyntaxKind::L_PAREN)
        {
            continue;
        }
        while let Some(modifier) = tokens.next_if(|next| {
            matches!(
                next.kind(),
                SyntaxKind::CONST_KW
                    | SyntaxKind::ASYNC_KW
                    | SyntaxKind::UNSAFE_KW
                    | SyntaxKind::EXTERN_KW
            )
        }) {
            if modifier.kind() == SyntaxKind::EXTERN_KW {
                tokens.next_if(|abi| abi.kind() == SyntaxKind::STRING);
            } else if modifier.kind() == SyntaxKind::CONST_KW
                && let Some(ident) = tokens.next_if(|after| after.kind() == SyntaxKind::IDENT)
            {
                out.push(RustPubDeclaration {
                    name: ident.text().to_owned(),
                    enclosing_type: None,
                    name_span: span_from_rowan_range(ident.text_range()),
                });
                break;
            }
        }
        if tokens
            .next_if(|next| {
                matches!(
                    next.kind(),
                    SyntaxKind::FN_KW
                        | SyntaxKind::STRUCT_KW
                        | SyntaxKind::ENUM_KW
                        | SyntaxKind::UNION_KW
                        | SyntaxKind::TRAIT_KW
                        | SyntaxKind::TYPE_KW
                        | SyntaxKind::STATIC_KW
                )
            })
            .is_some()
            && let Some(ident) = tokens.next_if(|after| after.kind() == SyntaxKind::IDENT)
        {
            out.push(RustPubDeclaration {
                name: ident.text().to_owned(),
                enclosing_type: None,
                name_span: span_from_rowan_range(ident.text_range()),
            });
        }
    }
}

/// Returns the names of external child modules declared with `#[cfg(test)] mod <name>;` in `file`.
#[must_use]
pub fn collect_test_only_external_mods(file: &ParsedFile) -> Vec<String> {
    let Some(parsed) = file.rs_parsed() else {
        return Vec::new();
    };
    parsed
        .tree()
        .items()
        .filter_map(|item| match item {
            ast::Item::Module(module_item)
                if module_item.item_list().is_none()
                    && has_outer_test_attribute(module_item.syntax()) =>
            {
                module_item.name().map(|name| name.text().to_string())
            }
            _ => None,
        })
        .collect()
}

/// Collects consumer `(identifier, span)` pairs across `file` and its fenced rustdoc examples.
///
/// Skips `impl` target-type headers, visible `pub use` / `pub(...) use` re-exports, and—when
/// `!include_inline_tests`—inline `#[cfg(test)]` and `#[test]` items.
#[must_use]
pub fn collect_rust_consumer_identifiers(
    file: &ParsedFile,
    include_inline_tests: bool,
) -> Vec<(String, SourceSpan)> {
    let Some(parsed) = file.rs_parsed() else {
        return Vec::new();
    };
    let source_file = parsed.tree();
    let excluded_ranges = collect_non_consumer_ranges(&source_file, file, include_inline_tests);
    let mut identifiers = Vec::new();
    let mut doc_lines: Vec<String> = Vec::new();
    let mut doc_span: Option<SourceSpan> = None;
    let mut last_doc_is_inner: Option<bool> = None;

    for token in source_file
        .syntax()
        .descendants_with_tokens()
        .filter_map(SyntaxElement::into_token)
    {
        if token.kind() == SyntaxKind::WHITESPACE {
            continue;
        }
        let token_span = span_from_rowan_range(token.text_range());
        if excluded_ranges
            .iter()
            .any(|range| range.contains(&token_span.start))
        {
            continue;
        }
        if is_rust_doc_comment(token.text()) {
            let is_inner = token.text().starts_with("//!") || token.text().starts_with("/*!");
            if last_doc_is_inner.is_some_and(|prev| prev != is_inner) {
                flush_doc_fences(&mut doc_lines, doc_span.take(), &mut identifiers);
            }
            last_doc_is_inner = Some(is_inner);
            doc_span = Some(doc_span.map_or(token_span, |prev| {
                SourceSpan::new(prev.start, token_span.end)
            }));
            append_doc_comment_lines(token.text(), &mut doc_lines);
            continue;
        }
        if is_rust_comment_kind(token.kind()) {
            continue;
        }
        let in_attr = token
            .parent_ancestors()
            .any(|ancestor| ancestor.kind() == SyntaxKind::ATTR);
        if !in_attr {
            flush_doc_fences(&mut doc_lines, doc_span.take(), &mut identifiers);
            last_doc_is_inner = None;
        }
        if token.kind() == SyntaxKind::IDENT {
            identifiers.push((token.text().to_owned(), token_span));
        }
    }
    flush_doc_fences(&mut doc_lines, doc_span.take(), &mut identifiers);
    identifiers
}

fn collect_non_consumer_ranges(
    source_file: &ast::SourceFile,
    file: &ParsedFile,
    include_inline_tests: bool,
) -> Vec<Range<usize>> {
    let mut ranges = if include_inline_tests {
        Vec::new()
    } else {
        collect_inline_test_ranges(file).to_vec()
    };
    for node in source_file.syntax().descendants() {
        if let Some(use_item) = ast::Use::cast(node.clone())
            && use_item.visibility().is_some()
            && let Some(use_tree) = use_item.use_tree()
        {
            let span = span_from_rowan_range(use_tree.syntax().text_range());
            ranges.push(span.start..span.end);
        } else if let Some(impl_def) = ast::Impl::cast(node)
            && let Some(self_type) = impl_def.self_ty()
        {
            let span = span_from_rowan_range(self_type.syntax().text_range());
            ranges.push(span.start..span.end);
        }
    }
    ranges
}

fn append_doc_comment_lines(comment_text: &str, doc_lines: &mut Vec<String>) {
    if let Some(line) = comment_text
        .strip_prefix("///")
        .or_else(|| comment_text.strip_prefix("//!"))
    {
        doc_lines.push(line.strip_prefix(' ').unwrap_or(line).to_owned());
    } else if let Some(block) = comment_text
        .strip_prefix("/**")
        .or_else(|| comment_text.strip_prefix("/*!"))
        .and_then(|rest| rest.strip_suffix("*/"))
    {
        doc_lines.extend(block.lines().map(str::to_owned));
    }
}

fn flush_doc_fences(
    doc_lines: &mut Vec<String>,
    doc_span: Option<SourceSpan>,
    out: &mut Vec<(String, SourceSpan)>,
) {
    if doc_lines.is_empty() {
        return;
    }
    let span = doc_span.unwrap_or_else(|| SourceSpan::new(0, 0));
    let mut state = DocFenceState::Outside;
    let mut current_code = String::new();
    for line in doc_lines.drain(..) {
        let trimmed = line.trim_start();
        let fence_len = trimmed.bytes().take_while(|&byte| byte == b'`').count();
        if fence_len >= 3 {
            let rest = trimmed[fence_len..].trim();
            match state {
                DocFenceState::Rust { open_len } if fence_len >= open_len && rest.is_empty() => {
                    collect_rust_fence_identifiers(&current_code, span, out);
                    current_code.clear();
                    state = DocFenceState::Outside;
                    continue;
                }
                DocFenceState::NonRust { open_len } if fence_len >= open_len && rest.is_empty() => {
                    state = DocFenceState::Outside;
                    continue;
                }
                DocFenceState::Outside => {
                    let first_tag = rest.split([',', ' ', '\t']).next().unwrap_or("");
                    state = if matches!(
                        first_tag,
                        "" | "rust"
                            | "ignore"
                            | "no_run"
                            | "should_panic"
                            | "compile_fail"
                            | "edition2021"
                            | "edition2024"
                    ) {
                        DocFenceState::Rust {
                            open_len: fence_len,
                        }
                    } else {
                        DocFenceState::NonRust {
                            open_len: fence_len,
                        }
                    };
                    continue;
                }
                DocFenceState::Rust { .. } | DocFenceState::NonRust { .. } => {}
            }
        }
        if matches!(state, DocFenceState::Rust { .. }) {
            let unhidden = line
                .strip_prefix("# ")
                .or_else(|| (line == "#").then_some(""))
                .unwrap_or(&line);
            current_code.push_str(unhidden);
            current_code.push('\n');
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DocFenceState {
    Outside,
    Rust { open_len: usize },
    NonRust { open_len: usize },
}

fn collect_rust_fence_identifiers(
    code: &str,
    doc_span: SourceSpan,
    out: &mut Vec<(String, SourceSpan)>,
) {
    let parsed = ParsedFile::rust(code);
    let Some(rs_parsed) = parsed.rs_parsed() else {
        return;
    };
    for token in rs_parsed
        .tree()
        .syntax()
        .descendants_with_tokens()
        .filter_map(SyntaxElement::into_token)
    {
        if token.kind() == SyntaxKind::IDENT {
            out.push((token.text().to_owned(), doc_span));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_unrestricted_pub_declarations_covers_items_aliases_impls_and_rhs_macros() {
        let source = indoc::indoc! {r#"
            pub /* comment */ fn top_fn() {}
            pub struct TopStruct;
            pub enum TopEnum { A }
            pub union TopUnion { pub raw: u32 }
            pub trait TopTrait {}
            pub type TopAlias = TopStruct;
            pub const TOP_CONST: usize = 1;
            pub static TOP_STATIC: &str = "ok";
            pub const _: () = ();
            pub(crate) fn crate_only() {}
            pub use self::inner::InternalItem as ExportedAlias;
            pub use self::inner::PassthroughItem;

            pub mod inline_mod {
                pub fn nested_pub_fn() {}
                pub(super) fn nested_scoped_fn() {}
            }

            impl crate::sub::TopStruct {
                pub const ASSOC_CONST: usize = 2;
                pub fn inherent_method(&self) {}
                pub(super) fn scoped_method(&self) {}
            }

            impl TopTrait for TopStruct {}

            macro_rules! declare_api {
                (pub struct PatternMatcherIgnored;) => {
                    pub struct MacroGenerated;
                    pub extern "C" fn macro_ffi() {}
                    pub const MACRO_CONST: usize = 1;
                    pub(crate) struct MacroInternal;
                };
            }

            #[cfg(test)]
            pub fn test_only_export() {}
        "#};
        let file = ParsedFile::rust(source);

        let declarations = collect_unrestricted_pub_declarations(&file);
        let summary: Vec<_> = declarations
            .iter()
            .map(|decl| (decl.name.as_str(), decl.enclosing_type.as_deref()))
            .collect();

        assert_eq!(
            summary,
            vec![
                ("top_fn", None),
                ("TopStruct", None),
                ("TopEnum", None),
                ("TopUnion", None),
                ("TopTrait", None),
                ("TopAlias", None),
                ("TOP_CONST", None),
                ("TOP_STATIC", None),
                ("ExportedAlias", None),
                ("nested_pub_fn", None),
                ("ASSOC_CONST", Some("TopStruct")),
                ("inherent_method", Some("TopStruct")),
                ("MacroGenerated", None),
                ("macro_ffi", None),
                ("MACRO_CONST", None),
            ]
        );
    }

    #[test]
    fn collect_rust_consumer_identifiers_skips_reexports_impl_headers_and_inline_tests() {
        let source = indoc::indoc! {r#"
            //! ```rust
            //! UnclosedInnerFence::skip();
            /// Example:
            /// ````rust
            /// # use demo::HiddenHelper;
            /// let _ = "```";
            /// let _ = LiveDocTarget::run();
            /// ````
            // separator comment inside attribute preamble
            #[must_use]
            /// ```text
            /// PlainProseIgnored
            /// ```
            /**
            ```
            BlockDocTarget::build();
            ```
            */
            pub fn entry() {}

            pub struct DeadSelf;
            impl DeadSelf {
                pub fn unused() {}
            }
            pub use self::child::ReexportedOnly;

            #[cfg(test)]
            mod test_utils;

            #[cfg(test)]
            mod tests {
                use super::UnitTestOnlyTarget;
            }
        "#};
        let file = ParsedFile::rust(source);

        let test_only_mods = collect_test_only_external_mods(&file);
        let prod_tokens = collect_rust_consumer_identifiers(&file, false);
        let prod_names: Vec<&str> = prod_tokens.iter().map(|(name, _)| name.as_str()).collect();
        let all_spans_in_bounds = prod_tokens.iter().all(|(_, span)| span.end <= source.len());

        assert_eq!(test_only_mods, vec!["test_utils".to_owned()]);
        assert!(all_spans_in_bounds);
        assert_eq!(
            prod_names,
            vec![
                "must_use",
                "demo",
                "HiddenHelper",
                "LiveDocTarget",
                "run",
                "BlockDocTarget",
                "build",
                "entry",
                "DeadSelf",
                "unused",
            ]
        );
    }
}
