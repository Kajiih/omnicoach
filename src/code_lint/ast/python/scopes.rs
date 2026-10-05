//! Python binding extraction, lexical scope boundaries, and `call-before-definition` analysis.

// omni:disable-file [repeated-literal] -- Tree-sitter node kinds and field names (see ROADMAP)

use super::{
    AstNode, ParsedFile, RawNode, direct_function_definitions, extract_decorators_raw,
    method_receiver_name, parse_param_parts,
};
use std::collections::{HashMap, HashSet};

/// Recursively extracts binding identifiers from a pattern node, stopping at `attribute`,
/// `subscript`, and `type` nodes so `self.ctx = 1` or `items[idx] = 1` does not record `ctx`
/// or `idx` as a local variable binding.
pub(super) fn extract_from_pattern<'a>(node: &RawNode<'a>, bindings: &mut Vec<AstNode<'a>>) {
    let kind = node.kind();
    match kind.as_ref() {
        "attribute" | "subscript" | "type" => {}
        "identifier" => {
            if node.text() != "_" {
                bindings.push(AstNode::from_raw(node.clone()));
            }
        }
        "dotted_name" => {
            if !node.text().contains('.') {
                for child in node.children() {
                    extract_from_pattern(&child, bindings);
                }
            }
        }
        "class_pattern" => {
            // In a class match pattern like `case Point(x, y):`, the first child
            // is the class name identifier (`Point`), which is not a variable binding.
            let mut first = true;
            for child in node.children() {
                if first {
                    first = false;
                    continue;
                }
                extract_from_pattern(&child, bindings);
            }
        }
        "keyword_pattern" => {
            // In keyword match patterns like `case Point(x=z):`, the identifier
            // before the `=` (`x`) is the parameter name, and only the value (`z`) is the binding.
            let mut seen_equals = false;
            for child in node.children() {
                if child.kind() == "=" {
                    seen_equals = true;
                    continue;
                }
                if !seen_equals {
                    continue;
                }
                extract_from_pattern(&child, bindings);
            }
        }
        "typed_parameter" | "default_parameter" | "typed_default_parameter" => {
            if let Some(name_node) = node.field("name") {
                extract_from_pattern(&name_node, bindings);
            } else if let Some(first_child) = node.child(0)
                && matches!(
                    first_child.kind().as_ref(),
                    "identifier" | "list_splat_pattern" | "dictionary_splat_pattern"
                )
            {
                extract_from_pattern(&first_child, bindings);
            }
        }
        _ => {
            for child in node.children() {
                extract_from_pattern(&child, bindings);
            }
        }
    }
}

/// Inserts all identifier names bound by `pattern_node` into `names`.
fn extend_binding_names(pattern_node: &RawNode<'_>, names: &mut HashSet<String>) {
    let mut nodes = Vec::new();
    extract_from_pattern(pattern_node, &mut nodes);
    for node in nodes {
        names.insert(node.text().into_owned());
    }
}

/// Helper to extract the first segment from a dotted name.
fn extract_first_segment<'a>(node: &RawNode<'a>) -> RawNode<'a> {
    node.child(0).unwrap_or_else(|| node.clone())
}

/// Extracts bindings from Python import statements.
fn extract_from_import<'a>(node: &RawNode<'a>, bindings: &mut Vec<AstNode<'a>>) {
    match node.kind().as_ref() {
        "import_statement" => {
            for child in node.children() {
                let kind = child.kind();
                if kind != "import" && kind != "," {
                    extract_from_import(&child, bindings);
                }
            }
        }
        "import_from_statement" => {
            let mut seen_import = false;
            for child in node.children() {
                if child.kind() == "import" {
                    seen_import = true;
                    continue;
                }
                if !seen_import {
                    continue;
                }
                let kind = child.kind();
                if kind != "," && kind != "(" && kind != ")" {
                    extract_from_import(&child, bindings);
                }
            }
        }
        "aliased_import" => {
            if let Some(alias) = node.field("alias")
                && alias.text() != "_"
            {
                bindings.push(AstNode::from_raw(alias));
            }
        }
        "dotted_name" | "identifier" => {
            let first_seg = extract_first_segment(node);
            if first_seg.text() != "_" {
                bindings.push(AstNode::from_raw(first_seg));
            }
        }
        _ => {}
    }
}

