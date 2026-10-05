//! AST helper predicates and structural extractors for Python.

// omni:disable-file [repeated-literal] -- Tree-sitter node kinds and field names (see ROADMAP)

mod annotations;
mod classes;
mod format_strings;
mod functions;
mod logging;
mod scopes;
mod strings;

pub use self::annotations::{
    collect_concrete_collection_types, collect_mutable_collection_types,
    collect_nullable_collection_return_types, collect_nullable_collection_returns,
    collect_specific_collection_types, has_unaliased_collections_abc_set_import,
    immutable_constant_collection_replacements, read_only_collection_replacements,
};
pub use self::classes::{
    PythonAnnotatedAttribute, PythonBaseClass, PythonClassInfo, PythonInstanceAttributeAnnotation,
    collect_instance_attribute_annotations, collect_public_class_attributes, extract_classes,
};
pub use self::format_strings::{
    PythonFormatPlaceholder, PythonFormatString, PythonFormatStyle, collect_format_strings,
    extract_valid_field_root, named_format_field_roots,
};
pub use self::functions::{
    PythonFunctionSignature, PythonParameterInfo, PythonParameterKind, extract_function_signatures,
    extract_parameters, find_nested_functions, has_override_decorator, is_trait_impl_member,
};
pub use self::logging::{PythonLoggerCall, collect_logger_calls};
pub use self::scopes::{
    PythonFunctionScope, PythonScopeFunction, PythonSiblingCall, collect_bindings,
    collect_function_scopes,
};

use self::annotations::{
    AnnotationTraversalDepth, MUTABLE_COLLECTION_ABCS, collect_type_constructors_raw,
    has_final_annotation, is_bare_final_annotation, is_concrete_collection_constructor,
    is_std_type_constructor,
};
use self::classes::is_in_protocol_or_abc_class;
use self::functions::{direct_function_definitions, method_receiver_name, parse_param_parts};
use self::logging::extract_logger_call;
use self::scopes::scope_shadows_parameter;
use self::strings::{
    fstring_segments_and_interpolations, is_triple_quoted, outermost_string_expression,
    positional_call_arguments, preceding_concatenated_literal_text, static_string_text,
    string_prefix_flags,
};
use crate::code_lint::ast::{
    AstNode, LiteralOccurrence, LiteralRole, LiteralValue, NullableCollectionReturn, ParsedFile,
    PositionalRead, RawNode, ScopePositionalReads, delimited_string_parts, parse_float_literal,
    parse_integer_literal,
};
use std::collections::{HashMap, HashSet};

/// Returns true for Python node kinds that hold statements as direct children.
///
/// `module` is the file root and `block` is an indented suite.
#[must_use]
pub(super) fn is_statement_container(kind: &str) -> bool {
    matches!(kind, "module" | "block")
}

/// Returns the `def` or `class` wrapped by a `decorated_definition` statement, so its header
/// spans from the first decorator to the end of the definition's own header.
#[must_use]
pub(super) fn decorated_definition<'a>(statement: &RawNode<'a>) -> Option<RawNode<'a>> {
    if statement.kind() == "decorated_definition" {
        statement.field("definition")
    } else {
        None
    }
}

/// Returns the 1-indexed starting line of the earliest outer attribute sibling attached to
/// `statement`. Python has none: `@decorator` syntax wraps the definition in a
/// `decorated_definition` node handled by [`decorated_definition`].
#[must_use]
pub(super) const fn earliest_attribute_start_line(_statement: &RawNode<'_>) -> Option<usize> {
    None
}

/// Returns true for Python comment node kinds.
///
/// Python spells every comment `comment`, whether or not it is used as documentation.
#[must_use]
pub(super) fn is_comment_kind(kind: &str) -> bool {
    kind == "comment"
}

/// Returns true if a Python node of `parent_kind` makes a child identifier an import binding.
#[must_use]
pub(super) fn is_import_binding_parent(parent_kind: &str) -> bool {
    matches!(
        parent_kind,
        "import_statement" | "import_from_statement" | "aliased_import" | "dotted_name"
    )
}

/// Returns true if a Python node of `parent_kind` declares a structural definition name.
#[must_use]
pub(super) fn is_structural_definition_parent(parent_kind: &str) -> bool {
    matches!(parent_kind, "class_definition" | "function_definition")
}

/// Returns true if `kind` is a call expression in Python.
#[must_use]
pub(super) fn is_call_kind(kind: &str) -> bool {
    kind == "call"
}

/// If `function` is a method access (e.g. `obj.method`), returns the method identifier node.
#[must_use]
pub(super) fn extract_method_call_target<'a>(function: &RawNode<'a>) -> Option<RawNode<'a>> {
    if function.kind().as_ref() == "attribute" {
        function.field("attribute")
    } else {
        None
    }
}

/// Returns true if a Python `function_definition` node is a test function (`test` or `test_*`).
fn is_test_function_raw(func_node: &RawNode<'_>) -> bool {
    func_node.field("name").is_some_and(|name_node| {
        let func_name = name_node.text();
        func_name == "test" || func_name.starts_with("test_")
    })
}

/// Returns true if a Python `call` node is a test assertion call
/// (`self.assert*()`, `pytest.raises(...)`, `raises(...)`, `pytest.warns(...)`, `self.fail(...)`).
fn is_assertion_call_raw(call_node: &RawNode<'_>) -> bool {
    let Some(func) = call_node.field("function") else {
        return false;
    };
    match func.kind().as_ref() {
        "identifier" => func.text() == "raises",
        "attribute" => {
            let Some(attr) = func.field("attribute") else {
                return false;
            };
            let attr_name = attr.text();
            if attr_name.starts_with("assert") {
                return true;
            }
            let Some(obj) = func.field("object") else {
                return false;
            };
            let obj_text = obj.text();
            (obj_text == "pytest" && (attr_name == "raises" || attr_name == "warns"))
                || (obj_text == "self" && attr_name == "fail")
        }
        _ => false,
    }
}

/// Represents a keyword argument (`key=value`) in a Python call or argument list.
#[derive(Clone)]
pub struct KeywordArg<'a> {
    /// The keyword parameter name.
    pub name: String,
    /// AST node for the argument identifier name.
    pub name_node: AstNode<'a>,
    /// AST node for the argument value expression.
    pub value_node: AstNode<'a>,
}

impl KeywordArg<'_> {
    /// Evaluates literal boolean arguments (`True` / `False`).
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self.value_node.raw.kind().as_ref() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    }
}

/// Extracts all keyword arguments from any Python `call` or `argument_list` node.
fn extract_keyword_args_raw<'a>(call_or_args_node: &RawNode<'a>) -> Vec<KeywordArg<'a>> {
    let args_node = if call_or_args_node.kind() == "argument_list" {
        Some(call_or_args_node.clone())
    } else {
        call_or_args_node.field("arguments")
    };

    let Some(args) = args_node else {
        return Vec::new();
    };

    let mut result = Vec::new();
    for child in args.children() {
        if child.kind() == "keyword_argument"
            && let (Some(name_n), Some(val_n)) = (child.field("name"), child.field("value"))
        {
            result.push(KeywordArg {
                name: name_n.text().to_string(),
                name_node: AstNode::from_raw(name_n),
                value_node: AstNode::from_raw(val_n),
            });
        }
    }
    result
}

/// Structured metadata for a Python decorator.
#[derive(Clone)]
pub struct DecoratorInfo<'a> {
    /// Full decorator AST node (including `@`).
    pub node: AstNode<'a>,
    /// Full path string (e.g. `"dataclasses.dataclass"`, `"pytest.mark.parametrize"`).
    pub path: String,
    /// Terminal identifier (e.g. `"dataclass"`, `"parametrize"`).
    pub terminal_name: String,
    /// Call node if the decorator was invoked with parentheses `@dec(...)`.
    pub call_node: Option<AstNode<'a>>,
    /// Parsed keyword arguments if invoked as a call.
    pub keyword_args: Vec<KeywordArg<'a>>,
}

impl<'a> DecoratorInfo<'a> {
    /// Looks up a keyword argument by name.
    #[must_use]
    pub fn get_arg(&self, key: &str) -> Option<&KeywordArg<'a>> {
        self.keyword_args.iter().find(|kw| kw.name == key)
    }

    /// Returns true if a keyword argument with `key` was explicitly passed.
    #[must_use]
    pub fn has_arg(&self, key: &str) -> bool {
        self.get_arg(key).is_some()
    }
}

/// Helper to resolve the dotted expression path and terminal identifier.
fn resolve_path_and_terminal_raw(expr: &RawNode<'_>) -> (String, String) {
    let path = expr.text().to_string();
    let terminal = expr.field("attribute").map_or_else(
        || path.rsplit('.').next().unwrap_or("").to_string(),
        |attr| attr.text().to_string(),
    );
    (path, terminal)
}

/// Extracts all decorators from a `decorated_definition` or a definition node inside one.
fn extract_decorators_raw<'a>(node: &RawNode<'a>) -> Vec<DecoratorInfo<'a>> {
    let parent = if node.kind() == "decorated_definition" {
        Some(node.clone())
    } else {
        node.parent()
    };

    let Some(dec_def) = parent else {
        return Vec::new();
    };
    if dec_def.kind() != "decorated_definition" {
        return Vec::new();
    }

    let mut decorators = Vec::new();
    for child in dec_def.children() {
        if child.kind() == "decorator" {
            // decorator children: "@" and expression (call or identifier/attribute)
            let expr_node = child.children().find(|c| c.kind() != "@");
            let Some(expr) = expr_node else {
                continue;
            };

            let (call_node, target_expr, keyword_args) = if expr.kind() == "call" {
                let call = expr.clone();
                let func = call.field("function").unwrap_or_else(|| call.clone());
                let kwargs = extract_keyword_args_raw(&call);
                (Some(AstNode::from_raw(call)), func, kwargs)
            } else {
                (None, expr, Vec::new())
            };

            let (path, terminal_name) = resolve_path_and_terminal_raw(&target_expr);

            decorators.push(DecoratorInfo {
                node: AstNode::from_raw(child),
                path,
                terminal_name,
                call_node,
                keyword_args,
            });
        }
    }
    decorators
}

/// Extracts all decorators from a `decorated_definition` or a definition node inside one.
#[must_use]
pub fn extract_decorators<'a>(node: &AstNode<'a>) -> Vec<DecoratorInfo<'a>> {
    extract_decorators_raw(&node.raw)
}

/// Returns true if a Python `function_definition` or `class_definition` has a decorator whose
/// terminal identifier or full path satisfies `predicate`.
#[must_use]
pub fn has_decorator(node: &AstNode<'_>, predicate: impl Fn(&str) -> bool) -> bool {
    extract_decorators(node)
        .into_iter()
        .any(|dec| predicate(&dec.terminal_name) || predicate(&dec.path))
}

/// A Python module-level constant whose type annotation or initializer uses a mutable collection.
pub struct PythonMutableModuleConstant<'a> {
    /// Constant identifier name (e.g. `"ALLOWED"` or `"_PORTS"`).
    pub name: String,
    /// Matched mutable collection type constructors (e.g. `["list"]`, `["MutableSequence"]`).
    pub matched_types: Vec<String>,
    /// The AST node to highlight: the `type` annotation node when the annotation is mutable,
    /// or the RHS initializer expression node when the value is mutable.
    pub target_node: AstNode<'a>,
}

/// Returns true if `type_node` specifies a read-only `Mapping` contract (optionally wrapped in
/// transparent wrappers such as `Final[...]`, `Optional[...]`, or `Annotated[...]`).
fn has_read_only_mapping_annotation(type_node: &RawNode<'_>) -> bool {
    let mut matched = Vec::new();
    collect_type_constructors_raw(
        type_node,
        AnnotationTraversalDepth::TransparentWrappersOnly,
        &|full_path, terminal| is_std_type_constructor(full_path, terminal, &["Mapping"]),
        &mut matched,
    );
    !matched.is_empty()
}

/// Returns true if `(path, terminal)` is a runtime mutable collection constructor (`list`, `dict`,
/// `set`, or the `collections` containers `defaultdict`, `deque`, `Counter`, `OrderedDict`).
fn is_runtime_mutable_collection_constructor(path: &str, terminal: &str) -> bool {
    match terminal {
        "list" | "dict" | "set" => {
            path == terminal || path.strip_suffix(terminal) == Some("builtins.")
        }
        "defaultdict" | "deque" | "Counter" | "OrderedDict" => {
            path == terminal || path.strip_suffix(terminal) == Some("collections.")
        }
        _ => false,
    }
}

/// Returns the mutable collection constructor name if `right` is a mutable collection literal,
/// comprehension, or constructor call.
fn mutable_collection_initializer_type(
    right: &RawNode<'_>,
    is_read_only_mapping: bool,
) -> Option<String> {
    match right.kind().as_ref() {
        "list" | "list_comprehension" => Some("list".to_owned()),
        "set" | "set_comprehension" => Some("set".to_owned()),
        "dictionary" | "dictionary_comprehension" if !is_read_only_mapping => {
            Some("dict".to_owned())
        }
        "call" => {
            let callee = right.field("function")?;
            let callee_base = if callee.kind() == "subscript" {
                callee.field("value")?
            } else {
                callee
            };
            let (path, terminal) = resolve_path_and_terminal_raw(&callee_base);
            if is_read_only_mapping && terminal == "dict" {
                return None;
            }
            is_runtime_mutable_collection_constructor(&path, &terminal).then_some(path)
        }
        _ => None,
    }
}

