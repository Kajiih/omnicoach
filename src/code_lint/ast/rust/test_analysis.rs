//! Test-function, assertion, and macro analysis for Rust (`ra_ap_syntax`).

use super::declaration_order::collect_rust_fn_local_names;
use super::{
    SELF_KEYWORD, SUPER_KEYWORD, TEST_IDENTIFIER, attribute_terminal_name, cast_at_span,
    macro_call_terminal_name, resolve_preceding_macro_path, syntax_node,
};
use crate::code_lint::ast::{
    AstCallCandidate, AstNode, ParsedFile, TestFunctionSetup, TestToTestCall,
    normalize_statement_text, span_from_rowan_range,
};
use crate::diagnostic::SourceSpan;
use ra_ap_syntax::ast::{self, HasAttrs as _, HasModuleItem as _, HasName as _};
use ra_ap_syntax::{AstNode as _, SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};
use std::collections::{HashMap, HashSet};

/// Collects byte spans for all inline test items (`#[cfg(test)]` modules/items and `#[test]` functions)
/// within a Rust source file.
#[must_use]
pub fn collect_inline_test_ranges(file: &ParsedFile) -> &[std::ops::Range<usize>] {
    file.rust_inline_test_ranges.get_or_init(|| {
        let Some(parsed) = file.rs_parsed() else {
            return Vec::new();
        };
        let mut ranges = Vec::new();
        collect_inline_test_ranges_rec(parsed.tree().syntax(), &mut ranges);
        ranges
    })
}

fn collect_inline_test_ranges_rec(node: &SyntaxNode, ranges: &mut Vec<std::ops::Range<usize>>) {
    let has_test_attr = node
        .children()
        .filter_map(ast::Attr::cast)
        .any(|attribute| {
            attribute.kind() == ast::AttrKind::Outer
                && (is_conditional_test_attribute(&attribute) || is_test_attribute(&attribute))
        });
    if has_test_attr {
        let start: usize = node.text_range().start().into();
        let end: usize = node.text_range().end().into();
        ranges.push(start..end);
        return;
    }
    for child in node.children() {
        if child.kind() != SyntaxKind::TOKEN_TREE {
            collect_inline_test_ranges_rec(&child, ranges);
        }
    }
}

/// Returns true if `attribute` represents a `#[cfg(test)]` attribute.
#[must_use]
fn is_conditional_test_attribute(attribute: &ast::Attr) -> bool {
    let Some(ast::Meta::CfgMeta(conditional_meta)) = attribute.meta() else {
        return false;
    };
    let Some(ast::CfgPredicate::CfgAtom(atom)) = conditional_meta.cfg_predicate() else {
        return false;
    };
    atom.eq_token().is_none()
        && atom
            .ident_token()
            .is_some_and(|ident| ident.text() == TEST_IDENTIFIER)
}

/// Extracts the terminal macro identifier from a Rust `macro_invocation` node (e.g. `assert` from `std::assert!`).
#[must_use]
pub fn macro_terminal_name<'tree>(macro_node: &AstNode<'tree>) -> std::borrow::Cow<'tree, str> {
    cast_at_span::<ast::MacroCall>(macro_node)
        .map_or(std::borrow::Cow::Borrowed(""), |macro_call| {
            std::borrow::Cow::Owned(macro_call_terminal_name(&macro_call))
        })
}

/// Collects all outermost Rust test functions together with their `(name_node, func_name, assertion_count)`.
#[must_use]
pub(in crate::code_lint::ast) fn collect_test_function_assertion_counts(
    file: &ParsedFile,
) -> Vec<(AstNode<'_>, String, usize)> {
    let Some(parsed) = file.rs_parsed() else {
        return Vec::new();
    };
    let tree = parsed.tree();
    let root = tree.syntax();
    let assertion_helpers = collect_rust_assertion_helpers(root);
    let mut test_functions = Vec::new();
    collect_outer_test_functions_rec(root, &mut test_functions);
    test_functions
        .into_iter()
        .filter_map(|function| {
            let name_node = function.name()?;
            let body_node = function.body()?;
            let function_name = name_node.text().to_string();
            let count = count_rust_assertions(body_node.syntax(), &assertion_helpers);
            Some((
                AstNode::from_span(file, span_from_rowan_range(name_node.syntax().text_range())),
                function_name,
                count,
            ))
        })
        .collect()
}

