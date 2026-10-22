//! Function return contracts ([`ReturnShape`], [`FunctionReturn`]) and predicate-name
//! classification.

use super::tree::{AstNode, ParsedFile};
use crate::diagnostic::Language;

/// Verbs that read as a yes/no question alone or as a prefix, such as `text.contains(word)` or
/// `contains_key`.
const PREDICATE_VERBS: &[&str] = &["contains", "starts_with", "ends_with", "matches"];

/// Words that read as a yes/no question only as a prefix, such as `is_valid`: a bare `is`,
/// `requires` or `needs` names a value as often as a question (`Distribution.requires`).
const PREDICATE_PREFIXES: &[&str] = &[
    "is", "has", "can", "should", "supports", "allows", "requires", "needs",
];

/// What a function returns, as far as its signature (or, for an unannotated Python function, its
/// own body) shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReturnShape {
    /// No value: Rust `()` or no return type (also as the success type of `Result` / `Poll`);
    /// Python `-> None`, or an unannotated function whose body neither returns a value nor yields.
    Unit,
    /// A boolean: `bool`, Python `TypeGuard[T]` / `TypeIs[T]`, and Rust `Result<bool, E>` /
    /// `Poll<bool>`.
    Boolean,
    /// A fixed-length tuple of at least two elements, alone or inside an optional, a union, a
    /// Rust `Result` / `Poll` / pointer wrapper, or a Python generator's yield type.
    PositionalTuple {
        /// The number of elements (the largest when several tuples are returned).
        arity: usize,
    },
    /// An unannotated Python function that returns a value, or whose body is a stub.
    Unknown,
    /// Any other return type.
    Other,
}

/// The return contract of a function or method whose signature its author chose.
///
/// Signatures imposed from outside are not collected: Python `@override`, `@overload`,
/// `@abstractmethod`, fixtures, `@register`, data-model dunders and `Protocol` / `ABC` members;
/// Rust trait items, trait implementations and inline tests.
#[derive(Clone)]
pub struct FunctionReturn<'a> {
    /// Function identifier name.
    pub name: String,
    /// Identifier AST node of the function declaration.
    pub name_node: AstNode<'a>,
    /// The declared return type (`-> T`), if any.
    pub declared_type: Option<AstNode<'a>>,
    /// What the function returns.
    pub shape: ReturnShape,
}

impl FunctionReturn<'_> {
    /// The declared return type as written, or the language's unit type (`None` in Python, `()`
    /// in Rust) when there is none.
    #[must_use]
    pub fn declared_type_text(&self) -> String {
        self.declared_type.map_or_else(
            || {
                match self.name_node.lang() {
                    Language::Python => "None",
                    Language::Rust => "()",
                }
                .to_owned()
            },
            |declared_type| declared_type.text().into_owned(),
        )
    }
}

/// Collects the return contract of every function and method in `file` whose signature its
/// author chose, in source order.
#[must_use]
pub fn collect_function_returns(file: &ParsedFile) -> Vec<FunctionReturn<'_>> {
    dispatch_lang!(file.lang(), collect_function_returns(file))
}

/// Returns true if function name `name` reads as a yes/no question.
///
/// Without leading underscores, the name is `contains`, `starts_with`, `ends_with` or `matches`,
/// or one of those or `is`, `has`, `can`, `should`, `supports`, `allows`, `requires`, `needs`
/// followed by `_` and more (`is_valid`, `_has_key`, `contains_key`).
#[must_use]
pub fn is_predicate_name(name: &str) -> bool {
    let bare = name.trim_start_matches('_');
    PREDICATE_VERBS.contains(&bare)
        || PREDICATE_VERBS
            .iter()
            .chain(PREDICATE_PREFIXES)
            .any(|prefix| {
                bare.strip_prefix(prefix)
                    .and_then(|rest| rest.strip_prefix('_'))
                    .is_some_and(|rest| !rest.is_empty())
            })
}
