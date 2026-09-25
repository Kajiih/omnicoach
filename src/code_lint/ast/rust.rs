//! AST helper predicates for structural traversal in Rust.

architecture_component!(CodeSyntaxAdapters);

use crate::code_lint::ast::{AstNode, ParsedFile, RawNode};

/// Returns true for Rust node kinds that hold statements as direct children.
///
/// `source_file` is the file root, `block` is a braced body, and `declaration_list` is the
/// body of an `impl`, `trait`, or inline `mod`.
#[must_use]
pub fn is_statement_container(kind: &str) -> bool {
    matches!(kind, "source_file" | "block" | "declaration_list")
}

/// Returns true for Rust comment node kinds.
///
/// Rust distinguishes `//` from `/* */`. Doc comments are not separate kinds: `/// text`
/// parses as a `line_comment` wrapping `outer_doc_comment_marker` and `doc_comment`, so
/// matching only the outer kinds covers documentation without counting it twice.
#[must_use]
pub fn is_comment_kind(kind: &str) -> bool {
    matches!(kind, "line_comment" | "block_comment")
}

/// Returns true if a Rust node of `parent_kind` makes a child identifier an import binding.
#[must_use]
pub fn is_import_binding_parent(parent_kind: &str) -> bool {
    matches!(
        parent_kind,
        "use_declaration" | "use_list" | "use_as_clause" | "scoped_identifier"
    )
}

/// Returns true if a Rust node of `parent_kind` makes a child identifier an import binding
/// carrying no local alias.
///
/// `use_as_clause` is excluded precisely because it introduces one.
#[must_use]
pub fn is_unaliased_import_binding_parent(parent_kind: &str) -> bool {
    matches!(
        parent_kind,
        "use_declaration" | "use_list" | "scoped_identifier"
    )
}

/// Returns true if a Rust node of `parent_kind` declares a structural definition name.
#[must_use]
pub fn is_structural_definition_parent(parent_kind: &str) -> bool {
    matches!(
        parent_kind,
        "struct_item"
            | "enum_item"
            | "trait_item"
            | "type_item"
            | "associated_type"
            | "function_item"
    )
}

/// Returns true if `kind` is a call expression in Rust.
#[must_use]
pub fn is_call_kind(kind: &str) -> bool {
    kind == "call_expression"
}

/// If `function` is a method access (e.g. `obj.method`), returns the method identifier node.
#[must_use]
pub(super) fn extract_method_call_target<'a>(function: &RawNode<'a>) -> Option<RawNode<'a>> {
    if function.kind().as_ref() == "field_expression" {
        function.field("field")
    } else {
        None
    }
}

/// Returns true if `item`, the definition owning a name, has that name mandated by a contract.
///
/// The contract is an `impl Trait for Type` block: the member sits in the `declaration_list`
/// of an `impl_item` that names a trait.
#[must_use]
pub fn is_trait_impl_member(item: &AstNode<'_>) -> bool {
    let raw = &item.raw;
    if !matches!(
        raw.kind().as_ref(),
        "function_item" | "type_item" | "associated_type" | "const_item"
    ) {
        return false;
    }
    let Some(body) = raw.parent() else {
        return false;
    };
    if body.kind().as_ref() != "declaration_list" {
        return false;
    }
    let Some(impl_item) = body.parent() else {
        return false;
    };
    impl_item.kind().as_ref() == "impl_item" && impl_item.field("trait").is_some()
}