/// Collects Python module-level constants whose type annotation or initializer value uses a
/// mutable collection.
#[must_use]
pub fn collect_mutable_module_constants(file: &ParsedFile) -> Vec<PythonMutableModuleConstant<'_>> {
    let abc_set_imported = has_unaliased_collections_abc_set_import(file);
    let mut out = Vec::new();
    for node in file.grep.root().dfs() {
        if node.kind() != "assignment" {
            continue;
        }
        let is_top_level = node
            .parent()
            .filter(|statement| statement.kind() == "expression_statement")
            .is_some_and(|statement| is_module_level(&statement));
        if !is_top_level {
            continue;
        }
        let Some(left) = node.field("left") else {
            continue;
        };
        if left.kind() != "identifier" {
            continue;
        }
        let name = left.text().into_owned();
        if name.starts_with("__") && name.ends_with("__") {
            continue;
        }
        let type_node = node.field("type");
        let is_final = type_node.as_ref().is_some_and(has_final_annotation);
        if !is_constant_name(&name) && !is_final {
            continue;
        }

        if let Some(ref annotation) = type_node {
            let mut matched = Vec::new();
            collect_type_constructors_raw(
                annotation,
                AnnotationTraversalDepth::CovariantPositions,
                &|full_path, terminal| {
                    if abc_set_imported && full_path == "Set" {
                        return false;
                    }
                    is_concrete_collection_constructor(full_path, terminal)
                        || is_std_type_constructor(full_path, terminal, MUTABLE_COLLECTION_ABCS)
                },
                &mut matched,
            );
            if !matched.is_empty() {
                out.push(PythonMutableModuleConstant {
                    name,
                    matched_types: matched,
                    target_node: AstNode::from_raw(annotation.clone()),
                });
                continue;
            }
        }

        let Some(right) = node.field("right").map(without_parentheses) else {
            continue;
        };
        let is_read_only_mapping = type_node
            .as_ref()
            .is_some_and(has_read_only_mapping_annotation);
        if let Some(matched_type) =
            mutable_collection_initializer_type(&right, is_read_only_mapping)
        {
            out.push(PythonMutableModuleConstant {
                name,
                matched_types: vec![matched_type],
                target_node: AstNode::from_raw(right),
            });
        }
    }
    out
}

/// Traverses upward from an expression to find if it is enclosed in a `with_item`.
/// Transparently handles expressions wrapped in `parenthesized_expression`.
#[must_use]
pub fn find_enclosing_with_item<'a>(node: &AstNode<'a>) -> Option<AstNode<'a>> {
    for ancestor in node.raw.ancestors() {
        match ancestor.kind().as_ref() {
            "with_item" => return Some(AstNode::from_raw(ancestor)),
            "parenthesized_expression" => {}
            _ => return None,
        }
    }
    None
}

/// Traverses upward from a node to find its nearest enclosing `with_statement`.
#[must_use]
pub fn find_enclosing_with_statement<'a>(node: &AstNode<'a>) -> Option<AstNode<'a>> {
    node.raw
        .ancestors()
        .find(|parent| parent.kind() == "with_statement")
        .map(AstNode::from_raw)
}

/// Returns true if `node` is invoked as a context manager inside a Python `with` statement header.
#[must_use]
pub fn is_with_context_manager(node: &AstNode<'_>) -> bool {
    find_enclosing_with_item(node).is_some() && find_enclosing_with_statement(node).is_some()
}

/// Returns true if `node` is enclosed inside an `except_clause` block within the same scope.
#[must_use]
pub fn is_inside_except_clause(node: &AstNode<'_>) -> bool {
    for ancestor in node.raw.ancestors() {
        match ancestor.kind().as_ref() {
            "except_clause" => return true,
            "function_definition" | "lambda" | "class_definition" => return false,
            _ => {}
        }
    }
    false
}

/// Returns true if `node` is a Python `tuple` or `list` consisting solely of `>= 2` boolean literals (`True` / `False`).
fn is_boolean_literal_collection_raw(node: &RawNode<'_>) -> bool {
    let kind = node.kind();
    if kind != "tuple" && kind != "list" {
        return false;
    }
    let items: Vec<_> = node
        .children()
        .filter(|child| !matches!(child.kind().as_ref(), "(" | ")" | "[" | "]" | ","))
        .collect();
    items.len() >= 2
        && items
            .iter()
            .all(|item| matches!(item.kind().as_ref(), "true" | "false"))
}

/// Collects all Python `assert_statement` nodes in `file`.
#[must_use]
pub fn collect_assert_statements(file: &ParsedFile) -> Vec<AstNode<'_>> {
    file.grep
        .root()
        .dfs()
        .filter(|node| node.kind() == "assert_statement")
        .map(AstNode::from_raw)
        .collect()
}

/// Returns true if a Python `assert_statement` node has a top-level `and` boolean operator.
#[must_use]
pub fn has_top_level_logical_and(assert_node: &AstNode<'_>) -> bool {
    assert_node
        .raw
        .children()
        .any(|c| c.kind() == "boolean_operator" && c.children().any(|op| op.kind() == "and"))
}

/// Returns true if a Python `assert_statement` node compares against a boolean literal tuple/list.
#[must_use]
pub fn has_boolean_literal_comparison(assert_node: &AstNode<'_>) -> bool {
    assert_node
        .raw
        .children()
        .find(|c| c.kind() == "comparison_operator")
        .is_some_and(|comp| {
            comp.children()
                .any(|c| is_boolean_literal_collection_raw(&c))
        })
}

/// Collects all outermost Python test function definitions (`def test` or `def test_*`).
#[must_use]
pub fn collect_outer_test_functions(file: &ParsedFile) -> Vec<AstNode<'_>> {
    let mut out = Vec::new();
    collect_outer_test_functions_rec(&file.grep.root(), &mut out);
    out
}

fn collect_outer_test_functions_rec<'a>(node: &RawNode<'a>, out: &mut Vec<AstNode<'a>>) {
    if node.kind() == "function_definition" {
        if is_test_function_raw(node) {
            out.push(AstNode::from_raw(node.clone()));
        }
        return;
    }
    for child in node.children() {
        collect_outer_test_functions_rec(&child, out);
    }
}

/// Recursively counts top-level assertion constructs in a Python test function body.
fn count_python_assertions(node: &RawNode<'_>) -> usize {
    let kind = node.kind();
    if matches!(kind.as_ref(), "function_definition" | "class_definition") {
        return 0;
    }
    if kind == "assert_statement" || (kind == "call" && is_assertion_call_raw(node)) {
        return 1;
    }
    node.children()
        .map(|child| count_python_assertions(&child))
        .sum()
}

/// Collects all outermost Python test functions along with their identifier node, name, and assertion count.
#[must_use]
pub fn collect_test_function_assertion_counts(
    file: &ParsedFile,
) -> Vec<(AstNode<'_>, String, usize)> {
    collect_outer_test_functions(file)
        .into_iter()
        .filter_map(|func_node| {
            let name_node = func_node.raw.field("name")?;
            let body_node = func_node.raw.field("body")?;
            let func_name = name_node.text().to_string();
            let count = count_python_assertions(&body_node);
            Some((AstNode::from_raw(name_node), func_name, count))
        })
        .collect()
}

/// If `node` is a Python `function_definition`, returns its name and whether it is declared at top-level `module` scope.
#[must_use]
pub(super) fn function_name_and_is_top_level<'a>(
    node: &RawNode<'a>,
) -> Option<(std::borrow::Cow<'a, str>, bool)> {
    if node.kind() != "function_definition" {
        return None;
    }
    let name_node = node.field("name")?;
    let is_top_level = node
        .parent()
        .is_some_and(|parent| parent.kind() == "module");
    Some((name_node.text(), is_top_level))
}