fn traverse_children_skipping<'a>(
    node: &RawNode<'a>,
    skip: Option<&RawNode<'a>>,
    bindings: &mut Vec<AstNode<'a>>,
) {
    for child in node.children() {
        if let Some(skip_node) = skip
            && child.range() == skip_node.range()
        {
            continue;
        }
        traverse_python(&child, bindings);
    }
}

fn traverse_python<'a>(node: &RawNode<'a>, bindings: &mut Vec<AstNode<'a>>) {
    let kind = node.kind();
    match kind.as_ref() {
        "assignment" => {
            let left = node.field("left");
            if let Some(ref left_node) = left {
                extract_from_pattern(left_node, bindings);
            } else if let Some(first_child) = node.child(0) {
                extract_from_pattern(&first_child, bindings);
            }
            traverse_children_skipping(node, left.as_ref(), bindings);
        }
        "for_statement" | "for_in_clause" => {
            let left = node.field("left");
            if let Some(ref left_node) = left {
                extract_from_pattern(left_node, bindings);
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
            traverse_children_skipping(node, left.as_ref(), bindings);
        }
        "as_pattern" => {
            let alias = node.field("alias");
            if let Some(ref alias_node) = alias {
                extract_from_pattern(alias_node, bindings);
            }
            traverse_children_skipping(node, alias.as_ref(), bindings);
        }
        "named_expression" => {
            let name_node = node.field("name");
            if let Some(ref name) = name_node {
                extract_from_pattern(name, bindings);
            }
            traverse_children_skipping(node, name_node.as_ref(), bindings);
        }
        "parameters" | "lambda_parameters" => {
            for child in node.children() {
                if child.kind() != "(" && child.kind() != ")" && child.kind() != "," {
                    extract_from_pattern(&child, bindings);
                }
            }
        }
        "case_clause" => {
            for child in node.children() {
                if child.kind() == "case_pattern" {
                    extract_from_pattern(&child, bindings);
                } else if child.kind() != "case" && child.kind() != ":" {
                    traverse_python(&child, bindings);
                }
            }
        }
        "function_definition" | "class_definition" => {
            let name_node = node.field("name");
            if let Some(ref name) = name_node {
                bindings.push(AstNode::from_raw(name.clone()));
            }
            traverse_children_skipping(node, name_node.as_ref(), bindings);
        }
        "import_statement" | "import_from_statement" => {
            extract_from_import(node, bindings);
        }
        _ => {
            for child in node.children() {
                traverse_python(&child, bindings);
            }
        }
    }
}

/// Collects all binding definitions (variables, functions, classes, etc.) within a Python file.
#[must_use]
pub fn collect_bindings(file: &ParsedFile) -> Vec<AstNode<'_>> {
    let mut bindings = Vec::new();
    traverse_python(&file.grep.root(), &mut bindings);
    bindings
}

/// Returns true if a nested `function_definition` or `lambda` declares a parameter named `parameter_name`.
pub(super) fn scope_shadows_parameter(scope_node: &RawNode<'_>, parameter_name: &str) -> bool {
    let Some(params) = scope_node.field("parameters") else {
        return false;
    };
    params
        .children()
        .filter_map(|child| parse_param_parts(&child))
        .any(|parts| parts.name == parameter_name)
}

/// A call inside a Python function or method to a sibling function or method defined later in
/// the same module or class scope.
#[derive(Clone)]
pub struct ForwardCall<'a> {
    /// The `call` expression AST node (`callee(...)` or `self.callee(...)`).
    pub node: AstNode<'a>,
    /// Name of the enclosing function or method making the call.
    pub caller_name: String,
    /// Name of the called sibling function or method defined later in the scope.
    pub callee_name: String,
}

/// A logical function or method in a module or class scope (grouping `@overload` stubs with
/// their implementation and `@property` getters with `@<name>.setter` / `@<name>.deleter` at
/// their first declaration position).
struct LogicalFunction<'a> {
    name: String,
    definition_order: usize,
    has_overload: bool,
    has_non_overload_definition: bool,
    parts: Vec<RawNode<'a>>,
}

