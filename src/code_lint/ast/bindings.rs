//! Binding definitions and binding-kind classification across languages.

use super::tree::{AstNode, ParsedFile, dispatch_lang};

/// What introduced a binding name, recorded by the walk that collects the binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingKind {
    /// A name introduced by an import (`import x`, `from m import y`, `use a::b`).
    Import,
    /// The name of a member mandated by a contract: a function, type alias, or constant defined
    /// in a Rust `impl Trait for Type`, or a Python method decorated with `@override`.
    ContractMember,
    /// The declared name of a function, class, struct, enum, trait, or type alias that is not a
    /// [`Self::ContractMember`].
    StructuralDefinition,
    /// Any other binding: a variable, parameter, constant, static, field, or attribute.
    Value,
}

/// A binding definition name and what introduced it.
#[derive(Clone, Copy)]
pub struct Binding<'a> {
    /// The binding identifier node.
    pub node: AstNode<'a>,
    /// What introduced the binding.
    pub kind: BindingKind,
}

/// Appends each node that `extract` collects to `bindings` as a binding of `kind`.
pub(super) fn push_bindings<'a>(
    bindings: &mut Vec<Binding<'a>>,
    kind: BindingKind,
    extract: impl FnOnce(&mut Vec<AstNode<'a>>),
) {
    let mut nodes = Vec::new();
    extract(&mut nodes);
    bindings.extend(nodes.into_iter().map(|node| Binding { node, kind }));
}

/// Collects all binding definitions from a parsed file, in source order.
#[must_use]
pub fn collect_bindings(file: &ParsedFile) -> Vec<Binding<'_>> {
    let cached = file.bindings.get_or_init(|| {
        dispatch_lang!(file.lang(), collect_bindings(file))
            .into_iter()
            .map(|binding| (binding.node.span(), binding.kind))
            .collect()
    });
    cached
        .iter()
        .map(|&(span, kind)| Binding {
            node: AstNode::from_span(file, span),
            kind,
        })
        .collect()
}
