//! Parameter type contracts ([`ParameterAnnotation`]).

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

/// Collects the parameter type contracts of every function and method in `file` whose signature
/// its author chose, in source order.
#[must_use]
pub fn collect_parameter_annotations(file: &ParsedFile) -> Vec<ParameterAnnotation<'_>> {
    dispatch_lang!(file.lang(), collect_parameter_annotations(file))
}
