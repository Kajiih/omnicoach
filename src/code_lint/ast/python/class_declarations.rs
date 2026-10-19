//! Python class declarations: name, base classes, class kinds, decorators, and docstring.

use super::decorators::{DecoratorInfo, extract_decorators_from_slice};
use super::docstrings::{PythonDocstring, docstring_of};
use super::names::written_name;
use crate::code_lint::ast::{AstNode, ParsedFile, span_from_ruff_range};
use ruff_python_ast::{Expr, StmtClassDef};
use ruff_text_size::Ranged as _;

const DATACLASS_DECORATOR_PATHS: &[&str] = &["dataclass", "dataclasses.dataclass"];
const PROTOCOL_PATHS: &[&str] = &["Protocol", "typing.Protocol", "typing_extensions.Protocol"];
const ABC_PATHS: &[&str] = &["ABC", "abc.ABC"];
const NAMED_TUPLE_PATHS: &[&str] = &[
    "NamedTuple",
    "typing.NamedTuple",
    "typing_extensions.NamedTuple",
];
const STRUCTURAL_MARKER_PATHS: &[&str] = &[
    "object",
    "builtins.object",
    "Generic",
    "typing.Generic",
    "typing_extensions.Generic",
    "Protocol",
    "typing.Protocol",
    "typing_extensions.Protocol",
    "ABC",
    "abc.ABC",
];
const ENUM_BASE_PATHS: &[&str] = &[
    "Enum",
    "IntEnum",
    "StrEnum",
    "Flag",
    "IntFlag",
    "ReprEnum",
    "enum.Enum",
    "enum.IntEnum",
    "enum.StrEnum",
    "enum.Flag",
    "enum.IntFlag",
    "enum.ReprEnum",
];
const FIELD_SYNTHESIZING_DECORATOR_PATHS: &[&str] = &[
    "dataclass",
    "dataclasses.dataclass",
    "pydantic.dataclasses.dataclass",
    "define",
    "frozen",
    "mutable",
    "s",
    "attrs",
    "attr.define",
    "attr.frozen",
    "attr.mutable",
    "attr.s",
    "attr.attrs",
    "attr.dataclass",
    "attrs.define",
    "attrs.frozen",
    "attrs.mutable",
    "attrs.s",
    "attrs.attrs",
    "attrs.dataclass",
];

/// Represents a base class expression in a Python class definition.
#[derive(Clone)]
pub struct PythonBaseClass {
    /// Base class identifier or dotted path text (e.g. `"Protocol"`, `"abc.ABC"`).
    pub name: String,
    /// Canonical path of the unsubscripted base class resolved through
    /// [`crate::code_lint::ast::resolve_name`], or `None` when the first segment is defined in the
    /// file.
    resolved_path: Option<String>,
}

impl PythonBaseClass {
    /// Base class name with any generic type argument subscript (`[...]`) stripped.
    #[must_use]
    pub fn unsubscripted_name(&self) -> &str {
        self.name
            .split_once('[')
            .map_or(self.name.as_str(), |(base, _)| base.trim())
    }

    /// Returns true if this base class is a structural marker rather than a supertype:
    /// `object`, `Generic`, `Protocol`, or `ABC` itself (optionally subscripted, qualified, or
    /// imported under an alias).
    #[must_use]
    pub fn is_structural_marker(&self) -> bool {
        self.resolved_path
            .as_deref()
            .is_some_and(|path| STRUCTURAL_MARKER_PATHS.contains(&path))
    }
}

/// Declaration header and metadata of a Python `class` statement (`class C(B, metaclass=M):`),
/// including its decorators and docstring.
#[derive(Clone)]
pub struct PythonClassDeclaration<'a> {
    /// Class name identifier text.
    pub name: String,
    /// Class name AST node.
    pub name_node: AstNode<'a>,
    /// Base classes from `class Foo(Base1, Base2):`.
    pub bases: Vec<PythonBaseClass>,
    /// The class docstring, if the class body starts with one.
    pub docstring: Option<PythonDocstring<'a>>,
    /// Parsed decorators on this class definition.
    pub(super) decorators: Vec<DecoratorInfo>,
    /// The parsed definition, for analyses of the body.
    pub(super) definition: &'a StmtClassDef,
}

impl<'a> PythonClassDeclaration<'a> {
    /// Builds [`PythonClassDeclaration`] from a parsed `class` statement.
    pub(super) fn from_ast(class_def: &'a StmtClassDef, file: &'a ParsedFile) -> Self {
        let bases = class_def
            .bases()
            .iter()
            .map(|base| {
                let base_span = span_from_ruff_range(base.range());
                PythonBaseClass {
                    name: file.source[base_span.start..base_span.end].to_string(),
                    resolved_path: resolved_base_class_path(base, file),
                }
            })
            .collect();
        let decorators = extract_decorators_from_slice(&class_def.decorator_list, file);
        Self {
            name: class_def.name.id.to_string(),
            name_node: AstNode::from_span(file, span_from_ruff_range(class_def.name.range)),
            bases,
            docstring: docstring_of(&class_def.body, file),
            decorators,
            definition: class_def,
        }
    }
}

