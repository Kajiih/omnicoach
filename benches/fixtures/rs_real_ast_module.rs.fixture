//! Python type annotation unwrapping, constructor resolution, union flattening, and collection type vocabularies.

use super::{AstNode, ParsedFile, find_expr_at_span, resolve_path_and_terminal_expr};
use crate::code_lint::ast::{ResolvedName, resolve_name, span_from_ruff_range};
use ruff_python_ast::{Expr, Operator};
use ruff_text_size::Ranged as _;

/// Controls how deeply [`collect_collection_types`] traverses a Python type annotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnotationTraversalDepth {
    /// Unwraps only transparent wrappers (`|`, `Optional`, `Union`, `Annotated[T, ...]`,
    /// `ClassVar[T]`, `Final[T]`, `Required[T]`, `NotRequired[T]`, `ReadOnly[T]`).
    TransparentWrappersOnly,
    /// Unwraps transparent wrappers and recurses into covariant type parameter positions of
    /// read-only containers (`Sequence[T]`, `Mapping[K, V]` value `V`, `tuple[...]`,
    /// `Awaitable[T]`, `Callable[[...], Ret]` return `Ret`, `Generator`/`Coroutine` yield and
    /// return). Invariant containers (`list`, `dict`, `set`, `Mutable*`) are not entered.
    CovariantPositions,
}

const TYPING_PREFIX: &str = "typing.";
const TYPING_EXTENSIONS_PREFIX: &str = "typing_extensions.";
const TYPE_ANNOTATED: &str = "Annotated";
const TYPE_FINAL: &str = "Final";
const TYPE_COROUTINE: &str = "Coroutine";
const TYPE_MAPPING: &str = "Mapping";
const LIST_CONSTRUCTOR: &str = "list";
const SET_CONSTRUCTOR: &str = "set";
const TYPING_SET_CONSTRUCTOR: &str = "Set";
const DICT_CONSTRUCTOR: &str = "dict";
const TYPING_DICT_CONSTRUCTOR: &str = "Dict";
const TYPE_DEFAULTDICT: &str = "defaultdict";
const TYPE_TYPING_DEFAULTDICT: &str = "DefaultDict";
const TYPE_COUNTER: &str = "Counter";
const ORDERED_DICT_CONSTRUCTOR: &str = "OrderedDict";

