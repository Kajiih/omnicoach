//! Python type annotation unwrapping, constructor resolution, union flattening, and collection type vocabularies.

use super::names::written_name;
use super::tree::find_expr;
use crate::code_lint::ast::{AstNode, ParsedFile, QualifiedName, span_from_ruff_range};
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
const TUPLE_CONSTRUCTOR: &str = "tuple";
const TYPING_TUPLE_CONSTRUCTOR: &str = "Tuple";
const TYPE_CALLABLE: &str = "Callable";
const BOOL_CONSTRUCTOR: &str = "bool";

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
const IMMUTABLE_COLLECTIONS: &[&str] = &[
    TUPLE_CONSTRUCTOR,
    TYPING_TUPLE_CONSTRUCTOR,
    "frozenset",
    "FrozenSet",
];

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

/// A subscripted type annotation (`dict[str, int]`).
pub(super) struct GenericType<'a> {
    /// The subscripted base expression (`dict`).
    pub(super) base: &'a Expr,
    /// The type argument expressions, in source order (`str` and `int`).
    pub(super) arguments: Vec<&'a Expr>,
}

/// Extracts the base and type arguments from a Python `Expr::Subscript` node inside a type
/// annotation.
pub(super) fn extract_generic_base_and_args(expr: &Expr) -> Option<GenericType<'_>> {
    let Expr::Subscript(subscript) = expr else {
        return None;
    };
    let type_args = if let Expr::Tuple(tuple) = subscript.slice.as_ref() {
        tuple.elts.iter().collect()
    } else {
        vec![subscript.slice.as_ref()]
    };
    Some(GenericType {
        base: subscript.value.as_ref(),
        arguments: type_args,
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

/// True if `type_expr` is `Final` or `Final[T]` (qualified or not, optionally wrapped in `Annotated`).
pub(super) fn has_final_annotation_expr(type_expr: &Expr, file: &ParsedFile) -> bool {
    let unwrapped = unwrap_annotated_expr(type_expr, file);
    let candidate =
        extract_generic_base_and_args(unwrapped).map_or(unwrapped, |generic| generic.base);
    written_name(candidate, &file.source)
        .resolve(file)
        .is_some_and(|name| is_std_type_constructor(&name.path, &name.terminal, &[TYPE_FINAL]))
}

/// Returns true if `type_expr` is an unparameterized `Final` qualifier (`Final`, `typing.Final`,
/// or `typing_extensions.Final`, optionally wrapped in `Annotated[Final, ...]`), which PEP 591
/// forbids in a class body without an initializer (`x: Final` is invalid; `x: Final[T]` is valid).
pub(super) fn is_bare_final_annotation_expr(type_expr: &Expr, file: &ParsedFile) -> bool {
    let unwrapped = unwrap_annotated_expr(type_expr, file);
    if matches!(unwrapped, Expr::Subscript(_)) {
        return false;
    }
    written_name(unwrapped, &file.source)
        .resolve(file)
        .is_some_and(|name| is_std_type_constructor(&name.path, &name.terminal, &[TYPE_FINAL]))
}

/// Collects the standard-library collection types in `type_node` according to `depth`, in
/// source order and deduplicated by path.
///
/// Names are resolved through the file's imports (see [`crate::code_lint::ast::resolve_name`]), so
/// `from collections.abc import Set` makes `Set` the abstract `collections.abc.Set` rather than
/// the concrete `typing.Set`, and a locally defined `class Set` is not a collection type.
#[must_use]
pub fn collect_collection_types(
    type_node: &AstNode<'_>,
    depth: AnnotationTraversalDepth,
) -> Vec<PythonCollectionType> {
    let Some(expr) = find_expr(type_node) else {
        return Vec::new();
    };
    let mut collection_types = Vec::new();
    collect_collection_types_expr(expr, type_node.file, depth, &mut collection_types);
    collection_types
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
            let Some(GenericType {
                base: base_node,
                arguments: type_args,
            }) = extract_generic_base_and_args(expr)
            else {
                return;
            };
            let Some(QualifiedName {
                path: base_path,
                terminal: base_terminal,
            }) = written_name(base_node, &file.source).resolve(file)
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
                && !is_concrete_collection_constructor(&base_path, &base_terminal)
            {
                match base_terminal.as_str() {
                    terminal_name if SINGLE_ARG_COVARIANT_CONTAINERS.contains(&terminal_name) => {
                        if let Some(first_arg) = type_args.first() {
                            collect_collection_types_expr(first_arg, file, depth, out);
                        }
                    }
                    TUPLE_CONSTRUCTOR | TYPING_TUPLE_CONSTRUCTOR => {
                        for arg in &type_args {
                            collect_collection_types_expr(arg, file, depth, out);
                        }
                    }
                    TYPE_MAPPING | TYPE_CALLABLE => {
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

/// The standard-library collection type that `expression` (an identifier or a dotted attribute,
/// such as the callee of `deque()` or `collections.Counter()`) names.
///
/// Names are resolved through the file's imports as in [`collect_collection_types`].
#[must_use]
pub fn collection_type(expression: &AstNode<'_>) -> Option<PythonCollectionType> {
    let expr = find_expr(expression)?;
    collection_type_expr(expr, expression.file)
}

/// The builtin collection that `expression` builds if it is a list, set or dictionary display
/// (`[...]`, `{a, b}`, `{k: v}`) or comprehension.
#[must_use]
pub fn collection_display(expression: &AstNode<'_>) -> Option<PythonCollectionType> {
    let expr = find_expr(expression)?;
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

impl PythonReturnTypeUnion<'_> {
    /// Returns true if the union admits `None` and at least one other type (`int | None`,
    /// `Optional[str]`).
    #[must_use]
    pub const fn is_nullable(&self) -> bool {
        self.has_none && !self.branches.is_empty()
    }

    /// Returns true if the type is exactly a boolean: `bool`, `TypeGuard[T]` or `TypeIs[T]`,
    /// without `None`.
    #[must_use]
    pub fn is_boolean(&self) -> bool {
        self.single_branch()
            .is_some_and(|branch| branch.is_std_type(BOOLEAN_TYPES))
    }

    /// Returns true if the type is exactly `Any` or `object`, which says nothing about the value.
    #[must_use]
    pub fn is_opaque(&self) -> bool {
        self.single_branch()
            .is_some_and(|branch| branch.is_std_type(OPAQUE_TYPES))
    }

    /// The largest element count among the fixed-length tuple branches of at least two elements
    /// (`tuple[str, int]`, `Tuple[a, b, c]`; not `tuple[int, ...]` or `tuple[int]`).
    #[must_use]
    pub fn positional_tuple_arity(&self) -> Option<usize> {
        self.branches
            .iter()
            .filter_map(PythonReturnTypeBranch::positional_tuple_arity)
            .max()
    }

    /// The only branch, when the type is a single type without `None`.
    const fn single_branch(&self) -> Option<&PythonReturnTypeBranch<'_>> {
        match self.branches.as_slice() {
            [branch] if !self.has_none => Some(branch),
            _ => None,
        }
    }
}

/// Types that are booleans to their callers.
const BOOLEAN_TYPES: &[&str] = &[BOOL_CONSTRUCTOR, "TypeGuard", "TypeIs"];

/// Types that say nothing about the value.
const OPAQUE_TYPES: &[&str] = &["Any", "object"];

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
    /// The constructor (the branch, or the base of its subscript) resolved through the file's
    /// imports, as a path and its terminal name.
    constructor: Option<QualifiedName>,
}

impl PythonReturnTypeBranch<'_> {
    /// The element count if the branch is a fixed-length tuple of at least two elements.
    #[must_use]
    pub fn positional_tuple_arity(&self) -> Option<usize> {
        let collection = self.collection.as_ref()?;
        if !matches!(
            collection.name.as_str(),
            TUPLE_CONSTRUCTOR | TYPING_TUPLE_CONSTRUCTOR
        ) {
            return None;
        }
        let arguments = self.type_arguments.as_ref()?;
        let is_fixed_length = arguments
            .iter()
            .all(|argument| argument.text().trim() != "...");
        (arguments.len() >= 2 && is_fixed_length).then_some(arguments.len())
    }

    /// Returns true if the branch's constructor is one of the standard-library types `names`.
    fn is_std_type(&self, names: &[&str]) -> bool {
        self.constructor.as_ref().is_some_and(|constructor| {
            is_std_type_constructor(&constructor.path, &constructor.terminal, names)
        })
    }
}

/// Flattens `return_type_node` across union constructs (`|`, `Union[...]`, `Optional[...]`)
/// after unwrapping outer async and metadata envelopes (`Awaitable`, `Coroutine`, `Annotated`).
#[must_use]
pub fn return_type_union<'a>(return_type_node: &AstNode<'a>) -> PythonReturnTypeUnion<'a> {
    let Some(expr) = find_expr(return_type_node) else {
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
                let constructor = generic.as_ref().map_or(branch, |generic| generic.base);
                PythonReturnTypeBranch {
                    node: node_of(branch),
                    collection: collection_type_expr(constructor, file),
                    type_arguments: generic
                        .map(|generic| generic.arguments.into_iter().map(node_of).collect()),
                    constructor: written_name(constructor, &file.source).resolve(file),
                }
            })
            .collect(),
    }
}