/// Collects control-flow and conditional constructs directly inside outermost Rust test functions.
#[must_use]
pub(in crate::code_lint::ast) fn collect_test_logic_constructs(
    file: &ParsedFile,
) -> Vec<(AstNode<'_>, String, &'static str)> {
    let Some(parsed) = file.rs_parsed() else {
        return Vec::new();
    };
    let mut test_functions = Vec::new();
    collect_outer_test_functions_rec(parsed.tree().syntax(), &mut test_functions);
    let mut out = Vec::new();
    for function in test_functions {
        let Some(name_node) = function.name() else {
            continue;
        };
        let Some(body_node) = function.body() else {
            continue;
        };
        let func_name = name_node.text().to_string();
        collect_rust_test_logic_rec(body_node.syntax(), file, &func_name, &mut out);
    }
    out
}

fn collect_rust_test_logic_rec<'a>(
    node: &SyntaxNode,
    file: &'a ParsedFile,
    func_name: &str,
    out: &mut Vec<(AstNode<'a>, String, &'static str)>,
) {
    if (ast::Item::can_cast(node.kind()) && !ast::MacroCall::can_cast(node.kind()))
        || ast::ClosureExpr::can_cast(node.kind())
    {
        return;
    }
    let construct = match node.kind() {
        SyntaxKind::IF_EXPR => Some("an `if` expression"),
        SyntaxKind::MATCH_EXPR => Some("a `match` expression"),
        SyntaxKind::FOR_EXPR => Some("a `for` loop"),
        SyntaxKind::WHILE_EXPR => Some("a `while` loop"),
        SyntaxKind::LOOP_EXPR => Some("a `loop` expression"),
        _ => None,
    };
    if let Some(description) = construct {
        out.push((
            AstNode::from_span(file, span_from_rowan_range(node.text_range())),
            func_name.to_string(),
            description,
        ));
        return;
    }
    for child in node.children() {
        collect_rust_test_logic_rec(&child, file, func_name, out);
    }
}

/// Collects outermost Rust test functions that interleave non-assertion statements between
/// behavioral assertions.
#[must_use]
pub(in crate::code_lint::ast) fn collect_multi_phase_test_functions(
    file: &ParsedFile,
) -> Vec<(AstNode<'_>, String)> {
    let Some(parsed) = file.rs_parsed() else {
        return Vec::new();
    };
    let tree = parsed.tree();
    let root = tree.syntax();
    let assertion_helpers = collect_rust_assertion_helpers(root);
    let mut test_functions = Vec::new();
    collect_outer_test_functions_rec(root, &mut test_functions);
    test_functions
        .into_iter()
        .filter_map(|function| {
            let name_node = function.name()?;
            let body_node = function.body()?;
            if !is_multi_phase_rust_test(&body_node, &assertion_helpers) {
                return None;
            }
            Some((
                AstNode::from_span(file, span_from_rowan_range(name_node.syntax().text_range())),
                name_node.text().to_string(),
            ))
        })
        .collect()
}

fn is_multi_phase_rust_test(
    body_node: &ast::BlockExpr,
    assertion_helpers: &HashSet<String>,
) -> bool {
    let Some(block_statements) = body_node.stmt_list() else {
        return false;
    };
    let mut seen_assertion = false;
    let mut seen_action_after_assertion = false;
    for statement_node in rust_executable_block_statements(&block_statements) {
        if count_rust_assertions(&statement_node, assertion_helpers) > 0 {
            if seen_action_after_assertion {
                return true;
            }
            seen_assertion = true;
        } else if seen_assertion && !is_rust_inspection_binding(&statement_node) {
            seen_action_after_assertion = true;
        }
    }
    false
}

fn is_rust_inspection_binding(statement_node: &SyntaxNode) -> bool {
    ast::LetStmt::cast(statement_node.clone())
        .and_then(|let_statement| let_statement.initializer())
        .is_some_and(|initializer| is_rust_projection_expr(&initializer))
}

