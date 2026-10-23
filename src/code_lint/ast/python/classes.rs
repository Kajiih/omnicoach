//! Python class model: the class declaration (name, bases, decorators, docstring, and the kinds
//! they declare), its members and hierarchy, and a function's role in its enclosing class.

use super::annotations::is_bare_final_annotation_expr;
use super::decorators::{DecoratorInfo, extract_decorators_from_slice};
use super::docstrings::{PythonDocstring, docstring_of};
use super::effects::in_place_mutated_receiver_expr;
use super::functions::{
    CLS_RECEIVER, PythonFunctionSignature, SELF_RECEIVER, function_signature,
    method_receiver_name_ast,
};
use super::names::written_name;
use super::tree::direct_function_definitions;
use crate::code_lint::ast::call_clusters::PlacementAnchor;
use crate::code_lint::ast::tree::{AstNode, ParsedFile, span_from_ruff_range};
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_expr, walk_stmt};
use ruff_python_ast::{Expr, Stmt, StmtAnnAssign, StmtClassDef, StmtFunctionDef};
use ruff_text_size::Ranged as _;
use std::collections::HashSet;

const INIT_METHOD: &str = "__init__";
/// Methods whose `self.<name>` assignments create instance attributes.
const INITIALIZER_METHODS: &[&str] = &[
    INIT_METHOD,
    "__post_init__",
    "__attrs_post_init__",
    "model_post_init",
];
const INIT_VAR_PATHS: &[&str] = &["InitVar", "dataclasses.InitVar"];
const DATACLASS_DECORATOR_PATHS: &[&str] = &["dataclass", "dataclasses.dataclass"];
const PROTOCOL_PATHS: &[&str] = &["Protocol", "typing.Protocol", "typing_extensions.Protocol"];
const ABC_PATHS: &[&str] = &["ABC", "abc.ABC"];
const ABC_METACLASS_PATHS: &[&str] = &["ABCMeta", "abc.ABCMeta"];
const NAMED_TUPLE_PATHS: &[&str] = &[
    "NamedTuple",
    "typing.NamedTuple",
    "typing_extensions.NamedTuple",
];
const TYPED_DICT_PATHS: &[&str] = &[
    "TypedDict",
    "typing.TypedDict",
    "typing_extensions.TypedDict",
];
const PYDANTIC_BASE_MODEL_PATHS: &[&str] =
    &["BaseModel", "pydantic.BaseModel", "pydantic.main.BaseModel"];
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
    pub(in crate::code_lint) name: String,
    /// Canonical path of the unsubscripted base class resolved through
    /// [`crate::code_lint::ast::resolve_name`], or `None` when the first segment is defined in the
    /// file.
    resolved_path: Option<String>,
}

impl PythonBaseClass {
    /// Base class name with any generic type argument subscript (`[...]`) stripped.
    #[must_use]
    pub(in crate::code_lint) fn unsubscripted_name(&self) -> &str {
        self.name
            .split_once('[')
            .map_or(self.name.as_str(), |(base, _)| base.trim())
    }

    /// Returns true if this base class is a structural marker rather than a supertype:
    /// `object`, `Generic`, `Protocol`, or `ABC` itself (optionally subscripted, qualified, or
    /// imported under an alias).
    #[must_use]
    pub(in crate::code_lint) fn is_structural_marker(&self) -> bool {
        self.resolved_path
            .as_deref()
            .is_some_and(|path| STRUCTURAL_MARKER_PATHS.contains(&path))
    }
}

/// Structured representation of a Python class definition: its declaration (name, bases,
/// decorators, docstring), the kinds that declaration makes it, and its members and hierarchy.
///
/// Every class kind is derived from `bases` and `decorators`, which are resolved through the
/// file's imports once, when the class is extracted.
#[derive(Clone)]
pub struct PythonClassInfo<'a> {
    /// Class name identifier text.
    pub(in crate::code_lint) name: String,
    /// Class name AST node.
    pub(in crate::code_lint) name_node: AstNode<'a>,
    /// Base classes from `class Foo(Base1, Base2):`.
    pub(in crate::code_lint) bases: Vec<PythonBaseClass>,
    /// The class docstring, if the class body starts with one.
    pub(in crate::code_lint) docstring: Option<PythonDocstring<'a>>,
    /// Parsed decorators on this class definition.
    decorators: Vec<DecoratorInfo>,
    /// The parsed definition, for analyses of the body.
    definition: &'a StmtClassDef,
}

