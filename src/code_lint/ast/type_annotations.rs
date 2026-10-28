//! Parameter and attribute type contracts ([`ParameterAnnotation`], [`AttributeAnnotation`]) and
//! structural type-annotation shape extraction ([`TypeAnnotationCandidate`], [`TypeShape`]).

use super::imports::QualifiedName;
use super::returns::collect_function_returns;
use super::tree::{AstNode, ParsedFile};

/// The type contract of a function or method parameter whose signature its author chose.
///
/// Signatures imposed from outside are not collected: Python `@override`, `@overload`,
/// `@abstractmethod`, fixtures, `@register`, property setters, data-model dunders other than
/// `__init__`, `__new__` and `__call__`, and `Protocol` / `ABC` members; Rust trait items, trait
/// implementations and inline tests. Python receiver (`self`, `cls`) and variadic (`*args`,
/// `**kwargs`) parameters and Rust `self` receivers are not collected.
#[derive(Clone)]
pub struct ParameterAnnotation<'a> {
    /// Parameter identifier or pattern text.
    pub name: String,
    /// Enclosing function or method name.
    pub function: String,
    /// The declared parameter type annotation node.
    pub type_node: AstNode<'a>,
    /// The element count if the parameter type is a non-empty fixed-length positional tuple
    /// (alone or inside an optional, union, qualifier, or Rust reference / pointer wrapper).
    pub positional_tuple_arity: Option<usize>,
}

/// The type contract of a class attribute (Python) or named `struct` / `enum` field (Rust).
///
/// Members of Python `Protocol` and `ABC` interface classes and Rust inline tests are not
/// collected.
#[derive(Clone)]
pub struct AttributeAnnotation<'a> {
    /// Attribute or field identifier name.
    pub name: String,
    /// Enclosing class, `struct`, or `enum` name.
    pub class: String,
    /// The declared attribute or field type annotation node.
    pub type_node: AstNode<'a>,
    /// The element count if the attribute or field type is a non-empty fixed-length positional
    /// tuple (alone or inside an optional, union, qualifier, or Rust reference / pointer wrapper).
    pub positional_tuple_arity: Option<usize>,
}

/// A structural representation of a type annotation expression for pattern matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeShape {
    /// The `None` type in Python (`None`, `Optional[...]`).
    None,
    /// The literal ellipsis `...` in a Python type subscript (`tuple[int, ...]`).
    Ellipsis,
    /// A union of alternative type branches (`A | B`, `Union[A, B]`, `Optional[A]`).
    Union(Vec<Self>),
    /// A tuple type (`tuple[A, B]` or `typing.Tuple[A, B]` in Python, `(A, B)` in Rust).
    Tuple(Vec<Self>),
    /// A reference type (`&T` or `&mut T` in Rust).
    Reference {
        /// True for `&mut T`.
        mutable: bool,
        /// The referenced inner type.
        inner: Box<Self>,
    },
    /// A named or generic type (`Width`, `Iterator[Dimensions]`, `Coroutine[object, object, T]`,
    /// `Option<Bound>`).
    Named {
        /// The import-resolved canonical path of the constructor (`typing.List`, `anyio.Event`),
        /// or `None` when the constructor's first segment is defined in the file (`Local`).
        resolved_path: Option<String>,
        /// The last segment of `resolved_path` (or of the written name when `Local`).
        terminal: String,
        /// Generic / subscript type arguments in source order (`[]` when unparameterized).
        arguments: Vec<Self>,
    },
    /// Any other type syntax (`Callable[[A], B]`, array/slice `[T]`, literal type).
    Other,
}

impl TypeShape {
    /// Builds a [`Self::Named`] by resolving `written` through the imports of `file`.
    pub(super) fn named(written: QualifiedName, arguments: Vec<Self>, file: &ParsedFile) -> Self {
        let fallback_terminal = written.terminal.clone();
        let (resolved_path, terminal) = written
            .resolve(file)
            .map_or((None, fallback_terminal), |resolved| {
                (Some(resolved.path), resolved.terminal)
            });
        Self::Named {
            resolved_path,
            terminal,
            arguments,
        }
    }
}

/// A type annotation subnode inside a parameter, return, or attribute/field annotation, together
/// with its enclosing full annotation text and its structural [`TypeShape`].
#[derive(Clone)]
pub struct TypeAnnotationCandidate<'a> {
    /// The AST node of this type expression (or sub-expression).
    pub node: AstNode<'a>,
    /// The source text of the full enclosing parameter, return, or attribute/field annotation.
    pub expression: String,
    /// The structural shape of `node`.
    pub shape: TypeShape,
}

/// Collects the parameter type contracts of every function and method in `file` whose signature
/// its author chose, in source order.
#[must_use]
pub fn collect_parameter_annotations(file: &ParsedFile) -> Vec<ParameterAnnotation<'_>> {
    dispatch_lang!(file.lang(), collect_parameter_annotations(file))
}

/// Collects the attribute / field type contracts of every non-interface class, `struct`, and
/// `enum` in `file`, in source order.
#[must_use]
pub fn collect_attribute_annotations(file: &ParsedFile) -> Vec<AttributeAnnotation<'_>> {
    dispatch_lang!(file.lang(), collect_attribute_annotations(file))
}

/// Collects all type annotation nodes and nested type argument subnodes across parameter, return,
/// and attribute/field annotations in `file`, in source order.
#[must_use]
pub fn collect_type_annotation_candidates(file: &ParsedFile) -> Vec<TypeAnnotationCandidate<'_>> {
    let mut roots: Vec<AstNode<'_>> = collect_parameter_annotations(file)
        .into_iter()
        .map(|parameter| parameter.type_node)
        .chain(
            collect_function_returns(file)
                .into_iter()
                .filter_map(|function| function.declared_type),
        )
        .chain(
            collect_attribute_annotations(file)
                .into_iter()
                .map(|attribute| attribute.type_node),
        )
        .collect();
    roots.sort_by_key(AstNode::span);
    dispatch_lang!(
        file.lang(),
        collect_type_annotation_candidates(file, &roots)
    )
}