/// Returns true if `(path, terminal)` refers to an unqualified or standard-library (`typing`,
/// `typing_extensions`, `collections.abc`, `builtins`) type constructor.
fn is_std_type_constructor_prefix(path: &str, terminal: &str) -> bool {
    path == terminal
        || path.strip_suffix(terminal).is_some_and(|prefix| {
            matches!(
                prefix,
                TYPING_PREFIX | TYPING_EXTENSIONS_PREFIX | "collections.abc." | "builtins."
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
        LIST_CONSTRUCTOR
        | "List"
        | DICT_CONSTRUCTOR
        | TYPING_DICT_CONSTRUCTOR
        | SET_CONSTRUCTOR => is_std_type_constructor_prefix(path, terminal),
        TYPING_SET_CONSTRUCTOR => {
            matches!(
                path,
                TYPING_SET_CONSTRUCTOR | "typing.Set" | "typing_extensions.Set"
            )
        }
        TYPE_DEFAULTDICT
        | TYPE_TYPING_DEFAULTDICT
        | "deque"
        | "Deque"
        | TYPE_COUNTER
        | ORDERED_DICT_CONSTRUCTOR => {
            path == terminal
                || path.strip_suffix(terminal).is_some_and(|prefix| {
                    matches!(
                        prefix,
                        "collections." | TYPING_PREFIX | TYPING_EXTENSIONS_PREFIX
                    )
                })
        }
        _ => false,
    }
}

/// Extracts `(base_expr, type_argument_exprs)` from a Python `Expr::Subscript` node inside a
/// type annotation.
pub(super) fn extract_generic_base_and_args(expr: &Expr) -> Option<(&Expr, Vec<&Expr>)> {
    let Expr::Subscript(subscript) = expr else {
        return None;
    };
    let type_args = if let Expr::Tuple(tuple) = subscript.slice.as_ref() {
        tuple.elts.iter().collect()
    } else {
        vec![subscript.slice.as_ref()]
    };
    Some((subscript.value.as_ref(), type_args))
}

/// The `(path, terminal)` that the type constructor expression `expr` names, resolved through the
/// imports of `file` (`t.List` after `import typing as t` is `("typing.List", "List")`), or `None`
/// if it names a definition of `file` (see [`resolve_name`]).
fn resolved_path_and_terminal_expr(expr: &Expr, file: &ParsedFile) -> Option<(String, String)> {
    let (path, terminal) = resolve_path_and_terminal_expr(expr, &file.source);
    match resolve_name(file, &path) {
        ResolvedName::Imported(resolved) => {
            let terminal = resolved
                .rsplit_once('.')
                .map_or(resolved.as_str(), |(_, terminal)| terminal)
                .to_owned();
            Some((resolved, terminal))
        }
        ResolvedName::Local => None,
        ResolvedName::Unbound => Some((path, terminal)),
    }
}

/// Unwraps outer `Annotated[T, ...]` wrappers from `type_expr`.
fn unwrap_annotated_expr<'a>(mut current: &'a Expr, file: &ParsedFile) -> &'a Expr {
    while let Some((base_node, type_args)) = extract_generic_base_and_args(current)
        && let Some((base_path, base_terminal)) = resolved_path_and_terminal_expr(base_node, file)
        && is_std_type_constructor(&base_path, &base_terminal, &[TYPE_ANNOTATED])
        && let Some(&first_arg) = type_args.first()
    {
        current = first_arg;
    }
    current
}

/// True if `type_expr` is `Final` or `Final[T]` (qualified or not, optionally wrapped in `Annotated`).
pub(super) fn has_final_annotation_expr(type_expr: &Expr, file: &ParsedFile) -> bool {
    let unwrapped = unwrap_annotated_expr(type_expr, file);
    let candidate =
        extract_generic_base_and_args(unwrapped).map_or(unwrapped, |(base_node, _)| base_node);
    resolved_path_and_terminal_expr(candidate, file)
        .is_some_and(|(_, terminal)| terminal == TYPE_FINAL)
}

/// Returns true if `type_expr` is an unparameterized `Final` qualifier (`Final`, `typing.Final`,
/// or `typing_extensions.Final`, optionally wrapped in `Annotated[Final, ...]`), which PEP 591
/// forbids in a class body without an initializer (`x: Final` is invalid; `x: Final[T]` is valid).
pub(super) fn is_bare_final_annotation_expr(type_expr: &Expr, file: &ParsedFile) -> bool {
    let unwrapped = unwrap_annotated_expr(type_expr, file);
    if matches!(unwrapped, Expr::Subscript(_)) {
        return false;
    }
    resolved_path_and_terminal_expr(unwrapped, file)
        .is_some_and(|(path, terminal)| is_std_type_constructor(&path, &terminal, &[TYPE_FINAL]))
}

/// Transparent type wrappers whose all type arguments preserve the enclosing variance.
const TRANSPARENT_UNION_WRAPPERS: &[&str] = &["Optional", "Union"];

/// Transparent type qualifiers whose first type argument (`arg 0`) preserves the enclosing variance.
const TRANSPARENT_FIRST_ARG_WRAPPERS: &[&str] = &[
    TYPE_ANNOTATED,
    "ClassVar",
    TYPE_FINAL,
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
    TYPING_SET_CONSTRUCTOR,
    "frozenset",
    "FrozenSet",
];

/// Abstract mutable collection constructors in `collections.abc` and `typing`.
pub(super) const MUTABLE_COLLECTION_ABCS: &[&str] =
    &["MutableSequence", "MutableMapping", "MutableSet"];

