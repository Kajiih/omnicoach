//! Python type annotation unwrapping, constructor resolution, union flattening, and collection type vocabularies.

// omni:disable-file [repeated-literal] -- Tree-sitter node kinds and field names (see ROADMAP)

use super::{AstNode, ParsedFile, RawNode, resolve_path_and_terminal_raw};

/// Controls how deeply [`collect_type_constructors`] traverses a Python type annotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AnnotationTraversalDepth {
    /// Unwraps only transparent wrappers (`|`, `Optional`, `Union`, `Annotated[T, ...]`,
    /// `ClassVar[T]`, `Final[T]`, `Required[T]`, `NotRequired[T]`, `ReadOnly[T]`).
    TransparentWrappersOnly,
    /// Unwraps transparent wrappers and recurses into covariant type parameter positions of
    /// read-only containers (`Sequence[T]`, `Mapping[K, V]` value `V`, `tuple[...]`,
    /// `Awaitable[T]`, `Callable[[...], Ret]` return `Ret`, `Generator`/`Coroutine` yield and
    /// return). Invariant containers (`list`, `dict`, `set`, `Mutable*`) are not entered.
    CovariantPositions,
}

/// Unwraps outer `"type"` and `"parenthesized_expression"` nodes from `node`.
pub(super) fn unwrap_type_and_parens<'a>(node: &RawNode<'a>) -> Option<RawNode<'a>> {
    let mut current = node.clone();
    while matches!(current.kind().as_ref(), "type" | "parenthesized_expression") {
        let inner = current
            .children()
            .find(|child| child.is_named() && !child.is_extra())?;
        current = inner;
    }
    Some(current)
}

/// Returns true if `(path, terminal)` refers to an unqualified or standard-library (`typing`,
/// `typing_extensions`, `collections.abc`, `builtins`) type constructor.
fn is_std_type_constructor_prefix(path: &str, terminal: &str) -> bool {
    path == terminal
        || path.strip_suffix(terminal).is_some_and(|prefix| {
            matches!(
                prefix,
                "typing." | "typing_extensions." | "collections.abc." | "builtins."
            )
        })
}

/// Returns true if `(path, terminal)` matches one of `targets` in the standard typing namespaces.
pub(super) fn is_std_type_constructor(path: &str, terminal: &str, targets: &[&str]) -> bool {
    targets.contains(&terminal) && is_std_type_constructor_prefix(path, terminal)
}

/// Returns true if `(path, terminal)` is a concrete mutable collection constructor (`list`, `dict`,
/// `set`, `List`, `Dict`, `Set`, `typing.List`, `typing.Dict`, `typing.Set`, etc.), or a
/// `collections` container (`defaultdict`, `deque`, `Counter`, `OrderedDict`) or its `typing`
/// alias (`DefaultDict`, `Deque`).
///
/// Qualified `collections.abc.Set` is excluded because it is the abstract set ABC, whereas
/// unqualified `Set` and `typing.Set` are flagged as concrete (per Ruff `PYI025`, `collections.abc.Set`
/// should be imported `as AbstractSet`).
pub(super) fn is_concrete_collection_constructor(path: &str, terminal: &str) -> bool {
    match terminal {
        "list" | "List" | "dict" | "Dict" | "set" => is_std_type_constructor_prefix(path, terminal),
        "Set" => matches!(path, "Set" | "typing.Set" | "typing_extensions.Set"),
        "defaultdict" | "DefaultDict" | "deque" | "Deque" | "Counter" | "OrderedDict" => {
            path == terminal
                || path.strip_suffix(terminal).is_some_and(|prefix| {
                    matches!(prefix, "collections." | "typing." | "typing_extensions.")
                })
        }
        _ => false,
    }
}

/// Returns true if `file` contains an unaliased `from collections.abc import Set` statement.
#[must_use]
pub fn has_unaliased_collections_abc_set_import(file: &ParsedFile) -> bool {
    file.grep.root().dfs().any(|node| {
        node.kind() == "import_from_statement"
            && node
                .field("module_name")
                .is_some_and(|module_node| module_node.text() == "collections.abc")
            && node
                .field_children("name")
                .any(|imported| imported.kind() == "dotted_name" && imported.text() == "Set")
    })
}