fn is_rust_projection_expr(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::PathExpr(_) => true,
        ast::Expr::FieldExpr(field) => field
            .expr()
            .is_some_and(|base| is_rust_projection_expr(&base)),
        ast::Expr::IndexExpr(index) => {
            index
                .base()
                .is_some_and(|base| is_rust_projection_expr(&base))
                && index
                    .index()
                    .is_some_and(|index_expr| !rust_expr_contains_call(&index_expr))
        }
        ast::Expr::RefExpr(reference) => reference
            .expr()
            .is_some_and(|inner| is_rust_projection_expr(&inner)),
        ast::Expr::PrefixExpr(prefix) if prefix.op_kind() == Some(ast::UnaryOp::Deref) => prefix
            .expr()
            .is_some_and(|inner| is_rust_projection_expr(&inner)),
        ast::Expr::ParenExpr(paren) => paren
            .expr()
            .is_some_and(|inner| is_rust_projection_expr(&inner)),
        ast::Expr::TupleExpr(tuple) => {
            let mut has_fields = false;
            for field in tuple.fields() {
                has_fields = true;
                if !is_rust_projection_expr(&field) {
                    return false;
                }
            }
            has_fields
        }
        _ => false,
    }
}

fn rust_expr_contains_call(expr: &ast::Expr) -> bool {
    expr.syntax().descendants().any(|node| {
        matches!(
            node.kind(),
            SyntaxKind::CALL_EXPR | SyntaxKind::METHOD_CALL_EXPR | SyntaxKind::MACRO_EXPR
        )
    })
}

/// Collects Rust test functions grouped by enclosing file root or `mod` scope (segmented on
/// duplicate function names) along with their leading non-assertion setup statements.
#[must_use]
pub(in crate::code_lint::ast) fn collect_test_setup_scopes(
    file: &ParsedFile,
) -> Vec<Vec<TestFunctionSetup<'_>>> {
    let Some(parsed) = file.rs_parsed() else {
        return Vec::new();
    };
    let assertion_helpers = collect_rust_assertion_helpers(parsed.tree().syntax());
    let mut out = Vec::new();
    let root_items: Vec<ast::Item> = parsed.tree().items().collect();
    collect_rust_test_setup_scope(&root_items, file, &assertion_helpers, &mut out);
    out
}

fn collect_rust_test_setup_scope<'a>(
    items: &[ast::Item],
    file: &'a ParsedFile,
    assertion_helpers: &HashSet<String>,
    out: &mut Vec<Vec<TestFunctionSetup<'a>>>,
) {
    let mut current_segment = Vec::new();
    let mut seen_names = HashSet::new();
    for item in items {
        let ast::Item::Fn(function) = item else {
            continue;
        };
        if !is_test_function(function) {
            continue;
        }
        let Some(name_node) = function.name() else {
            continue;
        };
        let Some(body_node) = function.body() else {
            continue;
        };
        let name = name_node.text().trim().to_owned();
        if !seen_names.insert(name.clone()) {
            out.push(std::mem::take(&mut current_segment));
            seen_names.clear();
            seen_names.insert(name.clone());
        }
        current_segment.push(TestFunctionSetup {
            name_node: AstNode::from_span(
                file,
                span_from_rowan_range(name_node.syntax().text_range()),
            ),
            name,
            leading_setup_statements: extract_rust_leading_setup_statements(
                &body_node,
                file,
                assertion_helpers,
            ),
        });
    }
    if !current_segment.is_empty() {
        out.push(current_segment);
    }
    for item in items {
        if let ast::Item::Module(module) = item
            && let Some(body_items) = module.item_list()
        {
            let module_items: Vec<ast::Item> = body_items.items().collect();
            collect_rust_test_setup_scope(&module_items, file, assertion_helpers, out);
        }
    }
}

fn extract_rust_leading_setup_statements(
    body_node: &ast::BlockExpr,
    file: &ParsedFile,
    assertion_helpers: &HashSet<String>,
) -> Vec<String> {
    let Some(block_statements) = body_node.stmt_list() else {
        return Vec::new();
    };
    rust_executable_block_statements(&block_statements)
        .take_while(|statement_node| count_rust_assertions(statement_node, assertion_helpers) == 0)
        .map(|statement_node| {
            let span = span_from_rowan_range(statement_node.text_range());
            normalize_statement_text(&file.source[span.start..span.end])
        })
        .collect()
}