/// Abstract read-only collection constructors in `collections.abc` and `typing`.
const READ_ONLY_COLLECTION_ABCS: &[&str] = &[
    "Sequence",
    TYPE_MAPPING,
    TYPING_SET_CONSTRUCTOR,
    "AbstractSet",
    "Collection",
    "Iterable",
    "Iterator",
    "Reversible",
];

/// Immutable collection constructors in `builtins` and `typing`.
const IMMUTABLE_COLLECTIONS: &[&str] = &["tuple", "Tuple", "frozenset", "FrozenSet"];

/// What a standard-library collection type lets its holder do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionKind {
    /// A concrete mutable container: `list`, `dict`, `set`, their `typing` aliases, and the
    /// `collections` containers (`defaultdict`, `deque`, `Counter`, `OrderedDict`).
    ConcreteMutable,
    /// An abstract mutable interface: `MutableSequence`, `MutableMapping`, `MutableSet`.
    AbstractMutable,
    /// An abstract read-only interface: `Sequence`, `Mapping`, `collections.abc.Set`,
    /// `AbstractSet`, `Collection`, `Iterable`, `Iterator`, `Reversible`.
    AbstractReadOnly,
    /// An immutable container: `tuple`, `frozenset` and their `typing` aliases.
    Immutable,
}

/// How a standard-library collection type gives access to its elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionShape {
    /// By key: `dict`, `Mapping`, `MutableMapping`, `defaultdict`, `Counter`, `OrderedDict`.
    Mapping,
    /// By membership: `set`, `frozenset`, `AbstractSet`, `MutableSet`.
    Set,
    /// By position: `list`, `tuple`, `deque`, `Sequence`, `MutableSequence`.
    Sequence,
    /// By iteration only: `Collection`, `Iterable`, `Iterator`, `Reversible`.
    Iterable,
}

/// A standard-library collection type, as named in source or built by a display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonCollectionType {
    /// The type as written (`list`, `typing.Dict`, `collections.abc.Sequence`).
    pub path: String,
    /// The constructor's unqualified name, resolved through the file's imports (`list`, `Dict`,
    /// `Sequence`; `Set` for `ReadOnlySet` after `from collections.abc import Set as ReadOnlySet`).
    pub name: String,
    /// What the type lets its holder do.
    pub kind: CollectionKind,
    /// How the type gives access to its elements.
    pub shape: CollectionShape,
}