/// Extracts `(base_node, type_argument_nodes)` from a Python `generic_type` or expression-fallback
/// `subscript` node inside a type annotation.
pub(super) fn extract_generic_base_and_args<'a>(
    node: &RawNode<'a>,
) -> Option<(RawNode<'a>, Vec<RawNode<'a>>)> {
    match node.kind().as_ref() {
        "generic_type" => {
            let mut base_node = None;
            let mut type_args = Vec::new();
            for child in node.children() {
                if !child.is_named() || child.is_extra() {
                    continue;
                }
                if child.kind() == "type_parameter" {
                    for param_child in child.children() {
                        if param_child.is_named() && !param_child.is_extra() {
                            type_args.push(param_child);
                        }
                    }
                } else if base_node.is_none() {
                    base_node = Some(child);
                }
            }
            Some((base_node?, type_args))
        }
        "subscript" => {
            let base_node = node.field("value")?;
            let raw_args: Vec<_> = node
                .field_children("subscript")
                .filter(|child| child.is_named() && !child.is_extra())
                .collect();
            let type_args = if raw_args.len() == 1 && raw_args[0].kind() == "tuple" {
                raw_args[0]
                    .children()
                    .filter(|child| child.is_named() && !child.is_extra())
                    .collect()
            } else {
                raw_args
            };
            Some((base_node, type_args))
        }
        _ => None,
    }
}

/// Unwraps outer `Annotated[T, ...]`, `type`, and `parenthesized_expression` wrappers from `type_node`.
fn unwrap_annotated_and_parens<'a>(type_node: &RawNode<'a>) -> Option<RawNode<'a>> {
    let mut current = unwrap_type_and_parens(type_node)?;
    while matches!(current.kind().as_ref(), "generic_type" | "subscript") {
        let Some((base_node, type_args)) = extract_generic_base_and_args(&current) else {
            break;
        };
        let (base_path, base_terminal) = resolve_path_and_terminal_raw(&base_node);
        if is_std_type_constructor(&base_path, &base_terminal, &["Annotated"])
            && let Some(first_arg) = type_args.into_iter().next()
        {
            current = unwrap_type_and_parens(&first_arg)?;
        } else {
            break;
        }
    }
    Some(current)
}

/// True if `type_node` is `Final` or `Final[T]` (qualified or not, optionally wrapped in `Annotated`).
pub(super) fn has_final_annotation(type_node: &RawNode<'_>) -> bool {
    let Some(unwrapped) = unwrap_annotated_and_parens(type_node) else {
        return false;
    };
    let candidate = if matches!(unwrapped.kind().as_ref(), "generic_type" | "subscript") {
        let Some((base_node, _)) = extract_generic_base_and_args(&unwrapped) else {
            return false;
        };
        base_node
    } else {
        unwrapped
    };
    resolve_path_and_terminal_raw(&candidate).1 == "Final"
}

/// Returns true if `type_node` is an unparameterized `Final` qualifier (`Final`, `typing.Final`,
/// or `typing_extensions.Final`, optionally wrapped in `Annotated[Final, ...]`), which PEP 591
/// forbids in a class body without an initializer (`x: Final` is invalid; `x: Final[T]` is valid).
pub(super) fn is_bare_final_annotation(type_node: &RawNode<'_>) -> bool {
    let Some(unwrapped) = unwrap_annotated_and_parens(type_node) else {
        return false;
    };
    if matches!(unwrapped.kind().as_ref(), "generic_type" | "subscript") {
        return false;
    }
    let (path, terminal) = resolve_path_and_terminal_raw(&unwrapped);
    is_std_type_constructor(&path, &terminal, &["Final"])
}

/// Transparent type wrappers whose all type arguments preserve the enclosing variance.
const TRANSPARENT_UNION_WRAPPERS: &[&str] = &["Optional", "Union"];

/// Transparent type qualifiers whose first type argument (`arg 0`) preserves the enclosing variance.
const TRANSPARENT_FIRST_ARG_WRAPPERS: &[&str] = &[
    "Annotated",
    "ClassVar",
    "Final",
    "Required",
    "NotRequired",
    "ReadOnly",
];

/// Single-parameter generic containers that are covariant in their element type (`arg 0`).
const SINGLE_ARG_COVARIANT_CONTAINERS: &[&str] = &[
    "Sequence",
    "Collection",
    "Iterable",
    "Iterator",
    "Reversible",
    "Container",
    "AsyncIterable",
    "AsyncIterator",
    "Awaitable",
    "AbstractSet",
    "Set",
    "frozenset",
    "FrozenSet",
];

/// Abstract mutable collection constructors in `collections.abc` and `typing`.
pub(super) const MUTABLE_COLLECTION_ABCS: &[&str] =
    &["MutableSequence", "MutableMapping", "MutableSet"];

