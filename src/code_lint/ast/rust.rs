//! AST helper predicates for structural traversal in Rust.

// omni:disable-file [repeated-literal] -- Tree-sitter node kinds and field names (see ROADMAP)

use crate::code_lint::ast::{
    AstNode, LiteralOccurrence, LiteralRole, LiteralValue, NullableCollectionReturn, ParsedFile,
    PositionalRead, RawNode, ScopePositionalReads, delimited_string_parts, parse_float_literal,
    parse_integer_literal,
};

/// Returns true for Rust node kinds that hold statements as direct children.
///
/// `source_file` is the file root, `block` is a braced body, and `declaration_list` is the
/// body of an `impl`, `trait`, or inline `mod`.
#[must_use]
pub fn is_statement_container(kind: &str) -> bool {
    matches!(kind, "source_file" | "block" | "declaration_list")
}

/// Returns the definition wrapped by a decorator statement. Rust has none: attributes
/// (`#[...]`) are sibling nodes of the item they annotate, not wrappers around it.
#[must_use]
pub const fn decorated_definition<'a>(_statement: &RawNode<'a>) -> Option<RawNode<'a>> {
    None
}

/// Returns the 1-indexed starting line of the earliest outer `#[...]` attribute sibling attached
/// to `statement`, or `None` if `statement` has no preceding outer attributes.
#[must_use]
pub(super) fn earliest_attribute_start_line(statement: &RawNode<'_>) -> Option<usize> {
    preceding_attributes(statement)
        .last()
        .map(|attr| attr.start_pos().line() + 1)
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
            let name_range = node.field("name").map(|name_node| {
                let range = name_node.range();
                bindings.push(AstNode::from_raw(name_node));
                range
            });
            for child in node.children() {
                if name_range.as_ref() == Some(&child.range()) {
                    continue;
                }
                traverse_rust(&child, bindings);
            }
        }
        "use_declaration" => {
            extract_from_use(node, bindings, None);
        }
        "field_declaration" => {
            if let Some(name) = node.field("name") {
                bindings.push(AstNode::from_raw(name));
            }
        }
        _ => {
            for child in node.children() {
                traverse_rust(&child, bindings);
            }
        }
    }
}

/// Collects all binding definitions (variables, functions, structs, named struct fields, etc.)
/// within `file`.
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
    matches!(
        non_delimiter_children(&token_tree).as_slice(),
        [argument] if argument.kind() == "identifier" && argument.text() == "test"
    )
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