/// Collects all `macro_invocation` nodes in `file`.
#[must_use]
pub fn collect_macro_invocations(file: &ParsedFile) -> Vec<AstNode<'_>> {
    let Some(parsed) = file.rs_parsed() else {
        return Vec::new();
    };
    parsed
        .tree()
        .syntax()
        .descendants()
        .filter_map(ast::MacroCall::cast)
        .map(|macro_call| {
            AstNode::from_span(
                file,
                span_from_rowan_range(macro_call.syntax().text_range()),
            )
        })
        .collect()
}

/// Returns true if a Rust `macro_invocation`'s `token_tree` contains a top-level `&&` logical operator.
#[must_use]
pub fn has_top_level_logical_and(macro_node: &AstNode<'_>) -> bool {
    let Some(macro_call) = cast_at_span::<ast::MacroCall>(macro_node) else {
        return false;
    };
    let Some(token_tree) = macro_call.token_tree() else {
        return false;
    };
    if token_tree_has_direct_logical_and(&token_tree) {
        return true;
    }
    let meaningful = meaningful_token_tree_elements(&token_tree);
    matches!(
        meaningful.as_slice(),
        [SyntaxElement::Node(only_child)]
            if ast::TokenTree::cast(only_child.clone())
                .is_some_and(|inner| token_tree_has_direct_logical_and(&inner))
    )
}

/// Returns true if `token_tree` has a direct `&&` operator among its children.
fn token_tree_has_direct_logical_and(token_tree: &ast::TokenTree) -> bool {
    let elements: Vec<_> = token_tree.syntax().children_with_tokens().collect();
    elements.windows(2).any(|pair| {
        matches!(
            pair,
            [SyntaxElement::Token(first), SyntaxElement::Token(second)]
                if first.kind() == SyntaxKind::AMP && second.kind() == SyntaxKind::AMP
        )
    })
}

/// Extracts the argument nodes inside a Rust `macro_invocation`'s `token_tree`.
#[must_use]
pub fn extract_macro_arguments<'a>(macro_node: &AstNode<'a>) -> Vec<AstNode<'a>> {
    let Some(macro_call) = cast_at_span::<ast::MacroCall>(macro_node) else {
        return Vec::new();
    };
    let Some(token_tree) = macro_call.token_tree() else {
        return Vec::new();
    };
    meaningful_token_tree_elements(&token_tree)
        .into_iter()
        .map(|element| {
            AstNode::from_span(macro_node.file, span_from_rowan_range(element.text_range()))
        })
        .collect()
}

/// Returns true if `node` is a standalone Rust macro argument `token_tree` (tuple, array, or
/// `vec![...]`) consisting solely of `>= 2` boolean literals (`true` / `false`).
#[must_use]
pub fn is_boolean_literal_collection(node: &AstNode<'_>) -> bool {
    let Some(token_tree) = standalone_collection_token_tree(node) else {
        return false;
    };
    let items = meaningful_token_tree_elements(&token_tree);
    items.len() >= 2
        && items.iter().all(|item| {
            matches!(
                item,
                SyntaxElement::Token(token)
                    if matches!(token.kind(), SyntaxKind::TRUE_KW | SyntaxKind::FALSE_KW)
            )
        })
}

/// Returns true if `node` is a standalone Rust tuple, array, or `vec![...]` macro argument
/// `token_tree` containing `>= 2` comma-separated elements (such as `(a, b)` or `[a, b]`).
#[must_use]
pub fn is_multi_element_collection(node: &AstNode<'_>) -> bool {
    let Some(token_tree) = standalone_collection_token_tree(node) else {
        return false;
    };
    let has_comma = token_tree
        .syntax()
        .children_with_tokens()
        .any(|element| matches!(element, SyntaxElement::Token(token) if token.kind() == SyntaxKind::COMMA));
    has_comma && meaningful_token_tree_elements(&token_tree).len() >= 2
}