/// Read-only abstract and immutable collection constructors in `collections.abc`, `typing`, and
/// `builtins` that have a natural empty value (`()`, `{}`, `frozenset()`, `iter(())`).
const READONLY_AND_IMMUTABLE_COLLECTION_CONSTRUCTORS: &[&str] = &[
    "Sequence",
    "Mapping",
    "Set",
    "AbstractSet",
    "Collection",
    "Iterable",
    "Iterator",
    "Reversible",
    "frozenset",
    "FrozenSet",
];

/// Recursively collects matching type constructor paths from `node` according to `depth`.
pub(super) fn collect_type_constructors_raw<F>(
    node: &RawNode<'_>,
    depth: AnnotationTraversalDepth,
    predicate: &F,
    out: &mut Vec<String>,
) where
    F: Fn(&str, &str) -> bool,
{
    match node.kind().as_ref() {
        "type" | "parenthesized_expression" | "union_type" => {
            for child in node.children() {
                if child.is_named() && !child.is_extra() {
                    collect_type_constructors_raw(&child, depth, predicate, out);
                }
            }
        }
        "binary_operator" => {
            if node.field("operator").is_some_and(|op| op.text() == "|") {
                if let Some(left) = node.field("left") {
                    collect_type_constructors_raw(&left, depth, predicate, out);
                }
                if let Some(right) = node.field("right") {
                    collect_type_constructors_raw(&right, depth, predicate, out);
                }
            }
        }
        "identifier" | "attribute" => {
            let (path, terminal) = resolve_path_and_terminal_raw(node);
            if predicate(&path, &terminal) && !out.contains(&path) {
                out.push(path);
            }
        }
        "generic_type" | "subscript" => {
            let Some((base_node, type_args)) = extract_generic_base_and_args(node) else {
                return;
            };
            let (base_path, base_terminal) = resolve_path_and_terminal_raw(&base_node);

            if is_std_type_constructor(&base_path, &base_terminal, TRANSPARENT_UNION_WRAPPERS) {
                for arg in &type_args {
                    collect_type_constructors_raw(arg, depth, predicate, out);
                }
                return;
            }

            if is_std_type_constructor(&base_path, &base_terminal, TRANSPARENT_FIRST_ARG_WRAPPERS) {
                if let Some(first_arg) = type_args.first() {
                    collect_type_constructors_raw(first_arg, depth, predicate, out);
                }
                return;
            }

            if predicate(&base_path, &base_terminal) && !out.contains(&base_path) {
                out.push(base_path.clone());
            }

            if depth == AnnotationTraversalDepth::CovariantPositions
                && is_std_type_constructor_prefix(&base_path, &base_terminal)
            {
                match base_terminal.as_str() {
                    terminal_name if SINGLE_ARG_COVARIANT_CONTAINERS.contains(&terminal_name) => {
                        if let Some(first_arg) = type_args.first() {
                            collect_type_constructors_raw(first_arg, depth, predicate, out);
                        }
                    }
                    "tuple" | "Tuple" => {
                        for arg in &type_args {
                            collect_type_constructors_raw(arg, depth, predicate, out);
                        }
                    }
                    "Mapping" | "Callable" => {
                        if let Some(second_arg) = type_args.get(1) {
                            collect_type_constructors_raw(second_arg, depth, predicate, out);
                        }
                    }
                    "Generator" | "Coroutine" => {
                        if let Some(yield_arg) = type_args.first() {
                            collect_type_constructors_raw(yield_arg, depth, predicate, out);
                        }
                        if let Some(return_arg) = type_args.get(2) {
                            collect_type_constructors_raw(return_arg, depth, predicate, out);
                        }
                    }
                    "AsyncGenerator" => {
                        if let Some(yield_arg) = type_args.first() {
                            collect_type_constructors_raw(yield_arg, depth, predicate, out);
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

/// Collects matching type constructor strings (in source order, deduplicated) from a Python
/// type annotation node according to `depth` and `predicate(full_path, terminal_name)`.
fn collect_type_constructors<F>(
    type_node: &AstNode<'_>,
    depth: AnnotationTraversalDepth,
    predicate: F,
) -> Vec<String>
where
    F: Fn(&str, &str) -> bool,
{
    let mut out = Vec::new();
    collect_type_constructors_raw(&type_node.raw, depth, &predicate, &mut out);
    out
}

/// Collects concrete mutable collection constructors (`list`, `dict`, `set`, `Set`, etc.) from `type_node`.
///
/// If `abc_set_imported` is true (`from collections.abc import Set` is present in the file),
/// unqualified `Set` is treated as the abstract `collections.abc.Set` rather than concrete `typing.Set`.
#[must_use]
pub fn collect_concrete_collection_types(
    type_node: &AstNode<'_>,
    abc_set_imported: bool,
) -> Vec<String> {
    collect_type_constructors(
        type_node,
        AnnotationTraversalDepth::CovariantPositions,
        |full_path, terminal| {
            if abc_set_imported && full_path == "Set" {
                return false;
            }
            is_concrete_collection_constructor(full_path, terminal)
        },
    )
}

/// Collects abstract mutable collection constructors (`MutableSequence`, `MutableMapping`,
/// `MutableSet`) from `type_node`, unwrapping only transparent wrappers.
#[must_use]
pub fn collect_mutable_collection_types(type_node: &AstNode<'_>) -> Vec<String> {
    collect_type_constructors(
        type_node,
        AnnotationTraversalDepth::TransparentWrappersOnly,
        |full_path, terminal| is_std_type_constructor(full_path, terminal, MUTABLE_COLLECTION_ABCS),
    )
}

/// Collects specific read-only abstract collection constructors (`Sequence`, `Collection`)
/// from `type_node`, unwrapping only transparent wrappers.
#[must_use]
pub fn collect_specific_collection_types(type_node: &AstNode<'_>) -> Vec<String> {
    collect_type_constructors(
        type_node,
        AnnotationTraversalDepth::TransparentWrappersOnly,
        |full_path, terminal| {
            is_std_type_constructor(full_path, terminal, &["Sequence", "Collection"])
        },
    )
}

/// Formats deduplicated collection replacements for `type_paths`, mapping dictionary-like types
/// to `mapping`, set-like types to `set`, and sequence-like types to `sequence`.
fn format_collection_replacements(
    type_paths: &[String],
    mapping: &'static str,
    set: &'static str,
    sequence: &'static str,
) -> String {
    let mut replacements: Vec<&str> = Vec::new();
    for type_path in type_paths {
        let terminal = type_path.rsplit('.').next().unwrap_or(type_path);
        let replacement = match terminal {
            "dict" | "Dict" | "defaultdict" | "DefaultDict" | "Counter" | "OrderedDict"
            | "MutableMapping" => mapping,
            "set" | "Set" | "MutableSet" => set,
            _ => sequence,
        };
        if !replacements.contains(&replacement) {
            replacements.push(replacement);
        }
    }
    replacements.join(", ")
}

/// Returns the read-only `collections.abc` replacements of collection `type_paths`, joined with `", "`.
#[must_use]
pub fn read_only_collection_replacements(type_paths: &[String]) -> String {
    format_collection_replacements(
        type_paths,
        "collections.abc.Mapping",
        "collections.abc.Set",
        "collections.abc.Sequence",
    )
}

/// Returns the immutable constant collection replacements for `type_paths`, joined with `", "`.
#[must_use]
pub fn immutable_constant_collection_replacements(type_paths: &[String]) -> String {
    format_collection_replacements(type_paths, "frozendict", "frozenset", "tuple")
}

/// Returns true if `(path, terminal)` is a concrete or abstract collection type constructor
/// (excluding `tuple` / `Tuple`, which requires variadic-vs-fixed arity inspection when subscripted).
fn is_non_tuple_collection_constructor(path: &str, terminal: &str) -> bool {
    is_concrete_collection_constructor(path, terminal)
        || is_std_type_constructor(path, terminal, MUTABLE_COLLECTION_ABCS)
        || is_std_type_constructor(
            path,
            terminal,
            READONLY_AND_IMMUTABLE_COLLECTION_CONSTRUCTORS,
        )
}

/// Returns true if `node` (unwrapping `type` and `parenthesized_expression`) is an `ellipsis` (`...`).
fn is_ellipsis_type_arg(node: &RawNode<'_>) -> bool {
    unwrap_type_and_parens(node).is_some_and(|inner| inner.kind() == "ellipsis")
}

/// Unwraps outer return-annotation envelopes (`type`, `parenthesized_expression`,
/// `Annotated[T, ...]`, `Awaitable[T]`, and `Coroutine[YieldT, SendT, ReturnT]`).
fn unwrap_return_envelope<'a>(node: &RawNode<'a>) -> RawNode<'a> {
    let mut current = node.clone();
    loop {
        let Some(unwrapped) = unwrap_type_and_parens(&current) else {
            return current;
        };
        current = unwrapped;
        if matches!(current.kind().as_ref(), "generic_type" | "subscript")
            && let Some((base_node, type_args)) = extract_generic_base_and_args(&current)
        {
            let (base_path, base_terminal) = resolve_path_and_terminal_raw(&base_node);
            if is_std_type_constructor(&base_path, &base_terminal, &["Annotated", "Awaitable"])
                && let Some(first_arg) = type_args.first().cloned()
            {
                current = first_arg;
                continue;
            }
            if is_std_type_constructor(&base_path, &base_terminal, &["Coroutine"])
                && let Some(return_arg) = type_args.get(2).cloned()
            {
                current = return_arg;
                continue;
            }
        }
        return current;
    }
}

/// Flattens top-level union constructs (`|`, `Optional[...]`, `Union[...]`, and `Annotated[T, ...]`)
/// into `branches` and sets `*has_none = true` if `None` (`none`) or `Optional[...]` is part of the union.
fn collect_union_branches<'a>(
    node: &RawNode<'a>,
    has_none: &mut bool,
    branches: &mut Vec<RawNode<'a>>,
) {
    match node.kind().as_ref() {
        "type" | "parenthesized_expression" | "union_type" => {
            for child in node.children() {
                if child.is_named() && !child.is_extra() {
                    collect_union_branches(&child, has_none, branches);
                }
            }
        }
        "binary_operator"
            if node
                .field("operator")
                .is_some_and(|operator| operator.text() == "|") =>
        {
            if let Some(left) = node.field("left") {
                collect_union_branches(&left, has_none, branches);
            }
            if let Some(right) = node.field("right") {
                collect_union_branches(&right, has_none, branches);
            }
        }
        "none" => {
            *has_none = true;
        }
        "generic_type" | "subscript" => {
            let Some((base_node, type_args)) = extract_generic_base_and_args(node) else {
                branches.push(node.clone());
                return;
            };
            let (base_path, base_terminal) = resolve_path_and_terminal_raw(&base_node);
            if is_std_type_constructor(&base_path, &base_terminal, &["Optional"]) {
                *has_none = true;
                for argument in &type_args {
                    collect_union_branches(argument, has_none, branches);
                }
                return;
            }
            if is_std_type_constructor(&base_path, &base_terminal, &["Union"]) {
                for argument in &type_args {
                    collect_union_branches(argument, has_none, branches);
                }
                return;
            }
            if is_std_type_constructor(&base_path, &base_terminal, &["Annotated"]) {
                if let Some(first_arg) = type_args.first() {
                    collect_union_branches(first_arg, has_none, branches);
                }
                return;
            }
            branches.push(node.clone());
        }
        _ => {
            branches.push(node.clone());
        }
    }
}

/// Returns the collection type constructor path (e.g. `"Sequence"`, `"list"`, `"tuple"`) if
/// `branch` is a collection type, or `None` otherwise.
fn collection_branch_type_path(branch: &RawNode<'_>) -> Option<String> {
    match branch.kind().as_ref() {
        "identifier" | "attribute" => {
            let (path, terminal) = resolve_path_and_terminal_raw(branch);
            if is_non_tuple_collection_constructor(&path, &terminal)
                || is_std_type_constructor(&path, &terminal, &["tuple", "Tuple"])
            {
                Some(path)
            } else {
                None
            }
        }
        "generic_type" | "subscript" => {
            let (base_node, type_args) = extract_generic_base_and_args(branch)?;
            let (base_path, base_terminal) = resolve_path_and_terminal_raw(&base_node);
            let is_variadic_tuple =
                is_std_type_constructor(&base_path, &base_terminal, &["tuple", "Tuple"])
                    && type_args.len() == 2
                    && is_ellipsis_type_arg(&type_args[1]);
            if is_non_tuple_collection_constructor(&base_path, &base_terminal) || is_variadic_tuple
            {
                Some(base_path)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Collects collection type constructors from a nullable collection return annotation.
#[must_use]
pub fn collect_nullable_collection_return_types(type_node: &AstNode<'_>) -> Vec<String> {
    let root = unwrap_return_envelope(&type_node.raw);
    let mut has_none = false;
    let mut branches = Vec::new();
    collect_union_branches(&root, &mut has_none, &mut branches);

    if !has_none || branches.is_empty() {
        return Vec::new();
    }

    let mut matched = Vec::new();
    for branch in &branches {
        let Some(type_path) = collection_branch_type_path(branch) else {
            return Vec::new();
        };
        if !matched.contains(&type_path) {
            matched.push(type_path);
        }
    }
    matched
}