/// Recursively extracts binding identifiers from a pattern node.
fn extract_from_pattern<'a>(node: &RawNode<'a>, bindings: &mut Vec<AstNode<'a>>) {
    let kind = node.kind();
    match kind.as_ref() {
        "identifier" => {
            // Exclude uppercase names (variants/constants like None, Ok, MAX)
            // which are type constructor references rather than variable bindings.
            if let Some(first_char) = node.text().chars().next()
                && first_char.is_ascii_uppercase()
            {
                return;
            }
            if node.text() != "_" {
                bindings.push(AstNode::from_raw(node.clone()));
            }
        }
        "shorthand_field_identifier" => {
            bindings.push(AstNode::from_raw(node.clone()));
        }
        "struct_pattern" | "tuple_struct_pattern" => {
            let type_node = node.field("type");
            for child in node.children() {
                if let Some(ref struct_type) = type_node
                    && struct_type.range() == child.range()
                {
                    continue;
                }
                if child.kind() == "type_identifier" {
                    continue;
                }
                extract_from_pattern(&child, bindings);
            }
        }
        "field_pattern" => {
            for child in node.children() {
                if child.kind() != "field_identifier" && child.kind() != ":" {
                    extract_from_pattern(&child, bindings);
                }
            }
        }
        "match_pattern" => {
            let cond = node.field("condition");
            for child in node.children() {
                if let Some(ref condition) = cond
                    && condition.range() == child.range()
                {
                    continue;
                }
                if child.kind() == "if" {
                    continue;
                }
                extract_from_pattern(&child, bindings);
            }
        }
        _ => {
            for child in node.children() {
                extract_from_pattern(&child, bindings);
            }
        }
    }
}

/// Helper to extract the last segment identifier from a path node.
fn extract_last_segment<'a>(node: &RawNode<'a>) -> Option<RawNode<'a>> {
    match node.kind().as_ref() {
        "identifier" => Some(node.clone()),
        "scoped_identifier" => node.field("name"),
        _ => None,
    }
}

/// Recursively extracts bindings from a use declaration.
fn extract_from_use<'a>(
    node: &RawNode<'a>,
    bindings: &mut Vec<AstNode<'a>>,
    prefix_last_segment: Option<&RawNode<'a>>,
) {
    match node.kind().as_ref() {
        "use_declaration" => {
            for child in node.children() {
                if child.kind() != "use" && child.kind() != ";" {
                    extract_from_use(&child, bindings, None);
                }
            }
        }
        "identifier" => {
            if node.text() != "_" {
                bindings.push(AstNode::from_raw(node.clone()));
            }
        }
        "self" => {
            if let Some(parent) = prefix_last_segment {
                bindings.push(AstNode::from_raw(parent.clone()));
            }
        }
        "scoped_identifier" => {
            if let Some(last_seg) = extract_last_segment(node)
                && last_seg.text() != "_"
            {
                bindings.push(AstNode::from_raw(last_seg));
            }
        }
        "use_as_clause" => {
            if let Some(alias) = node.field("alias")
                && alias.text() != "_"
            {
                bindings.push(AstNode::from_raw(alias));
            }
        }
        "scoped_use_list" => {
            if let (Some(path), Some(list)) = (node.field("path"), node.field("list")) {
                let last_seg = extract_last_segment(&path);
                extract_from_use(&list, bindings, last_seg.as_ref());
            }
        }
        "use_list" => {
            for child in node.children() {
                let kind = child.kind();
                if kind != "{" && kind != "}" && kind != "," {
                    extract_from_use(&child, bindings, prefix_last_segment);
                }
            }
        }
        _ => {}
    }
}

fn traverse_rust<'a>(node: &RawNode<'a>, bindings: &mut Vec<AstNode<'a>>) {
    let kind = node.kind();
    match kind.as_ref() {
        "let_declaration" | "let_condition" => {
            if let Some(pattern) = node.field("pattern") {
                extract_from_pattern(&pattern, bindings);
            } else {
                let mut found_let = false;
                for child in node.children() {
                    if child.kind() == "let" {
                        found_let = true;
                        continue;
                    }
                    if found_let {
                        extract_from_pattern(&child, bindings);
                        break;
                    }
                }
            }
            for child in node.children() {
                traverse_rust(&child, bindings);
            }
        }
        "for_expression" => {
            if let Some(pattern) = node.field("pattern") {
                extract_from_pattern(&pattern, bindings);
            } else {
                let mut found_for = false;
                for child in node.children() {
                    if child.kind() == "for" {
                        found_for = true;
                        continue;
                    }
                    if found_for {
                        extract_from_pattern(&child, bindings);
                        break;
                    }
                }
            }
            for child in node.children() {
                traverse_rust(&child, bindings);
            }
        }
        "parameter" => {
            if let Some(pattern) = node.field("pattern") {
                extract_from_pattern(&pattern, bindings);
            }
            for child in node.children() {
                traverse_rust(&child, bindings);
            }
        }
        "closure_parameters" => {
            for child in node.children() {
                if child.kind() != "|" {
                    extract_from_pattern(&child, bindings);
                }
            }
        }
        "match_arm" => {
            for child in node.children() {
                if child.kind() == "match_pattern" {
                    extract_from_pattern(&child, bindings);
                } else if child.kind() != "=>" {
                    traverse_rust(&child, bindings);
                }
            }
        }
        "const_item" | "static_item" | "function_item" | "struct_item" | "enum_item"
        | "trait_item" | "type_item" | "associated_type" => {
            if let Some(name_node) = node.field("name") {
                bindings.push(AstNode::from_raw(name_node));
            }
            for child in node.children() {
                if let Some(name_node) = node.field("name")
                    && child.range() == name_node.range()
                {
                    continue;
                }
                traverse_rust(&child, bindings);
            }
        }
        "use_declaration" => {
            extract_from_use(node, bindings, None);
        }
        _ => {
            for child in node.children() {
                traverse_rust(&child, bindings);
            }
        }
    }
}