impl PythonClassDeclaration<'_> {
    /// Returns true if a direct base class resolves to an `enum` base (`Enum`, `IntEnum`,
    /// `StrEnum`, `Flag`, `IntFlag`, `ReprEnum`). Subclasses of a project enum are not
    /// recognized.
    #[must_use]
    pub fn is_enum(&self) -> bool {
        resolved_base_class_paths(self.definition, self.name_node.file)
            .iter()
            .any(|path| ENUM_BASE_PATHS.contains(&path.as_str()))
    }

    /// Returns true if a direct base class resolves to `typing.Protocol` or
    /// `typing_extensions.Protocol` (optionally subscripted), making the class a `Protocol`
    /// rather than an implementation of one.
    #[must_use]
    pub fn is_protocol(&self) -> bool {
        is_protocol_class(self.definition, self.name_node.file)
    }

    /// Returns true if a direct base class resolves to `abc.ABC` or the class declares
    /// `metaclass=ABCMeta`.
    #[must_use]
    pub fn is_abc(&self) -> bool {
        is_abc_class(self.definition, self.name_node.file)
    }

    /// Returns true if a direct base class resolves to `typing.NamedTuple` or
    /// `typing_extensions.NamedTuple`.
    #[must_use]
    pub fn is_named_tuple(&self) -> bool {
        resolved_base_class_paths(self.definition, self.name_node.file)
            .iter()
            .any(|path| NAMED_TUPLE_PATHS.contains(&path.as_str()))
    }

    /// Returns true if the class carries a `@dataclass` or `@dataclasses.dataclass` decorator
    /// that does not pass the keyword argument `key`.
    #[must_use]
    pub fn is_dataclass_missing_arg(&self, key: &str) -> bool {
        self.dataclass_decorator()
            .is_some_and(|decorator| !decorator.has_arg(key))
    }

    /// The `@dataclass` or `@dataclasses.dataclass` decorator, resolved through the file's
    /// imports, if the class carries one.
    fn dataclass_decorator(&self) -> Option<&DecoratorInfo> {
        self.decorators
            .iter()
            .find(|decorator| decorator.matches_any(DATACLASS_DECORATOR_PATHS))
    }
}

/// Returns true if a Python `StmtClassDef` inherits from `TypedDict`.
pub(super) fn is_typed_dict_class(class_def: &StmtClassDef, file: &ParsedFile) -> bool {
    resolved_base_class_paths(class_def, file)
        .iter()
        .any(|path| {
            matches!(
                path.as_str(),
                "TypedDict" | "typing.TypedDict" | "typing_extensions.TypedDict"
            )
        })
}

/// Returns true if a Python `StmtClassDef` inherits from `Protocol` or `ABC` or declares
/// `metaclass=ABCMeta`.
pub(super) fn is_protocol_or_abc_class(class_def: &StmtClassDef, file: &ParsedFile) -> bool {
    is_protocol_class(class_def, file) || is_abc_class(class_def, file)
}

/// Returns true if `class_def` synthesizes constructor fields from class-body annotations
/// (`@dataclass`, `attrs` decorators, or Pydantic `BaseModel` subclasses).
pub(super) fn is_field_synthesizing_class(class_def: &StmtClassDef, file: &ParsedFile) -> bool {
    let has_field_decorator = extract_decorators_from_slice(&class_def.decorator_list, file)
        .iter()
        .any(|decorator| decorator.matches_any(FIELD_SYNTHESIZING_DECORATOR_PATHS));
    has_field_decorator
        || resolved_base_class_paths(class_def, file)
            .iter()
            .any(|path| {
                matches!(
                    path.as_str(),
                    "BaseModel" | "pydantic.BaseModel" | "pydantic.main.BaseModel"
                )
            })
}

/// Returns true if a Python `StmtClassDef` inherits from `Protocol`.
fn is_protocol_class(class_def: &StmtClassDef, file: &ParsedFile) -> bool {
    resolved_base_class_paths(class_def, file)
        .iter()
        .any(|path| PROTOCOL_PATHS.contains(&path.as_str()))
}

/// Returns true if a Python `StmtClassDef` inherits from `ABC` or declares `metaclass=ABCMeta`.
fn is_abc_class(class_def: &StmtClassDef, file: &ParsedFile) -> bool {
    let has_abc_metaclass = class_def.keywords().iter().any(|keyword| {
        keyword
            .arg
            .as_ref()
            .is_some_and(|arg| arg.id == "metaclass")
            && written_name(&keyword.value, &file.source)
                .resolve(file)
                .is_some_and(|name| matches!(name.path.as_str(), "ABCMeta" | "abc.ABCMeta"))
    });
    has_abc_metaclass
        || resolved_base_class_paths(class_def, file)
            .iter()
            .any(|path| ABC_PATHS.contains(&path.as_str()))
}

/// Returns the import-resolved paths of each positional base class of a Python `StmtClassDef`,
/// excluding base classes defined in the file.
fn resolved_base_class_paths(class_def: &StmtClassDef, file: &ParsedFile) -> Vec<String> {
    class_def
        .bases()
        .iter()
        .filter_map(|base| resolved_base_class_path(base, file))
        .collect()
}

/// Resolves the canonical path of a base class expression `base`, unwrapping generic subscripts
/// (`Protocol[T]` yields `"typing.Protocol"` when imported from `typing`).
fn resolved_base_class_path(base: &Expr, file: &ParsedFile) -> Option<String> {
    let base_expr = if let Expr::Subscript(subscript) = base {
        subscript.value.as_ref()
    } else {
        base
    };
    written_name(base_expr, &file.source)
        .resolve(file)
        .map(|name| name.path)
}