/// Iterator and generator types whose first type argument is the yielded item type.
const GENERATOR_TYPES: &[&str] = &[
    "Iterator",
    "Iterable",
    "Generator",
    "AsyncIterator",
    "AsyncIterable",
    "AsyncGenerator",
];

/// The yielded item type of iterator or generator annotation `return_type_node`.
///
/// It is the first type argument of `Iterator[...]`, `Iterable[...]`, `Generator[...]` or their
/// `Async` forms from `typing`, `typing_extensions` or `collections.abc`, resolved through the
/// file's imports.
#[must_use]
pub fn generator_yield_type<'a>(return_type_node: &AstNode<'a>) -> Option<AstNode<'a>> {
    let file = return_type_node.file;
    let expr = find_expr(return_type_node)?;
    let GenericType {
        base: base_node,
        arguments: type_args,
    } = extract_generic_base_and_args(expr)?;
    let QualifiedName {
        path: base_path,
        terminal: base_terminal,
    } = written_name(base_node, &file.source).resolve(file)?;
    if !is_std_type_constructor(&base_path, &base_terminal, GENERATOR_TYPES) {
        return None;
    }
    let item_type = type_args.first()?;
    Some(AstNode::from_span(
        file,
        span_from_ruff_range(item_type.range()),
    ))
}