impl<'a> PythonClassInfo<'a> {
    /// Builds [`PythonClassInfo`] from a parsed `class` statement.
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
        Self {
            name: class_def.name.id.to_string(),
            name_node: AstNode::from_span(file, span_from_ruff_range(class_def.name.range)),
            bases,
            docstring: docstring_of(&class_def.body, file),
            decorators: extract_decorators_from_slice(&class_def.decorator_list, file),
            definition: class_def,
        }
    }

    /// The functions defined directly in the class body (not nested deeper), in source order.
    #[must_use]
    pub(in crate::code_lint) fn methods(&self) -> Vec<PythonFunctionSignature<'a>> {
        let file = self.name_node.file;
        self.definition
            .body
            .iter()
            .filter_map(|statement| match statement {
                Stmt::FunctionDef(func_def) => Some(function_signature(
                    func_def,
                    file,
                    Some(self.definition),
                    None,
                )),
                _ => None,
            })
            .collect()
    }

    /// The `__init__` defined directly in the class body, skipping `@overload` stubs; the last
    /// one when it is redefined.
    #[must_use]
    pub(in crate::code_lint) fn constructor(&self) -> Option<PythonFunctionSignature<'a>> {
        self.methods()
            .into_iter()
            .rfind(|method| method.name == INIT_METHOD && !method.is_overload())
    }

    /// The first definition of each attribute name the class creates, in source order.
    ///
    /// Definitions are the targets of class-body assignments (`x = …`, `x: T = …`, `x: T`,
    /// including multiple and tuple-unpacking targets) and of `<receiver>.x = …` /
    /// `<receiver>.x: T = …` assignments directly in an instance initializer (`__init__`,
    /// `__post_init__`, `__attrs_post_init__`, `model_post_init`), where the receiver is the
    /// method's `self` parameter. Nested functions, lambdas and classes are not read, and
    /// other methods are not read. Private and dunder names are included.
    #[must_use]
    pub(in crate::code_lint) fn attribute_definitions(&self) -> Vec<PythonAttributeDefinition<'a>> {
        let file = self.name_node.file;
        let mut collector = AttributeCollector {
            file,
            seen: HashSet::new(),
            definitions: Vec::new(),
        };
        for statement in &self.definition.body {
            match statement {
                Stmt::Assign(assign) => {
                    for target in &assign.targets {
                        collector.add_class_body_targets(target, None);
                    }
                }
                Stmt::AnnAssign(annotated) => {
                    collector
                        .add_class_body_targets(&annotated.target, Some(&annotated.annotation));
                }
                Stmt::FunctionDef(function_def)
                    if INITIALIZER_METHODS.contains(&function_def.name.id.as_str()) =>
                {
                    if let Some(receiver) = method_receiver_name_ast(function_def, false, file) {
                        collector.add_initializer_targets(function_def, &receiver);
                    }
                }
                _ => {}
            }
        }
        collector.definitions
    }

    /// The annotated attributes the class declares, in source order: class-body annotations
    /// (`x: T`), then `self.<name>: T` annotations in `__init__`, each recording whether a method
    /// of the class mutates it in place.
    #[must_use]
    pub fn annotated_attributes(&self) -> Vec<PythonAnnotatedAttribute<'a>> {
        let file = self.name_node.file;
        let body = &self.definition.body;
        let mutated = collect_mutated_class_attr_names(body, &self.name);
        let mut attributes = Vec::new();
        for statement in body {
            if let Stmt::AnnAssign(annotated) = statement
                && let Expr::Name(target) = annotated.target.as_ref()
            {
                let name = target.id.to_string();
                attributes.push(PythonAnnotatedAttribute {
                    is_mutated_in_class: mutated.contains(&name),
                    name,
                    type_node: AstNode::from_span(
                        file,
                        span_from_ruff_range(annotated.annotation.range()),
                    ),
                });
            }
        }
        for function_def in direct_function_definitions(body) {
            if function_def.name.id == INIT_METHOD
                && method_receiver_name_ast(function_def, false, file).is_some()
            {
                for (_, name, annotation) in collect_self_annotated_assignments(&function_def.body)
                {
                    attributes.push(PythonAnnotatedAttribute {
                        is_mutated_in_class: mutated.contains(&name),
                        name,
                        type_node: AstNode::from_span(
                            file,
                            span_from_ruff_range(annotation.range()),
                        ),
                    });
                }
            }
        }
        attributes
    }

    /// Inline type annotations on instance attributes (`self.<attr>: <type>`) inside the direct
    /// instance methods of the class, in source order.
    ///
    /// Instance methods are those not decorated with `@staticmethod` or `@classmethod` whose first
    /// parameter is the receiver `self`.
    #[must_use]
    pub(in crate::code_lint) fn instance_attribute_annotations(
        &self,
    ) -> Vec<PythonInstanceAttributeAnnotation<'a>> {
        let file = self.name_node.file;
        let direct_methods = direct_function_definitions(&self.definition.body);
        let Some(first) = direct_methods.first() else {
            return Vec::new();
        };
        let first_method = PlacementAnchor::before(
            first.name.id.as_str(),
            AstNode::from_span(file, span_from_ruff_range(first.range)),
        );
        let mut annotations = Vec::new();
        for function_def in direct_methods {
            if method_receiver_name_ast(function_def, false, file).is_none() {
                continue;
            }
            let method_name = function_def.name.id.to_string();
            for (assignment, name, annotation) in
                collect_self_annotated_assignments(&function_def.body)
            {
                annotations.push(PythonInstanceAttributeAnnotation {
                    method_name: method_name.clone(),
                    first_method: first_method.clone(),
                    name,
                    annotation: AstNode::from_span(file, span_from_ruff_range(annotation.range())),
                    assignment_node: AstNode::from_span(
                        file,
                        span_from_ruff_range(assignment.range),
                    ),
                    is_bare_final: is_bare_final_annotation_expr(annotation, file),
                });
            }
        }
        annotations
    }

    /// Returns true if the class body defines a function (method, property accessor) named
    /// `name`.
    #[must_use]
    pub(in crate::code_lint) fn defines_function(&self, name: &str) -> bool {
        direct_function_definitions(&self.definition.body)
            .iter()
            .any(|function_def| function_def.name.id == name)
    }

    /// Returns true if a direct base class resolves to an `enum` base (`Enum`, `IntEnum`,
    /// `StrEnum`, `Flag`, `IntFlag`, `ReprEnum`). Subclasses of a project enum are not
    /// recognized.
    #[must_use]
    pub(in crate::code_lint) fn is_enum(&self) -> bool {
        self.has_base_in(ENUM_BASE_PATHS)
    }

    /// Returns true if a direct base class resolves to `typing.Protocol` or
    /// `typing_extensions.Protocol` (optionally subscripted), making the class a `Protocol`
    /// rather than an implementation of one.
    #[must_use]
    pub(in crate::code_lint) fn is_protocol(&self) -> bool {
        self.has_base_in(PROTOCOL_PATHS)
    }

    /// Returns true if a direct base class resolves to `abc.ABC` or the class declares
    /// `metaclass=ABCMeta`.
    #[must_use]
    pub(in crate::code_lint) fn is_abc(&self) -> bool {
        let file = self.name_node.file;
        let has_abc_metaclass = self.definition.keywords().iter().any(|keyword| {
            keyword
                .arg
                .as_ref()
                .is_some_and(|arg| arg.id == "metaclass")
                && written_name(&keyword.value, &file.source)
                    .resolve(file)
                    .is_some_and(|name| ABC_METACLASS_PATHS.contains(&name.path.as_str()))
        });
        has_abc_metaclass || self.has_base_in(ABC_PATHS)
    }

    /// Returns true if a direct base class resolves to `typing.NamedTuple` or
    /// `typing_extensions.NamedTuple`.
    #[must_use]
    pub(in crate::code_lint) fn is_named_tuple(&self) -> bool {
        self.has_base_in(NAMED_TUPLE_PATHS)
    }

    /// Returns true if the class carries a `@dataclass` or `@dataclasses.dataclass` decorator
    /// that does not pass the keyword argument `key`.
    #[must_use]
    pub(in crate::code_lint) fn is_dataclass_missing_arg(&self, key: &str) -> bool {
        self.dataclass_decorator()
            .is_some_and(|decorator| !decorator.has_arg(key))
    }

    /// Names of parameters of [`Self::constructor`] that are stored unchanged on `self`
    /// (`self.<name> = <name>` or `self.<name>: <T> = <name>`) and are not rebound in `__init__`.
    #[must_use]
    pub(in crate::code_lint) fn unchanged_constructor_parameters(&self) -> HashSet<String> {
        let Some(constructor) = self.constructor() else {
            return HashSet::new();
        };
        let Some(receiver) =
            method_receiver_name_ast(constructor.definition, false, self.name_node.file)
        else {
            return HashSet::new();
        };
        let parameter_names: HashSet<&str> = constructor
            .parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect();
        unchanged_self_copies(&constructor.definition.body, &receiver, &parameter_names)
    }

    /// Returns true if the class is a `Protocol` or an `ABC`, so its members declare an
    /// interface rather than implement one.
    #[must_use]
    pub(in crate::code_lint) fn is_interface(&self) -> bool {
        self.is_protocol() || self.is_abc()
    }

    /// Returns true if a direct base class resolves to `TypedDict`, so the class-body
    /// annotations declare dictionary keys.
    #[must_use]
    pub(in crate::code_lint) fn is_typed_dict(&self) -> bool {
        self.has_base_in(TYPED_DICT_PATHS)
    }

    /// Returns true if the class synthesizes constructor fields from class-body annotations
    /// (`@dataclass`, `attrs` decorators, or Pydantic `BaseModel` subclasses).
    #[must_use]
    pub(in crate::code_lint) fn is_field_synthesizing(&self) -> bool {
        self.decorators
            .iter()
            .any(|decorator| decorator.matches_any(FIELD_SYNTHESIZING_DECORATOR_PATHS))
            || self.has_base_in(PYDANTIC_BASE_MODEL_PATHS)
    }

    /// The `@dataclass` or `@dataclasses.dataclass` decorator, resolved through the file's
    /// imports, if the class carries one.
    fn dataclass_decorator(&self) -> Option<&DecoratorInfo> {
        self.decorators
            .iter()
            .find(|decorator| decorator.matches_any(DATACLASS_DECORATOR_PATHS))
    }

    /// Returns true if the import-resolved path of a direct base class is one of `candidates`;
    /// bases defined in the file never match.
    fn has_base_in(&self, candidates: &[&str]) -> bool {
        self.bases.iter().any(|base| {
            base.resolved_path
                .as_deref()
                .is_some_and(|path| candidates.contains(&path))
        })
    }
}