impl PythonCollectionType {
    /// The paths of `collection_types`, joined with `", "`.
    #[must_use]
    pub fn joined_paths(collection_types: &[Self]) -> String {
        collection_types
            .iter()
            .map(|collection_type| collection_type.path.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Classifies the import-resolved `(path, terminal)` as a standard-library collection type
/// constructor.
fn classify_collection(path: &str, terminal: &str) -> Option<(CollectionKind, CollectionShape)> {
    let kind = if is_concrete_collection_constructor(path, terminal) {
        CollectionKind::ConcreteMutable
    } else if is_std_type_constructor(path, terminal, MUTABLE_COLLECTION_ABCS) {
        CollectionKind::AbstractMutable
    } else if is_std_type_constructor(path, terminal, READ_ONLY_COLLECTION_ABCS) {
        CollectionKind::AbstractReadOnly
    } else if is_std_type_constructor(path, terminal, IMMUTABLE_COLLECTIONS) {
        CollectionKind::Immutable
    } else {
        return None;
    };
    let shape = match terminal {
        DICT_CONSTRUCTOR
        | TYPING_DICT_CONSTRUCTOR
        | TYPE_DEFAULTDICT
        | TYPE_TYPING_DEFAULTDICT
        | TYPE_COUNTER
        | ORDERED_DICT_CONSTRUCTOR
        | "MutableMapping"
        | TYPE_MAPPING => CollectionShape::Mapping,
        SET_CONSTRUCTOR
        | TYPING_SET_CONSTRUCTOR
        | "MutableSet"
        | "AbstractSet"
        | "frozenset"
        | "FrozenSet" => CollectionShape::Set,
        "Collection" | "Iterable" | "Iterator" | "Reversible" => CollectionShape::Iterable,
        _ => CollectionShape::Sequence,
    };
    Some((kind, shape))
}

/// Recursively collects the collection types named in `expr` according to `depth`, deduplicated
/// by path.
fn collect_collection_types_expr(
    expr: &Expr,
    file: &ParsedFile,
    depth: AnnotationTraversalDepth,
    out: &mut Vec<PythonCollectionType>,
) {
    match expr {
        Expr::BinOp(bin_op) if bin_op.op == Operator::BitOr => {
            collect_collection_types_expr(&bin_op.left, file, depth, out);
            collect_collection_types_expr(&bin_op.right, file, depth, out);
        }
        Expr::Name(_) | Expr::Attribute(_) => push_collection_type(expr, file, out),
        Expr::Subscript(_) => {
            let Some((base_node, type_args)) = extract_generic_base_and_args(expr) else {
                return;
            };
            let Some((base_path, base_terminal)) = resolved_path_and_terminal_expr(base_node, file)
            else {
                return;
            };

            if is_std_type_constructor(&base_path, &base_terminal, TRANSPARENT_UNION_WRAPPERS) {
                for arg in &type_args {
                    collect_collection_types_expr(arg, file, depth, out);
                }
                return;
            }

            if is_std_type_constructor(&base_path, &base_terminal, TRANSPARENT_FIRST_ARG_WRAPPERS) {
                if let Some(first_arg) = type_args.first() {
                    collect_collection_types_expr(first_arg, file, depth, out);
                }
                return;
            }

            push_collection_type(base_node, file, out);

            if depth == AnnotationTraversalDepth::CovariantPositions
                && is_std_type_constructor_prefix(&base_path, &base_terminal)
            {
                match base_terminal.as_str() {
                    terminal_name if SINGLE_ARG_COVARIANT_CONTAINERS.contains(&terminal_name) => {
                        if let Some(first_arg) = type_args.first() {
                            collect_collection_types_expr(first_arg, file, depth, out);
                        }
                    }
                    "tuple" | "Tuple" => {
                        for arg in &type_args {
                            collect_collection_types_expr(arg, file, depth, out);
                        }
                    }
                    TYPE_MAPPING | "Callable" => {
                        if let Some(second_arg) = type_args.get(1) {
                            collect_collection_types_expr(second_arg, file, depth, out);
                        }
                    }
                    "Generator" | TYPE_COROUTINE => {
                        if let Some(yield_arg) = type_args.first() {
                            collect_collection_types_expr(yield_arg, file, depth, out);
                        }
                        if let Some(return_arg) = type_args.get(2) {
                            collect_collection_types_expr(return_arg, file, depth, out);
                        }
                    }
                    "AsyncGenerator" => {
                        if let Some(yield_arg) = type_args.first() {
                            collect_collection_types_expr(yield_arg, file, depth, out);
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

/// Appends the collection type that `expr` names to `out` if its path is not there yet.
fn push_collection_type(expr: &Expr, file: &ParsedFile, out: &mut Vec<PythonCollectionType>) {
    if let Some(collection) = collection_type_expr(expr, file)
        && !out.iter().any(|seen| seen.path == collection.path)
    {
        out.push(collection);
    }
}

/// Collects the standard-library collection types in `type_node` according to `depth`, in
/// source order and deduplicated by path.
///
/// Names are resolved through the file's imports (see [`resolve_name`]), so
/// `from collections.abc import Set` makes `Set` the abstract `collections.abc.Set` rather than
/// the concrete `typing.Set`, and a locally defined `class Set` is not a collection type.
#[must_use]
pub fn collect_collection_types(
    type_node: &AstNode<'_>,
    depth: AnnotationTraversalDepth,
) -> Vec<PythonCollectionType> {
    let Some(parsed) = type_node.file.py_module() else {
        return Vec::new();
    };
    let Some(expr) = find_expr_at_span(parsed.syntax(), type_node.span()) else {
        return Vec::new();
    };
    let mut collection_types = Vec::new();
    collect_collection_types_expr(expr, type_node.file, depth, &mut collection_types);
    collection_types
}

/// The standard-library collection type that `expression` (an identifier or a dotted attribute,
/// such as the callee of `deque()` or `collections.Counter()`) names.
///
/// Names are resolved through the file's imports as in [`collect_collection_types`].
#[must_use]
pub fn collection_type(expression: &AstNode<'_>) -> Option<PythonCollectionType> {
    let expr = find_expr_at_span(expression.file.py_module()?.syntax(), expression.span())?;
    collection_type_expr(expr, expression.file)
}

/// See [`collection_type`].
fn collection_type_expr(expr: &Expr, file: &ParsedFile) -> Option<PythonCollectionType> {
    if !matches!(expr, Expr::Name(_) | Expr::Attribute(_)) {
        return None;
    }
    let (resolved_path, name) = resolved_path_and_terminal_expr(expr, file)?;
    let (kind, shape) = classify_collection(&resolved_path, &name)?;
    let span = span_from_ruff_range(expr.range());
    Some(PythonCollectionType {
        path: file.source[span.start..span.end].to_owned(),
        name,
        kind,
        shape,
    })
}

/// The builtin collection that `expression` builds if it is a list, set or dictionary display
/// (`[...]`, `{a, b}`, `{k: v}`) or comprehension.
#[must_use]
pub fn collection_display(expression: &AstNode<'_>) -> Option<PythonCollectionType> {
    let expr = find_expr_at_span(expression.file.py_module()?.syntax(), expression.span())?;
    let (name, shape) = match expr {
        Expr::List(_) | Expr::ListComp(_) => (LIST_CONSTRUCTOR, CollectionShape::Sequence),
        Expr::Set(_) | Expr::SetComp(_) => (SET_CONSTRUCTOR, CollectionShape::Set),
        Expr::Dict(_) | Expr::DictComp(_) => (DICT_CONSTRUCTOR, CollectionShape::Mapping),
        _ => return None,
    };
    Some(PythonCollectionType {
        path: name.to_owned(),
        name: name.to_owned(),
        kind: CollectionKind::ConcreteMutable,
        shape,
    })
}

/// A Python return type flattened across union constructs (`|`, `Union[...]`, `Optional[...]`),
/// with async envelopes (`Awaitable`, `Coroutine`) and metadata wrappers (`Annotated`) unwrapped.
#[derive(Clone)]
pub struct PythonReturnTypeUnion<'a> {
    /// True if `None` (`none`) or an `Optional[...]` wrapper is part of the union.
    pub has_none: bool,
    /// The non-`None` alternative branches in source order.
    pub branches: Vec<PythonReturnTypeBranch<'a>>,
}

/// A non-`None` alternative of a [`PythonReturnTypeUnion`].
#[derive(Clone)]
pub struct PythonReturnTypeBranch<'a> {
    /// The branch type expression.
    pub node: AstNode<'a>,
    /// The collection type the branch names, directly (`list`) or as the base of a subscript
    /// (`list[int]`); see [`collection_type`].
    pub collection: Option<PythonCollectionType>,
    /// The type arguments if the branch is a subscript (`int` and `...` in `tuple[int, ...]`).
    pub type_arguments: Option<Vec<AstNode<'a>>>,
}

/// Unwraps outer return-annotation envelopes (`Annotated[T, ...]`, `Awaitable[T]`, and
/// `Coroutine[YieldT, SendT, ReturnT]`).
fn unwrap_return_envelope_expr<'a>(mut current: &'a Expr, file: &ParsedFile) -> &'a Expr {
    while let Some((base_node, type_args)) = extract_generic_base_and_args(current)
        && let Some((base_path, base_terminal)) = resolved_path_and_terminal_expr(base_node, file)
    {
        if is_std_type_constructor(&base_path, &base_terminal, &[TYPE_ANNOTATED, "Awaitable"])
            && let Some(&first_arg) = type_args.first()
        {
            current = first_arg;
            continue;
        }
        if is_std_type_constructor(&base_path, &base_terminal, &[TYPE_COROUTINE])
            && let Some(&return_arg) = type_args.get(2)
        {
            current = return_arg;
            continue;
        }
        break;
    }
    current
}

/// Flattens top-level union constructs (`|`, `Optional[...]`, `Union[...]`, and `Annotated[T, ...]`)
/// into `branches` and sets `*has_none = true` if `None` (`none`) or `Optional[...]` is part of the union.
fn collect_union_branches_expr<'a>(
    expr: &'a Expr,
    file: &ParsedFile,
    has_none: &mut bool,
    branches: &mut Vec<&'a Expr>,
) {
    match expr {
        Expr::BinOp(bin_op) if bin_op.op == Operator::BitOr => {
            collect_union_branches_expr(&bin_op.left, file, has_none, branches);
            collect_union_branches_expr(&bin_op.right, file, has_none, branches);
        }
        Expr::NoneLiteral(_) => {
            *has_none = true;
        }
        Expr::Subscript(_) => {
            let Some((base_node, type_args)) = extract_generic_base_and_args(expr) else {
                branches.push(expr);
                return;
            };
            let Some((base_path, base_terminal)) = resolved_path_and_terminal_expr(base_node, file)
            else {
                branches.push(expr);
                return;
            };
            if is_std_type_constructor(&base_path, &base_terminal, &["Optional"]) {
                *has_none = true;
                for argument in type_args {
                    collect_union_branches_expr(argument, file, has_none, branches);
                }
                return;
            }
            if is_std_type_constructor(&base_path, &base_terminal, &["Union"]) {
                for argument in type_args {
                    collect_union_branches_expr(argument, file, has_none, branches);
                }
                return;
            }
            if is_std_type_constructor(&base_path, &base_terminal, &[TYPE_ANNOTATED]) {
                if let Some(&first_arg) = type_args.first() {
                    collect_union_branches_expr(first_arg, file, has_none, branches);
                }
                return;
            }
            branches.push(expr);
        }
        _ => {
            branches.push(expr);
        }
    }
}

/// Flattens `return_type_node` across union constructs (`|`, `Union[...]`, `Optional[...]`)
/// after unwrapping outer async and metadata envelopes (`Awaitable`, `Coroutine`, `Annotated`).
#[must_use]
pub fn return_type_union<'a>(return_type_node: &AstNode<'a>) -> PythonReturnTypeUnion<'a> {
    let Some(parsed) = return_type_node.file.py_module() else {
        return PythonReturnTypeUnion {
            has_none: false,
            branches: Vec::new(),
        };
    };
    let Some(expr) = find_expr_at_span(parsed.syntax(), return_type_node.span()) else {
        return PythonReturnTypeUnion {
            has_none: false,
            branches: Vec::new(),
        };
    };
    let file = return_type_node.file;
    let unwrapped = unwrap_return_envelope_expr(expr, file);
    let mut has_none = false;
    let mut branches = Vec::new();
    collect_union_branches_expr(unwrapped, file, &mut has_none, &mut branches);
    let node_of = |expr: &Expr| AstNode::from_span(file, span_from_ruff_range(expr.range()));
    PythonReturnTypeUnion {
        has_none,
        branches: branches
            .into_iter()
            .map(|branch| {
                let generic = extract_generic_base_and_args(branch);
                let constructor = generic.as_ref().map_or(branch, |&(base, _)| base);
                PythonReturnTypeBranch {
                    node: node_of(branch),
                    collection: collection_type_expr(constructor, file),
                    type_arguments: generic
                        .map(|(_, arguments)| arguments.into_iter().map(node_of).collect()),
                }
            })
            .collect(),
    }
}