/// Returns true if `type_node` (flattened across `|`, `Union[...]`, `Optional[...]` and
/// `Annotated[T, ...]`) includes `None`, `bool` or `Callable`.
#[must_use]
pub fn includes_none_bool_or_callable(type_node: &AstNode<'_>) -> bool {
    let file = type_node.file;
    let Some(expr) = find_expr(type_node) else {
        return false;
    };
    let mut has_none = false;
    let mut branches = Vec::new();
    collect_union_branches_expr(expr, file, &mut has_none, &mut branches);
    has_none
        || branches
            .into_iter()
            .any(|branch| is_bool_or_callable_type_expr(branch, file))
}

/// The element count of the positional tuple that `return_type_node` returns or, for a generator
/// or iterator annotation, yields (see [`PythonReturnTypeUnion::positional_tuple_arity`]).
#[must_use]
pub fn positional_tuple_return_arity(return_type_node: &AstNode<'_>) -> Option<usize> {
    let target = generator_yield_type(return_type_node).unwrap_or(*return_type_node);
    return_type_union(&target).positional_tuple_arity()
}

/// Unwraps outer return-annotation envelopes (`Annotated[T, ...]`, `Awaitable[T]`, and
/// `Coroutine[YieldT, SendT, ReturnT]`).
fn unwrap_return_envelope_expr<'a>(mut current: &'a Expr, file: &ParsedFile) -> &'a Expr {
    while let Some(GenericType {
        base: base_node,
        arguments: type_args,
    }) = extract_generic_base_and_args(current)
        && let Some(QualifiedName {
            path: base_path,
            terminal: base_terminal,
        }) = written_name(base_node, &file.source).resolve(file)
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
            let Some(GenericType {
                base: base_node,
                arguments: type_args,
            }) = extract_generic_base_and_args(expr)
            else {
                branches.push(expr);
                return;
            };
            let Some(QualifiedName {
                path: base_path,
                terminal: base_terminal,
            }) = written_name(base_node, &file.source).resolve(file)
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