/// The class whose body directly contains the function of `signature` (when that `class` is its
/// nearest enclosing `def` or `class`), or `None` for module-level and nested functions.
#[must_use]
pub fn enclosing_class<'a>(signature: &PythonFunctionSignature<'a>) -> Option<PythonClassInfo<'a>> {
    signature
        .enclosing_class
        .map(|class_def| PythonClassInfo::from_ast(class_def, signature.name_node.file))
}

/// Returns true if the function of `signature` is a method of an interface class
/// ([`PythonClassInfo::is_interface`]: `Protocol` or `ABC`).
#[must_use]
pub fn is_interface_method(signature: &PythonFunctionSignature<'_>) -> bool {
    enclosing_class(signature).is_some_and(|class| class.is_interface())
}

/// Returns true if the function of `signature` is a constructor (`__init__` or `__new__`) whose
/// enclosing class has a docstring.
#[must_use]
pub fn is_documented_by_enclosing_class(signature: &PythonFunctionSignature<'_>) -> bool {
    signature.is_constructor()
        && enclosing_class(signature).is_some_and(|class| class.docstring.is_some())
}

/// Discovers and extracts all class definitions from a Python file.
#[must_use]
pub fn extract_classes(file: &ParsedFile) -> Vec<PythonClassInfo<'_>> {
    struct ClassVisitor<'a> {
        file: &'a ParsedFile,
        classes: Vec<PythonClassInfo<'a>>,
    }

    impl<'a> SourceOrderVisitor<'a> for ClassVisitor<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if let Stmt::ClassDef(class_def) = statement {
                self.classes
                    .push(PythonClassInfo::from_ast(class_def, self.file));
            }
            walk_stmt(self, statement);
        }
    }

    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut visitor = ClassVisitor {
        file,
        classes: Vec::new(),
    };
    visitor.visit_body(&parsed.syntax().body);
    visitor.classes
}