/// Collects all binding definitions (variables, functions, structs, etc.) within `file`.
#[must_use]
pub fn collect_bindings(file: &ParsedFile) -> Vec<AstNode<'_>> {
    let mut bindings = Vec::new();
    traverse_rust(&file.grep.root(), &mut bindings);
    bindings
}

/// Extracts the terminal identifier of the attribute path inside an `attribute_item` node
/// (e.g. `Some("test")` for `#[test]` or `#[tokio::test]`, `Some("cfg")` for `#[cfg(test)]`).
fn attribute_terminal_name<'a>(attr_item: &RawNode<'a>) -> Option<RawNode<'a>> {
    let attr = attr_item
        .children()
        .find(|child| child.kind() == "attribute")?;
    let path = attr
        .children()
        .find(|child| matches!(child.kind().as_ref(), "identifier" | "scoped_identifier"))?;
    extract_last_segment(&path)
}

/// Returns true if an `attribute_item` AST node represents a Rust test attribute
/// (`#[test]`, `#[tokio::test]`, `#[rstest]`, `#[test_case(...)]`).
#[must_use]
fn is_test_attribute(attr_item: &RawNode<'_>) -> bool {
    attribute_terminal_name(attr_item)
        .is_some_and(|terminal| matches!(terminal.text().as_ref(), "test" | "rstest" | "test_case"))
}

/// Returns true if an `attribute_item` AST node represents a `#[cfg(test)]` attribute.
#[must_use]
fn is_conditional_test_attribute(attr_item: &RawNode<'_>) -> bool {
    if attribute_terminal_name(attr_item).is_none_or(|terminal| terminal.text() != "cfg") {
        return false;
    }
    let Some(attr) = attr_item
        .children()
        .find(|child| child.kind() == "attribute")
    else {
        return false;
    };
    let Some(token_tree) = attr.children().find(|child| child.kind() == "token_tree") else {
        return false;
    };
    let arguments = non_delimiter_children(&token_tree);
    arguments.len() == 1 && arguments[0].kind() == "identifier" && arguments[0].text() == "test"
}

/// Returns true if an `attribute_item` AST node represents a `#[doc = "..."]` attribute.
#[must_use]
fn is_doc_attribute(attr_item: &RawNode<'_>) -> bool {
    attribute_terminal_name(attr_item).is_some_and(|terminal| terminal.text() == "doc")
}

/// Yields the contiguous preceding `attribute_item` siblings attached to a non-trivia `node`.
fn preceding_attributes<'a>(node: &RawNode<'a>) -> impl Iterator<Item = RawNode<'a>> {
    let first = (!matches!(
        node.kind().as_ref(),
        "attribute_item" | "line_comment" | "block_comment"
    ))
    .then(|| node.prev())
    .flatten();
    std::iter::successors(first, RawNode::prev)
        .take_while(|sibling| {
            matches!(
                sibling.kind().as_ref(),
                "attribute_item" | "line_comment" | "block_comment"
            )
        })
        .filter(|sibling| sibling.kind() == "attribute_item")
}