/// Returns true if `expr` is the `bool` builtin or a `Callable` constructor (optionally
/// subscripted), resolved through the file's imports.
fn is_bool_or_callable_type_expr(expr: &Expr, file: &ParsedFile) -> bool {
    let base = extract_generic_base_and_args(expr).map_or(expr, |generic| generic.base);
    written_name(base, &file.source)
        .resolve(file)
        .is_some_and(|name| {
            is_std_type_constructor(
                &name.path,
                &name.terminal,
                &[BOOL_CONSTRUCTOR, TYPE_CALLABLE],
            )
        })
}

/// Unwraps outer `Annotated[T, ...]` wrappers from `type_expr`.
fn unwrap_annotated_expr<'a>(mut current: &'a Expr, file: &ParsedFile) -> &'a Expr {
    while let Some(GenericType {
        base: base_node,
        arguments: type_args,
    }) = extract_generic_base_and_args(current)
        && let Some(QualifiedName {
            path: base_path,
            terminal: base_terminal,
        }) = written_name(base_node, &file.source).resolve(file)
        && is_std_type_constructor(&base_path, &base_terminal, &[TYPE_ANNOTATED])
        && let Some(&first_arg) = type_args.first()
    {
        current = first_arg;
    }
    current
}

/// See [`collection_type`].
fn collection_type_expr(expr: &Expr, file: &ParsedFile) -> Option<PythonCollectionType> {
    if !matches!(expr, Expr::Name(_) | Expr::Attribute(_)) {
        return None;
    }
    let QualifiedName {
        path: resolved_path,
        terminal: name,
    } = written_name(expr, &file.source).resolve(file)?;
    let kind = collection_kind(&resolved_path, &name)?;
    let shape = collection_shape(&name);
    let span = span_from_ruff_range(expr.range());
    Some(PythonCollectionType {
        path: file.source[span.start..span.end].to_owned(),
        name,
        kind,
        shape,
    })
}

/// Classifies the import-resolved `(path, terminal)` as a standard-library collection type
/// constructor.
fn collection_kind(path: &str, terminal: &str) -> Option<CollectionKind> {
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
    Some(kind)
}

/// The element access shape of the collection type constructor named `terminal`.
fn collection_shape(terminal: &str) -> CollectionShape {
    match terminal {
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
    }
}

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

#[cfg(test)]
mod tests {
    use super::super::calls::call_callee;
    use super::super::functions::extract_function_signatures;
    use super::super::literals::collect_module_assignments;
    use super::*;
    use crate::diagnostic::Language;
    use rstest::rstest;