/// The indexes of the in-file ancestors of `classes[index]`, depth-first and left to right.
///
/// Each class of [`local_base_classes`], transitively, appears once: an approximation of the
/// method resolution order as far as the file shows it.
#[must_use]
pub fn local_ancestors(classes: &[PythonClassInfo<'_>], index: usize) -> Vec<usize> {
    let mut ancestors = Vec::new();
    let mut pending: Vec<usize> = local_base_classes(classes, index)
        .into_iter()
        .rev()
        .collect();
    while let Some(next) = pending.pop() {
        if !ancestors.contains(&next) {
            ancestors.push(next);
            pending.extend(local_base_classes(classes, next).into_iter().rev());
        }
    }
    ancestors
}

/// The indexes in `classes` of the classes that the bases of `classes[index]` name, in base
/// order. `classes` is in source order, as [`extract_classes`] returns it.
///
/// As at runtime, a base written as a simple name (optionally subscripted, `Base[T]`) refers to
/// the last class of that name defined before the subclass. Dotted bases (`module.Base`),
/// imported names and classes defined after the subclass are not resolved.
#[must_use]
fn local_base_classes(classes: &[PythonClassInfo<'_>], index: usize) -> Vec<usize> {
    classes[index]
        .bases
        .iter()
        .filter_map(|base| {
            let name = base.unsubscripted_name();
            classes[..index]
                .iter()
                .rposition(|candidate| candidate.name == name)
        })
        .collect()
}

/// A method docstring inherited from an in-file ancestor class.
pub struct PythonInheritedDocstring<'c, 'a> {
    /// The ancestor class whose method declares the docstring.
    pub ancestor: &'c PythonClassInfo<'a>,
    /// The inherited docstring.
    pub docstring: PythonDocstring<'a>,
}

/// The docstring that documentation tools show for method `name` of `classes[index]` when the
/// method has none of its own, as far as the file shows, with the ancestor that declares it.
///
/// Like `inspect.getdoc`, the lookup takes the first of [`local_ancestors`] whose body has a
/// documented `def name`, skipping ancestors whose `def name` has no docstring. Within that body,
/// the last documented `def name` counts, since `@overload` stubs and property setters usually
/// have none.
#[must_use]
pub fn inherited_method_docstring<'c, 'a>(
    classes: &'c [PythonClassInfo<'a>],
    index: usize,
    name: &str,
) -> Option<PythonInheritedDocstring<'c, 'a>> {
    local_ancestors(classes, index)
        .into_iter()
        .find_map(|ancestor| {
            let class = &classes[ancestor];
            class
                .methods()
                .iter()
                .rev()
                .filter(|method| method.name == name)
                .find_map(PythonFunctionSignature::docstring)
                .map(|docstring| PythonInheritedDocstring {
                    ancestor: class,
                    docstring,
                })
        })
}

/// The field names declared by `TypedDict` classes in `file`, including subclasses of an
/// in-file `TypedDict`.
#[must_use]
pub(super) fn typed_dict_keys(file: &ParsedFile) -> HashSet<String> {
    let classes = extract_classes(file);
    classes
        .iter()
        .enumerate()
        .filter(|&(index, class)| {
            class.is_typed_dict()
                || local_ancestors(&classes, index)
                    .into_iter()
                    .any(|ancestor| classes[ancestor].is_typed_dict())
        })
        .flat_map(|(_, class)| {
            class
                .annotated_attributes()
                .into_iter()
                .map(|attribute| attribute.name)
        })
        .collect()
}

/// The first definition of a Python class attribute, from
/// [`PythonClassInfo::attribute_definitions`].
#[derive(Clone)]
pub struct PythonAttributeDefinition<'a> {
    /// Attribute name.
    pub name: String,
    /// The assignment target: the class-body name or the `<receiver>.<name>` attribute.
    pub node: AstNode<'a>,
    /// The type annotation, if the definition is annotated.
    pub annotation: Option<AstNode<'a>>,
    /// True if the definition is in the class body rather than in an initializer method.
    pub is_in_class_body: bool,
    /// True if the annotation is `dataclasses.InitVar` (bare or subscripted): a constructor-only
    /// pseudo-field.
    pub is_init_var: bool,
}

/// A class-body or `__init__` instance attribute carrying a type annotation, from
/// [`PythonClassInfo::annotated_attributes`].
pub struct PythonAnnotatedAttribute<'a> {
    /// Attribute identifier name (e.g. `"items"`).
    pub(in crate::code_lint) name: String,
    /// Type annotation AST node (`type`).
    pub(in crate::code_lint) type_node: AstNode<'a>,
    /// True if any method in the class mutates this attribute in place.
    pub(in crate::code_lint) is_mutated_in_class: bool,
}

/// A Python instance attribute annotated inline (`self.<name>: <type>`) inside an instance
/// method, from [`PythonClassInfo::instance_attribute_annotations`].
#[derive(Clone)]
pub struct PythonInstanceAttributeAnnotation<'a> {
    /// Name of the enclosing instance method.
    pub method_name: String,
    /// First method in the enclosing class body, directly above which the attribute annotation belongs.
    pub first_method: PlacementAnchor,
    /// Attribute identifier, without `"self."`.
    pub name: String,
    /// Type annotation AST node (`int` or `Final[int]`).
    pub annotation: AstNode<'a>,
    /// Full `assignment` AST node (`self.foo: int = 1` or `self.foo: int`).
    pub assignment_node: AstNode<'a>,
    /// True if `annotation` is an unparameterized `Final` qualifier.
    is_bare_final: bool,
}

impl PythonInstanceAttributeAnnotation<'_> {
    /// Returns true if the annotation is an unparameterized `Final` qualifier (`Final`,
    /// `typing.Final`, `Annotated[Final, ...]`), which PEP 591 forbids in a class body without
    /// an initializer.
    #[must_use]
    pub const fn is_bare_final(&self) -> bool {
        self.is_bare_final
    }
}