/// Returns true if `node` is preceded by an `attribute_item` sibling matching `predicate`.
fn has_matching_attribute(node: &RawNode<'_>, predicate: fn(&RawNode<'_>) -> bool) -> bool {
    preceding_attributes(node).any(|sibling| predicate(&sibling))
}

/// Returns true if a Rust item is preceded by a test attribute (`#[test]`, `#[tokio::test]`, `#[rstest]`, etc.).
#[must_use]
fn has_test_attribute(node: &RawNode<'_>) -> bool {
    has_matching_attribute(node, is_test_attribute)
}

/// Returns true if a Rust item is preceded by a `#[cfg(test)]` attribute.
#[must_use]
fn has_conditional_test_attribute(node: &RawNode<'_>) -> bool {
    has_matching_attribute(node, is_conditional_test_attribute)
}

/// Collects byte spans for all inline test items (`#[cfg(test)]` modules/items and `#[test]` functions)
/// within a Rust source file.
#[must_use]
pub fn collect_inline_test_ranges(file: &ParsedFile) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    collect_inline_test_ranges_rec(&file.grep.root(), &mut ranges);
    ranges
}

fn collect_inline_test_ranges_rec(node: &RawNode<'_>, ranges: &mut Vec<std::ops::Range<usize>>) {
    if has_conditional_test_attribute(node) || has_test_attribute(node) {
        let start = preceding_attributes(node)
            .last()
            .map_or_else(|| node.range().start, |attribute| attribute.range().start);
        ranges.push(start..node.range().end);
        return;
    }
    for child in node.children() {
        collect_inline_test_ranges_rec(&child, ranges);
    }
}

/// Returns true if a Rust `function_item` node is a test function (`#[test]` / `#[rstest]` or named `test` / `test_*`).
#[must_use]
fn is_test_function(func_node: &RawNode<'_>) -> bool {
    let is_named_test = func_node.field("name").is_some_and(|name_node| {
        let func_name = name_node.text();
        func_name == "test" || func_name.starts_with("test_")
    });
    is_named_test || has_test_attribute(func_node)
}

fn collect_outer_test_functions_rec<'a>(node: &RawNode<'a>, out: &mut Vec<RawNode<'a>>) {
    if node.kind() == "function_item" {
        if is_test_function(node) {
            out.push(node.clone());
        }
        return;
    }
    for child in node.children() {
        collect_outer_test_functions_rec(&child, out);
    }
}

/// Extracts the terminal macro identifier from a Rust `macro_invocation` node (e.g. `assert` from `std::assert!`).
#[must_use]
pub fn macro_terminal_name<'tree>(macro_node: &AstNode<'tree>) -> std::borrow::Cow<'tree, str> {
    macro_terminal_name_raw(&macro_node.raw)
}

fn macro_terminal_name_raw<'tree>(macro_node: &RawNode<'tree>) -> std::borrow::Cow<'tree, str> {
    let Some(macro_id) = macro_node.field("macro") else {
        return std::borrow::Cow::Borrowed("");
    };
    let text = macro_id.text();
    match text {
        std::borrow::Cow::Borrowed(borrowed) => {
            std::borrow::Cow::Borrowed(borrowed.rsplit("::").next().unwrap_or("").trim())
        }
        std::borrow::Cow::Owned(owned) => {
            std::borrow::Cow::Owned(owned.rsplit("::").next().unwrap_or("").trim().to_string())
        }
    }
}

/// Returns true if a Rust `macro_invocation` node invokes an assertion macro
/// (`assert!`, `assert_*!`, `debug_assert!`, `debug_assert_*!`, `insta::assert_snapshot!`, etc.).
#[must_use]
fn is_assertion_macro_raw(macro_node: &RawNode<'_>) -> bool {
    let terminal = macro_terminal_name_raw(macro_node);
    terminal == "assert"
        || terminal.starts_with("assert_")
        || terminal == "debug_assert"
        || terminal.starts_with("debug_assert_")
}

/// Recursively counts top-level assertion macro invocations in a Rust test function body.
fn count_rust_assertions(node: &RawNode<'_>) -> usize {
    let kind = node.kind();
    if kind == "function_item" {
        return 0;
    }
    if kind == "macro_invocation" && is_assertion_macro_raw(node) {
        return 1;
    }
    node.children()
        .map(|child| count_rust_assertions(&child))
        .sum()
}