/// Collects calls in a Rust test file where a function calls a `#[test]` function defined in the
/// same file.
#[must_use]
pub(in crate::code_lint::ast) fn collect_test_to_test_calls(
    file: &ParsedFile,
) -> Vec<TestToTestCall<'_>> {
    let Some(parsed) = file.rs_parsed() else {
        return Vec::new();
    };
    let mut test_functions: HashMap<SourceSpan, HashSet<String>> = HashMap::new();
    let mut function_locals: HashMap<SourceSpan, HashSet<String>> = HashMap::new();
    for fn_decl in parsed
        .tree()
        .syntax()
        .descendants()
        .filter_map(ast::Fn::cast)
    {
        let Some(name_node) = fn_decl.name() else {
            continue;
        };
        let name_span = span_from_rowan_range(name_node.syntax().text_range());
        function_locals.insert(name_span, collect_rust_fn_local_names(&fn_decl));
        if has_test_attribute(&fn_decl) {
            let module_span = enclosing_rust_module_span(fn_decl.syntax());
            test_functions
                .entry(module_span)
                .or_default()
                .insert(name_node.text().trim().to_owned());
        }
    }
    let mut out = Vec::new();
    for candidate in crate::code_lint::ast::collect_call_candidates(file) {
        let Some(caller_fn) = candidate.enclosing_functions.first() else {
            continue;
        };
        let Some(LocalRustCallee {
            super_count,
            name: callee_name,
        }) = local_rust_callee(&candidate)
        else {
            continue;
        };
        let is_local = super_count == 0
            && candidate.enclosing_functions.iter().any(|func| {
                function_locals
                    .get(&func.name_span)
                    .is_some_and(|locals| locals.contains(callee_name))
            });
        let Some(target_module) = target_rust_module_span(&candidate.node, super_count) else {
            continue;
        };
        let is_test_target = test_functions
            .get(&target_module)
            .is_some_and(|names| names.contains(callee_name));
        if !is_test_target || is_local {
            continue;
        }
        if super_count == 0 && callee_name == caller_fn.name {
            continue;
        }
        out.push(TestToTestCall {
            node: candidate.node,
            caller: caller_fn.name.clone(),
            callee: candidate.callee,
        });
    }
    out
}

/// Relative module depth (`super::` count) and terminal function name of a local Rust call.
struct LocalRustCallee<'a> {
    super_count: usize,
    name: &'a str,
}

/// Extracts `LocalRustCallee` from a Rust call candidate when the call is not a method call and
/// any leading `::` path segments are relative module qualifiers (`self` or `super`).
fn local_rust_callee<'a>(candidate: &'a AstCallCandidate<'_>) -> Option<LocalRustCallee<'a>> {
    if candidate.method_name.is_some() {
        return None;
    }
    let mut segments = candidate.callee.rsplit("::");
    let terminal = segments.next()?.trim();
    let prefix: Vec<&str> = segments.map(str::trim).collect();
    if terminal.is_empty()
        || !prefix
            .iter()
            .all(|segment| matches!(*segment, SELF_KEYWORD | SUPER_KEYWORD))
    {
        return None;
    }
    let super_count = prefix
        .iter()
        .filter(|&&segment| segment == SUPER_KEYWORD)
        .count();
    Some(LocalRustCallee {
        super_count,
        name: terminal,
    })
}

/// Returns the byte span of the innermost `mod` enclosing `node`, or `0..0` at the file root.
fn enclosing_rust_module_span(node: &SyntaxNode) -> SourceSpan {
    node.ancestors()
        .find_map(ast::Module::cast)
        .map_or(SourceSpan::new(0, 0), |module| {
            span_from_rowan_range(module.syntax().text_range())
        })
}

/// Resolves the target `mod` span (`0..0` for the file root) reached by stepping up `super_count`
/// enclosing `mod` items from `node`.
fn target_rust_module_span(node: &AstNode<'_>, super_count: usize) -> Option<SourceSpan> {
    let syntax = syntax_node(node)?;
    let modules: Vec<ast::Module> = syntax.ancestors().filter_map(ast::Module::cast).collect();
    (super_count <= modules.len()).then(|| {
        modules
            .get(super_count)
            .map_or(SourceSpan::new(0, 0), |module| {
                span_from_rowan_range(module.syntax().text_range())
            })
    })
}