/// Walks `body` (without entering nested `Stmt::ClassDef`s) and records attribute names
/// mutated in place on `self`, `cls`, or `class_name`.
fn collect_mutated_class_attr_names(body: &[Stmt], class_name: &str) -> HashSet<String> {
    struct MutationVisitor<'a> {
        class_name: &'a str,
        out: HashSet<String>,
    }

    impl<'a> SourceOrderVisitor<'a> for MutationVisitor<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            match statement {
                Stmt::ClassDef(_) => return,
                Stmt::AugAssign(aug) => {
                    record_mutated_class_receiver(&aug.target, self.class_name, &mut self.out);
                }
                _ => {}
            }
            walk_stmt(self, statement);
        }

        fn visit_expr(&mut self, expr: &'a Expr) {
            if let Some(receiver) = in_place_mutated_receiver_expr(expr) {
                record_mutated_class_receiver(receiver, self.class_name, &mut self.out);
            }
            walk_expr(self, expr);
        }
    }

    let mut visitor = MutationVisitor {
        class_name,
        out: HashSet::new(),
    };
    visitor.visit_body(body);
    visitor.out
}

/// Checks if `receiver` is `self.<attr>`, `cls.<attr>`, or `<class_name>.<attr>`, and inserts
/// `<attr>` into `out`.
fn record_mutated_class_receiver(receiver: &Expr, class_name: &str, out: &mut HashSet<String>) {
    if let Expr::Attribute(attr) = receiver
        && let Expr::Name(object) = attr.value.as_ref()
        && (matches!(object.id.as_str(), SELF_RECEIVER | CLS_RECEIVER)
            || object.id.as_str() == class_name)
    {
        out.insert(attr.attr.to_string());
    }
}

/// Walks statements inside a method body (without entering nested functions, classes, or lambdas)
/// and collects `(ann_assign, attr_name, annotation_expr)` for `self.<attr>: <type>` annotations.
fn collect_self_annotated_assignments(body: &[Stmt]) -> Vec<(&StmtAnnAssign, String, &Expr)> {
    struct SelfAnnVisitor<'a> {
        out: Vec<(&'a StmtAnnAssign, String, &'a Expr)>,
    }

    impl<'a> SourceOrderVisitor<'a> for SelfAnnVisitor<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            match statement {
                Stmt::FunctionDef(_) | Stmt::ClassDef(_) => return,
                Stmt::AnnAssign(ann) => {
                    if let Expr::Attribute(left) = ann.target.as_ref()
                        && let Expr::Name(object) = left.value.as_ref()
                        && object.id == SELF_RECEIVER
                    {
                        self.out
                            .push((ann, left.attr.to_string(), ann.annotation.as_ref()));
                    }
                }
                _ => {}
            }
            walk_stmt(self, statement);
        }

        fn visit_expr(&mut self, _expr: &'a Expr) {}
    }

    let mut visitor = SelfAnnVisitor { out: Vec::new() };
    visitor.visit_body(body);
    visitor.out
}

/// Accumulates [`PythonAttributeDefinition`]s, keeping the first definition of each name.
struct AttributeCollector<'a> {
    file: &'a ParsedFile,
    seen: HashSet<String>,
    definitions: Vec<PythonAttributeDefinition<'a>>,
}

impl AttributeCollector<'_> {
    /// Adds the names bound by class-body assignment target `target`.
    fn add_class_body_targets(&mut self, target: &Expr, annotation: Option<&Expr>) {
        for target in unpacked_targets(target) {
            if let Expr::Name(name) = target {
                self.add(name.id.as_str(), target, annotation, true);
            }
        }
    }

    /// Adds the `<receiver>.<name>` targets assigned in the body of initializer `function_def`,
    /// without entering nested scopes.
    fn add_initializer_targets(&mut self, function_def: &StmtFunctionDef, receiver: &str) {
        for (target, annotation) in direct_assignment_targets(&function_def.body) {
            for unpacked in unpacked_targets(target) {
                if let Expr::Attribute(attribute) = unpacked
                    && matches!(attribute.value.as_ref(), Expr::Name(object) if object.id == receiver)
                {
                    self.add(attribute.attr.id.as_str(), unpacked, annotation, false);
                }
            }
        }
    }

    fn add(
        &mut self,
        name: &str,
        target: &Expr,
        annotation: Option<&Expr>,
        is_in_class_body: bool,
    ) {
        if !self.seen.insert(name.to_owned()) {
            return;
        }
        let file = self.file;
        self.definitions.push(PythonAttributeDefinition {
            name: name.to_owned(),
            node: AstNode::from_span(file, span_from_ruff_range(target.range())),
            annotation: annotation.map(|annotation| {
                AstNode::from_span(file, span_from_ruff_range(annotation.range()))
            }),
            is_in_class_body,
            is_init_var: annotation
                .is_some_and(|annotation| is_init_var_annotation(annotation, file)),
        });
    }
}

/// The `(target, annotation)` of each assignment statement in `body`, including inside compound
/// statements, without entering nested functions or classes.
fn direct_assignment_targets(body: &[Stmt]) -> Vec<(&Expr, Option<&Expr>)> {
    struct AssignmentVisitor<'b> {
        out: Vec<(&'b Expr, Option<&'b Expr>)>,
    }

    impl<'b> SourceOrderVisitor<'b> for AssignmentVisitor<'b> {
        fn visit_stmt(&mut self, statement: &'b Stmt) {
            match statement {
                Stmt::FunctionDef(_) | Stmt::ClassDef(_) => return,
                Stmt::Assign(assign) => {
                    self.out
                        .extend(assign.targets.iter().map(|target| (target, None)));
                }
                Stmt::AnnAssign(annotated) => {
                    self.out
                        .push((&annotated.target, Some(&annotated.annotation)));
                }
                _ => {}
            }
            walk_stmt(self, statement);
        }

        fn visit_expr(&mut self, _expr: &'b Expr) {}
    }

    let mut visitor = AssignmentVisitor { out: Vec::new() };
    visitor.visit_body(body);
    visitor.out
}