/// Collects all outermost Rust test functions together with their `(name_node, func_name, assertion_count)`.
#[must_use]
pub fn collect_test_function_assertion_counts(
    file: &ParsedFile,
) -> Vec<(AstNode<'_>, String, usize)> {
    let mut test_funcs = Vec::new();
    collect_outer_test_functions_rec(&file.grep.root(), &mut test_funcs);
    test_funcs
        .into_iter()
        .filter_map(|func_node| {
            let name_node = func_node.field("name")?;
            let body_node = func_node.field("body")?;
            let func_name = name_node.text().to_string();
            let count = count_rust_assertions(&body_node);
            Some((AstNode::from_raw(name_node), func_name, count))
        })
        .collect()
}

/// Collects all `macro_invocation` nodes in `file`.
#[must_use]
pub fn collect_macro_invocations(file: &ParsedFile) -> Vec<AstNode<'_>> {
    file.grep
        .root()
        .dfs()
        .filter(|node| node.kind() == "macro_invocation")
        .map(AstNode::from_raw)
        .collect()
}

/// Extracts the non-delimiter child nodes (`(`, `)`, `[`, `]`, `,`) of a token tree or sequence node.
fn non_delimiter_children<'a>(node: &RawNode<'a>) -> Vec<RawNode<'a>> {
    node.children()
        .filter(|child| !matches!(child.kind().as_ref(), "(" | ")" | "[" | "]" | ","))
        .collect()
}

/// Returns true if a Rust `macro_invocation`'s `token_tree` contains a top-level `&&` logical operator.
#[must_use]
pub fn has_top_level_logical_and(macro_node: &AstNode<'_>) -> bool {
    let Some(token_tree) = macro_node
        .raw
        .children()
        .find(|child| child.kind() == "token_tree")
    else {
        return false;
    };
    let meaningful: Vec<_> = token_tree
        .children()
        .filter(|child| child.kind() != "(" && child.kind() != ")")
        .collect();

    if meaningful.iter().any(|child| child.kind() == "&&") {
        return true;
    }

    meaningful.len() == 1
        && meaningful[0].kind() == "token_tree"
        && meaningful[0].children().any(|child| child.kind() == "&&")
}

/// Extracts the argument nodes inside a Rust `macro_invocation`'s `token_tree`.
#[must_use]
pub fn extract_macro_arguments<'a>(macro_node: &AstNode<'a>) -> Vec<AstNode<'a>> {
    macro_node
        .raw
        .children()
        .find(|child| child.kind() == "token_tree")
        .map_or_else(Vec::new, |token_tree| {
            non_delimiter_children(&token_tree)
                .into_iter()
                .map(AstNode::from_raw)
                .collect()
        })
}

/// Returns true if `node` is a Rust tuple, array, or parenthesized macro `token_tree` consisting
/// solely of `>= 2` boolean literals (`true` / `false`).
#[must_use]
pub fn is_boolean_literal_collection(node: &AstNode<'_>) -> bool {
    let kind = node.raw.kind();
    if !matches!(
        kind.as_ref(),
        "token_tree" | "array_expression" | "tuple_expression"
    ) {
        return false;
    }
    let items = non_delimiter_children(&node.raw);
    items.len() >= 2
        && items
            .iter()
            .all(|item| matches!(item.kind().as_ref(), "boolean_literal" | "true" | "false"))
}

/// Resolves `(full_path, terminal_name)` if `token_tree` is immediately preceded by `!` and a macro path
/// (such as `indoc! { ... }` or `indoc::indoc! { ... }` inside an outer `token_tree`).
fn resolve_preceding_macro_path(token_tree: &RawNode<'_>) -> Option<(String, String)> {
    let bang = token_tree.prev()?;
    if bang.text() != "!" {
        return None;
    }
    let macro_ident = bang.prev()?;
    let terminal = macro_ident.text().trim().to_string();
    let mut full_path = terminal.clone();
    let mut curr = macro_ident;
    while let Some(colon_colon) = curr.prev() {
        if colon_colon.text() == "::"
            && let Some(prev_ident) = colon_colon.prev()
        {
            full_path = format!("{prev}::{full_path}", prev = prev_ident.text().trim());
            curr = prev_ident;
        } else {
            break;
        }
    }
    Some((full_path, terminal))
}