/// If `node` is a standalone tuple, array, or `vec![...]` `token_tree` macro argument (not a call
/// argument list `foo(a, b)` or postfix receiver `(a, b).0`), returns its [`ast::TokenTree`].
fn standalone_collection_token_tree(node: &AstNode<'_>) -> Option<ast::TokenTree> {
    let token_tree = cast_at_span::<ast::TokenTree>(node)?;
    if span_from_rowan_range(token_tree.syntax().text_range()) != node.span() {
        return None;
    }
    let following = std::iter::successors(
        token_tree.syntax().last_token()?.next_token(),
        SyntaxToken::next_token,
    )
    .find(|candidate| !candidate.kind().is_trivia())?;
    if !matches!(
        following.kind(),
        SyntaxKind::COMMA | SyntaxKind::R_PAREN | SyntaxKind::R_BRACK | SyntaxKind::R_CURLY
    ) {
        return None;
    }
    if resolve_preceding_macro_path(token_tree.syntax()).is_some_and(|path| path.terminal == "vec")
    {
        return Some(token_tree);
    }
    let preceding = std::iter::successors(
        token_tree.syntax().first_token()?.prev_token(),
        SyntaxToken::prev_token,
    )
    .find(|candidate| {
        !candidate.kind().is_trivia()
            && !matches!(candidate.kind(), SyntaxKind::AMP | SyntaxKind::MUT_KW)
    })?;
    matches!(
        preceding.kind(),
        SyntaxKind::L_PAREN | SyntaxKind::L_BRACK | SyntaxKind::L_CURLY | SyntaxKind::COMMA
    )
    .then_some(token_tree)
}

fn collect_outer_test_functions_rec(node: &SyntaxNode, out: &mut Vec<ast::Fn>) {
    if let Some(function) = ast::Fn::cast(node.clone()) {
        if is_test_function(&function) {
            out.push(function);
        }
        return;
    }
    for child in node.children() {
        collect_outer_test_functions_rec(&child, out);
    }
}

/// Collects names of non-test unit-returning free functions in `root` whose bodies execute at
/// least one behavioral assertion transitively.
fn collect_rust_assertion_helpers(root: &SyntaxNode) -> HashSet<String> {
    let candidates = collect_non_test_unit_rust_functions(root);
    let mut helpers = HashSet::new();
    loop {
        let mut added = false;
        for (name, body) in &candidates {
            if helpers.contains(name) {
                continue;
            }
            if count_rust_assertions(body.syntax(), &helpers) > 0 {
                helpers.insert(name.clone());
                added = true;
            }
        }
        if !added {
            break;
        }
    }
    helpers
}

/// Collects `(name, body)` pairs for non-test, unit-returning free functions in `root`.
fn collect_non_test_unit_rust_functions(root: &SyntaxNode) -> Vec<(String, ast::BlockExpr)> {
    root.descendants()
        .filter_map(ast::Fn::cast)
        .filter(|function| {
            !is_test_function(function)
                && is_unit_rust_function(function)
                && !function
                    .syntax()
                    .ancestors()
                    .any(|ancestor| matches!(ancestor.kind(), SyntaxKind::TRAIT | SyntaxKind::IMPL))
        })
        .filter_map(|function| {
            let name = function.name()?.text().trim().to_owned();
            let body = function.body()?;
            Some((name, body))
        })
        .collect()
}

/// Returns true if `function` is a test function (`#[test]` / `#[rstest]` or named `test` / `test_*`).
#[must_use]
fn is_test_function(function: &ast::Fn) -> bool {
    let is_named_test = function.name().is_some_and(|name_node| {
        let function_name = name_node.text();
        function_name == TEST_IDENTIFIER || function_name.starts_with("test_")
    });
    is_named_test || has_test_attribute(function)
}

/// Returns true if `function` carries an outer test attribute (`#[test]`, `#[tokio::test]`,
/// `#[rstest]`, `#[test_case(...)]`).
#[must_use]
fn has_test_attribute(function: &ast::Fn) -> bool {
    function
        .attrs()
        .any(|attribute| attribute.kind() == ast::AttrKind::Outer && is_test_attribute(&attribute))
}

/// Returns true if `attribute` is a Rust test attribute (`#[test]`, `#[tokio::test]`, `#[rstest]`, `#[test_case(...)]`).
#[must_use]
fn is_test_attribute(attribute: &ast::Attr) -> bool {
    attribute_terminal_name(attribute)
        .is_some_and(|terminal| matches!(terminal.text(), TEST_IDENTIFIER | "rstest" | "test_case"))
}