/// Collects `named_expression` (`:=`) targets inside a comprehension or generator expression,
/// which PEP 572 binds in the enclosing function scope rather than the comprehension scope.
fn collect_walrus_bindings_in_comprehension(node: &RawNode<'_>, bindings: &mut HashSet<String>) {
    if matches!(
        node.kind().as_ref(),
        "function_definition" | "class_definition" | "lambda"
    ) {
        return;
    }
    if node.kind() == "named_expression"
        && let Some(name_node) = node.field("name")
    {
        extend_binding_names(&name_node, bindings);
    }
    for child in node.children() {
        collect_walrus_bindings_in_comprehension(&child, bindings);
    }
}

/// Collects local variable, parameter, nested definition, and import bindings directly owned by
/// `scope_node` (`function_definition` or `lambda`), without descending into nested
/// `function_definition`, `lambda`, or `class_definition` bodies.
fn collect_local_scope_bindings(
    scope_node: &RawNode<'_>,
    excluded_parameter: Option<&str>,
) -> HashSet<String> {
    let mut bindings = HashSet::new();
    let mut globals = HashSet::new();

    if let Some(parameters) = scope_node.field("parameters") {
        let mut parameter_nodes = Vec::new();
        for child in parameters.children() {
            if !matches!(child.kind().as_ref(), "(" | ")" | ",") {
                extract_from_pattern(&child, &mut parameter_nodes);
            }
        }
        for node in parameter_nodes {
            let text = node.text();
            if excluded_parameter != Some(text.as_ref()) {
                bindings.insert(text.into_owned());
            }
        }
    }

    if let Some(body) = scope_node.field("body") {
        collect_bindings_in_subtree(&body, &mut bindings, &mut globals);
    }

    for global_name in globals {
        bindings.remove(&global_name);
    }
    bindings
}

/// Walks `node` to collect local bindings and `global` declarations within a single function
/// scope, stopping at nested scope boundaries (while still capturing PEP 572 `:=` targets inside
/// comprehensions).
fn collect_bindings_in_subtree(
    node: &RawNode<'_>,
    bindings: &mut HashSet<String>,
    globals: &mut HashSet<String>,
) {
    let kind = node.kind();
    match kind.as_ref() {
        "assignment" | "augmented_assignment" => {
            if let Some(left) = node.field("left") {
                extend_binding_names(&left, bindings);
            }
            if let Some(right) = node.field("right") {
                collect_bindings_in_subtree(&right, bindings, globals);
            }
            return;
        }
        "for_statement" => {
            if let Some(left) = node.field("left") {
                extend_binding_names(&left, bindings);
            }
        }
        "as_pattern" => {
            if let Some(alias) = node.field("alias") {
                extend_binding_names(&alias, bindings);
            }
        }
        "named_expression" => {
            if let Some(name_node) = node.field("name") {
                extend_binding_names(&name_node, bindings);
            }
        }
        "case_clause" => {
            for child in node.children() {
                if child.kind() == "case_pattern" {
                    extend_binding_names(&child, bindings);
                } else {
                    collect_bindings_in_subtree(&child, bindings, globals);
                }
            }
            return;
        }
        "function_definition" | "class_definition" => {
            if let Some(name_node) = node.field("name") {
                bindings.insert(name_node.text().into_owned());
            }
            return;
        }
        "lambda" => return,
        "list_comprehension"
        | "set_comprehension"
        | "dictionary_comprehension"
        | "generator_expression" => {
            collect_walrus_bindings_in_comprehension(node, bindings);
            return;
        }
        "import_statement" | "import_from_statement" => {
            let mut imported = Vec::new();
            extract_from_import(node, &mut imported);
            for item in imported {
                bindings.insert(item.text().into_owned());
            }
            return;
        }
        "global_statement" => {
            for child in node.children() {
                if child.kind() == "identifier" {
                    globals.insert(child.text().into_owned());
                }
            }
            return;
        }
        _ => {}
    }

    for child in node.children() {
        collect_bindings_in_subtree(&child, bindings, globals);
    }
}

/// Extracts comprehension loop variable bindings (`for_in_clause`) owned directly by a
/// comprehension or generator expression node.
fn collect_comprehension_bindings(comprehension_node: &RawNode<'_>) -> HashSet<String> {
    let mut bindings = HashSet::new();
    for child in comprehension_node.children() {
        if child.kind() == "for_in_clause"
            && let Some(left) = child.field("left")
        {
            extend_binding_names(&left, &mut bindings);
        }
    }
    bindings
}