/// Returns true if a Rust item is preceded by a test attribute (`#[test]`, `#[tokio::test]`, `#[rstest]`, etc.).
#[must_use]
fn has_test_attribute(node: &RawNode<'_>) -> bool {
    preceding_attributes(node).any(|sibling| is_test_attribute(&sibling))
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
    let mut first_attribute_start: Option<usize> = None;
    let mut pending_test_attribute = false;

    for child in node.children() {
        let kind = child.kind();
        if kind == "attribute_item" {
            first_attribute_start.get_or_insert_with(|| child.range().start);
            if is_conditional_test_attribute(&child) || is_test_attribute(&child) {
                pending_test_attribute = true;
            }
            continue;
        }
        if is_comment_kind(kind.as_ref()) {
            continue;
        }
        if pending_test_attribute {
            let start = first_attribute_start.unwrap_or_else(|| child.range().start);
            ranges.push(start..child.range().end);
            first_attribute_start = None;
            pending_test_attribute = false;
            continue;
        }
        first_attribute_start = None;
        if kind != "token_tree" {
            collect_inline_test_ranges_rec(&child, ranges);
        }
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

    matches!(
        meaningful.as_slice(),
        [only_child] if only_child.kind() == "token_tree"
            && only_child.children().any(|child| child.kind() == "&&")
    )
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
                let full_text = ancestor.field("macro").map(|macro_id| macro_id.text());
                let full_path = full_text.as_deref().map_or("", str::trim);
                if predicate(full_path, terminal.as_ref()) {
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
    let mut lines = text.lines();
    lines.next_back();
    lines.any(|line| !line.trim_end().ends_with('\\'))
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
    /// `"super::AstNode"`, `"self::rust::collect_bindings"`, `"ast_grep_core::Node"`).
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
    /// and `::`-qualified paths inside macro `token_tree` arguments).
    pub referenced_paths: Vec<RustPathReference>,
}

fn compact_path_text(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn join_use_prefix(prefix: &str, segment: &str) -> String {
    let clean = compact_path_text(segment);
    if prefix.is_empty() {
        clean
    } else if clean == "self" {
        prefix.to_string()
    } else if let Some(rest) = clean.strip_prefix("self::") {
        format!("{prefix}::{rest}")
    } else {
        format!("{prefix}::{clean}")
    }
}

fn expand_use_tree(node: &RawNode<'_>, prefix: &str, out: &mut Vec<String>) {
    match node.kind().as_ref() {
        "use_declaration" => {
            if let Some(argument) = node.field("argument") {
                expand_use_tree(&argument, prefix, out);
            }
        }
        "identifier" | "type_identifier" | "crate" | "self" | "super" => {
            let text = node.text();
            if text != "_" {
                out.push(join_use_prefix(prefix, text.as_ref()));
            }
        }
        "scoped_identifier" => {
            out.push(join_use_prefix(prefix, node.text().as_ref()));
        }
        "use_as_clause" => {
            if let Some(path) = node.field("path") {
                expand_use_tree(&path, prefix, out);
            }
        }
        "use_wildcard" => {
            if let Some(path_child) = node.children().find(|child| {
                let kind = child.kind();
                kind != "::" && kind != "*" && !is_comment_kind(kind.as_ref())
            }) {
                expand_use_tree(&path_child, prefix, out);
            } else if !prefix.is_empty() {
                out.push(prefix.to_string());
            }
        }
        "scoped_use_list" => {
            let next_prefix = node.field("path").map_or_else(
                || prefix.to_string(),
                |path| join_use_prefix(prefix, path.text().as_ref()),
            );
            if let Some(list) = node.field("list") {
                expand_use_tree(&list, &next_prefix, out);
            }
        }
        "use_list" => {
            for child in node.children() {
                let kind = child.kind();
                if kind != "{" && kind != "}" && kind != "," && !is_comment_kind(kind.as_ref()) {
                    expand_use_tree(&child, prefix, out);
                }
            }
        }
        _ => {}
    }
}

fn is_pure_path_segment(node: &RawNode<'_>) -> bool {
    matches!(
        node.kind().as_ref(),
        "identifier" | "type_identifier" | "crate" | "self" | "super"
    ) || is_pure_scoped_path(node)
}

fn is_pure_scoped_path(node: &RawNode<'_>) -> bool {
    matches!(
        node.kind().as_ref(),
        "scoped_identifier" | "scoped_type_identifier"
    ) && node
        .field("path")
        .is_some_and(|path| is_pure_path_segment(&path))
}

fn is_token_path_segment(kind: &str) -> bool {
    matches!(
        kind,
        "identifier" | "type_identifier" | "crate" | "self" | "super"
    )
}

fn collect_token_tree_paths(
    token_tree: &RawNode<'_>,
    statement_text: &str,
    out: &mut Vec<RustPathReference>,
) {
    let children: Vec<RawNode<'_>> = token_tree.children().collect();
    let mut index = 0;
    while index < children.len() {
        let current = &children[index];
        if current.kind() == "token_tree" {
            collect_token_tree_paths(current, statement_text, out);
            index += 1;
            continue;
        }
        if is_token_path_segment(current.kind().as_ref())
            && index + 2 < children.len()
            && children[index + 1].kind() == "::"
            && is_token_path_segment(children[index + 2].kind().as_ref())
        {
            let line = current.start_pos().line() + 1;
            let mut segments = vec![current.text().trim().to_string()];
            index += 1;
            while index + 1 < children.len()
                && children[index].kind() == "::"
                && is_token_path_segment(children[index + 1].kind().as_ref())
            {
                segments.push(children[index + 1].text().trim().to_string());
                index += 2;
            }
            out.push(RustPathReference {
                line,
                raw_path: segments.join("::"),
                statement_text: statement_text.to_string(),
            });
            continue;
        }
        index += 1;
    }
}

fn summarize_rust_node(
    node: &RawNode<'_>,
    test_ranges: &[std::ops::Range<usize>],
    defined_macros: &std::collections::HashSet<String>,
    summary: &mut RustFileSummary,
) {
    let start_offset = node.range().start;
    if test_ranges
        .iter()
        .any(|range| range.contains(&start_offset))
    {
        return;
    }

    let kind = node.kind();
    match kind.as_ref() {
        "attribute_item" => {
            if attribute_terminal_name(node)
                .is_some_and(|terminal| terminal.text() == "macro_export")
            {
                summary.macro_exports.push("#[macro_export]".to_string());
            }
        }
        "use_declaration" => {
            let line = node.start_pos().line() + 1;
            let declaration_text = node.text().trim().to_string();
            let mut target_paths = Vec::new();
            expand_use_tree(node, "", &mut target_paths);
            for raw_path in &target_paths {
                summary.referenced_paths.push(RustPathReference {
                    line,
                    raw_path: raw_path.clone(),
                    statement_text: declaration_text.clone(),
                });
            }
            if let Some(visibility) = node
                .children()
                .find(|child| child.kind() == "visibility_modifier")
            {
                let is_own_macro_path = visibility.text().trim() == "pub(crate)"
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
            return;
        }
        "macro_definition" => {
            return;
        }
        "macro_invocation" => {
            if macro_terminal_name_raw(node) == "architecture_component" {
                let component_argument: String =
                    extract_macro_arguments(&AstNode::from_raw(node.clone()))
                        .into_iter()
                        .map(|argument| argument.text().into_owned())
                        .collect();
                summary.architecture_components.push(component_argument);
            }
            if let Some(token_tree) = node.children().find(|child| child.kind() == "token_tree") {
                let statement_text = node.text().trim().to_string();
                collect_token_tree_paths(
                    &token_tree,
                    &statement_text,
                    &mut summary.referenced_paths,
                );
            }
        }
        "scoped_identifier" | "scoped_type_identifier" => {
            if is_pure_scoped_path(node) {
                if node
                    .parent()
                    .is_none_or(|parent| !is_pure_scoped_path(&parent))
                {
                    summary.referenced_paths.push(RustPathReference {
                        line: node.start_pos().line() + 1,
                        raw_path: compact_path_text(node.text().as_ref()),
                        statement_text: node.text().trim().to_string(),
                    });
                }
                return;
            }
        }
        _ => {}
    }

    for child in node.children() {
        if child.kind() != "token_tree" {
            summarize_rust_node(&child, test_ranges, defined_macros, summary);
        }
    }
}

/// Extracts a complete structural and dependency summary of the production code in `file`
/// in a single CST pass, skipping `#[cfg(test)]` and `#[test]` items.
#[must_use]
pub fn summarize_rust_file(file: &ParsedFile) -> RustFileSummary {
    let root = file.grep.root();
    let test_ranges = collect_inline_test_ranges(file);
    let mut summary = RustFileSummary::default();
    let mut defined_macros = std::collections::HashSet::new();

    for child in root.children() {
        let start_offset = child.range().start;
        if test_ranges
            .iter()
            .any(|range| range.contains(&start_offset))
        {
            continue;
        }
        let kind = child.kind();
        if child.is_extra()
            || is_comment_kind(kind.as_ref())
            || matches!(kind.as_ref(), "attribute_item" | "inner_attribute_item")
        {
            continue;
        }
        if kind == "mod_item" && child.field("body").is_none() {
            if let Some(name_node) = child.field("name") {
                let is_private = !child
                    .children()
                    .any(|grandchild| grandchild.kind() == "visibility_modifier");
                summary.external_mods.push(ExternalModDeclaration {
                    name: name_node.text().trim().to_string(),
                    declaration_text: child.text().trim().to_string(),
                    is_private,
                });
            }
            continue;
        }
        if kind == "macro_definition" {
            if let Some(name_node) = child.field("name") {
                defined_macros.insert(name_node.text().into_owned());
            }
            summary.macro_definitions.push((
                child.start_pos().line() + 1,
                child.text().trim().to_string(),
            ));
            continue;
        }
        summary.non_namespace_items.push((
            child.start_pos().line() + 1,
            child.text().trim().to_string(),
        ));
    }

    for child in root.children() {
        summarize_rust_node(&child, &test_ranges, &defined_macros, &mut summary);
    }

    summary
}

/// Returns true if `receiver` names a stable value: a name, `self` or field chain with no call
/// inside, so that identical text means the same value.
fn is_stable_receiver(receiver: &RawNode<'_>) -> bool {
    matches!(
        receiver.kind().as_ref(),
        "identifier" | "self" | "field_expression"
    ) && !receiver.dfs().any(|node| node.kind() == "call_expression")
}

/// Returns true if `node` is assigned to (`t.0 = x`, `t.0 += x`, `(t.0, t.1) = (b, a)`) or
/// mutably borrowed (`&mut t.0`).
fn is_mutated(node: &RawNode<'_>) -> bool {
    let mut place = node.clone();
    while let Some(parent) = place.parent() {
        match parent.kind().as_ref() {
            "tuple_expression" | "parenthesized_expression" => place = parent,
            "assignment_expression" | "compound_assignment_expr" => {
                return parent
                    .field("left")
                    .is_some_and(|left| left.range() == place.range());
            }
            "reference_expression" => {
                return parent
                    .children()
                    .any(|child| child.kind() == "mutable_specifier");
            }
            _ => return false,
        }
    }
    false
}

/// Records a tuple-field access (`span.0`) as a positional read, or its receiver as an exempt
/// receiver when the field is mutated.
fn record_field_expression<'a>(node: &RawNode<'a>, scope: &mut ScopePositionalReads<'a>) {
    let (Some(receiver), Some(field)) = (node.field("value"), node.field("field")) else {
        return;
    };
    if field.kind() != "integer_literal" {
        return;
    }
    if is_mutated(node) {
        scope.exempt_receivers.insert(receiver.text().into_owned());
    } else if is_stable_receiver(&receiver)
        && let Ok(position) = field.text().parse()
    {
        scope.reads.push(PositionalRead {
            node: AstNode::from_raw(node.clone()),
            receiver: receiver.text().into_owned(),
            position,
        });
    }
}

/// Walks `node`, recording reads and mutations into the enclosing function's `scope` (none
/// outside functions; closures belong to it) and pushing each finished function scope to `out`.
fn collect_positional_reads_rec<'a>(
    node: &RawNode<'a>,
    mut scope: Option<&mut ScopePositionalReads<'a>>,
    out: &mut Vec<ScopePositionalReads<'a>>,
) {
    match node.kind().as_ref() {
        "function_item" => {
            let mut function_scope = ScopePositionalReads::default();
            for child in node.children() {
                collect_positional_reads_rec(&child, Some(&mut function_scope), out);
            }
            out.push(function_scope);
            return;
        }
        "field_expression" => {
            if let Some(scope) = scope.as_deref_mut() {
                record_field_expression(node, scope);
            }
        }
        _ => {}
    }
    for child in node.children() {
        collect_positional_reads_rec(&child, scope.as_deref_mut(), out);
    }
}

/// Collects Rust tuple-field reads grouped by function (see [`super::collect_positional_reads`]).
#[must_use]
pub fn collect_positional_reads(file: &ParsedFile) -> Vec<ScopePositionalReads<'_>> {
    let mut out = Vec::new();
    collect_positional_reads_rec(&file.grep.root(), None, &mut out);
    out
}

/// Macros whose literal arguments the language or API requires as written: format strings,
/// assertion and logging messages, and compile-time inputs. Matched on the last path segment.
const LITERAL_EXEMPT_MACROS: &[&str] = &[
    "format",
    "print",
    "println",
    "eprint",
    "eprintln",
    "write",
    "writeln",
    "panic",
    "todo",
    "unimplemented",
    "unreachable",
    "assert",
    "assert_eq",
    "assert_ne",
    "debug_assert",
    "debug_assert_eq",
    "debug_assert_ne",
    "bail",
    "ensure",
    "anyhow",
    "error",
    "warn",
    "info",
    "debug",
    "trace",
    "indoc",
    "concat",
    "env",
    "option_env",
    "include_str",
    "include_bytes",
];

/// Returns the value of a Rust integer literal, resolving its type suffix (`30u64`, `0xffu8`);
/// a decimal literal with an `f32` / `f64` suffix (`1f32`) is a float.
fn integer_literal_value(text: &str) -> Option<LiteralValue> {
    let is_hex = text.starts_with("0x");
    let digits_start = if is_hex || text.starts_with("0o") || text.starts_with("0b") {
        2
    } else {
        0
    };
    let suffix_start = text[digits_start..]
        .find(|character: char| {
            if is_hex {
                matches!(character, 'u' | 'i')
            } else {
                character.is_ascii_alphabetic()
            }
        })
        .map_or(text.len(), |offset| digits_start + offset);
    let (number, suffix) = text.split_at(suffix_start);
    if matches!(suffix, "f32" | "f64") {
        parse_float_literal(number)
    } else {
        parse_integer_literal(number)
    }
}

/// Returns the value of a string, byte-string, integer or float literal, or of a `-` applied
/// to a number; `None` for anything else (C strings, characters) and overflow.
fn literal_value(node: &RawNode<'_>) -> Option<LiteralValue> {
    match node.kind().as_ref() {
        "string_literal" => {
            let (opening, content) = delimited_string_parts(node);
            match opening.chars().next() {
                Some('b') => Some(LiteralValue::Bytes(content)),
                Some('c') => None,
                _ => Some(LiteralValue::Str(content)),
            }
        }
        "raw_string_literal" => {
            // Raw backslashes and quotes are literal: spell them as a plain string would.
            let content = node
                .children()
                .find(|child| child.kind() == "string_content")
                .map(|child| child.text().replace('\\', "\\\\").replace('"', "\\\""))
                .unwrap_or_default();
            match node.text().chars().next() {
                Some('b') => Some(LiteralValue::Bytes(content)),
                Some('c') => None,
                _ => Some(LiteralValue::Str(content)),
            }
        }
        "integer_literal" => integer_literal_value(&node.text()),
        "float_literal" => {
            let text = node.text();
            let number = text
                .strip_suffix("f32")
                .or_else(|| text.strip_suffix("f64"))
                .unwrap_or(&text);
            parse_float_literal(number)
        }
        "unary_expression" | "negative_literal" => {
            let mut children = node.children();
            match (children.next(), children.next(), children.next()) {
                (Some(sign), Some(number), None)
                    if sign.text() == "-"
                        && matches!(
                            number.kind().as_ref(),
                            "integer_literal" | "float_literal"
                        ) =>
                {
                    literal_value(&number)?.negated()
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Returns true if `name` (a macro's last path segment) is in [`LITERAL_EXEMPT_MACROS`].
fn is_literal_exempt_macro(name: &str) -> bool {
    LITERAL_EXEMPT_MACROS.contains(&name)
}

/// Walks `node`, pushing collectable literals to `out` (see [`super::collect_literal_occurrences`]).
fn collect_literal_occurrences_rec<'a>(node: &RawNode<'a>, out: &mut Vec<LiteralOccurrence<'a>>) {
    match node.kind().as_ref() {
        "attribute_item" | "inner_attribute_item" | "extern_modifier" => return,
        "macro_invocation" if is_literal_exempt_macro(&macro_terminal_name_raw(node)) => return,
        "token_tree"
            if resolve_preceding_macro_path(node)
                .is_some_and(|(_, terminal)| is_literal_exempt_macro(&terminal)) =>
        {
            return;
        }
        // `pair.0`: the tuple position is a field name, not a value.
        "field_expression" => {
            if let Some(value) = node.field("value") {
                collect_literal_occurrences_rec(&value, out);
            }
            return;
        }
        // A scalar constant defines its value; a composite one (`const NAMES: &[&str] = ...`)
        // is a named value whose parts are not collected. A `static mut` is not a constant.
        "const_item" | "static_item" | "enum_variant"
            if !node
                .children()
                .any(|child| child.kind() == "mutable_specifier") =>
        {
            if let Some(value_node) = node.field("value")
                && let Some(value) = literal_value(&value_node)
            {
                out.push(LiteralOccurrence {
                    node: AstNode::from_raw(value_node),
                    value,
                    role: LiteralRole::ConstantDefinition,
                });
            }
            return;
        }
        _ => {
            if let Some(value) = literal_value(node) {
                out.push(LiteralOccurrence {
                    node: AstNode::from_raw(node.clone()),
                    value,
                    role: LiteralRole::Inline,
                });
                return;
            }
        }
    }
    let is_token_tree = node.kind() == "token_tree";
    let mut follows_minus = false;
    for child in node.children() {
        // A macro's `-42` is a bare `-` token and a number: skip the number rather than
        // collect it as `42`.
        let is_signed_number =
            follows_minus && matches!(child.kind().as_ref(), "integer_literal" | "float_literal");
        follows_minus = is_token_tree && child.kind() == "-";
        if !is_signed_number {
            collect_literal_occurrences_rec(&child, out);
        }
    }
}

/// Collects Rust literal occurrences (see [`super::collect_literal_occurrences`]).
#[must_use]
pub fn collect_literal_occurrences(file: &ParsedFile) -> Vec<LiteralOccurrence<'_>> {
    let mut out = Vec::new();
    collect_literal_occurrences_rec(&file.grep.root(), &mut out);
    out
}

/// Resolves `(full_path, terminal_name)` of a Rust type constructor node (`type_identifier` or
/// `scoped_type_identifier`), stripping whitespace and any leading `::`.
fn resolve_rust_type_path(node: &RawNode<'_>) -> Option<(String, String)> {
    if !matches!(
        node.kind().as_ref(),
        "type_identifier" | "scoped_type_identifier"
    ) {
        return None;
    }
    let raw_path = compact_path_text(node.text().as_ref());
    let normalized = raw_path
        .strip_prefix("::")
        .unwrap_or(raw_path.as_str())
        .to_owned();
    let terminal = normalized
        .rsplit("::")
        .next()
        .unwrap_or(normalized.as_str())
        .to_owned();
    Some((normalized, terminal))
}

/// Extracts `(base_type_node, type_argument_nodes)` from a Rust `generic_type` node, excluding
/// lifetime parameters (`'a`, `'static`) and associated type bindings (`Item = T`).
fn extract_rust_generic_base_and_args<'a>(
    node: &RawNode<'a>,
) -> Option<(RawNode<'a>, Vec<RawNode<'a>>)> {
    if node.kind() != "generic_type" {
        return None;
    }
    let base_node = node.field("type")?;
    let args_node = node.field("type_arguments")?;
    let type_args = args_node
        .children()
        .filter(|child| {
            child.is_named()
                && !child.is_extra()
                && !matches!(child.kind().as_ref(), "lifetime" | "type_binding")
        })
        .collect();
    Some((base_node, type_args))
}

/// Unwraps outer `Result<T, ...>` and `Poll<T>` return type envelopes from `node`.
fn unwrap_rust_return_envelope<'a>(node: &RawNode<'a>) -> RawNode<'a> {
    let mut current = node.clone();
    while current.kind() == "generic_type" {
        let Some((base_node, type_args)) = extract_rust_generic_base_and_args(&current) else {
            break;
        };
        let Some((base_path, base_terminal)) = resolve_rust_type_path(&base_node) else {
            break;
        };
        let is_result_or_poll = base_terminal == "Result"
            || (base_terminal == "Poll"
                && matches!(
                    base_path.as_str(),
                    "Poll" | "task::Poll" | "std::task::Poll" | "core::task::Poll"
                ));
        if is_result_or_poll && let Some(first_arg) = type_args.into_iter().next() {
            current = first_arg;
        } else {
            break;
        }
    }
    current
}

/// Unwraps transparent borrow and smart-pointer wrappers (`&T`, `&mut T`, `Box<T>`, `Rc<T>`,
/// `Arc<T>`, `Cow<'_, T>`) around a Rust type node.
fn unwrap_rust_pointer_wrappers<'a>(node: &RawNode<'a>) -> RawNode<'a> {
    let mut current = node.clone();
    loop {
        if current.kind() == "reference_type" {
            if let Some(inner) = current.field("type") {
                current = inner;
                continue;
            }
            break;
        }
        if current.kind() == "generic_type"
            && let Some((base_node, type_args)) = extract_rust_generic_base_and_args(&current)
            && let Some((base_path, base_terminal)) = resolve_rust_type_path(&base_node)
        {
            let is_pointer_wrapper = match base_terminal.as_str() {
                "Box" => matches!(
                    base_path.as_str(),
                    "Box" | "boxed::Box" | "std::boxed::Box" | "alloc::boxed::Box"
                ),
                "Rc" => matches!(
                    base_path.as_str(),
                    "Rc" | "rc::Rc" | "std::rc::Rc" | "alloc::rc::Rc"
                ),
                "Arc" => matches!(
                    base_path.as_str(),
                    "Arc" | "sync::Arc" | "std::sync::Arc" | "alloc::sync::Arc"
                ),
                "Cow" => matches!(
                    base_path.as_str(),
                    "Cow" | "borrow::Cow" | "std::borrow::Cow" | "alloc::borrow::Cow"
                ),
                _ => false,
            };
            if is_pointer_wrapper
                && type_args.len() == 1
                && let Some(first_arg) = type_args.into_iter().next()
            {
                current = first_arg;
                continue;
            }
        }
        break;
    }
    current
}