/// Names in `parameter_names` whose only `<receiver>.<name>` assignment in `body` (outside nested
/// functions or classes) is a plain copy `<receiver>.<name> = <name>` and that are not rebound as
/// locals in `body`.
fn unchanged_self_copies(
    body: &[Stmt],
    receiver: &str,
    parameter_names: &HashSet<&str>,
) -> HashSet<String> {
    struct CopyVisitor<'b> {
        receiver: &'b str,
        rebound_locals: HashSet<&'b str>,
        plain_copies: HashSet<&'b str>,
        transformed_attrs: HashSet<&'b str>,
    }

    impl<'b> CopyVisitor<'b> {
        fn record_assignment(&mut self, target: &'b Expr, value: Option<&'b Expr>) {
            for unpacked in unpacked_targets(target) {
                if let Expr::Name(name) = unpacked {
                    self.rebound_locals.insert(name.id.as_str());
                } else if let Expr::Attribute(attribute) = unpacked
                    && matches!(attribute.value.as_ref(), Expr::Name(object) if object.id == self.receiver)
                {
                    let attr = attribute.attr.id.as_str();
                    let is_plain_copy = matches!(target, Expr::Attribute(_))
                        && matches!(value, Some(Expr::Name(rhs)) if rhs.id == attr);
                    if is_plain_copy {
                        self.plain_copies.insert(attr);
                    } else if value.is_some() {
                        self.transformed_attrs.insert(attr);
                    }
                }
            }
        }
    }

    impl<'b> SourceOrderVisitor<'b> for CopyVisitor<'b> {
        fn visit_stmt(&mut self, statement: &'b Stmt) {
            match statement {
                Stmt::FunctionDef(_) | Stmt::ClassDef(_) => return,
                Stmt::Assign(assign) => {
                    for target in &assign.targets {
                        self.record_assignment(target, Some(&assign.value));
                    }
                }
                Stmt::AnnAssign(annotated) => {
                    self.record_assignment(&annotated.target, annotated.value.as_deref());
                }
                Stmt::AugAssign(aug_assign) => {
                    self.record_assignment(&aug_assign.target, Some(&aug_assign.value));
                }
                _ => {}
            }
            walk_stmt(self, statement);
        }

        fn visit_expr(&mut self, _expr: &'b Expr) {}
    }

    let mut visitor = CopyVisitor {
        receiver,
        rebound_locals: HashSet::new(),
        plain_copies: HashSet::new(),
        transformed_attrs: HashSet::new(),
    };
    visitor.visit_body(body);
    visitor
        .plain_copies
        .into_iter()
        .filter(|name| {
            parameter_names.contains(name)
                && !visitor.rebound_locals.contains(name)
                && !visitor.transformed_attrs.contains(name)
        })
        .map(str::to_owned)
        .collect()
}

/// The leaf targets of assignment target `target`, flattening tuple, list and starred targets.
fn unpacked_targets(target: &Expr) -> Vec<&Expr> {
    match target {
        Expr::Tuple(tuple) => tuple.elts.iter().flat_map(unpacked_targets).collect(),
        Expr::List(list) => list.elts.iter().flat_map(unpacked_targets).collect(),
        Expr::Starred(starred) => unpacked_targets(&starred.value),
        _ => vec![target],
    }
}