/// Returns the formatted subscript label (`"os.environ[...]"` or `"environ[...]"`) if `node`
/// indexes into Python's `os.environ` or `environ` mapping.
fn python_environ_subscript_label(node: &RawNode<'_>) -> Option<&'static str> {
    if node.kind() != "subscript" {
        return None;
    }
    let value = node.field("value")?;
    match value.kind().as_ref() {
        "identifier" if value.text() == "environ" => Some("environ[...]"),
        "attribute" => {
            let obj = value.field("object")?;
            let attr = value.field("attribute")?;
            (obj.text() == "os" && attr.text() == "environ").then_some("os.environ[...]")
        }
        _ => None,
    }
}

/// Collects all Python subscript expressions indexing into `os.environ` or `environ`.
#[must_use]
pub fn collect_environ_subscripts(file: &ParsedFile) -> Vec<(AstNode<'_>, &'static str)> {
    file.grep
        .root()
        .dfs()
        .filter_map(|node| {
            let label = python_environ_subscript_label(&node)?;
            Some((AstNode::from_raw(node), label))
        })
        .collect()
}

/// Returns true if `node` is a Python multiline triple-quoted string literal.
fn is_multiline_string_literal_raw(node: &RawNode<'_>) -> bool {
    node.kind() == "string"
        && node.end_pos().line() > node.start_pos().line()
        && is_triple_quoted(node)
}

/// Returns true if a Python `string` node is a standalone docstring statement.
fn is_docstring_raw(node: &RawNode<'_>) -> bool {
    node.parent()
        .is_some_and(|parent| parent.kind() == "expression_statement")
}

/// Returns true if `node` is enclosed in a Python `call` within the current scope whose
/// `(full_path, terminal_name)` satisfies `predicate`.
fn is_enclosed_in_call_raw(node: &RawNode<'_>, predicate: impl Fn(&str, &str) -> bool) -> bool {
    for ancestor in node.ancestors() {
        match ancestor.kind().as_ref() {
            "function_definition" | "class_definition" | "lambda" => break,
            "call" => {
                if let Some(func_node) = ancestor.field("function") {
                    let (path, terminal) = resolve_path_and_terminal_raw(&func_node);
                    if predicate(&path, &terminal) {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

/// Finds all multiline string literals in a Python file that are not docstrings
/// and not wrapped in an allowed call.
#[must_use]
pub fn find_unwrapped_multiline_strings(
    file: &ParsedFile,
    is_allowed_wrapper: impl Fn(&str, &str) -> bool,
) -> Vec<AstNode<'_>> {
    file.grep
        .root()
        .dfs()
        .filter(|node| {
            is_multiline_string_literal_raw(node)
                && !is_docstring_raw(node)
                && !is_enclosed_in_call_raw(node, &is_allowed_wrapper)
        })
        .map(AstNode::from_raw)
        .collect()
}

/// Returns the value of a Python decimal `integer` literal (not `0x1`, `1_000`, ...).
fn decimal_literal(node: &RawNode<'_>) -> Option<i64> {
    let text = node.text();
    if node.kind() != "integer" || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// Returns the position read by a Python `subscript` index: a decimal literal, or its negation
/// for end-relative reads (`xs[-1]`).
fn literal_position(index: &RawNode<'_>) -> Option<i64> {
    if index.kind() == "unary_operator" && index.field("operator")?.text() == "-" {
        return decimal_literal(&index.field("argument")?).map(std::ops::Neg::neg);
    }
    decimal_literal(index)
}

/// Returns true if `receiver` names a stable value: a name, attribute or subscript chain with no
/// call inside, so that identical text means the same value.
fn is_stable_receiver(receiver: &RawNode<'_>) -> bool {
    matches!(
        receiver.kind().as_ref(),
        "identifier" | "attribute" | "subscript"
    ) && !receiver.dfs().any(|node| node.kind() == "call")
}

/// Node kinds an assignment or deletion target can be nested in (`a[0], b = ...`).
const TARGET_CONTAINER_KINDS: &[&str] = &[
    "pattern_list",
    "tuple_pattern",
    "list_pattern",
    "expression_list",
    "parenthesized_expression",
    "list_splat_pattern",
];

/// Methods that mutate a `list`, `dict` or `set` in place.
const MUTATING_METHODS: &[&str] = &[
    "append",
    "extend",
    "insert",
    "pop",
    "remove",
    "clear",
    "sort",
    "reverse",
    "update",
    "setdefault",
    "popitem",
    "add",
    "discard",
    "intersection_update",
    "difference_update",
    "symmetric_difference_update",
];

/// Returns true if `node` is assigned to, augmented, deleted, or bound by a `for` loop.
fn is_write_target(node: &RawNode<'_>) -> bool {
    let mut target = node.clone();
    while let Some(parent) = target.parent() {
        match parent.kind().as_ref() {
            kind if TARGET_CONTAINER_KINDS.contains(&kind) => target = parent,
            "delete_statement" => return true,
            "assignment" | "augmented_assignment" | "for_statement" | "for_in_clause" => {
                return parent
                    .field("left")
                    .is_some_and(|left| left.range() == target.range());
            }
            _ => return false,
        }
    }
    false
}

/// If `node` mutates a collection receiver in place (`receiver.append(...)`, `receiver[k] = v`,
/// `del receiver[k]`, `receiver += ...`), returns that `receiver` node.
fn in_place_mutated_receiver<'a>(node: &RawNode<'a>) -> Option<RawNode<'a>> {
    match node.kind().as_ref() {
        "call" => {
            let function_node = node.field("function")?;
            if function_node.kind() != "attribute" {
                return None;
            }
            let method = function_node.field("attribute")?;
            if MUTATING_METHODS.contains(&method.text().as_ref()) {
                function_node.field("object")
            } else {
                None
            }
        }
        "subscript" if is_write_target(node) => node.field("value"),
        "augmented_assignment" => node.field("left"),
        _ => None,
    }
}

/// Extracts the terminal function/method name if `node` is a `call` expression.
fn called_terminal_name(node: &RawNode<'_>) -> Option<String> {
    if node.kind() != "call" {
        return None;
    }
    let function_node = node.field("function")?;
    let (_, terminal) = resolve_path_and_terminal_raw(&function_node);
    (!terminal.is_empty()).then_some(terminal)
}

/// Collects function or method names whose return values are mutated in place in `file`.
///
/// Matches direct call mutations (`fn().append(...)`, `fn()[0] = 1`) and local bindings
/// (`buf = fn(); buf.append(...)`, `(buf := fn())`). A binding only counts in the function
/// (or module) scope that assigns it. Callees are matched by name only, so a mutated
/// `obj.get()` result also exempts an unrelated function named `get`.
#[must_use]
pub fn collect_locally_mutated_return_functions(file: &ParsedFile) -> HashSet<String> {
    // A binding is keyed by its enclosing function (`None` at module level) and its name.
    type ScopedName = (Option<usize>, String);
    let enclosing_function = |node: &RawNode<'_>| {
        node.ancestors()
            .find(|ancestor| ancestor.kind() == "function_definition")
            .map(|function_node| function_node.range().start)
    };
    let mut mutated_functions = HashSet::new();
    let mut bindings_to_callee: HashMap<ScopedName, String> = HashMap::new();
    let mut mutated_identifiers: HashSet<ScopedName> = HashSet::new();

    for node in file.grep.root().dfs() {
        let binding = match node.kind().as_ref() {
            "assignment" => node.field("left").zip(node.field("right")),
            "named_expression" => node.field("name").zip(node.field("value")),
            _ => None,
        };
        if let Some((target, value)) = binding
            && target.kind() == "identifier"
            && let Some(callee_name) = called_terminal_name(&value)
        {
            let scoped_name = (enclosing_function(&node), target.text().into_owned());
            bindings_to_callee.insert(scoped_name, callee_name);
        }

        if let Some(receiver) = in_place_mutated_receiver(&node) {
            if let Some(callee_name) = called_terminal_name(&receiver) {
                mutated_functions.insert(callee_name);
            } else if receiver.kind() == "identifier" {
                mutated_identifiers
                    .insert((enclosing_function(&receiver), receiver.text().into_owned()));
            }
        }
    }

    for scoped_name in &mutated_identifiers {
        if let Some(callee_name) = bindings_to_callee.get(scoped_name) {
            mutated_functions.insert(callee_name.clone());
        }
    }

    mutated_functions
}

/// Read-only methods available on `Sequence`, `Mapping`, or `Set` (`collections.abc`).
const READONLY_COLLECTION_METHODS: &[&str] = &[
    "count",
    "index",
    "get",
    "keys",
    "values",
    "items",
    "isdisjoint",
    "issubset",
    "issuperset",
    "copy",
    "__contains__",
    "__len__",
    "__iter__",
    "__getitem__",
    "__reversed__",
];

/// Builtin functions that read or iterate a collection without mutating it in place or
/// retaining a mutable alias to the outer container.
const SAFE_READONLY_BUILTINS: &[&str] = &[
    "len",
    "max",
    "min",
    "sum",
    "sorted",
    "list",
    "tuple",
    "set",
    "frozenset",
    "dict",
    "bool",
    "any",
    "all",
    "enumerate",
    "zip",
    "reversed",
    "iter",
    "map",
    "filter",
    "repr",
    "str",
    "hash",
    "id",
    "type",
    "isinstance",
    "issubclass",
    "print",
    "range",
    "next",
];

/// Builtin functions that consume an `Iterable` in a single pass.
const SINGLE_PASS_ITERABLE_BUILTINS: &[&str] = &[
    "sum",
    "min",
    "max",
    "any",
    "all",
    "sorted",
    "list",
    "tuple",
    "set",
    "frozenset",
    "dict",
    "enumerate",
    "zip",
    "iter",
    "map",
    "filter",
];

/// Unwraps enclosing `parenthesized_expression` nodes around `node`, returning `(outermost_expr, parent)`.
fn unwrap_parenthesized_with_parent<'a>(node: &RawNode<'a>) -> Option<(RawNode<'a>, RawNode<'a>)> {
    let mut expr = node.clone();
    while let Some(parent) = expr.parent() {
        if parent.kind() == "parenthesized_expression" {
            expr = parent;
        } else {
            return Some((expr, parent));
        }
    }
    None
}

/// Returns true if `ident_node` (`identifier`) is a variable reference rather than an attribute
/// name (`obj.x`), keyword argument name (`f(x=1)`), or declaration name.
fn is_variable_reference(ident_node: &RawNode<'_>) -> bool {
    let Some(parent) = ident_node.parent() else {
        return false;
    };
    match parent.kind().as_ref() {
        "attribute" => !parent
            .field("attribute")
            .is_some_and(|attr_node| attr_node.range() == ident_node.range()),
        "keyword_argument" => !parent
            .field("name")
            .is_some_and(|name_node| name_node.range() == ident_node.range()),
        "function_definition" | "class_definition" => !parent
            .field("name")
            .is_some_and(|name_node| name_node.range() == ident_node.range()),
        "parameters"
        | "lambda_parameters"
        | "typed_parameter"
        | "default_parameter"
        | "typed_default_parameter"
        | "list_splat_pattern"
        | "dictionary_splat_pattern" => parent
            .field("value")
            .is_some_and(|value_node| value_node.range() == ident_node.range()),
        _ => true,
    }
}

/// Returns true if `call_node` is a direct call to a builtin in `SAFE_READONLY_BUILTINS`.
fn is_safe_readonly_builtin_call(call_node: &RawNode<'_>) -> bool {
    call_node.kind() == "call"
        && call_node.field("function").is_some_and(|function_node| {
            function_node.kind() == "identifier"
                && SAFE_READONLY_BUILTINS.contains(&function_node.text().as_ref())
        })
}

/// Returns true if `node` sits inside a boolean test position (`if`, `elif`, `while`, `assert`, or `bool(...)`).
fn is_in_boolean_context(node: &RawNode<'_>) -> bool {
    let mut current = node.clone();
    while let Some(parent) = current.parent() {
        match parent.kind().as_ref() {
            "parenthesized_expression" | "boolean_operator" | "not_operator" => {
                current = parent;
            }
            "if_statement" | "elif_clause" | "while_statement" => {
                return parent
                    .field("condition")
                    .is_some_and(|condition| condition.range() == current.range());
            }
            "assert_statement" => return true,
            "argument_list" => {
                return parent.parent().is_some_and(|call_node| {
                    call_node.kind() == "call"
                        && call_node
                            .field("function")
                            .is_some_and(|func_node| func_node.text() == "bool")
                });
            }
            _ => return false,
        }
    }
    false
}

/// Returns true if `expr` is the middle condition child of a Python `conditional_expression` (`a if cond else b`).
fn is_conditional_expression_condition(cond_expr: &RawNode<'_>, expr: &RawNode<'_>) -> bool {
    let mut named = cond_expr
        .children()
        .filter(|child| child.is_named() && !child.is_extra());
    let _consequence = named.next();
    named
        .next()
        .is_some_and(|condition| condition.range() == expr.range())
}

/// Returns true if `ident_node` is used in a strictly read-only, non-escaping position.
fn is_safe_readonly_parameter_reference(ident_node: &RawNode<'_>) -> bool {
    if is_write_target(ident_node) {
        return false;
    }
    let Some((expr, parent)) = unwrap_parenthesized_with_parent(ident_node) else {
        return false;
    };
    match parent.kind().as_ref() {
        "attribute" => {
            parent
                .field("object")
                .is_some_and(|object_node| object_node.range() == expr.range())
                && parent.field("attribute").is_some_and(|attr_node| {
                    READONLY_COLLECTION_METHODS.contains(&attr_node.text().as_ref())
                })
                && unwrap_parenthesized_with_parent(&parent).is_some_and(|(attr_expr, grand)| {
                    grand.kind() == "call"
                        && grand
                            .field("function")
                            .is_some_and(|func_node| func_node.range() == attr_expr.range())
                })
        }
        "subscript" => !is_write_target(&parent),
        "argument_list" => parent
            .parent()
            .is_some_and(|call_node| is_safe_readonly_builtin_call(&call_node)),
        "keyword_argument" => parent.parent().is_some_and(|arguments| {
            arguments.kind() == "argument_list"
                && arguments
                    .parent()
                    .is_some_and(|call_node| is_safe_readonly_builtin_call(&call_node))
        }),
        "list_splat" | "dictionary_splat" => {
            unwrap_parenthesized_with_parent(&parent).is_some_and(|(_, grand)| {
                matches!(
                    grand.kind().as_ref(),
                    "list" | "tuple" | "set" | "dictionary"
                ) || (grand.kind() == "argument_list"
                    && grand
                        .parent()
                        .is_some_and(|call_node| is_safe_readonly_builtin_call(&call_node)))
            })
        }
        "for_statement" | "for_in_clause" => parent
            .field("right")
            .is_some_and(|right| right.range() == expr.range()),
        "comparison_operator" | "not_operator" | "binary_operator" | "assert_statement" => true,
        "boolean_operator" => is_in_boolean_context(&parent),
        "if_statement" | "elif_clause" | "while_statement" => parent
            .field("condition")
            .is_some_and(|condition| condition.range() == expr.range()),
        "conditional_expression" => is_conditional_expression_condition(&parent, &expr),
        _ => false,
    }
}

fn check_parameter_mutated_or_escaping_rec(node: &RawNode<'_>, parameter_name: &str) -> bool {
    if node.kind() == "type" {
        return false;
    }
    if matches!(node.kind().as_ref(), "function_definition" | "lambda")
        && scope_shadows_parameter(node, parameter_name)
    {
        // Defaults are evaluated in the enclosing scope; only the body is shadowed.
        return node.field("parameters").is_some_and(|params| {
            check_parameter_mutated_or_escaping_rec(&params, parameter_name)
        });
    }
    if node.kind() == "identifier"
        && node.text() == parameter_name
        && is_variable_reference(node)
        && !is_safe_readonly_parameter_reference(node)
    {
        return true;
    }
    node.children()
        .any(|child| check_parameter_mutated_or_escaping_rec(&child, parameter_name))
}

/// Returns true if `parameter_name` is mutated in place or escapes (aliased, returned, yielded,
/// or passed to an unknown function/method) anywhere in `func_node`'s body.
#[must_use]
pub fn is_parameter_mutated_or_escaping(func_node: &AstNode<'_>, parameter_name: &str) -> bool {
    let Some(body) = func_node.raw.field("body") else {
        return false;
    };
    check_parameter_mutated_or_escaping_rec(&body, parameter_name)
}

/// Minimum read-only `collections.abc` capability required by a parameter's usages inside
/// its function body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ParameterCollectionCapability {
    /// Parameter is never referenced in the function body.
    Unused,
    /// Parameter is only iterated once at top-level depth (`Iterable` suffices).
    Iterable,
    /// Parameter uses `len(x)`, `v in x`, truthiness (`if x:`), or multi-pass iteration (`Collection` suffices).
    Collection,
    /// Parameter uses indexing/slicing, `reversed(x)`, `.index()`/`.count()`, pattern matching, or escapes (`Sequence` required).
    Sequence,
}

#[derive(Default)]
struct CapabilityTracker {
    iteration_count: usize,
    needs_collection: bool,
    needs_sequence: bool,
}

/// Inspects a `comparison_operator` node containing `expr` and updates `tracker`.
fn record_comparison_capability(
    comp_node: &RawNode<'_>,
    expr: &RawNode<'_>,
    tracker: &mut CapabilityTracker,
) {
    let has_in_operator = comp_node
        .children()
        .any(|child| matches!(child.kind().as_ref(), "in" | "not in"));
    let is_identity_check = comp_node
        .children()
        .any(|child| matches!(child.kind().as_ref(), "is" | "is not"));
    let last_named = comp_node
        .children()
        .filter(|child| child.is_named() && !child.is_extra())
        .last();
    if has_in_operator && last_named.is_some_and(|last| last.range() == expr.range()) {
        tracker.needs_collection = true;
    } else if !is_identity_check {
        tracker.needs_sequence = true;
    }
}

/// Updates `tracker` for a single variable reference `ident_node` at `loop_or_closure_depth`.
fn record_reference_capability(
    ident_node: &RawNode<'_>,
    loop_or_closure_depth: usize,
    tracker: &mut CapabilityTracker,
) {
    if is_write_target(ident_node) {
        tracker.needs_sequence = true;
        return;
    }
    let Some((expr, parent)) = unwrap_parenthesized_with_parent(ident_node) else {
        tracker.needs_sequence = true;
        return;
    };
    match parent.kind().as_ref() {
        "for_statement" | "for_in_clause"
            if parent
                .field("right")
                .is_some_and(|right| right.range() == expr.range()) =>
        {
            tracker.iteration_count += 1;
            if loop_or_closure_depth > 0 {
                tracker.needs_collection = true;
            }
        }
        "list_splat" => {
            if let Some((_, grand)) = unwrap_parenthesized_with_parent(&parent)
                && matches!(grand.kind().as_ref(), "list" | "tuple" | "set")
            {
                tracker.iteration_count += 1;
                if loop_or_closure_depth > 0 {
                    tracker.needs_collection = true;
                }
            } else {
                tracker.needs_sequence = true;
            }
        }
        "argument_list" => {
            let Some(call_node) = parent.parent() else {
                tracker.needs_sequence = true;
                return;
            };
            let Some(func_node) = call_node.field("function") else {
                tracker.needs_sequence = true;
                return;
            };
            if func_node.kind() != "identifier" {
                tracker.needs_sequence = true;
                return;
            }
            match func_node.text().as_ref() {
                "len" | "bool" => tracker.needs_collection = true,
                name if SINGLE_PASS_ITERABLE_BUILTINS.contains(&name) => {
                    tracker.iteration_count += 1;
                    if loop_or_closure_depth > 0 {
                        tracker.needs_collection = true;
                    }
                }
                _ => tracker.needs_sequence = true,
            }
        }
        "comparison_operator" => record_comparison_capability(&parent, &expr, tracker),
        "not_operator" => tracker.needs_collection = true,
        "boolean_operator" => {
            if is_in_boolean_context(&parent) {
                tracker.needs_collection = true;
            } else {
                tracker.needs_sequence = true;
            }
        }
        "if_statement" | "elif_clause" | "while_statement"
            if parent
                .field("condition")
                .is_some_and(|condition| condition.range() == expr.range()) =>
        {
            tracker.needs_collection = true;
        }
        "conditional_expression" if is_conditional_expression_condition(&parent, &expr) => {
            tracker.needs_collection = true;
        }
        _ => tracker.needs_sequence = true,
    }
}

fn analyze_capability_rec(
    node: &RawNode<'_>,
    parameter_name: &str,
    loop_or_closure_depth: usize,
    tracker: &mut CapabilityTracker,
) {
    if tracker.needs_sequence || node.kind() == "type" {
        return;
    }
    match node.kind().as_ref() {
        "function_definition" | "lambda" => {
            if scope_shadows_parameter(node, parameter_name) {
                // Defaults are evaluated in the enclosing scope; only the body is shadowed.
                if let Some(params) = node.field("parameters") {
                    analyze_capability_rec(&params, parameter_name, loop_or_closure_depth, tracker);
                }
                return;
            }
            for child in node.children() {
                analyze_capability_rec(&child, parameter_name, loop_or_closure_depth + 1, tracker);
            }
            return;
        }
        "for_statement" => {
            let right_range = node.field("right").map(|right| right.range());
            for child in node.children() {
                let child_depth = if right_range.as_ref() == Some(&child.range()) {
                    loop_or_closure_depth
                } else {
                    loop_or_closure_depth + 1
                };
                analyze_capability_rec(&child, parameter_name, child_depth, tracker);
            }
            return;
        }
        "while_statement" => {
            for child in node.children() {
                analyze_capability_rec(&child, parameter_name, loop_or_closure_depth + 1, tracker);
            }
            return;
        }
        "list_comprehension"
        | "set_comprehension"
        | "dictionary_comprehension"
        | "generator_expression" => {
            let mut seen_first_for_clause = false;
            for child in node.children() {
                if child.kind() == "for_in_clause" && !seen_first_for_clause {
                    seen_first_for_clause = true;
                    let right_range = child.field("right").map(|right| right.range());
                    for clause_child in child.children() {
                        let clause_depth = if right_range.as_ref() == Some(&clause_child.range()) {
                            loop_or_closure_depth
                        } else {
                            loop_or_closure_depth + 1
                        };
                        analyze_capability_rec(
                            &clause_child,
                            parameter_name,
                            clause_depth,
                            tracker,
                        );
                    }
                } else {
                    analyze_capability_rec(
                        &child,
                        parameter_name,
                        loop_or_closure_depth + 1,
                        tracker,
                    );
                }
            }
            return;
        }
        "identifier" if node.text() == parameter_name && is_variable_reference(node) => {
            record_reference_capability(node, loop_or_closure_depth, tracker);
            return;
        }
        _ => {}
    }

    for child in node.children() {
        analyze_capability_rec(&child, parameter_name, loop_or_closure_depth, tracker);
    }
}

/// Determines the minimum read-only collection capability (`Iterable`, `Collection`, or `Sequence`)
/// required by `parameter_name` across `func_node`'s body.
#[must_use]
pub fn analyze_parameter_collection_capability(
    func_node: &AstNode<'_>,
    parameter_name: &str,
) -> ParameterCollectionCapability {
    let Some(body) = func_node.raw.field("body") else {
        return ParameterCollectionCapability::Unused;
    };
    let mut tracker = CapabilityTracker::default();
    analyze_capability_rec(&body, parameter_name, 0, &mut tracker);

    if tracker.needs_sequence {
        ParameterCollectionCapability::Sequence
    } else if tracker.needs_collection || tracker.iteration_count > 1 {
        ParameterCollectionCapability::Collection
    } else if tracker.iteration_count == 1 {
        ParameterCollectionCapability::Iterable
    } else {
        ParameterCollectionCapability::Unused
    }
}

/// Returns the position a `subscript` reads, if its index is a single literal.
fn literal_index(subscript: &RawNode<'_>) -> Option<i64> {
    let mut indices = subscript.field_children("subscript");
    match (indices.next(), indices.next()) {
        (Some(index), None) => literal_position(&index),
        _ => None,
    }
}

/// Records a `subscript` as a positional read, or its receiver as an exempt receiver when it
/// is written to or indexed by anything but a single literal.
fn record_subscript<'a>(subscript: &RawNode<'a>, scope: &mut ScopePositionalReads<'a>) {
    let Some(receiver) = subscript.field("value") else {
        return;
    };
    match literal_index(subscript) {
        Some(position) if !is_write_target(subscript) => {
            if is_stable_receiver(&receiver) {
                scope.reads.push(PositionalRead {
                    node: AstNode::from_raw(subscript.clone()),
                    receiver: receiver.text().into_owned(),
                    position,
                });
            }
        }
        _ => {
            scope.exempt_receivers.insert(receiver.text().into_owned());
        }
    }
}

/// Builtins that iterate or size their positional arguments (`len(xs)`, `enumerate(xs)`,
/// `zip(xs, ys)`).
const COLLECTION_BUILTINS: &[&str] = &["len", "enumerate", "zip", "reversed", "sorted"];

/// Returns the collections a `call` iterates, sizes (`len(xs)`, `zip(xs, ys)`) or mutates
/// (`xs.append(...)`).
fn called_collections<'a>(call: &RawNode<'a>) -> Vec<RawNode<'a>> {
    let Some(function) = call.field("function") else {
        return Vec::new();
    };
    match function.kind().as_ref() {
        "identifier" if COLLECTION_BUILTINS.contains(&function.text().as_ref()) => {
            call.field("arguments").map_or_else(Vec::new, |arguments| {
                arguments
                    .children()
                    .filter(|child| {
                        child.is_named() && !child.is_extra() && child.kind() != "keyword_argument"
                    })
                    .collect()
            })
        }
        "attribute" => {
            let is_mutating = function
                .field("attribute")
                .is_some_and(|method| MUTATING_METHODS.contains(&method.text().as_ref()));
            if is_mutating {
                function.field("object").into_iter().collect()
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    }
}

/// Returns the collections a node iterates (`for x in xs`), sizes or mutates.
fn collection_uses<'a>(node: &RawNode<'a>) -> Vec<RawNode<'a>> {
    match node.kind().as_ref() {
        "for_statement" | "for_in_clause" => node.field("right").into_iter().collect(),
        "call" => called_collections(node),
        _ => Vec::new(),
    }
}