/// Returns true if `(path, terminal)` is a standard Rust collection type constructor (`Vec`,
/// `VecDeque`, `LinkedList`, `HashMap`, `BTreeMap`, `HashSet`, `BTreeSet`, `BinaryHeap`).
fn is_rust_collection_constructor(path: &str, terminal: &str) -> bool {
    if !matches!(
        terminal,
        "Vec"
            | "VecDeque"
            | "LinkedList"
            | "HashMap"
            | "BTreeMap"
            | "HashSet"
            | "BTreeSet"
            | "BinaryHeap"
    ) {
        return false;
    }
    path == terminal
        || path.strip_suffix(terminal).is_some_and(|prefix| {
            matches!(
                prefix,
                "vec::"
                    | "std::vec::"
                    | "alloc::vec::"
                    | "collections::"
                    | "std::collections::"
                    | "alloc::collections::"
            )
        })
}

/// If `return_type_node` (or the `Ok` / `Ready` payload inside `Result` / `Poll`) is
/// `Option<Collection>`, returns the matched collection type constructor or slice type text.
fn extract_rust_nullable_collection_type(return_type_node: &RawNode<'_>) -> Option<String> {
    let unwrapped_envelope = unwrap_rust_return_envelope(return_type_node);
    let (option_base, option_args) = extract_rust_generic_base_and_args(&unwrapped_envelope)?;
    let (option_path, option_terminal) = resolve_rust_type_path(&option_base)?;
    if option_terminal != "Option"
        || !matches!(
            option_path.as_str(),
            "Option" | "option::Option" | "std::option::Option" | "core::option::Option"
        )
        || option_args.len() != 1
    {
        return None;
    }

    let inner = unwrap_rust_pointer_wrappers(&option_args[0]);
    match inner.kind().as_ref() {
        "slice_type" => Some(inner.text().trim().to_owned()),
        "array_type" if inner.field("length").is_none() => Some(inner.text().trim().to_owned()),
        "generic_type" => {
            let (base_node, _) = extract_rust_generic_base_and_args(&inner)?;
            let (base_path, base_terminal) = resolve_rust_type_path(&base_node)?;
            is_rust_collection_constructor(&base_path, &base_terminal)
                .then(|| compact_path_text(base_node.text().as_ref()))
        }
        _ => None,
    }
}