/// Returns true if `annotation` is `InitVar` or `InitVar[...]` from `dataclasses`.
fn is_init_var_annotation(annotation: &Expr, file: &ParsedFile) -> bool {
    let base = if let Expr::Subscript(subscript) = annotation {
        subscript.value.as_ref()
    } else {
        annotation
    };
    written_name(base, &file.source)
        .resolve(file)
        .is_some_and(|name| INIT_VAR_PATHS.contains(&name.path.as_str()))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Language;

    #[test]
    fn methods_are_direct_definitions_in_source_order() {
        let source = indoc::indoc! {r"
            class Outer:
                def b(self): pass
                x = 1
                def a(self):
                    def nested(): pass
                class Inner:
                    def inner(self): pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let method_names: Vec<String> = extract_classes(&file)[0]
            .methods()
            .into_iter()
            .map(|method| method.name)
            .collect();
        assert_eq!(method_names, ["b", "a"]);
    }

    /// `(name, annotation text, is_in_class_body, is_init_var)` of each attribute definition of
    /// the first class in `source`.
    fn attribute_facts(source: &str) -> Vec<(String, Option<String>, bool, bool)> {
        let file = ParsedFile::new(source, Language::Python);
        extract_classes(&file)[0]
            .attribute_definitions()
            .into_iter()
            .map(|definition| {
                (
                    definition.name,
                    definition
                        .annotation
                        .map(|annotation| annotation.text().into_owned()),
                    definition.is_in_class_body,
                    definition.is_init_var,
                )
            })
            .collect()
    }

    /// Owned copies of `facts`, to compare with [`attribute_facts`].
    fn owned_facts(
        facts: &[(&str, Option<&str>, bool, bool)],
    ) -> Vec<(String, Option<String>, bool, bool)> {
        facts
            .iter()
            .map(|&(name, annotation, is_in_class_body, is_init_var)| {
                (
                    name.to_owned(),
                    annotation.map(str::to_owned),
                    is_in_class_body,
                    is_init_var,
                )
            })
            .collect()
    }

    #[test]
    fn attribute_definitions_read_class_body_targets() {
        let source = indoc::indoc! {r"
            from dataclasses import InitVar
            class C:
                a = b = 1
                c, (d, *e) = f()
                g: int
                h: InitVar[int] = 0
                a: str = ''
                if x:
                    hidden = 1
        "};
        assert_eq!(
            attribute_facts(source),
            owned_facts(&[
                ("a", None, true, false),
                ("b", None, true, false),
                ("c", None, true, false),
                ("d", None, true, false),
                ("e", None, true, false),
                ("g", Some("int"), true, false),
                ("h", Some("InitVar[int]"), true, true),
            ])
        );
    }

    #[test]
    fn attribute_definitions_read_initializers_without_nested_scopes() {
        let source = indoc::indoc! {r"
            class C:
                def __init__(self, host, port):
                    self.host = host
                    self.port: int = int(port)
                    self.a, self.b = host, port
                    other.x = 1
                    if host:
                        self.flag = True
                    def bind():
                        self.leak = 1
                    f = lambda: setattr(self, 'y', 1)
                def __post_init__(self, host):
                    self.later = host
                def connect(self):
                    self.socket = 1
        "};
        assert_eq!(
            attribute_facts(source),
            owned_facts(&[
                ("host", None, false, false),
                ("port", Some("int"), false, false),
                ("a", None, false, false),
                ("b", None, false, false),
                ("flag", None, false, false),
                ("later", None, false, false),
            ])
        );
    }

    #[test]
    fn attribute_definitions_keep_the_first_definition_in_source_order() {
        let source = indoc::indoc! {r"
            class C:
                def __init__(self, x):
                    self.x = 0
                    self.x: int = x
                x: int
        "};
        assert_eq!(
            attribute_facts(source),
            owned_facts(&[("x", None, false, false)])
        );
    }

    #[test]
    fn constructor_skips_overloads_and_nested_definitions() {
        let source = indoc::indoc! {r"
            from typing import overload
            class C:
                @overload
                def __init__(self, x: int) -> None: ...
                def __init__(self, x, y=0):
                    pass
                class Inner:
                    def __init__(self): pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let classes = extract_classes(&file);
        let parameter_names: Option<Vec<String>> = classes[0].constructor().map(|constructor| {
            constructor
                .parameters
                .into_iter()
                .map(|parameter| parameter.name)
                .collect()
        });
        assert_eq!(
            parameter_names,
            Some(vec!["self".to_owned(), "x".to_owned(), "y".to_owned()])
        );
        assert!(
            extract_classes(&ParsedFile::new("class D:\n    x = 1\n", Language::Python))[0]
                .constructor()
                .is_none()
        );
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
    fn is_interface_method_detects_protocol_and_abc_methods(
        #[case] source: &str,
        #[case] expected: bool,
    ) {
        let file = ParsedFile::new(source, Language::Python);
        let signatures = super::super::extract_function_signatures(&file);
        assert_eq!(is_interface_method(&signatures[0]), expected);
    }

    #[test]
    fn is_enum_resolves_direct_enum_bases() {
        let source = indoc::indoc! {r"
            import enum
            from enum import IntEnum as Code
            class A(enum.Enum): pass
            class B(Code): pass
            class C(A): pass
            class D: pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let enum_names: Vec<String> = extract_classes(&file)
            .into_iter()
            .filter(PythonClassInfo::is_enum)
            .map(|class| class.name)
            .collect();
        assert_eq!(enum_names, ["A", "B"]);
    }

    #[test]
    fn defines_function_reads_direct_definitions_only() {
        let source = indoc::indoc! {r"
            class C:
                @property
                def area(self): pass
                def m(self):
                    def inner(): pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let class = &extract_classes(&file)[0];
        let defined: Vec<&str> = ["area", "m", "inner"]
            .into_iter()
            .filter(|name| class.defines_function(name))
            .collect();
        assert_eq!(defined, ["area", "m"]);
    }

    #[test]
    fn base_binds_to_last_class_of_that_name_defined_before() {
        let source = indoc::indoc! {r"
            class Base: pass
            class Base: pass
            class Sub(Base, mod.Base, Later): pass
            class Later: pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        assert_eq!(local_base_classes(&extract_classes(&file), 2), [1]);
    }

    #[test]
    fn ancestors_are_depth_first_left_to_right_without_repeats() {
        let source = indoc::indoc! {r"
            class Root: pass
            class Left(Root): pass
            class Right(Root): pass
            class Sub(Left, Right): pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        assert_eq!(local_ancestors(&extract_classes(&file), 3), [1, 0, 2]);
    }

    #[test]
    fn test_extract_classes_bases_metadata() {
        let source = "class FakeService(abc.ABC, Protocol):\n    pass";
        let file = ParsedFile::new(source, Language::Python);
        let classes = extract_classes(&file);
        let cls = &classes[0];
        let base_names: Vec<&str> = cls.bases.iter().map(|base| base.name.as_str()).collect();
        assert_eq!(classes.len(), 1);
        assert_eq!(cls.name, "FakeService");
        assert_eq!(base_names, vec!["abc.ABC", "Protocol"]);
    }

    #[test]
    fn test_extract_classes_dataclass() {
        let source = indoc::indoc! {r"
            @dataclass(frozen=True)
            class Config:
                pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let classes = extract_classes(&file);

        let cls = &classes[0];
        assert_eq!(cls.name, "Config");
        assert!(cls.is_field_synthesizing());
        assert!(!cls.is_dataclass_missing_arg("frozen"));
        assert!(cls.is_dataclass_missing_arg("slots"));
    }

    #[rstest::rstest]
    #[case::class_level("class C:\n    items: list[int]", &[("items", false, false)])]
    #[case::private_included("class C:\n    _items: list[int]", &[("_items", false, false)])]
    #[case::init_attribute(indoc::indoc! {r"
        class C:
            def __init__(self):
                self.items: list[int] = []"}, &[("items", false, false)])]
    #[case::self_mutation(indoc::indoc! {r"
        class C:
            items: list[int]
            def add(self):
                self.items.append(1)"}, &[("items", true, false)])]
    #[case::cls_mutation(indoc::indoc! {r"
        class C:
            items: list[int]
            @classmethod
            def add(cls):
                cls.items.append(1)"}, &[("items", true, false)])]
    #[case::class_name_mutation(indoc::indoc! {r"
        class C:
            items: list[int]
            def add(self):
                C.items.append(1)"}, &[("items", true, false)])]
    #[case::subscript_delete(indoc::indoc! {r"
        class C:
            items: list[int]
            def pop(self):
                del self.items[0]"}, &[("items", true, false)])]
    #[case::protocol_member("class P(Protocol):\n    items: list[int]", &[("items", false, true)])]
    fn test_annotated_attributes(#[case] source: &str, #[case] expected: &[(&str, bool, bool)]) {
        let file = ParsedFile::new(source, Language::Python);
        let actual: Vec<(String, bool, bool)> = extract_classes(&file)
            .iter()
            .flat_map(|class| {
                class.annotated_attributes().into_iter().map(|attribute| {
                    (
                        attribute.name,
                        attribute.is_mutated_in_class,
                        class.is_interface(),
                    )
                })
            })
            .collect();
        let expected: Vec<(String, bool, bool)> = expected
            .iter()
            .map(|(name, mutated, in_protocol)| ((*name).to_string(), *mutated, *in_protocol))
            .collect();
        assert_eq!(actual, expected);
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
        let file = ParsedFile::new(source, Language::Python);
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
        let file = ParsedFile::new(source, Language::Python);
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
        indoc::indoc! {r"
            class C:
                def __init__(self):
                    self.x: int = 1
                async def reset(self):
                    if True:
                        self.y: str"},
        &[("C", "__init__", "x", "int"), ("C", "reset", "y", "str")]
    )]
    #[case::private_collected_unannotated_skipped(
        indoc::indoc! {r"
            class C:
                def __init__(self):
                    self._p: int = 1
                    self.pub = 2"},
        &[("C", "__init__", "_p", "int")]
    )]
    #[case::staticmethod_classmethod_and_nested_func_skipped(
        indoc::indoc! {r"
            class C:
                @staticmethod
                def sm(self):
                    self.a: int = 1
                @classmethod
                def cm(cls):
                    cls.b: int = 2
                def run(self):
                    def inner(self):
                        self.c: int = 3"},
        &[]
    )]
    fn test_instance_attribute_annotations(
        #[case] source: &str,
        #[case] expected: &[(&str, &str, &str, &str)],
    ) {
        let file = ParsedFile::new(source, Language::Python);
        let actual: Vec<(String, String, String, String)> = extract_classes(&file)
            .iter()
            .flat_map(|class| {
                class
                    .instance_attribute_annotations()
                    .into_iter()
                    .map(|attribute| {
                        (
                            class.name.clone(),
                            attribute.method_name,
                            attribute.name,
                            attribute.annotation.text().into_owned(),
                        )
                    })
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
        indoc::indoc! {r"
            class C:
                def __init__(self):
                    self.a: Final = 1"},
        (true, false)
    )]
    #[case::qualified_bare_final(
        indoc::indoc! {r"
            class C:
                def __init__(self):
                    self.a: typing.Final = 1"},
        (true, false)
    )]
    #[case::annotated_bare_final(
        indoc::indoc! {r"
            class C:
                def __init__(self):
                    self.a: Annotated[Final, 'm'] = 1"},
        (true, false)
    )]
    #[case::parameterized_final(
        indoc::indoc! {r"
            class C:
                def __init__(self):
                    self.a: Final[int] = 1"},
        (false, false)
    )]
    #[case::dataclass(
        indoc::indoc! {r"
            @dataclass
            class C:
                def __post_init__(self):
                    self.a: int = 1"},
        (false, true)
    )]
    #[case::pydantic_model(
        indoc::indoc! {r"
            class C(BaseModel):
                def model_post_init(self, context):
                    self.a: int = 1"},
        (false, true)
    )]
    fn test_instance_attribute_annotation_facts(
        #[case] source: &str,
        #[case] expected: (bool, bool),
    ) {
        let file = ParsedFile::new(source, Language::Python);
        let actual: Vec<(bool, bool)> = extract_classes(&file)
            .iter()
            .flat_map(|class| {
                class
                    .instance_attribute_annotations()
                    .iter()
                    .map(|attribute| (attribute.is_bare_final(), class.is_field_synthesizing()))
                    .collect::<Vec<_>>()
            })
            .collect();
        assert_eq!(actual, [expected]);
    }

    #[test]
    fn test_inline_attribute_annotation_placement_anchor() {
        let attribute_file = ParsedFile::new(
            indoc::indoc! {r"
                class Client:
                    @classmethod
                    def default_port(cls) -> int:
                        return 80

                    def __init__(self, host: str) -> None:
                        self.host: str = host
            "},
            Language::Python,
        );
        let attributes = extract_classes(&attribute_file)[0].instance_attribute_annotations();
        assert_eq!(attributes.len(), 1);
        assert_eq!(
            attributes[0].first_method,
            PlacementAnchor {
                name: "default_port".to_string(),
                line: 2,
            }
        );
    }

    #[test]
    fn typed_dict_keys_reads_typed_dict_classes_and_local_subclasses() {
        let source = indoc::indoc! {r"
            import typing
            class BaseFill(typing.TypedDict):
                symbol: str
            class TradeFill(BaseFill, total=False):
                price: float
            class RegularClass:
                ignored: int
        "};
        let file = ParsedFile::new(source, Language::Python);
        assert_eq!(
            typed_dict_keys(&file),
            HashSet::from(["symbol".to_owned(), "price".to_owned()])
        );
    }
}