/// Returns true if `function` has no return type annotation or an explicit `-> ()` unit return type.
fn is_unit_rust_function(function: &ast::Fn) -> bool {
    function.ret_type().is_none_or(|ret_type| {
        matches!(
            ret_type.ty(),
            Some(ast::Type::TupleType(tuple)) if tuple.fields().next().is_none()
        )
    })
}

/// Extracts the non-delimiter, non-trivia direct child elements of `token_tree`.
fn meaningful_token_tree_elements(token_tree: &ast::TokenTree) -> Vec<SyntaxElement> {
    token_tree
        .syntax()
        .children_with_tokens()
        .filter(|element| !is_token_tree_delimiter_or_trivia(element))
        .collect()
}

/// Returns true if `element` is a delimiter or comma token inside a `TokenTree`.
fn is_token_tree_delimiter_or_trivia(element: &SyntaxElement) -> bool {
    match element {
        SyntaxElement::Node(_) => false,
        SyntaxElement::Token(token) => {
            token.kind().is_trivia()
                || matches!(
                    token.kind(),
                    SyntaxKind::L_PAREN
                        | SyntaxKind::R_PAREN
                        | SyntaxKind::L_BRACK
                        | SyntaxKind::R_BRACK
                        | SyntaxKind::L_CURLY
                        | SyntaxKind::R_CURLY
                        | SyntaxKind::COMMA
                )
        }
    }
}

/// Returns the executable top-level statements and tail expression of `block_statements`,
/// skipping local item declarations other than macro calls.
fn rust_executable_block_statements(
    block_statements: &ast::StmtList,
) -> impl Iterator<Item = SyntaxNode> {
    block_statements
        .statements()
        .filter(|statement| {
            !matches!(
                statement,
                ast::Stmt::Item(item) if !matches!(item, ast::Item::MacroCall(_))
            )
        })
        .map(|statement| statement.syntax().clone())
        .chain(
            block_statements
                .tail_expr()
                .map(|tail| tail.syntax().clone()),
        )
}

/// Recursively counts top-level assertion macro and assertion-helper invocations in a Rust test body.
fn count_rust_assertions(node: &SyntaxNode, assertion_helpers: &HashSet<String>) -> usize {
    if ast::Fn::can_cast(node.kind()) {
        return 0;
    }
    if let Some(macro_call) = ast::MacroCall::cast(node.clone())
        && is_assertion_macro(&macro_call)
    {
        return 1;
    }
    if is_rust_assertion_call_node(node, assertion_helpers) {
        return 1;
    }
    node.children()
        .map(|child| count_rust_assertions(&child, assertion_helpers))
        .sum()
}

/// Returns true if `node` is a Rust function or method call to an `assert_*` / `_assert_*` helper
/// or an unqualified call to a same-file assertion helper in `assertion_helpers`.
fn is_rust_assertion_call_node(node: &SyntaxNode, assertion_helpers: &HashSet<String>) -> bool {
    if let Some(call_expr) = ast::CallExpr::cast(node.clone())
        && let Some(ast::Expr::PathExpr(path_expr)) = call_expr.expr()
        && let Some(path) = path_expr.path()
        && let Some(segment) = path.segment()
        && let Some(name_ref) = segment.name_ref()
    {
        let name = name_ref.text().trim();
        return super::is_assertion_function_name(name)
            || (path.qualifier().is_none() && assertion_helpers.contains(name));
    }
    if let Some(method_call) = ast::MethodCallExpr::cast(node.clone())
        && let Some(name_ref) = method_call.name_ref()
    {
        return super::is_assertion_function_name(name_ref.text().trim());
    }
    false
}

/// Returns true if `macro_call` invokes an assertion macro (`assert!`, `assert_*!`, `debug_assert!`, `debug_assert_*!`).
#[must_use]
fn is_assertion_macro(macro_call: &ast::MacroCall) -> bool {
    let terminal = macro_call_terminal_name(macro_call);
    terminal == "assert"
        || terminal.starts_with("assert_")
        || terminal == "debug_assert"
        || terminal.starts_with("debug_assert_")
}