/// Walks `node`, recording reads and collection uses into `scope` (none in class bodies) and
/// pushing each finished function scope to `out`. Lambdas are skipped.
fn collect_positional_reads_rec<'a>(
    node: &RawNode<'a>,
    mut scope: Option<&mut ScopePositionalReads<'a>>,
    out: &mut Vec<ScopePositionalReads<'a>>,
) {
    match node.kind().as_ref() {
        "lambda" => return,
        "function_definition" => {
            // Default values and annotations are evaluated in the enclosing scope.
            let body = node.field("body");
            for child in node.children() {
                if body
                    .as_ref()
                    .is_some_and(|body| body.range() == child.range())
                {
                    let mut function_scope = ScopePositionalReads::default();
                    collect_positional_reads_rec(&child, Some(&mut function_scope), out);
                    out.push(function_scope);
                } else {
                    collect_positional_reads_rec(&child, scope.as_deref_mut(), out);
                }
            }
            return;
        }
        "class_definition" => scope = None,
        _ => {}
    }
    if let Some(scope) = scope.as_deref_mut() {
        if node.kind() == "subscript" {
            record_subscript(node, scope);
        } else {
            for collection in collection_uses(node) {
                scope
                    .exempt_receivers
                    .insert(collection.text().into_owned());
            }
        }
    }
    for child in node.children() {
        collect_positional_reads_rec(&child, scope.as_deref_mut(), out);
    }
}

/// Collects Python positional reads grouped by scope (see [`super::collect_positional_reads`]).
#[must_use]
pub fn collect_positional_reads(file: &ParsedFile) -> Vec<ScopePositionalReads<'_>> {
    let mut module_scope = ScopePositionalReads::default();
    let mut out = Vec::new();
    collect_positional_reads_rec(&file.grep.root(), Some(&mut module_scope), &mut out);
    out.push(module_scope);
    out
}

/// Calls whose first argument is a name or type the language requires as a string
/// (`TypeVar("T")`, `cast("Node", value)`).
const TYPE_NAME_FIRST_ARGUMENT_CALLS: &[&str] = &[
    "TypeVar",
    "NewType",
    "ParamSpec",
    "TypeVarTuple",
    "NamedTuple",
    "TypedDict",
    "cast",
];