    #[rstest]
    #[case::iterator(
        "from typing import Iterator\ndef f() -> Iterator[str]: pass",
        Some("str")
    )]
    #[case::abc_alias(
        "import collections.abc as abc\ndef f() -> abc.AsyncGenerator[bytes, None]: pass",
        Some("bytes")
    )]
    #[case::generator(
        "def f() -> Generator[tuple[int, str], None, None]: pass",
        Some("tuple[int, str]")
    )]
    #[case::not_a_generator("def f() -> list[str]: pass", None)]
    #[case::unsubscripted("def f() -> Iterator: pass", None)]
    #[case::local_class("class Iterator: pass\ndef f() -> Iterator[str]: pass", None)]
    fn generator_yield_type_is_the_first_type_argument(
        #[case] source: &str,
        #[case] expected: Option<&str>,
    ) {
        let file = ParsedFile::new(source, Language::Python);
        let signatures = extract_function_signatures(&file);
        let return_type_node = signatures[0].return_type_node.as_ref().expect("annotation");
        let yield_type = generator_yield_type(return_type_node);
        assert_eq!(
            yield_type
                .as_ref()
                .map(|node| node.text().into_owned())
                .as_deref(),
            expected
        );
    }

    #[rstest]
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
    fn test_collect_collection_types_concrete_mutable_matrix_a(
        #[case] source: &str,
        #[case] expected_per_param: &[&[&str]],
    ) {
        let file = ParsedFile::new(source, Language::Python);
        let sigs = extract_function_signatures(&file);
        let actual: Vec<Vec<String>> = sigs[0]
            .parameters
            .iter()
            .map(|parameter| {
                let type_node = parameter.type_node.as_ref().expect("param should be typed");
                collect_collection_types(type_node, AnnotationTraversalDepth::CovariantPositions)
                    .into_iter()
                    .filter(|collection_type| {
                        collection_type.kind == CollectionKind::ConcreteMutable
                    })
                    .map(|collection_type| collection_type.path)
                    .collect()
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
        assert_eq!(sigs.len(), 1);
        assert_eq!(actual, expected);
    }

    /// Collects the top-level collection types of the annotation of the first parameter of the
    /// first function in `source`, as `(path, kind, shape)`.
    fn top_level_collection_types(source: &str) -> Vec<(String, CollectionKind, CollectionShape)> {
        let file = ParsedFile::new(source, Language::Python);
        let sigs = extract_function_signatures(&file);
        let type_node = sigs[0].parameters[0]
            .type_node
            .as_ref()
            .expect("param should be typed");
        collect_collection_types(type_node, AnnotationTraversalDepth::TransparentWrappersOnly)
            .into_iter()
            .map(|collection_type| {
                (
                    collection_type.path,
                    collection_type.kind,
                    collection_type.shape,
                )
            })
            .collect()
    }

    #[rstest]
    #[case::list(
        "def f(x: list[int]): pass",
        "list",
        CollectionKind::ConcreteMutable,
        CollectionShape::Sequence
    )]
    #[case::typing_dict(
        "def f(x: typing.Dict[str, int]): pass",
        "typing.Dict",
        CollectionKind::ConcreteMutable,
        CollectionShape::Mapping
    )]
    #[case::unqualified_set(
        "def f(x: Set[int]): pass",
        "Set",
        CollectionKind::ConcreteMutable,
        CollectionShape::Set
    )]
    #[case::deque(
        "def f(x: collections.deque[int]): pass",
        "collections.deque",
        CollectionKind::ConcreteMutable,
        CollectionShape::Sequence
    )]
    #[case::counter(
        "def f(x: collections.Counter[str]): pass",
        "collections.Counter",
        CollectionKind::ConcreteMutable,
        CollectionShape::Mapping
    )]
    #[case::mutable_sequence(
        "def f(x: MutableSequence[int]): pass",
        "MutableSequence",
        CollectionKind::AbstractMutable,
        CollectionShape::Sequence
    )]
    #[case::mutable_mapping(
        "def f(x: collections.abc.MutableMapping[str, int]): pass",
        "collections.abc.MutableMapping",
        CollectionKind::AbstractMutable,
        CollectionShape::Mapping
    )]
    #[case::mutable_set(
        "def f(x: typing.MutableSet[int] | None): pass",
        "typing.MutableSet",
        CollectionKind::AbstractMutable,
        CollectionShape::Set
    )]
    #[case::sequence(
        "def f(x: Optional[collections.abc.Sequence[int]]): pass",
        "collections.abc.Sequence",
        CollectionKind::AbstractReadOnly,
        CollectionShape::Sequence
    )]
    #[case::mapping(
        "def f(x: Mapping[str, Sequence[int]]): pass",
        "Mapping",
        CollectionKind::AbstractReadOnly,
        CollectionShape::Mapping
    )]
    #[case::abc_set(
        "def f(x: collections.abc.Set[int]): pass",
        "collections.abc.Set",
        CollectionKind::AbstractReadOnly,
        CollectionShape::Set
    )]
    #[case::imported_abc_set(
        "from collections.abc import Set\ndef f(x: Set[int]): pass",
        "Set",
        CollectionKind::AbstractReadOnly,
        CollectionShape::Set
    )]
    #[case::collection(
        "def f(x: typing.Collection[int]): pass",
        "typing.Collection",
        CollectionKind::AbstractReadOnly,
        CollectionShape::Iterable
    )]
    #[case::iterable(
        "def f(x: Iterable[int]): pass",
        "Iterable",
        CollectionKind::AbstractReadOnly,
        CollectionShape::Iterable
    )]
    #[case::tuple(
        "def f(x: tuple[int, ...]): pass",
        "tuple",
        CollectionKind::Immutable,
        CollectionShape::Sequence
    )]
    #[case::frozenset(
        "def f(x: frozenset[int]): pass",
        "frozenset",
        CollectionKind::Immutable,
        CollectionShape::Set
    )]
    fn test_collection_type_taxonomy(
        #[case] source: &str,
        #[case] path: &str,
        #[case] kind: CollectionKind,
        #[case] shape: CollectionShape,
    ) {
        assert_eq!(
            top_level_collection_types(source),
            [(path.to_string(), kind, shape)]
        );
    }

    #[rstest]
    #[case::nested_not_collected("def f(x: Sequence[MutableSequence[int]]): pass", &["Sequence"])]
    #[case::unknown_module_ignored("def f(x: mylib.MutableSequence[int]): pass", &[])]
    #[case::union_in_order("def f(x: dict[str, int] | list[int] | dict[str, str]): pass", &["dict", "list"])]
    fn test_collect_top_level_collection_types(#[case] source: &str, #[case] expected: &[&str]) {
        let paths: Vec<String> = top_level_collection_types(source)
            .into_iter()
            .map(|(path, _, _)| path)
            .collect();
        assert_eq!(paths, expected);
    }

    #[rstest]
    #[case::list_display("[1]", Some(("list", CollectionShape::Sequence)))]
    #[case::set_comprehension("{x for x in y}", Some(("set", CollectionShape::Set)))]
    #[case::empty_dict_display("{}", Some(("dict", CollectionShape::Mapping)))]
    #[case::generic_constructor_call("list[str]()", Some(("list", CollectionShape::Sequence)))]
    #[case::qualified_constructor_call(
        "collections.Counter()",
        Some(("collections.Counter", CollectionShape::Mapping))
    )]
    #[case::tuple_display("(1, 2)", None)]
    #[case::unknown_call("make()", None)]
    fn test_collection_display_and_call_callee_type(
        #[case] value: &str,
        #[case] expected: Option<(&str, CollectionShape)>,
    ) {
        let file = ParsedFile::new(&format!("VALUE = {value}"), Language::Python);
        let assignments = collect_module_assignments(&file);
        let value = assignments[0].value.as_ref().expect("value");
        let built = collection_display(value)
            .or_else(|| call_callee(value).and_then(|callee| collection_type(&callee)));
        assert_eq!(
            built
                .as_ref()
                .map(|collection| (collection.path.as_str(), collection.shape)),
            expected
        );
    }

    #[rstest]
    #[case::simple_union("def f() -> str | int: pass", false, &["str", "int"])]
    #[case::optional_shorthand("def f() -> Optional[list[int]]: pass", true, &["list[int]"])]
    #[case::pep604_none("def f() -> Sequence[str] | None: pass", true, &["Sequence[str]"])]
    #[case::none_first("def f() -> None | set[int]: pass", true, &["set[int]"])]
    #[case::multiple_branches_with_none("def f() -> list[int] | set[str] | None: pass", true, &["list[int]", "set[str]"])]
    #[case::awaitable_coroutine_envelopes(
        "def f() -> Awaitable[Coroutine[Any, Any, Mapping[str, int] | None]]: pass",
        true,
        &["Mapping[str, int]"]
    )]
    #[case::annotated_wrapper("def f() -> Annotated[tuple[int, ...] | None, 'meta']: pass", true, &["tuple[int, ...]"])]
    fn test_return_type_union(
        #[case] source: &str,
        #[case] expected_has_none: bool,
        #[case] expected_branches: &[&str],
    ) {
        let file = ParsedFile::new(source, Language::Python);
        let signatures = extract_function_signatures(&file);
        let return_type_node = signatures[0]
            .return_type_node
            .as_ref()
            .expect("function should have return annotation");
        let union = return_type_union(return_type_node);
        let branches: Vec<_> = union
            .branches
            .iter()
            .map(|branch| branch.node.text())
            .collect();
        assert_eq!(union.has_none, expected_has_none);
        assert_eq!(branches, expected_branches);
    }

    #[test]
    fn test_return_type_branch_generic_type() {
        let file = ParsedFile::new("def f() -> tuple[int, ...]: pass", Language::Python);
        let signatures = extract_function_signatures(&file);
        let return_type_node = signatures[0].return_type_node.as_ref().unwrap();
        let union = return_type_union(return_type_node);
        let branch = &union.branches[0];
        let collection = branch.collection.as_ref().expect("collection type");
        let args = branch.type_arguments.as_ref().expect("generic type");
        assert_eq!(collection.name, "tuple");
        assert_eq!(args.len(), 2);
        assert_eq!(args[0].text(), "int");
        assert_eq!(args[1].text(), "...");
    }
}