/// Collects direct function definitions in `scope_body` (`module` or class `block`), grouping
/// `@overload` stubs with their implementation and `@property` getters with `@<name>.setter` /
/// `@<name>.deleter` at their first declaration position.
fn collect_scope_functions<'a>(scope_body: &RawNode<'a>) -> Vec<LogicalFunction<'a>> {
    let mut functions: Vec<LogicalFunction<'a>> = Vec::new();
    let mut index_by_name: HashMap<String, usize> = HashMap::new();

    for (order, (statement, function_node)) in direct_function_definitions(scope_body)
        .into_iter()
        .enumerate()
    {
        let Some(name_node) = function_node.field("name") else {
            continue;
        };
        let name = name_node.text().into_owned();
        let decorators = extract_decorators_raw(&statement);
        let is_overload = decorators
            .iter()
            .any(|decorator| decorator.terminal_name == "overload");
        let is_property_accessor = decorators
            .iter()
            .any(|decorator| matches!(decorator.terminal_name.as_str(), "setter" | "deleter"));

        if let Some(&existing_index) = index_by_name.get(&name) {
            let existing = &mut functions[existing_index];
            let continues_overload =
                existing.has_overload && (!existing.has_non_overload_definition || is_overload);
            if is_property_accessor || continues_overload {
                existing.has_non_overload_definition |= !is_overload;
                existing.parts.push(function_node);
                continue;
            }
        }

        let index = functions.len();
        index_by_name.insert(name.clone(), index);
        functions.push(LogicalFunction {
            name,
            definition_order: order,
            has_overload: is_overload,
            has_non_overload_definition: !is_overload,
            parts: vec![function_node],
        });
    }

    functions
}

/// Context for walking a function or method body to collect sibling calls.
struct CallWalkContext<'a> {
    sibling_names: &'a HashSet<&'a str>,
    receiver_name: Option<&'a str>,
    is_class: bool,
}

/// Recursively walks `node` inside a function/method body, tracking scoped local bindings and
/// recording calls to sibling functions/methods in `out`.
fn collect_calls_in_body<'a>(
    node: &RawNode<'a>,
    context: &CallWalkContext<'_>,
    active_bindings: &HashSet<String>,
    out: &mut Vec<(String, AstNode<'a>)>,
) {
    let kind = node.kind();
    match kind.as_ref() {
        "class_definition" => return,
        "function_definition" | "lambda" => {
            // Parameter default values, annotations, and decorators of a nested `def` or `lambda`
            // are evaluated in the enclosing scope; only `body` uses `inner_bindings`.
            let body = node.field("body");
            for child in node.children() {
                if body
                    .as_ref()
                    .is_some_and(|body_node| child.range() == body_node.range())
                {
                    continue;
                }
                collect_calls_in_body(&child, context, active_bindings, out);
            }
            if let Some(body_node) = body {
                let mut inner_bindings = active_bindings.clone();
                inner_bindings.extend(collect_local_scope_bindings(node, None));
                collect_calls_in_body(&body_node, context, &inner_bindings, out);
            }
            return;
        }
        "list_comprehension"
        | "set_comprehension"
        | "dictionary_comprehension"
        | "generator_expression" => {
            let mut comprehension_bindings = active_bindings.clone();
            comprehension_bindings.extend(collect_comprehension_bindings(node));
            for child in node.children() {
                collect_calls_in_body(&child, context, &comprehension_bindings, out);
            }
            return;
        }
        "call" => {
            if let Some(callee_name) = match_sibling_call(node, context, active_bindings) {
                out.push((callee_name, AstNode::from_raw(node.clone())));
            }
        }
        _ => {}
    }

    for child in node.children() {
        collect_calls_in_body(&child, context, active_bindings, out);
    }
}

/// Checks whether `call_node` invokes a sibling function or method in `context.sibling_names`.
fn match_sibling_call(
    call_node: &RawNode<'_>,
    context: &CallWalkContext<'_>,
    active_bindings: &HashSet<String>,
) -> Option<String> {
    let function = call_node.field("function")?;
    if context.is_class {
        let receiver = context.receiver_name?;
        if active_bindings.contains(receiver) || function.kind() != "attribute" {
            return None;
        }
        let object = function.field("object")?;
        let attribute = function.field("attribute")?;
        let attribute_text = attribute.text();
        if object.kind() == "identifier"
            && object.text() == receiver
            && context.sibling_names.contains(attribute_text.as_ref())
        {
            return Some(attribute_text.into_owned());
        }
        None
    } else {
        if function.kind() != "identifier" {
            return None;
        }
        let name = function.text();
        if context.sibling_names.contains(name.as_ref()) && !active_bindings.contains(name.as_ref())
        {
            Some(name.into_owned())
        } else {
            None
        }
    }
}