/// Returns the value of a non-interpolated `string`, `integer` or `float` node, or of a `-`
/// applied to a number; `None` for anything else, imaginary numbers and overflow.
fn literal_value(node: &RawNode<'_>) -> Option<LiteralValue> {
    match node.kind().as_ref() {
        "string" if !node.children().any(|child| child.kind() == "interpolation") => {
            let (opening, content) = delimited_string_parts(node);
            // Raw backslashes are literal: spell them as a plain string would (`r"\d"` is `"\\d"`).
            let content = if opening.contains(['r', 'R']) {
                content.replace('\\', "\\\\")
            } else {
                content
            };
            Some(if opening.contains(['b', 'B']) {
                LiteralValue::Bytes(content)
            } else {
                LiteralValue::Str(content)
            })
        }
        "integer" | "float" if node.text().ends_with(['j', 'J']) => None,
        "integer" => parse_integer_literal(&node.text()),
        "float" => parse_float_literal(&node.text()),
        "unary_operator" if node.field("operator")?.text() == "-" => {
            let argument = node.field("argument")?;
            if matches!(argument.kind().as_ref(), "integer" | "float") {
                literal_value(&argument)?.negated()
            } else {
                None
            }
        }
        // `case -4:` spells the negation as a `-` token and a number directly in the pattern.
        "case_pattern" => {
            let mut children = node.children();
            match (children.next(), children.next(), children.next()) {
                (Some(sign), Some(number), None)
                    if sign.text() == "-"
                        && matches!(number.kind().as_ref(), "integer" | "float") =>
                {
                    literal_value(&number)?.negated()
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Returns true if `name` is spelled as a constant (`MAX_RETRIES`, `_TIMEOUT_S`).
fn is_constant_name(name: &str) -> bool {
    name.chars().any(|character| character.is_ascii_uppercase())
        && name.chars().all(|character| {
            character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
        })
}

/// Statements whose bodies stay at the enclosing level for constants: a platform branch or an
/// `ImportError` fallback at module level still defines module constants.
const CONSTANT_TRANSPARENT_STATEMENTS: &[&str] = &[
    "if_statement",
    "elif_clause",
    "else_clause",
    "try_statement",
    "except_clause",
    "finally_clause",
    "with_statement",
];

/// Returns true if `statement` sits at module level, possibly inside the bodies of
/// [`CONSTANT_TRANSPARENT_STATEMENTS`].
fn is_module_level(statement: &RawNode<'_>) -> bool {
    let mut container = statement.parent();
    while let Some(node) = container {
        match node.kind().as_ref() {
            "module" => return true,
            "block" => {}
            kind if CONSTANT_TRANSPARENT_STATEMENTS.contains(&kind) => {}
            _ => return false,
        }
        container = node.parent();
    }
    false
}

/// Returns true if `statement` sits at module or class level, possibly inside the bodies of
/// [`CONSTANT_TRANSPARENT_STATEMENTS`].
fn is_module_or_class_level(statement: &RawNode<'_>) -> bool {
    let mut container = statement.parent();
    while let Some(node) = container {
        match node.kind().as_ref() {
            "module" => return true,
            "block" => {
                if node
                    .parent()
                    .is_some_and(|owner| owner.kind() == "class_definition")
                {
                    return true;
                }
            }
            kind if CONSTANT_TRANSPARENT_STATEMENTS.contains(&kind) => {}
            _ => return false,
        }
        container = node.parent();
    }
    false
}

/// Returns true if an `assignment` defines a module- or class-level constant: an
/// `UPPER_SNAKE_CASE` name or a `Final` annotation.
fn is_constant_assignment(assignment: &RawNode<'_>) -> bool {
    let is_module_or_class_level = assignment
        .parent()
        .filter(|statement| statement.kind() == "expression_statement")
        .is_some_and(|statement| is_module_or_class_level(&statement));
    let is_final = assignment
        .field("type")
        .is_some_and(|type_node| has_final_annotation(&type_node));
    let is_constant_target = assignment
        .field("left")
        .is_some_and(|target| target.kind() == "identifier" && is_constant_name(&target.text()));
    is_module_or_class_level && (is_constant_target || is_final)
}

/// Returns the expression inside any enclosing parentheses (`("x")` is `"x"`).
fn without_parentheses(node: RawNode<'_>) -> RawNode<'_> {
    let mut node = node;
    while node.kind() == "parenthesized_expression" {
        let inner = node
            .children()
            .find(|child| child.is_named() && !child.is_extra());
        let Some(inner) = inner else { break };
        node = inner;
    }
    node
}

/// Walks `node`, pushing collectable literals to `out` (see [`super::collect_literal_occurrences`]).
fn collect_literal_occurrences_rec<'a>(node: &RawNode<'a>, out: &mut Vec<LiteralOccurrence<'a>>) {
    match node.kind().as_ref() {
        // Annotations and docstrings are not values.
        "type" => return,
        "string" if is_docstring_raw(node) => return,
        "assignment" if is_constant_assignment(node) => {
            // A scalar constant defines its value; a composite one (`URLS = ["a", "b"]`)
            // is a named value whose parts are not collected.
            if let Some(right) = node.field("right").map(without_parentheses)
                && let Some(value) = literal_value(&right)
            {
                out.push(LiteralOccurrence {
                    node: AstNode::from_raw(right),
                    value,
                    role: LiteralRole::ConstantDefinition,
                });
            }
            return;
        }
        "subscript"
            if node
                .field("value")
                .is_some_and(|value| resolve_path_and_terminal_raw(&value).1 == "Literal") =>
        {
            return;
        }
        "call" => {
            let is_type_name_call = node.field("function").is_some_and(|function| {
                TYPE_NAME_FIRST_ARGUMENT_CALLS
                    .contains(&resolve_path_and_terminal_raw(&function).1.as_str())
            });
            if is_type_name_call {
                let arguments = node.field("arguments");
                let first_argument = arguments
                    .as_ref()
                    .and_then(|arguments| arguments.children().find(RawNode::is_named));
                for argument in arguments.iter().flat_map(RawNode::children) {
                    let is_first = first_argument
                        .as_ref()
                        .is_some_and(|first| first.range() == argument.range());
                    if !is_first {
                        collect_literal_occurrences_rec(&argument, out);
                    }
                }
                return;
            }
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
    // Interpolated f-strings are templates: their literal parts are not collected, but the
    // expressions inside `{...}` are walked like any other code.
    let is_pattern = node.kind().ends_with("_pattern");
    let mut follows_minus = false;
    for child in node.children() {
        // Inside a pattern, `-404` can be a bare `-` token and a number with no node spanning
        // both: skip the number rather than collect it as `404`.
        let is_signed_number =
            follows_minus && matches!(child.kind().as_ref(), "integer" | "float");
        follows_minus = is_pattern && child.kind() == "-";
        if !is_signed_number {
            collect_literal_occurrences_rec(&child, out);
        }
    }
}

/// Collects Python literal occurrences (see [`super::collect_literal_occurrences`]).
#[must_use]
pub fn collect_literal_occurrences(file: &ParsedFile) -> Vec<LiteralOccurrence<'_>> {
    let mut out = Vec::new();
    collect_literal_occurrences_rec(&file.grep.root(), &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::functions::is_stub_function_body;
    use super::*;
    use LiteralRole::{ConstantDefinition, Inline};
    use ast_grep_language::SupportLang;

    #[test]
    fn test_collect_bindings_python() {
        let source = indoc::indoc! {r"
            import os
            import os.path
            import numpy as np
            from sys import stderr as err
            from os import path
            import sys, re
            from os import * # wildcard
            c = 2
            d, e = 3, 4
            for x in range(10): pass
            [y for y in range(10)]
            def foo(b: int = 1):
                pass
            try:
                pass
            except Exception as g:
                pass
            (v := 1)
            class MyClass:
                def my_method(self):
                    pass
        "};
        let file = ParsedFile::new(source, SupportLang::Python);
        let bindings = collect_bindings(&file);
        let names: Vec<String> = bindings
            .iter()
            .map(|node| node.text().to_string())
            .collect();
        assert_eq!(
            names,
            vec![
                "os",
                "os",
                "np",
                "err",
                "path",
                "sys",
                "re",
                "c",
                "d",
                "e",
                "x",
                "y",
                "foo",
                "b",
                "g",
                "v",
                "MyClass",
                "my_method",
                "self",
            ]
        );
    }

    #[test]
    fn test_collect_bindings_python_match_case() {
        let source = indoc::indoc! {r"
            match val:
                case Point(x, y=z):
                    pass
                case [a, b]:
                    pass
        "};
        let file = ParsedFile::new(source, SupportLang::Python);
        let bindings = collect_bindings(&file);
        let names: Vec<String> = bindings
            .iter()
            .map(|node| node.text().to_string())
            .collect();
        assert_eq!(names, vec!["x", "z", "a", "b"]);
    }

    #[test]
    fn test_find_enclosing_with_helpers() {
        let source = indoc::indoc! {r"
            with suppress(FileNotFoundError):
                pass
            x = suppress(KeyError)
        "};
        let file = ParsedFile::new(source, SupportLang::Python);
        let calls: Vec<_> = file
            .grep
            .root()
            .find_all("suppress($$$ARGS)")
            .map(|matched| AstNode::from_raw(matched.get_node().clone()))
            .collect();
        assert_eq!(calls.len(), 2);

        // First call is inside a with_statement
        let with_item = find_enclosing_with_item(&calls[0]);
        assert!(with_item.is_some());
        assert!(find_enclosing_with_statement(&calls[0]).is_some());

        // Second call is outside a with_statement
        assert!(find_enclosing_with_item(&calls[1]).is_none());
    }

    #[test]
    fn test_extract_decorators_count() {
        let source = indoc::indoc! {r#"
            @dataclass(frozen=True, slots=False)
            @pytest.mark.parametrize("x", [1, 2])
            @custom
            def foo():
                pass
        "#};
        let file = ParsedFile::new(source, SupportLang::Python);
        let func = AstNode::from_raw(
            file.grep
                .root()
                .find("def foo(): $$$BODY")
                .unwrap()
                .get_node()
                .clone(),
        );
        let decorators = extract_decorators(&func);
        assert_eq!(decorators.len(), 3);
    }

    #[test]
    fn test_extract_decorator_keyword_args() {
        let source = indoc::indoc! {r"
            @dataclass(frozen=True, slots=False)
            def foo():
                pass
        "};
        let file = ParsedFile::new(source, SupportLang::Python);
        let func = AstNode::from_raw(
            file.grep
                .root()
                .find("def foo(): $$$BODY")
                .unwrap()
                .get_node()
                .clone(),
        );
        let decorators = extract_decorators(&func);

        let dec0 = &decorators[0];
        assert_eq!(dec0.terminal_name, "dataclass");
        assert_eq!(
            dec0.get_arg("frozen").and_then(KeywordArg::as_bool),
            Some(true)
        );
        assert_eq!(
            dec0.get_arg("slots").and_then(KeywordArg::as_bool),
            Some(false)
        );
        assert!(dec0.get_arg("unknown").is_none());
    }

    #[test]
    fn test_extract_decorators_metadata_and_path() {
        let source = indoc::indoc! {r#"
            @pytest.mark.parametrize("x", [1, 2])
            @custom
            def foo():
                pass
        "#};
        let file = ParsedFile::new(source, SupportLang::Python);
        let func = AstNode::from_raw(
            file.grep
                .root()
                .find("def foo(): $$$BODY")
                .unwrap()
                .get_node()
                .clone(),
        );
        let decorators = extract_decorators(&func);

        let dec0 = &decorators[0];
        assert_eq!(dec0.terminal_name, "parametrize");
        assert_eq!(dec0.path, "pytest.mark.parametrize");

        let dec1 = &decorators[1];
        assert_eq!(dec1.terminal_name, "custom");
        assert!(dec1.call_node.is_none());
    }

    #[test]
    fn test_has_decorator_predicate_matching() {
        let source = indoc::indoc! {r#"
            @pytest.mark.parametrize("x", [1, 2])
            def foo():
                pass
        "#};
        let file = ParsedFile::new(source, SupportLang::Python);
        let func = AstNode::from_raw(
            file.grep
                .root()
                .find("def foo(): $$$BODY")
                .unwrap()
                .get_node()
                .clone(),
        );

        assert!(has_decorator(&func, |name| name == "parametrize"));
        assert!(has_decorator(&func, |path| path == "pytest.mark.parametrize"));
        assert!(!has_decorator(&func, |name| name == "override"));
    }

    #[test]
    fn test_extract_classes_and_inheritance() {
        let source = indoc::indoc! {r"
            class FakeService(abc.ABC, Protocol):
                pass
        "};
        let file = ParsedFile::new(source, SupportLang::Python);
        let classes = extract_classes(&file);
        assert_eq!(classes.len(), 1);

        let cls = &classes[0];
        assert_eq!(cls.name, "FakeService");
        assert!(cls.inherits_from("Protocol"));
        assert!(cls.inherits_from("ABC"));
    }

    #[test]
    fn test_extract_classes_bases_metadata() {
        let source = indoc::indoc! {r"
            class FakeService(abc.ABC, Protocol):
                pass
        "};
        let file = ParsedFile::new(source, SupportLang::Python);
        let classes = extract_classes(&file);
        let cls = &classes[0];
        assert_eq!(cls.bases.len(), 2);
        assert_eq!(cls.bases[0].name, "abc.ABC");
        assert_eq!(cls.bases[1].name, "Protocol");
    }

    #[test]
    fn test_extract_classes_dataclass() {
        let source = indoc::indoc! {r"
            @dataclass(frozen=True)
            class Config:
                pass
        "};
        let file = ParsedFile::new(source, SupportLang::Python);
        let classes = extract_classes(&file);
        assert_eq!(classes.len(), 1);

        let cls = &classes[0];
        assert_eq!(cls.name, "Config");
        assert_eq!(cls.decorators[0].terminal_name, "dataclass");
        assert!(!cls.inherits_from("Protocol"));
    }

    #[test]
    fn test_extract_parameters_kinds() {
        let source = indoc::indoc! {r"
            def handler(self, a: int, b: str = 'hello', *, c: bool, **kwargs):
                pass
        "};
        let file = ParsedFile::new(source, SupportLang::Python);
        let root = AstNode::from_raw(file.grep.root());
        let params = extract_parameters(&root);

        assert_eq!(params[0].kind, PythonParameterKind::Receiver);
        assert_eq!(params[1].kind, PythonParameterKind::Positional);
        assert_eq!(params[3].kind, PythonParameterKind::KeywordOnly);
        assert_eq!(params[4].kind, PythonParameterKind::VarKeyword);
    }

    #[test]
    fn test_extract_parameters_type_annotations() {
        let source = indoc::indoc! {r"
            def handler(self, a: int, b: str = 'hello', *, c: bool, **kwargs):
                pass
        "};
        let file = ParsedFile::new(source, SupportLang::Python);
        let root = AstNode::from_raw(file.grep.root());
        let params = extract_parameters(&root);

        assert_eq!(params[1].name, "a");
        assert_eq!(params[1].type_text.as_deref(), Some("int"));
        assert_eq!(params[2].name, "b");
        assert_eq!(params[2].type_text.as_deref(), Some("str"));
    }

    /// Module-level reads form their own scope. `rule_test!` cannot cover this: repeating a
    /// fail case in one file merges both copies into the same module scope.
    #[test]
    fn test_collect_positional_reads_module_scope() {
        let source = indoc::indoc! {r"
            src, dst = sys.argv[1], sys.argv[2]

            def main(argv):
                return argv[1]
        "};
        let file = ParsedFile::new(source, SupportLang::Python);
        let reads_by_scope: Vec<Vec<(String, i64)>> = collect_positional_reads(&file)
            .into_iter()
            .map(|scope| {
                scope
                    .reads
                    .into_iter()
                    .map(|read| (read.receiver, read.position))
                    .collect()
            })
            .collect();

        let module_reads = vec![("sys.argv".to_string(), 1), ("sys.argv".to_string(), 2)];
        assert!(
            reads_by_scope.contains(&module_reads),
            "no module scope holding exactly the top-level reads: {reads_by_scope:?}"
        );
    }

    #[rstest::rstest]
    #[case::a1_bare_concrete("def f(x: list, y: dict, z: typing.List) -> None: pass", &[&["list"][..], &["dict"][..], &["typing.List"][..]])]
    #[case::a2_pep585_generic("def f(x: list[int], y: dict[str, int], z: set[str]) -> None: pass", &[&["list"][..], &["dict"][..], &["set"][..]])]
    #[case::a3_pep484_qualified_and_unqualified("def f(x: typing.List[int], y: typing.Dict[str, int], z: typing.Set[str], w: Set[str]) -> None: pass", &[&["typing.List"][..], &["typing.Dict"][..], &["typing.Set"][..], &["Set"][..]])]
    #[case::a4_pep604_union("def f(x: list[int] | None, y: int | list[str] | set[int]) -> None: pass", &[&["list"][..], &["list", "set"][..]])]
    #[case::a5_qualified_pep604_union("def f(x: typing.List[int] | None) -> None: pass", &[&["typing.List"][..]])]
    #[case::a6_optional_and_union("def f(x: Optional[list[int]], y: Union[list[int], str, None]) -> None: pass", &[&["list"][..], &["list"][..]])]
    #[case::a7_qualified_optional_and_union("def f(x: typing.Optional[typing.Dict[str, int]], y: typing.Union[list[int], None]) -> None: pass", &[&["typing.Dict"][..], &["list"][..]])]
    #[case::a8_annotated_unwraps_first_arg("def f(x: Annotated[list[int], 'meta'], y: typing.Annotated[set[str], Doc('x')]) -> None: pass", &[&["list"][..], &["set"][..]])]
    #[case::a9_annotated_ignores_metadata("def f(x: Annotated[Sequence[int], list]) -> None: pass", &[&[][..]])]
    #[case::a10_qualifiers("def f(x: ClassVar[list[str]], y: Final[dict[str, int]], z: Required[set[str]]) -> None: pass", &[&["list"][..], &["dict"][..], &["set"][..]])]
    #[case::a11_covariant_single_and_tuple_containers("def f(a: Sequence[list[int]], b: Collection[set[str]], c: Awaitable[list[int]], d: tuple[str, dict[str, int]], e: tuple[list[int], ...]) -> None: pass", &[&["list"][..], &["set"][..], &["list"][..], &["dict"][..], &["list"][..]])]
    #[case::a12_mapping_covariant_value("def f(x: Mapping[str, list[int]]) -> None: pass", &[&["list"][..]])]
    #[case::a13_callable_covariant_return("def f(cb: Callable[[int], list[str]]) -> None: pass", &[&["list"][..]])]
    #[case::a14_callable_contravariant_param_ignored("def f(cb: Callable[[list[int]], None]) -> None: pass", &[&[][..]])]
    #[case::a15_invariant_mutable_outer_ignored("def f(x: MutableMapping[str, list[int]], y: MutableSequence[list[int]]) -> None: pass", &[&[][..], &[][..]])]
    #[case::a16_unknown_generic_ignored("def f(x: CustomBox[list[int]]) -> None: pass", &[&[][..]])]
    #[case::a17_abstract_collections_pass("def f(a: Sequence[int], b: Mapping[str, int], c: AbstractSet[str], d: collections.abc.Set[str]) -> None: pass", &[&[][..], &[][..], &[][..], &[][..]])]
    #[case::a17b_unaliased_collections_abc_set_import("from collections.abc import Set\ndef f(a: Set[str], b: typing.Set[str]) -> None: pass", &[&[][..], &["typing.Set"][..]])]
    #[case::a18_immutable_builtins_pass("def f(a: tuple[int, ...], b: frozenset[str], c: bytes, d: str) -> None: pass", &[&[][..], &[][..], &[][..], &[][..]])]
    #[case::a19_invariant_concrete_outer_not_recursed("def f(x: list[set[str]], y: dict[str, list[int]]) -> None: pass", &[&["list"][..], &["dict"][..]])]
    #[case::a20_typed_dict_qualifiers("def f(x: NotRequired[list[int]], y: ReadOnly[dict[str, int]]) -> None: pass", &[&["list"][..], &["dict"][..]])]
    #[case::a21_generator_yield_and_return_positions("def f(a: Generator[list[int], set[str], dict[str, int]], b: Coroutine[None, set[str], list[int]], c: AsyncGenerator[list[int], set[str]]) -> None: pass", &[&["list", "dict"][..], &["list"][..], &["list"][..]])]
    #[case::a22_iterator_and_abstract_set_covariant("def f(a: Iterable[list[int]], b: Iterator[dict[str, int]], c: AbstractSet[frozenset[int]]) -> None: pass", &[&["list"][..], &["dict"][..], &[][..]])]
    #[case::a23_qualified_covariant_outer("def f(x: collections.abc.Sequence[list[int]], y: typing.Mapping[str, set[str]]) -> None: pass", &[&["list"][..], &["set"][..]])]
    fn test_collect_concrete_collection_types_matrix_a(
        #[case] source: &str,
        #[case] expected_per_param: &[&[&str]],
    ) {
        let file = ParsedFile::new(source, SupportLang::Python);
        let abc_set_imported = has_unaliased_collections_abc_set_import(&file);
        let sigs = extract_function_signatures(&file);
        assert_eq!(sigs.len(), 1);
        let actual: Vec<Vec<String>> = sigs[0]
            .parameters
            .iter()
            .map(|parameter| {
                let type_node = parameter.type_node.as_ref().expect("param should be typed");
                collect_concrete_collection_types(type_node, abc_set_imported)
            })
            .collect();
        let expected: Vec<Vec<String>> = expected_per_param
            .iter()
            .map(|slice| {
                slice
                    .iter()
                    .map(|expected_name| (*expected_name).to_string())
                    .collect()
            })
            .collect();
        assert_eq!(actual, expected);
    }

    #[rstest::rstest]
    #[case::ellipsis_stub("def f(x: list[int]) -> None: ...", true)]
    #[case::pass_stub("def f(x: list[int]) -> None:\n    pass", true)]
    #[case::docstring_and_ellipsis(
        "def f(x: list[int]) -> None:\n    \"\"\"Doc.\"\"\"\n    ...",
        true
    )]
    #[case::raise_not_implemented_bare(
        "def f(x: list[int]) -> None:\n    raise NotImplementedError",
        true
    )]
    #[case::raise_not_implemented_call(
        "def f(x: list[int]) -> None:\n    raise NotImplementedError('todo')",
        true
    )]
    #[case::real_body("def f(x: list[int]) -> int:\n    return len(x)", false)]
    #[case::raise_from_not_implemented_cause(
        "def f(x: list[int]) -> None:\n    raise ValueError() from NotImplementedError",
        false
    )]
    fn test_is_stub_function_body(#[case] source: &str, #[case] expected: bool) {
        let file = ParsedFile::new(source, SupportLang::Python);
        let sigs = extract_function_signatures(&file);
        assert_eq!(is_stub_function_body(&sigs[0].node), expected);
    }

    #[rstest::rstest]
    #[case::protocol_class(
        "class P(Protocol):\n    def f(self, x: list[int]) -> None: pass",
        true
    )]
    #[case::generic_protocol_class(
        "class P(typing.Protocol[T]):\n    def f(self, x: list[int]) -> None: pass",
        true
    )]
    #[case::abc_class("class A(abc.ABC):\n    def f(self, x: list[int]) -> None: pass", true)]
    #[case::abc_meta_class(
        "class A(metaclass=abc.ABCMeta):\n    def f(self, x: list[int]) -> None: pass",
        true
    )]
    #[case::regular_class("class C:\n    def f(self, x: list[int]) -> None: pass", false)]
    fn test_is_in_protocol_or_abc_class(#[case] source: &str, #[case] expected: bool) {
        let file = ParsedFile::new(source, SupportLang::Python);
        let sigs = extract_function_signatures(&file);
        assert_eq!(is_in_protocol_or_abc_class(&sigs[0].node), expected);
    }

    /// Parses `source` and applies `collect` to the annotation of its first function's first parameter.
    fn collect_from_first_annotation(
        source: &str,
        collect: fn(&AstNode<'_>) -> Vec<String>,
    ) -> Vec<String> {
        let file = ParsedFile::new(source, SupportLang::Python);
        let sigs = extract_function_signatures(&file);
        let type_node = sigs[0].parameters[0]
            .type_node
            .as_ref()
            .expect("param should be typed");
        collect(type_node)
    }

    #[rstest::rstest]
    #[case::unqualified("def f(x: MutableSequence[int]): pass", &["MutableSequence"])]
    #[case::qualified("def f(x: collections.abc.MutableMapping[str, int]): pass", &["collections.abc.MutableMapping"])]
    #[case::optional_unwrapped("def f(x: typing.MutableSet[int] | None): pass", &["typing.MutableSet"])]
    #[case::nested_not_collected("def f(x: Sequence[MutableSequence[int]]): pass", &[])]
    #[case::unknown_module_ignored("def f(x: mylib.MutableSequence[int]): pass", &[])]
    fn test_collect_mutable_collection_types(#[case] source: &str, #[case] expected: &[&str]) {
        assert_eq!(
            collect_from_first_annotation(source, collect_mutable_collection_types),
            expected
        );
    }

    #[rstest::rstest]
    #[case::sequence("def f(x: Sequence[int]): pass", &["Sequence"])]
    #[case::qualified_collection("def f(x: typing.Collection[int]): pass", &["typing.Collection"])]
    #[case::optional_unwrapped("def f(x: Optional[collections.abc.Sequence[int]]): pass", &["collections.abc.Sequence"])]
    #[case::nested_not_collected("def f(x: Mapping[str, Sequence[int]]): pass", &[])]
    #[case::iterable_not_collected("def f(x: Iterable[int]): pass", &[])]
    fn test_collect_specific_collection_types(#[case] source: &str, #[case] expected: &[&str]) {
        assert_eq!(
            collect_from_first_annotation(source, collect_specific_collection_types),
            expected
        );
    }

    #[rstest::rstest]
    #[case::sequence_kinds(&["list", "typing.List", "MutableSequence"], "collections.abc.Sequence")]
    #[case::mapping_kinds(&["dict", "Dict", "collections.abc.MutableMapping"], "collections.abc.Mapping")]
    #[case::set_kinds(&["set", "typing.Set", "MutableSet"], "collections.abc.Set")]
    #[case::collections_mappings(&["defaultdict", "typing.DefaultDict", "collections.Counter", "OrderedDict"], "collections.abc.Mapping")]
    #[case::collections_deque(&["collections.deque", "Deque"], "collections.abc.Sequence")]
    #[case::mixed_in_order(&["dict", "list", "dict"], "collections.abc.Mapping, collections.abc.Sequence")]
    fn test_read_only_collection_replacements(#[case] type_paths: &[&str], #[case] expected: &str) {
        let type_paths: Vec<String> = type_paths.iter().map(|path| (*path).to_string()).collect();
        assert_eq!(read_only_collection_replacements(&type_paths), expected);
    }

    #[rstest::rstest]
    #[case::sequence_kinds(&["list", "typing.List", "collections.deque", "Deque", "MutableSequence"], "tuple")]
    #[case::mapping_kinds(
        &["dict", "Dict", "defaultdict", "typing.DefaultDict", "collections.Counter", "OrderedDict", "MutableMapping"],
        "frozendict"
    )]
    #[case::set_kinds(&["set", "typing.Set", "MutableSet"], "frozenset")]
    #[case::mixed_in_order(&["set", "list", "set"], "frozenset, tuple")]
    fn test_immutable_constant_collection_replacements(
        #[case] type_paths: &[&str],
        #[case] expected: &str,
    ) {
        let type_paths: Vec<String> = type_paths.iter().map(|path| (*path).to_string()).collect();
        assert_eq!(
            immutable_constant_collection_replacements(&type_paths),
            expected
        );
    }

    #[rstest::rstest]
    #[case::annotated_list(
        "ALLOWED: list[str] = (\"a\",)",
        &[("ALLOWED", &["list"][..], "list[str]")]
    )]
    #[case::bare_final_dict(
        "from typing import Final\nPORTS: Final = {\"http\": 80}",
        &[("PORTS", &["dict"][..], "{\"http\": 80}")]
    )]
    #[case::mapping_exempts_dict_literal(
        "from collections.abc import Mapping\nPORTS: Mapping[str, int] = {\"http\": 80}",
        &[]
    )]
    #[case::mapping_does_not_exempt_defaultdict(
        "from collections import defaultdict\nfrom collections.abc import Mapping\nCOUNTS: Mapping[str, int] = defaultdict(int)",
        &[("COUNTS", &["defaultdict"][..], "defaultdict(int)")]
    )]
    #[case::multiline_parenthesized_with_comment(
        "ALLOWED = (\n    # Default roles\n    [\"admin\"]\n)",
        &[("ALLOWED", &["list"][..], "[\"admin\"]")]
    )]
    #[case::annotated_wrapped_final_on_lowercase(
        "from typing import Annotated, Final\nallowed: Annotated[Final[list[str]], \"doc\"] = [\"a\"]",
        &[("allowed", &["list"][..], "Annotated[Final[list[str]], \"doc\"]")]
    )]
    fn test_collect_mutable_module_constants(
        #[case] source: &str,
        #[case] expected: &[(&str, &[&str], &str)],
    ) {
        let file = ParsedFile::new(source, SupportLang::Python);
        let collected: Vec<_> = collect_mutable_module_constants(&file)
            .into_iter()
            .map(|constant| {
                (
                    constant.name,
                    constant.matched_types,
                    constant.target_node.text().into_owned(),
                )
            })
            .collect();
        let expected: Vec<(String, Vec<String>, String)> = expected
            .iter()
            .map(|(name, types, span)| {
                (
                    (*name).to_string(),
                    types.iter().map(|item| (*item).to_string()).collect(),
                    (*span).to_string(),
                )
            })
            .collect();
        assert_eq!(collected, expected);
    }

    #[rstest::rstest]
    #[case::unused("def f(x):\n    return 1", ParameterCollectionCapability::Unused)]
    #[case::single_for(
        "def f(x):\n    for i in x:\n        print(i)",
        ParameterCollectionCapability::Iterable
    )]
    #[case::single_comprehension(
        "def f(x):\n    return [i for i in x]",
        ParameterCollectionCapability::Iterable
    )]
    #[case::single_consuming_builtin(
        "def f(x):\n    return sum(x)",
        ParameterCollectionCapability::Iterable
    )]
    #[case::len(
        "def f(x):\n    return len(x)",
        ParameterCollectionCapability::Collection
    )]
    #[case::membership(
        "def f(x):\n    return 1 in x",
        ParameterCollectionCapability::Collection
    )]
    #[case::truthiness(
        "def f(x):\n    if x:\n        return 1",
        ParameterCollectionCapability::Collection
    )]
    #[case::negation(
        "def f(x):\n    return not x",
        ParameterCollectionCapability::Collection
    )]
    #[case::two_passes(
        "def f(x):\n    return sum(x) + max(x)",
        ParameterCollectionCapability::Collection
    )]
    #[case::iteration_inside_loop(
        "def f(x):\n    for _ in range(3):\n        for i in x:\n            print(i)",
        ParameterCollectionCapability::Collection
    )]
    #[case::index("def f(x):\n    return x[0]", ParameterCollectionCapability::Sequence)]
    #[case::reversed(
        "def f(x):\n    return list(reversed(x))",
        ParameterCollectionCapability::Sequence
    )]
    #[case::index_method(
        "def f(x):\n    return x.index(1)",
        ParameterCollectionCapability::Sequence
    )]
    #[case::pattern_matching(
        "def f(x):\n    match x:\n        case [a]:\n            return a",
        ParameterCollectionCapability::Sequence
    )]
    #[case::passed_to_unknown_function(
        "def f(x):\n    return helper(x)",
        ParameterCollectionCapability::Sequence
    )]
    #[case::shadowing_nested_def_default(
        "def f(x):\n    def g(x=x[0]):\n        return x\n    return g",
        ParameterCollectionCapability::Sequence
    )]
    fn test_analyze_parameter_collection_capability(
        #[case] source: &str,
        #[case] expected: ParameterCollectionCapability,
    ) {
        let file = ParsedFile::new(source, SupportLang::Python);
        let sigs = extract_function_signatures(&file);
        assert_eq!(
            analyze_parameter_collection_capability(&sigs[0].node, "x"),
            expected
        );
    }

    #[rstest::rstest]
    #[case::method_mutation("def f(x):\n    x.append(1)", true)]
    #[case::subscript_write("def f(x):\n    x[0] = 1", true)]
    #[case::subscript_delete("def f(x):\n    del x[0]", true)]
    #[case::augmented_assignment("def f(x):\n    x += [1]", true)]
    #[case::alias("def f(x):\n    y = x\n    return len(y)", true)]
    #[case::stored_in_container("def f(x):\n    y = [x]\n    return len(y)", true)]
    #[case::returned("def f(x):\n    return x", true)]
    #[case::yielded("def f(x):\n    yield x", true)]
    #[case::passed_to_unknown_function("def f(x):\n    helper(x)", true)]
    #[case::passed_to_method("def f(x):\n    registry.register(x)", true)]
    #[case::mutated_in_closure("def f(x):\n    def g():\n        x.append(1)\n    g()", true)]
    #[case::shadowing_nested_def_default(
        "def f(x):\n    def g(x=x):\n        x.append(1)\n    g()",
        true
    )]
    #[case::read_only_iteration("def f(x):\n    for i in x:\n        print(i)", false)]
    #[case::read_only_len("def f(x):\n    return len(x)", false)]
    #[case::shadowed_by_nested_def(
        "def f(x):\n    def g(x):\n        x.append(1)\n    g([])",
        false
    )]
    #[case::shadowed_by_lambda(
        "def f(x):\n    g = lambda x: x.append(1)\n    return len(x)",
        false
    )]
    fn test_is_parameter_mutated_or_escaping(#[case] source: &str, #[case] expected: bool) {
        let file = ParsedFile::new(source, SupportLang::Python);
        let sigs = extract_function_signatures(&file);
        assert_eq!(
            is_parameter_mutated_or_escaping(&sigs[0].node, "x"),
            expected
        );
    }

    #[rstest::rstest]
    #[case::direct_call_mutation("make().append(1)", &["make"])]
    #[case::assigned_then_mutated("def use():\n    items = make()\n    items.append(1)", &["make"])]
    #[case::walrus_then_mutated("def use():\n    if (items := make()):\n        items.append(1)", &["make"])]
    #[case::subscript_write("def use():\n    items = make()\n    items[0] = 1", &["make"])]
    #[case::read_only_use("def use():\n    items = make()\n    return len(items)", &[])]
    #[case::binding_in_other_function("def a():\n    items = make()\ndef b():\n    items.append(1)", &[])]
    fn test_collect_locally_mutated_return_functions(
        #[case] usage: &str,
        #[case] expected: &[&str],
    ) {
        let source = format!("def make():\n    return []\n{usage}");
        let file = ParsedFile::new(&source, SupportLang::Python);
        let expected: HashSet<String> = expected.iter().map(|name| (*name).to_string()).collect();
        assert_eq!(collect_locally_mutated_return_functions(&file), expected);
    }

    #[rstest::rstest]
    #[case::class_level("class C:\n    items: list[int]", &[("items", false)])]
    #[case::private_skipped("class C:\n    _items: list[int]", &[])]
    #[case::init_attribute("class C:\n    def __init__(self):\n        self.items: list[int] = []", &[("items", false)])]
    #[case::self_mutation("class C:\n    items: list[int]\n    def add(self):\n        self.items.append(1)", &[("items", true)])]
    #[case::cls_mutation("class C:\n    items: list[int]\n    @classmethod\n    def add(cls):\n        cls.items.append(1)", &[("items", true)])]
    #[case::class_name_mutation("class C:\n    items: list[int]\n    def add(self):\n        C.items.append(1)", &[("items", true)])]
    #[case::subscript_delete("class C:\n    items: list[int]\n    def pop(self):\n        del self.items[0]", &[("items", true)])]
    #[case::protocol_skipped("class P(Protocol):\n    items: list[int]", &[])]
    fn test_collect_public_class_attributes(
        #[case] source: &str,
        #[case] expected: &[(&str, bool)],
    ) {
        let file = ParsedFile::new(source, SupportLang::Python);
        let actual: Vec<(String, bool)> = collect_public_class_attributes(&file)
            .into_iter()
            .map(|attribute| (attribute.name, attribute.is_mutated_in_class))
            .collect();
        let expected: Vec<(String, bool)> = expected
            .iter()
            .map(|(name, mutated)| ((*name).to_string(), *mutated))
            .collect();
        assert_eq!(actual, expected);
    }

    #[rstest::rstest]
    #[case::module_upper_name_defines("MAX = 30", &[("30", ConstantDefinition)])]
    #[case::class_final_lowercase_defines("class C:\n    limit: Final[int] = -5", &[("-5", ConstantDefinition)])]
    #[case::aliased_final_defines("timeout: t.Final = 30", &[("30", ConstantDefinition)])]
    #[case::class_lowercase_is_inline("class C:\n    name = 'cc'", &[("'cc'", Inline)])]
    #[case::function_level_upper_name_is_inline("def f():\n    MAX = 30", &[("30", Inline)])]
    #[case::module_if_body_defines("if WIN:\n    RETRIES = 3\nelse:\n    RETRIES = 5", &[("3", ConstantDefinition), ("5", ConstantDefinition)])]
    #[case::module_except_body_defines("try:\n    import x\nexcept ImportError:\n    LIMIT = 9", &[("9", ConstantDefinition)])]
    #[case::function_if_body_is_inline("def f():\n    if a:\n        MAX = 30", &[("30", Inline)])]
    #[case::parenthesized_constant_defines("MSG = (\n    'refused'\n)", &[("'refused'", ConstantDefinition)])]
    #[case::composite_constant_not_collected("URLS = ['u1', 'u2']\nPAIR = 'aa' 'bb'", &[])]
    #[case::negation_anchored_on_operator("f(-42, 7 - 42)", &[("-42", Inline), ("7", Inline), ("42", Inline)])]
    #[case::negative_case_pattern("match m:\n    case [-42, 'xx']:\n        pass", &[("-42", Inline), ("'xx'", Inline)])]
    #[case::signed_numbers_in_patterns_skipped("match m:\n    case {-404: _} | Resp(code=-404) | -7:\n        pass", &[])]
    #[case::docstring_skipped("def f():\n    '''Doc.'''\n    return 'rv'", &[("'rv'", Inline)])]
    #[case::annotations_skipped("def f(a: 'T' = 'dv') -> 'R':\n    v: 'V' = 'vv'", &[("'dv'", Inline), ("'vv'", Inline)])]
    #[case::literal_type_skipped("v = Literal['y']", &[])]
    #[case::interpolated_fstring_walked("print(f\"{row['st']} and\", f'plain')", &[("'st'", Inline), ("f'plain'", Inline)])]
    #[case::imaginary_skipped("z = 2j", &[])]
    fn test_collect_literal_occurrences_python(
        #[case] source: &str,
        #[case] expected: &[(&str, LiteralRole)],
    ) {
        let file = ParsedFile::new(source, SupportLang::Python);
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

    #[rstest::rstest]
    fn test_collect_literal_occurrences_python_skips_type_name_argument(
        #[values(
            "TypeVar",
            "NewType",
            "ParamSpec",
            "TypeVarTuple",
            "NamedTuple",
            "TypedDict",
            "typing.cast"
        )]
        callee: &str,
    ) {
        let file = ParsedFile::new(&format!("t = {callee}('Nm', 'vv')"), SupportLang::Python);
        let texts: Vec<String> = collect_literal_occurrences(&file)
            .into_iter()
            .map(|occurrence| occurrence.node.text().into_owned())
            .collect();
        assert_eq!(texts, vec!["'vv'"]);
    }

    #[rstest::rstest]
    #[case::quote_style_ignored("'ab'", LiteralValue::Str("ab".to_string()))]
    #[case::triple_quoted("'''ab'''", LiteralValue::Str("ab".to_string()))]
    #[case::unicode_prefix("u'ab'", LiteralValue::Str("ab".to_string()))]
    #[case::bytes("b\"ab\"", LiteralValue::Bytes("ab".to_string()))]
    #[case::raw_bytes("Rb'ab'", LiteralValue::Bytes("ab".to_string()))]
    #[case::escapes_kept("'a\\nb'", LiteralValue::Str("a\\nb".to_string()))]
    #[case::raw_backslash_spelled_plain("r'a\\nb'", LiteralValue::Str("a\\\\nb".to_string()))]
    #[case::hex("0x1F", LiteralValue::Int(31))]
    #[case::separators("1_000", LiteralValue::Int(1000))]
    #[case::exponent("1e3", LiteralValue::Float(1000.0_f64.to_bits()))]
    #[case::leading_dot(".5", LiteralValue::Float(0.5_f64.to_bits()))]
    #[case::trailing_dot("3.", LiteralValue::Float(3.0_f64.to_bits()))]
    #[case::negative_float("-2.5", LiteralValue::Float((-2.5_f64).to_bits()))]
    #[case::negative_zero("-0.0", LiteralValue::Float(0.0_f64.to_bits()))]
    fn test_collect_literal_occurrences_python_values(
        #[case] literal: &str,
        #[case] expected: LiteralValue,
    ) {
        let file = ParsedFile::new(&format!("value = {literal}"), SupportLang::Python);
        let values: Vec<LiteralValue> = collect_literal_occurrences(&file)
            .into_iter()
            .map(|occurrence| occurrence.value)
            .collect();
        assert_eq!(values, vec![expected]);
    }

    #[test]
    fn test_extract_classes_ignores_comments_and_keywords_in_superclasses() {
        let source = indoc::indoc! {r"
            class FakeClient(
                # Not a base class
                metaclass=ABCMeta,
            ):
                pass
        "};
        let file = ParsedFile::new(source, SupportLang::Python);
        let classes = extract_classes(&file);
        assert_eq!(classes.len(), 1);
        assert!(classes[0].bases.is_empty());
    }

    #[test]
    fn test_python_base_class_is_structural_marker() {
        let source = indoc::indoc! {r"
            class C(
                HttpClient,
                object,
                Generic[T],
                Protocol,
                typing_extensions.Generic[T],
                typing_extensions.Protocol,
                ABC,
                abc.ABC,
                Repository[T],
                **kwargs,
            ):
                pass
        "};
        let file = ParsedFile::new(source, SupportLang::Python);
        let classes = extract_classes(&file);
        let summary: Vec<(&str, bool)> = classes[0]
            .bases
            .iter()
            .map(|base| (base.name.as_str(), base.is_structural_marker()))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("HttpClient", false),
                ("object", true),
                ("Generic[T]", true),
                ("Protocol", true),
                ("typing_extensions.Generic[T]", true),
                ("typing_extensions.Protocol", true),
                ("ABC", true),
                ("abc.ABC", true),
                ("Repository[T]", false),
            ]
        );
    }

    #[rstest::rstest]
    #[case::init_and_method(
        "class C:\n    def __init__(self):\n        self.x: int = 1\n    async def reset(self):\n        if True:\n            self.y: str",
        &[("C", "__init__", "x", "int"), ("C", "reset", "y", "str")]
    )]
    #[case::private_collected_unannotated_skipped(
        "class C:\n    def __init__(self):\n        self._p: int = 1\n        self.pub = 2",
        &[("C", "__init__", "_p", "int")]
    )]
    #[case::staticmethod_classmethod_and_nested_func_skipped(
        "class C:\n    @staticmethod\n    def sm(self):\n        self.a: int = 1\n    @classmethod\n    def cm(cls):\n        cls.b: int = 2\n    def run(self):\n        def inner(self):\n            self.c: int = 3",
        &[]
    )]
    fn test_collect_instance_attribute_annotations(
        #[case] source: &str,
        #[case] expected: &[(&str, &str, &str, &str)],
    ) {
        let file = ParsedFile::new(source, SupportLang::Python);
        let actual: Vec<(String, String, String, String)> =
            collect_instance_attribute_annotations(&file)
                .into_iter()
                .map(|attribute| {
                    (
                        attribute.class_name,
                        attribute.method_name,
                        attribute.name,
                        attribute.annotation.text().into_owned(),
                    )
                })
                .collect();
        let expected: Vec<(String, String, String, String)> = expected
            .iter()
            .map(|(class_name, method_name, name, annotation)| {
                (
                    (*class_name).to_string(),
                    (*method_name).to_string(),
                    (*name).to_string(),
                    (*annotation).to_string(),
                )
            })
            .collect();
        assert_eq!(actual, expected);
    }

    #[rstest::rstest]
    #[case::bare_final(
        "class C:\n    def __init__(self):\n        self.a: Final = 1",
        (true, false)
    )]
    #[case::qualified_bare_final(
        "class C:\n    def __init__(self):\n        self.a: typing.Final = 1",
        (true, false)
    )]
    #[case::annotated_bare_final(
        "class C:\n    def __init__(self):\n        self.a: Annotated[Final, 'm'] = 1",
        (true, false)
    )]
    #[case::parameterized_final(
        "class C:\n    def __init__(self):\n        self.a: Final[int] = 1",
        (false, false)
    )]
    #[case::dataclass(
        "@dataclass\nclass C:\n    def __post_init__(self):\n        self.a: int = 1",
        (false, true)
    )]
    #[case::pydantic_model(
        "class C(BaseModel):\n    def model_post_init(self, context):\n        self.a: int = 1",
        (false, true)
    )]
    fn test_instance_attribute_annotation_facts(
        #[case] source: &str,
        #[case] expected: (bool, bool),
    ) {
        let file = ParsedFile::new(source, SupportLang::Python);
        let actual: Vec<(bool, bool)> = collect_instance_attribute_annotations(&file)
            .iter()
            .map(|attribute| {
                (
                    attribute.is_bare_final(),
                    attribute.is_in_field_synthesizing_class,
                )
            })
            .collect();
        assert_eq!(actual, [expected]);
    }

    #[rstest::rstest]
    #[case::pep604_sequence("def f() -> Sequence[int] | None: pass", &["Sequence"])]
    #[case::optional_list("def f() -> Optional[list[int]]: pass", &["list"])]
    #[case::union_mapping("def f() -> typing.Union[typing.Mapping[str, int], None]: pass", &["typing.Mapping"])]
    #[case::deduplicated_branches("def f() -> list[int] | list[str] | None: pass", &["list"])]
    #[case::multiple_collection_branches("def f() -> list[int] | set[str] | None: pass", &["list", "set"])]
    #[case::variadic_tuple("def f() -> tuple[int, ...] | None: pass", &["tuple"])]
    #[case::bare_tuple("def f() -> tuple | None: pass", &["tuple"])]
    #[case::fixed_pair_tuple_ignored("def f() -> tuple[int, str] | None: pass", &[])]
    #[case::single_element_tuple_ignored("def f() -> tuple[int] | None: pass", &[])]
    #[case::mixed_scalar_and_collection_ignored("def f() -> str | Sequence[str] | None: pass", &[])]
    #[case::inner_nullable_element_ignored("def f() -> Sequence[int | None]: pass", &[])]
    #[case::annotated_branch_unwrapped("def f() -> Annotated[Sequence[int], 'meta'] | None: pass", &["Sequence"])]
    #[case::awaitable_coroutine_envelopes_unwrapped(
        "def f() -> Awaitable[Coroutine[Any, Any, MutableMapping[str, int] | None]]: pass",
        &["MutableMapping"]
    )]
    fn test_collect_nullable_collection_return_types(
        #[case] source: &str,
        #[case] expected: &[&str],
    ) {
        let file = ParsedFile::new(source, SupportLang::Python);
        let signatures = extract_function_signatures(&file);
        let return_type_node = signatures[0]
            .return_type_node
            .as_ref()
            .expect("function should have return annotation");
        assert_eq!(
            collect_nullable_collection_return_types(return_type_node),
            expected
        );
    }

    #[rstest::rstest]
    #[case::simple("Order {order_id} filled", Some(&["order_id"][..]))]
    #[case::compound_attribute("Order {order.id} filled", Some(&["order"][..]))]
    #[case::compound_subscript("Order {order[id]} filled", Some(&["order"][..]))]
    #[case::conversion_and_spec("Order {order_id!r} {amount:.2f}", Some(&["order_id", "amount"][..]))]
    #[case::positional_empty_and_numbered("Order {} and {0} and {0.id} and {1[key]}", Some(&[][..]))]
    #[case::escaped_double_braces("Literal {{order_id}} value {}", Some(&[][..]))]
    #[case::triple_braces_captures_inner("Literal {{{order_id}}}", Some(&["order_id"][..]))]
    #[case::nested_format_spec("Value {:>{width}}", Some(&["width"][..]))]
    #[case::non_identifier_braces("Payload {\"order_id\": 1} and {a, b}", Some(&[][..]))]
    #[case::unclosed_opening_brace("Malformed {order_id in input", None)]
    #[case::unmatched_closing_brace("Malformed {order_id} stray }", None)]
    fn test_named_format_field_roots(#[case] message: &str, #[case] expected: Option<&[&str]>) {
        let expected = expected.map(|roots| roots.iter().map(|root| (*root).to_owned()).collect());
        assert_eq!(named_format_field_roots(message), expected);
    }

    #[test]
    fn test_collect_logger_calls_reads_callee_message_and_arguments() {
        let source = indoc::indoc! {r#"
            logger.info("Order {order_id} filled", order_id)
            self.log.error("Peer " "{peer.id} failed", **extra)
            logging.log(20, f"User {user_id}", user_id=1)
            print("not a logger", value)
        "#};
        let file = ParsedFile::new(source, SupportLang::Python);
        let calls: Vec<_> = collect_logger_calls(&file)
            .into_iter()
            .map(|call| {
                let mut keyword_names: Vec<String> = call.keyword_names.into_iter().collect();
                keyword_names.sort();
                (
                    call.callee,
                    call.message,
                    call.has_trailing_positional_args,
                    call.has_keyword_splat,
                    keyword_names,
                )
            })
            .collect();
        assert_eq!(
            calls,
            vec![
                (
                    "logger.info".to_owned(),
                    Some("Order {order_id} filled".to_owned()),
                    true,
                    false,
                    vec![],
                ),
                (
                    "self.log.error".to_owned(),
                    Some("Peer {peer.id} failed".to_owned()),
                    false,
                    true,
                    vec![],
                ),
                (
                    "logging.log".to_owned(),
                    None,
                    false,
                    false,
                    vec!["user_id".to_owned()]
                ),
            ]
        );
    }

    #[test]
    fn test_collect_function_scopes_sibling_calls() {
        let source = indoc::indoc! {r"
            def orchestrate(x: int) -> int:
                first = step_one(x)
                second = step_two(first)
                return second + is_even(second)

            def step_one(x: int) -> int:
                return x + 1

            def step_two(x: int) -> int:
                return x * 2

            def is_even(n: int) -> bool:
                return True if n == 0 else is_odd(n - 1)

            def is_odd(n: int) -> bool:
                return False if n == 0 else is_even(n - 1)

            class Greeter:
                def greet(self) -> str:
                    return self.name() + format_name(self)

                def name(self) -> str:
                    return 'x'
        "};
        let file = ParsedFile::new(source, SupportLang::Python);
        let scopes = collect_function_scopes(&file);
        let summary: Vec<(bool, &str, usize, Vec<String>)> = scopes
            .iter()
            .flat_map(|scope| {
                scope.functions.iter().map(|function| {
                    (
                        scope.is_class,
                        function.name.as_str(),
                        function.definition_order,
                        function
                            .sibling_calls
                            .iter()
                            .map(|call| format!("{} at {}", call.callee_name, call.node.text()))
                            .collect(),
                    )
                })
            })
            .collect();
        let calls = |texts: &[&str]| -> Vec<String> {
            texts.iter().map(|text| (*text).to_string()).collect()
        };
        assert_eq!(
            summary,
            vec![
                (
                    false,
                    "orchestrate",
                    0,
                    calls(&[
                        "step_one at step_one(x)",
                        "step_two at step_two(first)",
                        "is_even at is_even(second)",
                    ])
                ),
                (false, "step_one", 1, calls(&[])),
                (false, "step_two", 2, calls(&[])),
                (false, "is_even", 3, calls(&["is_odd at is_odd(n - 1)"])),
                (false, "is_odd", 4, calls(&["is_even at is_even(n - 1)"])),
                (true, "greet", 0, calls(&["name at self.name()"])),
                (true, "name", 1, calls(&[])),
            ]
        );
    }

    /// `(style, preceding_text, literals, [(field, conversion, format_spec)])`.
    type FormatStringSummary = (
        PythonFormatStyle,
        String,
        Vec<String>,
        Vec<(String, Option<String>, Option<String>)>,
    );

    fn summarize_format_strings(python_code: &str) -> Vec<FormatStringSummary> {
        let file = ParsedFile::new(python_code, SupportLang::Python);
        collect_format_strings(&file)
            .into_iter()
            .map(|format_string| {
                let placeholders = format_string
                    .placeholders
                    .into_iter()
                    .map(|placeholder| {
                        (
                            placeholder.field,
                            placeholder.conversion,
                            placeholder.format_spec,
                        )
                    })
                    .collect();
                (
                    format_string.style,
                    format_string.preceding_text,
                    format_string.literals,
                    placeholders,
                )
            })
            .collect()
    }

    fn owned(texts: &[&str]) -> Vec<String> {
        texts.iter().map(|text| (*text).to_owned()).collect()
    }

    fn field(
        name: &str,
        conversion: Option<&str>,
        format_spec: Option<&str>,
    ) -> (String, Option<String>, Option<String>) {
        (
            name.to_owned(),
            conversion.map(str::to_owned),
            format_spec.map(str::to_owned),
        )
    }

    #[test]
    fn test_collect_format_strings_splits_fstrings() {
        assert_eq!(
            summarize_format_strings(r#"message = f"Copied '{ source !r:>8}' {count=}""#),
            vec![(
                PythonFormatStyle::FString,
                String::new(),
                owned(&["Copied '", "' ", ""]),
                vec![
                    field("source", Some("r"), Some(">8")),
                    field("count", None, None)
                ],
            )]
        );
    }

    #[test]
    fn test_collect_format_strings_splits_str_format_templates() {
        assert_eq!(
            summarize_format_strings(
                r#"message = "Copied '{}' to '{target.name!s}' {{raw}}".format(a, target=b)"#
            ),
            vec![(
                PythonFormatStyle::StrFormat,
                String::new(),
                owned(&["Copied '", "' to '", "' {{raw}}"]),
                vec![field("", None, None), field("target.name", Some("s"), None)],
            )]
        );
    }

    #[rstest::rstest]
    #[case::percent_operator(r#"message = "Got %s, %(key)5.2f and 100%%" % values"#)]
    #[case::logger_with_arguments(r#"logger.info("Got %s, %(key)5.2f and 100%%", values)"#)]
    fn test_collect_format_strings_splits_printf_templates(#[case] python_code: &str) {
        assert_eq!(
            summarize_format_strings(python_code),
            vec![(
                PythonFormatStyle::Printf,
                String::new(),
                owned(&["Got ", ", ", " and 100%%"]),
                vec![
                    field("", Some("s"), None),
                    field("key", Some("f"), Some("5.2"))
                ],
            )]
        );
    }

    #[test]
    fn test_collect_format_strings_reads_concatenated_prefix() {
        let summaries = summarize_format_strings("message = (\"Load \" f\"'{name}'\")");
        let (style, preceding_text, ..) = &summaries[0];
        assert_eq!(
            (summaries.len(), *style, preceding_text.as_str()),
            (1, PythonFormatStyle::FString, "Load ")
        );
    }

    #[test]
    fn test_collect_format_strings_skips_unformatted_raw_and_byte_strings() {
        let source = indoc::indoc! {r#"
            plain = "Invalid '{name}' or '%s'"
            raw = r"Invalid '{name}'".format(name=value)
            byte = b"Invalid '%s'" % value
            no_arguments = logger.info("Literal '%s'")
        "#};
        assert_eq!(summarize_format_strings(source), vec![]);
    }
}
