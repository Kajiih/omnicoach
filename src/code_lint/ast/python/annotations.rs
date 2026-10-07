//! Python type annotation unwrapping, constructor resolution, union flattening, and collection type vocabularies.

use super::{AstNode, ParsedFile, find_expr_at_span, resolve_path_and_terminal_expr};
use crate::code_lint::ast::span_from_ruff_range;
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_stmt};
use ruff_python_ast::{Expr, Operator, Stmt};
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

/// Returns true if `file` contains an unaliased `from collections.abc import Set` statement.
#[must_use]
pub fn has_unaliased_collections_abc_set_import(file: &ParsedFile) -> bool {
    *file.abc_set_imported.get_or_init(|| {
        struct ImportFinder {
            found: bool,
        }

        impl<'a> SourceOrderVisitor<'a> for ImportFinder {
            fn visit_stmt(&mut self, statement: &'a Stmt) {
                if self.found {
                    return;
                }
                if let Stmt::ImportFrom(import_from) = statement
                    && import_from.level == 0
                    && import_from.module.as_deref() == Some("collections.abc")
                    && import_from.names.iter().any(|alias| {
                        alias.name.as_str() == TYPING_SET_CONSTRUCTOR && alias.asname.is_none()
                    })
                {
                    self.found = true;
                    return;
                }
                walk_stmt(self, statement);
            }
        }

        let Some(parsed) = file.py_module() else {
            return false;
        };
        let mut finder = ImportFinder { found: false };
        finder.visit_body(&parsed.syntax().body);
        finder.found
    })
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

/// Unwraps outer `Annotated[T, ...]` wrappers from `type_expr`.
fn unwrap_annotated_expr<'a>(mut current: &'a Expr, source: &str) -> &'a Expr {
    while let Some((base_node, type_args)) = extract_generic_base_and_args(current) {
        let (base_path, base_terminal) = resolve_path_and_terminal_expr(base_node, source);
        if is_std_type_constructor(&base_path, &base_terminal, &[TYPE_ANNOTATED])
            && let Some(&first_arg) = type_args.first()
        {
            current = first_arg;
        } else {
            break;
        }
    }
    current
}

/// True if `type_expr` is `Final` or `Final[T]` (qualified or not, optionally wrapped in `Annotated`).
pub(super) fn has_final_annotation_expr(type_expr: &Expr, source: &str) -> bool {
    let unwrapped = unwrap_annotated_expr(type_expr, source);
    let candidate =
        extract_generic_base_and_args(unwrapped).map_or(unwrapped, |(base_node, _)| base_node);
    resolve_path_and_terminal_expr(candidate, source).1 == TYPE_FINAL
}