/// Returns true if `start` can reach `target` in the directed `call_graph`.
fn can_reach<'a>(
    start: &'a str,
    target: &'a str,
    call_graph: &HashMap<&'a str, HashSet<&'a str>>,
) -> bool {
    let mut visited = HashSet::new();
    let mut stack = vec![start];
    while let Some(current) = stack.pop() {
        if current == target {
            return true;
        }
        if visited.insert(current)
            && let Some(neighbors) = call_graph.get(current)
        {
            stack.extend(neighbors.iter().copied());
        }
    }
    false
}

/// Evaluates sibling functions or methods in a single scope and appends any forward calls to `out`.
fn evaluate_scope<'a>(
    functions: &[LogicalFunction<'a>],
    is_class: bool,
    out: &mut Vec<ForwardCall<'a>>,
) {
    let sibling_names: HashSet<&str> = functions
        .iter()
        .map(|function| function.name.as_str())
        .collect();
    let latest_order_by_name: HashMap<&str, usize> = functions
        .iter()
        .map(|function| (function.name.as_str(), function.definition_order))
        .collect();

    let mut calls_by_function: Vec<Vec<(String, AstNode<'a>)>> =
        Vec::with_capacity(functions.len());
    let mut call_graph: HashMap<&str, HashSet<&str>> = HashMap::new();

    for function in functions {
        let mut function_calls = Vec::new();
        for function_node in &function.parts {
            let Some(body) = function_node.field("body") else {
                continue;
            };
            let receiver = if is_class {
                method_receiver_name(function_node, true)
            } else {
                None
            };
            let initial_bindings = collect_local_scope_bindings(function_node, receiver.as_deref());
            let context = CallWalkContext {
                sibling_names: &sibling_names,
                receiver_name: receiver.as_deref(),
                is_class,
            };
            collect_calls_in_body(&body, &context, &initial_bindings, &mut function_calls);
        }
        let edges = call_graph.entry(function.name.as_str()).or_default();
        for (callee_name, _) in &function_calls {
            if let Some(&callee_ref) = sibling_names.get(callee_name.as_str()) {
                edges.insert(callee_ref);
            }
        }
        calls_by_function.push(function_calls);
    }

    for (function, function_calls) in functions.iter().zip(calls_by_function) {
        if is_class
            && matches!(
                function.name.as_str(),
                "__init__" | "__new__" | "__post_init__"
            )
        {
            continue;
        }
        let mut reported_callees: HashSet<String> = HashSet::new();
        for (callee_name, call_node) in function_calls {
            let Some(&callee_order) = latest_order_by_name.get(callee_name.as_str()) else {
                continue;
            };
            if callee_order <= function.definition_order {
                continue;
            }
            if can_reach(callee_name.as_str(), function.name.as_str(), &call_graph) {
                continue;
            }
            if reported_callees.insert(callee_name.clone()) {
                out.push(ForwardCall {
                    node: call_node,
                    caller_name: function.name.clone(),
                    callee_name,
                });
            }
        }
    }
}

/// Collects forward sibling calls in module and class scopes across `file`, in source order.
///
/// Exempts direct and mutual recursion (strongly connected components in the intra-scope call
/// graph), class constructor callers (`__init__`, `__new__`, `__post_init__`), locally shadowed
/// names, `@overload` / `@property` accessor groups, and non-call references. Reports at most
/// one [`ForwardCall`] per `(caller, callee)` pair per definition, anchored at the first
/// offending `call` node.
#[must_use]
pub fn collect_forward_calls(file: &ParsedFile) -> Vec<ForwardCall<'_>> {
    let mut results = Vec::new();
    let root = file.grep.root();

    let module_functions = collect_scope_functions(&root);
    evaluate_scope(&module_functions, false, &mut results);

    for node in root.dfs() {
        if node.kind() == "class_definition"
            && let Some(body) = node.field("body")
        {
            let class_methods = collect_scope_functions(&body);
            evaluate_scope(&class_methods, true, &mut results);
        }
    }

    results.sort_unstable_by_key(|call| call.node.span().start);
    results
}