/// Returns true if `func_node` is declared inside a `trait` block or an `impl Trait for Type` block.
fn is_trait_or_trait_impl_function(func_node: &RawNode<'_>) -> bool {
    let Some(body) = func_node.parent() else {
        return false;
    };
    if body.kind() != "declaration_list" {
        return false;
    }
    let Some(enclosing_item) = body.parent() else {
        return false;
    };
    enclosing_item.kind() == "trait_item"
        || (enclosing_item.kind() == "impl_item" && enclosing_item.field("trait").is_some())
}

/// Collects all non-exempt Rust function/method return types in `file` that wrap a collection
/// type in `Option<...>`.
#[must_use]
pub fn collect_nullable_collection_returns(file: &ParsedFile) -> Vec<NullableCollectionReturn<'_>> {
    let test_ranges = collect_inline_test_ranges(file);
    let mut out = Vec::new();
    for node in file.grep.root().dfs() {
        if node.kind() != "function_item" {
            continue;
        }
        let start_offset = node.range().start;
        if test_ranges
            .iter()
            .any(|range| range.contains(&start_offset))
            || is_trait_or_trait_impl_function(&node)
        {
            continue;
        }
        let (Some(name_node), Some(return_type_node)) =
            (node.field("name"), node.field("return_type"))
        else {
            continue;
        };
        if let Some(collection_type) = extract_rust_nullable_collection_type(&return_type_node) {
            out.push(NullableCollectionReturn {
                return_type_node: AstNode::from_raw(return_type_node),
                function_name: name_node.text().trim().to_owned(),
                collection_types: vec![collection_type],
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use LiteralRole::{ConstantDefinition, Inline};
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

            pub fn run(input: super::ParentType) {
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
            (summary.architecture_components, summary.visible_uses),
            (
                vec!["CodeLintAst".to_string()],
                vec![VisibleUseDeclaration {
                    line: 11,
                    declaration_text: "pub use self::detail::Exported;".to_string(),
                    target_paths: vec!["self::detail::Exported".to_string()],
                }],
            )
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

    #[rstest::rstest]
    #[case::const_defines("const MAX: u32 = 30;", &[("30", ConstantDefinition)])]
    #[case::static_defines("static MIN: i32 = -5;", &[("-5", ConstantDefinition)])]
    #[case::enum_discriminant_defines("enum E { V = 10 }", &[("10", ConstantDefinition)])]
    #[case::static_mut_is_inline("static mut LIMIT: u32 = 30;", &[("30", Inline)])]
    #[case::composite_constant_not_collected("const NAMES: &[&str] = &[\"n1\", \"n2\"];", &[])]
    #[case::negation_anchored_on_operator(
        "fn f() { g(-42, 7 - 42); match x { -42 => {} _ => {} } }",
        &[("-42", Inline), ("7", Inline), ("42", Inline), ("-42", Inline)]
    )]
    #[case::signed_number_in_token_tree_skipped("fn f() { vec![-42, 43]; }", &[("43", Inline)])]
    #[case::tuple_position_skipped("fn f() { g(pair.3, 30); }", &[("30", Inline)])]
    #[case::attribute_skipped("#[cfg(feature = \"ff\")]\nfn f() {}", &[])]
    #[case::inner_attribute_skipped("#![doc = \"dd\"]", &[])]
    #[case::extern_abi_skipped("extern \"C\" { fn g(); }", &[])]
    #[case::exempt_macro_by_path_skipped("fn f() { tracing::warn!(\"ww\"); }", &[])]
    #[case::exempt_macro_inside_collected_macro_skipped(
        "fn f() { vec![format!(\"{}\", 5), \"vv\"]; }",
        &[("\"vv\"", Inline)]
    )]
    #[case::matches_macro_collected("fn f() { matches!(k, \"ma\"); }", &[("\"ma\"", Inline)])]
    #[case::char_skipped("fn f() { g('c'); }", &[])]
    #[case::c_string_skipped("fn f() { g(c\"cs\"); }", &[])]
    fn test_collect_literal_occurrences_rust(
        #[case] source: &str,
        #[case] expected: &[(&str, LiteralRole)],
    ) {
        let file = ParsedFile::rust(source);
        let actual: Vec<(String, LiteralRole)> = collect_literal_occurrences(&file)
            .into_iter()
            .map(|occurrence| (occurrence.node.text().into_owned(), occurrence.role))
            .collect();
        let expected: Vec<(String, LiteralRole)> = expected
            .iter()
            .map(|(text, role)| ((*text).to_string(), *role))
            .collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_collect_literal_occurrences_rust_skips_every_exempt_macro() {
        for name in LITERAL_EXEMPT_MACROS {
            let file = ParsedFile::rust(&format!("fn f() {{ {name}!(\"ee\"); }}"));
            assert!(
                collect_literal_occurrences(&file).is_empty(),
                "`{name}!` arguments were collected"
            );
        }
    }

    #[rstest::rstest]
    #[case::raw_string("r#\"ab\"#", LiteralValue::Str("ab".to_string()))]
    #[case::raw_backslash_spelled_plain("r\"a\\tb\"", LiteralValue::Str("a\\\\tb".to_string()))]
    #[case::raw_quote_spelled_plain("r#\"a\"b\"#", LiteralValue::Str("a\\\"b".to_string()))]
    #[case::empty_raw_string("r\"\"", LiteralValue::Str(String::new()))]
    #[case::byte_string("b\"ab\"", LiteralValue::Bytes("ab".to_string()))]
    #[case::raw_byte_string("br\"ab\"", LiteralValue::Bytes("ab".to_string()))]
    #[case::integer_suffix("1_000_u32", LiteralValue::Int(1000))]
    #[case::hex_suffix("0xffu8", LiteralValue::Int(255))]
    #[case::hex_digits_ending_in_f32("0x1f32", LiteralValue::Int(0x1f32))]
    #[case::octal_suffix("0o17u8", LiteralValue::Int(15))]
    #[case::binary("0b11", LiteralValue::Int(3))]
    #[case::float_suffix_on_integer("1f32", LiteralValue::Float(1.0_f64.to_bits()))]
    #[case::float_suffix_after_separator("1_f32", LiteralValue::Float(1.0_f64.to_bits()))]
    #[case::float_suffix("1.5f64", LiteralValue::Float(1.5_f64.to_bits()))]
    #[case::exponent_with_suffix("1e3f32", LiteralValue::Float(1000.0_f64.to_bits()))]
    #[case::negative_zero("-0.0", LiteralValue::Float(0.0_f64.to_bits()))]
    fn test_collect_literal_occurrences_rust_values(
        #[case] literal: &str,
        #[case] expected: LiteralValue,
    ) {
        let file = ParsedFile::rust(&format!("fn f() {{ g({literal}); }}"));
        let values: Vec<LiteralValue> = collect_literal_occurrences(&file)
            .into_iter()
            .map(|occurrence| occurrence.value)
            .collect();
        assert_eq!(values, vec![expected]);
    }

    #[test]
    fn test_collect_literal_occurrences_rust_skips_overflow() {
        let file = ParsedFile::rust("fn f() { g(0xffff_ffff_ffff_ffff_ffff_ffff_ffff_ffff_f); }");
        assert!(collect_literal_occurrences(&file).is_empty());
    }

    #[rstest::rstest]
    #[case::option_vec("fn f() -> Option<Vec<String>> { None }", &[("f", "Vec")])]
    #[case::option_slice("fn f(s: &[u8]) -> Option<&[u8]> { None }", &[("f", "[u8]")])]
    #[case::option_fixed_array_ignored("fn f() -> Option<[u8; 4]> { None }", &[])]
    #[case::result_option_hashmap(
        "fn f() -> anyhow::Result<Option<std::collections::HashMap<String, i32>>> { Ok(None) }",
        &[("f", "std::collections::HashMap")]
    )]
    #[case::trait_impl_ignored("impl Iterator for S { type Item = Vec<u8>; fn next(&mut self) -> Option<Vec<u8>> { None } }", &[])]
    fn test_collect_nullable_collection_returns_rust(
        #[case] source: &str,
        #[case] expected: &[(&str, &str)],
    ) {
        let file = ParsedFile::rust(source);
        let actual: Vec<(String, Vec<String>)> = collect_nullable_collection_returns(&file)
            .into_iter()
            .map(|item| (item.function_name, item.collection_types))
            .collect();
        let expected: Vec<(String, Vec<String>)> = expected
            .iter()
            .map(|(name, col)| ((*name).to_string(), vec![(*col).to_string()]))
            .collect();
        assert_eq!(actual, expected);
    }
}