/// Returns true if `type_expr` is an unparameterized `Final` qualifier (`Final`, `typing.Final`,
/// or `typing_extensions.Final`, optionally wrapped in `Annotated[Final, ...]`), which PEP 591
/// forbids in a class body without an initializer (`x: Final` is invalid; `x: Final[T]` is valid).
pub(super) fn is_bare_final_annotation_expr(type_expr: &Expr, source: &str) -> bool {
    let unwrapped = unwrap_annotated_expr(type_expr, source);
    if matches!(unwrapped, Expr::Subscript(_)) {
        return false;
    }
    let (path, terminal) = resolve_path_and_terminal_expr(unwrapped, source);
    is_std_type_constructor(&path, &terminal, &[TYPE_FINAL])
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
    /// The constructor's unqualified name (`list`, `Dict`, `Sequence`).
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

/// Classifies `(path, terminal)` as a standard-library collection type constructor.
///
/// With `abc_set_imported`, unqualified `Set` is `collections.abc.Set` rather than `typing.Set`.
fn classify_collection(
    path: &str,
    terminal: &str,
    abc_set_imported: bool,
) -> Option<(CollectionKind, CollectionShape)> {
    let kind = if is_concrete_collection_constructor(path, terminal)
        && !(abc_set_imported && path == TYPING_SET_CONSTRUCTOR)
    {
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

/// Recursively collects matching type constructor `(path, terminal)` pairs from `expr`
/// according to `depth`, deduplicated by path.
pub(super) fn collect_type_constructors_expr<F>(
    expr: &Expr,
    source: &str,
    depth: AnnotationTraversalDepth,
    predicate: &F,
    out: &mut Vec<(String, String)>,
) where
    F: Fn(&str, &str) -> bool,
{
    match expr {
        Expr::BinOp(bin_op) if bin_op.op == Operator::BitOr => {
            collect_type_constructors_expr(&bin_op.left, source, depth, predicate, out);
            collect_type_constructors_expr(&bin_op.right, source, depth, predicate, out);
        }
        Expr::Name(_) | Expr::Attribute(_) => {
            let (path, terminal) = resolve_path_and_terminal_expr(expr, source);
            push_type_constructor(path, terminal, predicate, out);
        }
        Expr::Subscript(_) => {
            let Some((base_node, type_args)) = extract_generic_base_and_args(expr) else {
                return;
            };
            let (base_path, base_terminal) = resolve_path_and_terminal_expr(base_node, source);

            if is_std_type_constructor(&base_path, &base_terminal, TRANSPARENT_UNION_WRAPPERS) {
                for arg in &type_args {
                    collect_type_constructors_expr(arg, source, depth, predicate, out);
                }
                return;
            }

            if is_std_type_constructor(&base_path, &base_terminal, TRANSPARENT_FIRST_ARG_WRAPPERS) {
                if let Some(first_arg) = type_args.first() {
                    collect_type_constructors_expr(first_arg, source, depth, predicate, out);
                }
                return;
            }

            let is_std_constructor = is_std_type_constructor_prefix(&base_path, &base_terminal);
            let terminal = base_terminal.clone();
            push_type_constructor(base_path, base_terminal, predicate, out);

            if depth == AnnotationTraversalDepth::CovariantPositions && is_std_constructor {
                match terminal.as_str() {
                    terminal_name if SINGLE_ARG_COVARIANT_CONTAINERS.contains(&terminal_name) => {
                        if let Some(first_arg) = type_args.first() {
                            collect_type_constructors_expr(
                                first_arg, source, depth, predicate, out,
                            );
                        }
                    }
                    "tuple" | "Tuple" => {
                        for arg in &type_args {
                            collect_type_constructors_expr(arg, source, depth, predicate, out);
                        }
                    }
                    TYPE_MAPPING | "Callable" => {
                        if let Some(second_arg) = type_args.get(1) {
                            collect_type_constructors_expr(
                                second_arg, source, depth, predicate, out,
                            );
                        }
                    }
                    "Generator" | TYPE_COROUTINE => {
                        if let Some(yield_arg) = type_args.first() {
                            collect_type_constructors_expr(
                                yield_arg, source, depth, predicate, out,
                            );
                        }
                        if let Some(return_arg) = type_args.get(2) {
                            collect_type_constructors_expr(
                                return_arg, source, depth, predicate, out,
                            );
                        }
                    }
                    "AsyncGenerator" => {
                        if let Some(yield_arg) = type_args.first() {
                            collect_type_constructors_expr(
                                yield_arg, source, depth, predicate, out,
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

/// Appends `(path, terminal)` to `out` if it satisfies `predicate` and `path` is not there yet.
fn push_type_constructor<F>(
    path: String,
    terminal: String,
    predicate: &F,
    out: &mut Vec<(String, String)>,
) where
    F: Fn(&str, &str) -> bool,
{
    if predicate(&path, &terminal) && !out.iter().any(|(seen, _)| *seen == path) {
        out.push((path, terminal));
    }
}

/// Collects the standard-library collection types in `type_node` according to `depth`, in
/// source order and deduplicated by path.
///
/// With `abc_set_imported` (the file has an unaliased `from collections.abc import Set`, see
/// [`has_unaliased_collections_abc_set_import`]), unqualified `Set` is the abstract
/// `collections.abc.Set` rather than the concrete `typing.Set`.
#[must_use]
pub fn collect_collection_types(
    type_node: &AstNode<'_>,
    depth: AnnotationTraversalDepth,
    abc_set_imported: bool,
) -> Vec<PythonCollectionType> {
    let Some(parsed) = type_node.file.py_module() else {
        return Vec::new();
    };
    let Some(expr) = find_expr_at_span(parsed.syntax(), type_node.span()) else {
        return Vec::new();
    };
    let mut constructors = Vec::new();
    collect_type_constructors_expr(
        expr,
        &type_node.file.source,
        depth,
        &|path, terminal| classify_collection(path, terminal, abc_set_imported).is_some(),
        &mut constructors,
    );
    constructors
        .into_iter()
        .filter_map(|(path, name)| {
            let (kind, shape) = classify_collection(&path, &name, abc_set_imported)?;
            Some(PythonCollectionType {
                path,
                name,
                kind,
                shape,
            })
        })
        .collect()
}

/// The standard-library collection type that `expression` (an identifier or a dotted attribute,
/// such as the callee of `deque()` or `collections.Counter()`) names.
///
/// With `abc_set_imported`, unqualified `Set` is `collections.abc.Set` rather than `typing.Set`.
#[must_use]
pub fn collection_type(
    expression: &AstNode<'_>,
    abc_set_imported: bool,
) -> Option<PythonCollectionType> {
    let expr = find_expr_at_span(expression.file.py_module()?.syntax(), expression.span())?;
    collection_type_expr(expr, &expression.file.source, abc_set_imported)
}

/// See [`collection_type`].
fn collection_type_expr(
    expr: &Expr,
    source: &str,
    abc_set_imported: bool,
) -> Option<PythonCollectionType> {
    if !matches!(expr, Expr::Name(_) | Expr::Attribute(_)) {
        return None;
    }
    let (path, name) = resolve_path_and_terminal_expr(expr, source);
    let (kind, shape) = classify_collection(&path, &name, abc_set_imported)?;
    Some(PythonCollectionType {
        path,
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
fn unwrap_return_envelope_expr<'a>(mut current: &'a Expr, source: &str) -> &'a Expr {
    while let Some((base_node, type_args)) = extract_generic_base_and_args(current) {
        let (base_path, base_terminal) = resolve_path_and_terminal_expr(base_node, source);
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
    source: &str,
    has_none: &mut bool,
    branches: &mut Vec<&'a Expr>,
) {
    match expr {
        Expr::BinOp(bin_op) if bin_op.op == Operator::BitOr => {
            collect_union_branches_expr(&bin_op.left, source, has_none, branches);
            collect_union_branches_expr(&bin_op.right, source, has_none, branches);
        }
        Expr::NoneLiteral(_) => {
            *has_none = true;
        }
        Expr::Subscript(_) => {
            let Some((base_node, type_args)) = extract_generic_base_and_args(expr) else {
                branches.push(expr);
                return;
            };
            let (base_path, base_terminal) = resolve_path_and_terminal_expr(base_node, source);
            if is_std_type_constructor(&base_path, &base_terminal, &["Optional"]) {
                *has_none = true;
                for argument in type_args {
                    collect_union_branches_expr(argument, source, has_none, branches);
                }
                return;
            }
            if is_std_type_constructor(&base_path, &base_terminal, &["Union"]) {
                for argument in type_args {
                    collect_union_branches_expr(argument, source, has_none, branches);
                }
                return;
            }
            if is_std_type_constructor(&base_path, &base_terminal, &[TYPE_ANNOTATED]) {
                if let Some(&first_arg) = type_args.first() {
                    collect_union_branches_expr(first_arg, source, has_none, branches);
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
///
/// With `abc_set_imported`, unqualified `Set` is `collections.abc.Set` rather than `typing.Set`.
#[must_use]
pub fn return_type_union<'a>(
    return_type_node: &AstNode<'a>,
    abc_set_imported: bool,
) -> PythonReturnTypeUnion<'a> {
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
    let unwrapped = unwrap_return_envelope_expr(expr, &file.source);
    let mut has_none = false;
    let mut branches = Vec::new();
    collect_union_branches_expr(unwrapped, &file.source, &mut has_none, &mut branches);
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
                    collection: collection_type_expr(constructor, &file.source, abc_set_imported),
                    type_arguments: generic
                        .map(|(_, arguments)| arguments.into_iter().map(node_of).collect()),
                }
            })
            .collect(),
    }
}