/// Returns true if `node` is preceded by `@` (an `insta` inline snapshot literal `@"..."`).
#[must_use]
fn is_insta_inline_snapshot(node: &RawNode<'_>) -> bool {
    node.prev().is_some_and(|prev| prev.text() == "@")
}

/// Returns true if `node` is enclosed inside a `#[doc = "..."]` attribute.
#[must_use]
fn is_enclosed_in_doc_attribute(node: &RawNode<'_>) -> bool {
    node.ancestors()
        .any(|ancestor| ancestor.kind() == "attribute_item" && is_doc_attribute(&ancestor))
}

/// Returns true if `node` is enclosed in a Rust `macro_invocation` (or nested macro `token_tree`)
/// within the current scope whose `(full_path, terminal_name)` satisfies `predicate`.
#[must_use]
fn is_enclosed_in_macro(node: &RawNode<'_>, predicate: &impl Fn(&str, &str) -> bool) -> bool {
    for ancestor in node.ancestors() {
        match ancestor.kind().as_ref() {
            "function_item" | "closure_expression" => break,
            "macro_invocation" => {
                let terminal = macro_terminal_name_raw(&ancestor);
                let full_path = ancestor
                    .field("macro")
                    .map(|macro_id| macro_id.text().trim().to_string())
                    .unwrap_or_default();
                if predicate(&full_path, terminal.as_ref()) {
                    return true;
                }
            }
            "token_tree" => {
                if let Some((full_path, terminal)) = resolve_preceding_macro_path(&ancestor)
                    && predicate(&full_path, &terminal)
                {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// Returns true if `node` is a Rust string literal node that spans multiple lines and contains
/// runtime newlines (raw strings spanning lines, or standard strings with at least one intermediate
/// line not ending in a `\` line continuation).
#[must_use]
fn is_multiline_string_literal(node: &RawNode<'_>) -> bool {
    if node.end_pos().line() <= node.start_pos().line() {
        return false;
    }
    let kind = node.kind();
    let is_string = matches!(
        kind.as_ref(),
        "string_literal"
            | "raw_string_literal"
            | "byte_string_literal"
            | "raw_byte_string_literal"
            | "c_string_literal"
            | "raw_c_string_literal"
    );
    if !is_string {
        return false;
    }
    if kind.starts_with("raw_") {
        return true;
    }
    let text = node.text();
    let lines: Vec<&str> = text.lines().collect();
    lines
        .iter()
        .take(lines.len().saturating_sub(1))
        .any(|line| !line.trim_end().ends_with('\\'))
}

/// Collects all Rust multiline string literal nodes in `file` that are not doc attributes,
/// `insta` inline snapshots, or enclosed in a macro matching `is_allowed_wrapper`.
#[must_use]
pub fn find_unwrapped_multiline_strings(
    file: &ParsedFile,
    is_allowed_wrapper: impl Fn(&str, &str) -> bool,
) -> Vec<AstNode<'_>> {
    file.grep
        .root()
        .dfs()
        .filter(|node| {
            is_multiline_string_literal(node)
                && !is_insta_inline_snapshot(node)
                && !is_enclosed_in_doc_attribute(node)
                && !is_enclosed_in_macro(node, &is_allowed_wrapper)
        })
        .map(AstNode::from_raw)
        .collect()
}

/// If `node` is a Rust `function_item`, returns its `(name, is_top_level)` where `is_top_level`
/// is true when declared directly at `source_file` scope.
#[must_use]
pub(super) fn function_name_and_is_top_level<'a>(
    node: &RawNode<'a>,
) -> Option<(std::borrow::Cow<'a, str>, bool)> {
    if node.kind() != "function_item" {
        return None;
    }
    let name = node.field("name")?.text();
    let is_top_level = node
        .parent()
        .is_some_and(|parent| parent.kind() == "source_file");
    Some((name, is_top_level))
}

/// Collects all CST nodes in `file` that introduce a second path to an item:
/// - `#[macro_export]` attributes (`attribute_item`)
/// - Visible `use` declarations (`pub use`, `pub(crate) use`, etc.), except `pub(crate) use <name>;`
///   for a `macro_rules! <name>` defined in the same `file`.
#[must_use]
pub fn collect_second_path_declarations(file: &ParsedFile) -> Vec<AstNode<'_>> {
    let defined_macros: std::collections::HashSet<String> = file
        .grep
        .root()
        .dfs()
        .filter(|node| node.kind() == "macro_definition")
        .filter_map(|node| Some(node.field("name")?.text().into_owned()))
        .collect();

    file.grep
        .root()
        .dfs()
        .filter(|node| match node.kind().as_ref() {
            "attribute_item" => attribute_terminal_name(node)
                .is_some_and(|terminal| terminal.text() == "macro_export"),
            "use_declaration" => {
                let Some(visibility) = node
                    .children()
                    .find(|child| child.kind() == "visibility_modifier")
                else {
                    return false;
                };
                let is_own_macro_path = visibility.text().trim() == "pub(crate)"
                    && node.field("argument").is_some_and(|argument| {
                        argument.kind() == "identifier"
                            && defined_macros.contains(argument.text().as_ref())
                    });
                !is_own_macro_path
            }
            _ => false,
        })
        .map(AstNode::from_raw)
        .collect()
}

/// Collects all `use_declaration` CST nodes in `file` that import via a relative `super` path segment.
#[must_use]
pub fn collect_relative_use_declarations(file: &ParsedFile) -> Vec<AstNode<'_>> {
    file.grep
        .root()
        .dfs()
        .filter(|node| {
            node.kind() == "use_declaration" && node.dfs().any(|child| child.kind() == "super")
        })
        .map(AstNode::from_raw)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast_grep_language::SupportLang;

    #[test]
    fn test_collect_bindings_rust() {
        let source = indoc::indoc! {r"
            use std;
            use std::collections::HashMap;
            use std::io::{self, Read};
            use std::sync::Arc as MyArc;
            use std::{io::{self as my_io, Write}, fs};
            use std::collections::*; // wildcard
            fn main() {
                let a = b;
                let mut c = 1;
                let (d, e) = (1, 2);
                let Point { x: f, y: _ } = p;
                let Point { g, h } = p;
                for i in 0..10 {}
                let f = |x: i32| x + 1;
                if let Some(y) = val {}
                while let Some(z) = val {}
                match val {
                    Some(w) if w > 0 => {}
                    None => {}
                }
            }
            fn my_func(j: i32) {}
            struct MyStruct;
            enum MyEnum { Variant }
            trait MyTrait {}
            type MyAlias = i32;
            trait Other {
                type MyAssoc;
            }
            const MY_CONST: i32 = 1;
            static MY_STATIC: i32 = 2;
        "};
        let file = ParsedFile::new(source, SupportLang::Rust);
        let bindings = collect_bindings(&file);
        let names: Vec<String> = bindings
            .iter()
            .map(|node| node.text().to_string())
            .collect();
        assert_eq!(
            names,
            vec![
                "std",
                "HashMap",
                "io",
                "Read",
                "MyArc",
                "my_io",
                "Write",
                "fs",
                "main",
                "a",
                "c",
                "d",
                "e",
                "f",
                "g",
                "h",
                "i",
                "f",
                "x",
                "y",
                "z",
                "w",
                "my_func",
                "j",
                "MyStruct",
                "MyEnum",
                "MyTrait",
                "MyAlias",
                "Other",
                "MyAssoc",
                "MY_CONST",
                "MY_STATIC",
            ]
        );
    }

    #[test]
    fn test_collect_bindings_rust_negatives() {
        let source = "fn main() { let x: MyStruct = MyStruct; }";
        let file = ParsedFile::new(source, SupportLang::Rust);
        let bindings = collect_bindings(&file);
        let names: Vec<String> = bindings
            .iter()
            .map(|node| node.text().to_string())
            .collect();
        assert_eq!(names, vec!["main", "x"]);
    }
}
