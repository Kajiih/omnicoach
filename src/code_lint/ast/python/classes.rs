//! Python class definitions, base-class classification, and class/instance attribute walkers.

use super::docstrings::{DocstringSectionKind, PythonDocstring, docstring_of};
use super::functions::{PythonFunctionSignature, function_signature};
use super::scopes::{collect_local_bound_names, parameters_shadow_name};
use super::{
    AstNode, DecoratorInfo, ParsedFile, direct_function_definitions, extract_decorators_from_slice,
    in_place_mutated_receiver_expr, is_bare_final_annotation_expr, method_receiver_name_ast,
    resolved_path_and_terminal_expr,
};
use crate::code_lint::ast::{
    DeclarationScope, MethodVisibility, PlacementAnchor, ScopeItem, ScopeItemKind, TypeMethod,
    TypeMethodScope, span_from_ruff_range, split_at_redefinitions,
};
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_expr, walk_stmt};
use ruff_python_ast::{Expr, Stmt, StmtAnnAssign, StmtClassDef, StmtFunctionDef};
use ruff_text_size::Ranged as _;
use std::collections::{HashMap, HashSet};

const SELF_RECEIVER: &str = "self";
const CLS_RECEIVER: &str = "cls";
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
const PYTHON_CONSTRUCTOR_NAMES: &[&str] = &[
    "__prepare__",
    "__init_subclass__",
    "__new__",
    "__init__",
    "__post_init__",
    "__attrs_pre_init__",
    "__attrs_post_init__",
];
const INIT_METHOD: &str = "__init__";
/// Methods whose `self.<name>` assignments create instance attributes.
const INITIALIZER_METHODS: &[&str] = &[
    INIT_METHOD,
    "__post_init__",
    "__attrs_post_init__",
    "model_post_init",
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
const INIT_VAR_PATHS: &[&str] = &["InitVar", "dataclasses.InitVar"];

/// Represents a base class expression in a Python class definition.
#[derive(Clone)]
pub struct PythonBaseClass {
    /// Base class identifier or dotted path text (e.g. `"Protocol"`, `"abc.ABC"`).
    pub name: String,
    /// Canonical path of the unsubscripted base class resolved through [`super::resolve_name`], or
    /// `None` when the first segment is defined in the file.
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

/// Structured representation of a Python class definition.
#[derive(Clone)]
pub struct PythonClassInfo<'a> {
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

    /// The functions defined directly in the class body (not nested deeper), in source order.
    #[must_use]
    pub fn methods(&self) -> Vec<PythonFunctionSignature<'a>> {
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
    pub fn constructor(&self) -> Option<PythonFunctionSignature<'a>> {
        let file = self.name_node.file;
        self.methods().into_iter().rfind(|method| {
            method.name == INIT_METHOD
                && !extract_decorators_from_slice(&method.definition.decorator_list, file)
                    .iter()
                    .any(|decorator| decorator.matches_any(OVERLOAD_DECORATOR_PATHS))
        })
    }

    /// The first definition of each attribute name the class creates, in source order.
    ///
    /// Definitions are the targets of class-body assignments (`x = …`, `x: T = …`, `x: T`,
    /// including multiple and tuple-unpacking targets) and of `<receiver>.x = …` /
    /// `<receiver>.x: T = …` assignments directly in an instance initializer (`__init__`,
    /// `__post_init__`, `__attrs_post_init__`, `model_post_init`), where the receiver is the
    /// method's `self` parameter. Nested functions, lambdas and classes are not entered, and
    /// other methods are not read. Private and dunder names are included.
    #[must_use]
    pub fn attribute_definitions(&self) -> Vec<PythonAttributeDefinition<'a>> {
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
}

impl PythonClassInfo<'_> {
    /// Returns true if the class body defines a function (method, property accessor) named
    /// `name`.
    #[must_use]
    pub fn defines_function(&self, name: &str) -> bool {
        direct_function_definitions(&self.definition.body)
            .iter()
            .any(|function_def| function_def.name.id == name)
    }

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

    /// Names of parameters of [`Self::constructor`] that are stored unchanged on `self`
    /// (`self.<name> = <name>` or `self.<name>: <T> = <name>`) and are not rebound in `__init__`.
    #[must_use]
    pub fn unchanged_constructor_parameters(&self) -> HashSet<String> {
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

    /// Names of parameters of [`Self::constructor`] that are stored unchanged on `self`
    /// ([`Self::unchanged_constructor_parameters`]) and listed under `Attributes:` in the class
    /// docstring.
    #[must_use]
    pub fn documented_unchanged_constructor_parameters(&self) -> HashSet<String> {
        let Some(docstring) = &self.docstring else {
            return HashSet::new();
        };
        let documented = docstring.section_entry_names(DocstringSectionKind::Attributes);
        self.unchanged_constructor_parameters()
            .into_iter()
            .filter(|name| documented.contains(name))
            .collect()
    }

    /// The `@dataclass` or `@dataclasses.dataclass` decorator, resolved through the file's
    /// imports, if the class carries one.
    fn dataclass_decorator(&self) -> Option<&DecoratorInfo> {
        self.decorators
            .iter()
            .find(|decorator| decorator.matches_any(DATACLASS_DECORATOR_PATHS))
    }
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

/// The indexes in `classes` of the classes that the bases of `classes[index]` name, in base
/// order. `classes` is in source order, as [`extract_classes`] returns it.
///
/// As at runtime, a base written as a simple name (optionally subscripted, `Base[T]`) refers to
/// the last class of that name defined before the subclass. Dotted bases (`module.Base`),
/// imported names and classes defined after the subclass are not resolved.
#[must_use]
pub fn local_base_classes(classes: &[PythonClassInfo<'_>], index: usize) -> Vec<usize> {
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

/// Returns true if `name` starts with the word `Fake` after any leading `_`: `Fake` alone or
/// followed by anything but a lowercase letter, so `Faker` and `Fakeable` do not match.
#[must_use]
pub fn is_fake_class_name(name: &str) -> bool {
    name.trim_start_matches('_')
        .strip_prefix("Fake")
        .is_some_and(|rest| !rest.starts_with(|character: char| character.is_ascii_lowercase()))
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

/// A Python class or `__init__` instance attribute carrying a type annotation.
pub struct PythonAnnotatedAttribute<'a> {
    /// Name of the enclosing class.
    pub class_name: String,
    /// True if the enclosing class is a `Protocol` or `ABC`, so the annotation declares an
    /// interface member.
    pub is_in_protocol_or_abc: bool,
    /// Attribute identifier name (e.g. `"items"`).
    pub name: String,
    /// Type annotation AST node (`type`).
    pub type_node: AstNode<'a>,
    /// True if any method in the enclosing class mutates this attribute in place.
    pub is_mutated_in_class: bool,
    /// True if the enclosing class is a `TypedDict`, so the annotation declares a dictionary key.
    /// A `TypedDict` cannot define methods, so `__init__` attributes are never keys.
    pub is_typed_dict_key: bool,
}

/// Collects annotated class and `__init__` attributes, recording whether each attribute is
/// mutated in place within its class.
#[must_use]
pub fn collect_class_attributes(file: &ParsedFile) -> Vec<PythonAnnotatedAttribute<'_>> {
    struct ClassAttrVisitor<'a> {
        file: &'a ParsedFile,
        out: Vec<PythonAnnotatedAttribute<'a>>,
    }

    impl<'a> SourceOrderVisitor<'a> for ClassAttrVisitor<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if let Stmt::ClassDef(class_def) = statement {
                let class_name = class_def.name.id.to_string();
                let is_typed_dict = is_typed_dict_class(class_def, self.file);
                let is_in_protocol_or_abc = is_protocol_or_abc_class(class_def, self.file);
                let mutated_attrs = collect_mutated_class_attr_names(&class_def.body, &class_name);

                for child in &class_def.body {
                    if let Stmt::AnnAssign(ann) = child
                        && let Expr::Name(left) = ann.target.as_ref()
                    {
                        let attr_name = left.id.to_string();
                        let is_mutated_in_class = mutated_attrs.contains(&attr_name);
                        self.out.push(PythonAnnotatedAttribute {
                            class_name: class_name.clone(),
                            is_in_protocol_or_abc,
                            name: attr_name,
                            type_node: AstNode::from_span(
                                self.file,
                                span_from_ruff_range(ann.annotation.range()),
                            ),
                            is_mutated_in_class,
                            is_typed_dict_key: is_typed_dict,
                        });
                    }
                }

                for function_def in direct_function_definitions(&class_def.body) {
                    if function_def.name.id == INIT_METHOD
                        && method_receiver_name_ast(function_def, false, self.file).is_some()
                    {
                        for (_, attr_name, type_expr) in
                            collect_self_annotated_assignments(&function_def.body)
                        {
                            let is_mutated_in_class = mutated_attrs.contains(&attr_name);
                            self.out.push(PythonAnnotatedAttribute {
                                class_name: class_name.clone(),
                                is_in_protocol_or_abc,
                                name: attr_name,
                                type_node: AstNode::from_span(
                                    self.file,
                                    span_from_ruff_range(type_expr.range()),
                                ),
                                is_mutated_in_class,
                                is_typed_dict_key: false,
                            });
                        }
                    }
                }
            }
            walk_stmt(self, statement);
        }
    }

    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut visitor = ClassAttrVisitor {
        file,
        out: Vec::new(),
    };
    visitor.visit_body(&parsed.syntax().body);
    visitor.out
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

/// Returns true if a Python `StmtClassDef` inherits from `Protocol` or `ABC` or declares
/// `metaclass=ABCMeta`.
pub(super) fn is_protocol_or_abc_class(class_def: &StmtClassDef, file: &ParsedFile) -> bool {
    is_protocol_class(class_def, file) || is_abc_class(class_def, file)
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

/// A Python instance attribute annotated inline (`self.<name>: <type>`) inside an instance method.
#[derive(Clone)]
pub struct PythonInstanceAttributeAnnotation<'a> {
    /// Name of the enclosing class.
    pub class_name: String,
    /// True if the enclosing class synthesizes constructor fields from class-body annotations
    /// (`@dataclass`, `attrs` decorators, or Pydantic `BaseModel` subclasses).
    pub is_in_field_synthesizing_class: bool,
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

/// Collects inline type annotations on instance attributes (`self.<attr>: <type>`) inside
/// instance methods across `file`.
///
/// Only direct instance methods of a `class_definition` are inspected: not decorated with
/// `@staticmethod` or `@classmethod`, and whose first parameter is the receiver `self`.
#[must_use]
pub fn collect_instance_attribute_annotations(
    file: &ParsedFile,
) -> Vec<PythonInstanceAttributeAnnotation<'_>> {
    struct InstanceAttrVisitor<'a> {
        file: &'a ParsedFile,
        out: Vec<PythonInstanceAttributeAnnotation<'a>>,
    }

    impl<'a> SourceOrderVisitor<'a> for InstanceAttrVisitor<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if let Stmt::ClassDef(class_def) = statement {
                let direct_methods = direct_function_definitions(&class_def.body);
                let Some(first) = direct_methods.first() else {
                    walk_stmt(self, statement);
                    return;
                };
                let class_name = class_def.name.id.to_string();
                let is_in_field_synthesizing_class =
                    is_field_synthesizing_class(class_def, self.file);
                let first_method = PlacementAnchor::before(
                    first.name.id.as_str(),
                    AstNode::from_span(self.file, span_from_ruff_range(first.range)),
                );

                for function_def in direct_methods {
                    if method_receiver_name_ast(function_def, false, self.file).is_some() {
                        let method_name = function_def.name.id.to_string();
                        for (ann_assign, attribute_name, type_expr) in
                            collect_self_annotated_assignments(&function_def.body)
                        {
                            self.out.push(PythonInstanceAttributeAnnotation {
                                class_name: class_name.clone(),
                                is_in_field_synthesizing_class,
                                method_name: method_name.clone(),
                                first_method: first_method.clone(),
                                name: attribute_name,
                                annotation: AstNode::from_span(
                                    self.file,
                                    span_from_ruff_range(type_expr.range()),
                                ),
                                assignment_node: AstNode::from_span(
                                    self.file,
                                    span_from_ruff_range(ann_assign.range),
                                ),
                                is_bare_final: is_bare_final_annotation_expr(type_expr, self.file),
                            });
                        }
                    }
                }
            }
            walk_stmt(self, statement);
        }
    }

    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut visitor = InstanceAttrVisitor {
        file,
        out: Vec::new(),
    };
    visitor.visit_body(&parsed.syntax().body);
    visitor.out
}

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

/// Returns true if `class_def` synthesizes constructor fields from class-body annotations
/// (`@dataclass`, `attrs` decorators, or Pydantic `BaseModel` subclasses).
fn is_field_synthesizing_class(class_def: &StmtClassDef, file: &ParsedFile) -> bool {
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

/// Collects each Python `class` definition in `file` with its direct methods in source order.
#[must_use]
pub(in crate::code_lint::ast) fn collect_type_method_scopes(
    file: &ParsedFile,
) -> Vec<TypeMethodScope<'_>> {
    struct ClassMethodVisitor<'a> {
        file: &'a ParsedFile,
        scopes: Vec<TypeMethodScope<'a>>,
    }

    impl<'a> SourceOrderVisitor<'a> for ClassMethodVisitor<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if let Stmt::ClassDef(class_def) = statement {
                self.scopes.push(TypeMethodScope {
                    type_name: class_def.name.id.to_string(),
                    methods: collect_class_methods(class_def, self.file),
                });
            }
            walk_stmt(self, statement);
        }
    }

    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut visitor = ClassMethodVisitor {
        file,
        scopes: Vec::new(),
    };
    visitor.visit_body(&parsed.syntax().body);
    visitor.scopes
}

/// Collects the direct methods of `class_def` in source order, grouping `@overload` signatures
/// and `@<prop>.setter` / `@<prop>.deleter` accessors with their first definition.
fn collect_class_methods<'a>(
    class_def: &StmtClassDef,
    file: &'a ParsedFile,
) -> Vec<TypeMethod<'a>> {
    group_python_scope_items(&class_def.body, true, file)
        .into_iter()
        .map(|callable| TypeMethod {
            name_node: callable.name_node,
            name: callable.name,
            start_line: callable.start_line,
            visibility: callable.visibility,
            is_constructor: callable.is_constructor,
        })
        .collect()
}

/// Collects all module and `class` [`DeclarationScope`]s in a Python `file`.
#[must_use]
pub(in crate::code_lint::ast) fn collect_declaration_scopes(
    file: &ParsedFile,
) -> Vec<DeclarationScope<'_>> {
    struct ScopeCollector<'a> {
        file: &'a ParsedFile,
        future_annotations: bool,
        scopes: Vec<DeclarationScope<'a>>,
    }

    impl<'a> SourceOrderVisitor<'a> for ScopeCollector<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if let Stmt::ClassDef(class_def) = statement
                && let Some(scope) =
                    build_class_declaration_scope(class_def, self.future_annotations, self.file)
            {
                self.scopes.push(scope);
            }
            walk_stmt(self, statement);
        }
    }

    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let module_body = &parsed.syntax().body;
    let future_annotations = has_future_annotations(module_body);
    let scopes = build_module_declaration_scopes(module_body, future_annotations, file);
    let mut collector = ScopeCollector {
        file,
        future_annotations,
        scopes,
    };
    collector.visit_body(module_body);
    collector.scopes
}

/// A type-annotated class or instance attribute (`name: Type` or `name: Type = value`) declared
/// after a method definition in a Python class body.
pub struct PythonFieldAfterMethod<'a> {
    /// Name of the enclosing class.
    pub class_name: String,
    /// Attribute identifier name.
    pub name: String,
    /// First method in the enclosing class body, directly above which the attribute belongs.
    pub first_method: PlacementAnchor,
    /// The full `Stmt::AnnAssign` AST node.
    pub node: AstNode<'a>,
}

/// Collects type-annotated class-body attribute declarations (`Stmt::AnnAssign` with an
/// identifier target) that appear after at least one direct method definition in the same class.
#[must_use]
pub fn collect_fields_after_methods(file: &ParsedFile) -> Vec<PythonFieldAfterMethod<'_>> {
    struct FieldAfterMethodVisitor<'a> {
        file: &'a ParsedFile,
        out: Vec<PythonFieldAfterMethod<'a>>,
    }

    impl<'a> SourceOrderVisitor<'a> for FieldAfterMethodVisitor<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if let Stmt::ClassDef(class_def) = statement {
                let class_name = class_def.name.id.to_string();
                let mut first_method: Option<PlacementAnchor> = None;
                for child in &class_def.body {
                    match child {
                        Stmt::FunctionDef(func_def) => {
                            first_method.get_or_insert_with(|| {
                                PlacementAnchor::before(
                                    func_def.name.id.as_str(),
                                    AstNode::from_span(
                                        self.file,
                                        span_from_ruff_range(func_def.range),
                                    ),
                                )
                            });
                        }
                        Stmt::AnnAssign(ann) => {
                            if let Some(first_method) = &first_method
                                && let Expr::Name(target) = ann.target.as_ref()
                            {
                                self.out.push(PythonFieldAfterMethod {
                                    class_name: class_name.clone(),
                                    name: target.id.to_string(),
                                    first_method: first_method.clone(),
                                    node: AstNode::from_span(
                                        self.file,
                                        span_from_ruff_range(ann.range),
                                    ),
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
            walk_stmt(self, statement);
        }
    }

    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut visitor = FieldAfterMethodVisitor {
        file,
        out: Vec::new(),
    };
    visitor.visit_body(&parsed.syntax().body);
    visitor.out
}

/// Returns true if `module_body` contains `from __future__ import annotations`.
fn has_future_annotations(module_body: &[Stmt]) -> bool {
    module_body.iter().any(|stmt| {
        if let Stmt::ImportFrom(import_from) = stmt
            && import_from.module.as_deref() == Some("__future__")
        {
            import_from
                .names
                .iter()
                .any(|alias| alias.name.id.as_str() == "annotations")
        } else {
            false
        }
    })
}

/// Builds [`DeclarationScope`]s for top-level functions and classes in `module_body`, starting a
/// new scope segment whenever a non-overload, non-property declaration name is redefined.
fn build_module_declaration_scopes<'a>(
    module_body: &[Stmt],
    future_annotations: bool,
    file: &'a ParsedFile,
) -> Vec<DeclarationScope<'a>> {
    let grouped = group_python_scope_items(module_body, false, file);
    if grouped.len() < 2 {
        return Vec::new();
    }

    let segments = split_at_redefinitions(grouped, |item| item.name.as_str());
    let mut scopes = Vec::new();

    for (seg_idx, segment) in segments.iter().enumerate() {
        if segment.len() < 2 {
            continue;
        }
        let seg_start = if seg_idx == 0 {
            0
        } else {
            segment[0].first_stmt_idx
        };
        let seg_end = segments
            .get(seg_idx + 1)
            .map_or(module_body.len(), |next| next[0].first_stmt_idx);

        let item_by_name: HashMap<String, usize> = segment
            .iter()
            .enumerate()
            .map(|(idx, item)| (item.name.clone(), idx))
            .collect();
        let external_class_callees =
            collect_external_class_callees(&module_body[seg_start..seg_end], &item_by_name, file);

        let mut items: Vec<ScopeItem<'a>> = segment
            .iter()
            .map(|group| {
                build_python_module_scope_item(group, &item_by_name, &external_class_callees, file)
            })
            .collect();

        let first_stmt_indices: Vec<usize> =
            segment.iter().map(|item| item.first_stmt_idx).collect();
        let is_callable: Vec<bool> = segment
            .iter()
            .map(|item| matches!(item.item, PythonScopeItem::Callable(_)))
            .collect();
        let pinned = compute_python_pinned_before(
            module_body,
            seg_start..seg_end,
            &first_stmt_indices,
            &item_by_name,
            &items,
            &is_callable,
            future_annotations,
        );
        for (item, pin) in items.iter_mut().zip(pinned) {
            item.pinned_before = pin;
        }

        scopes.push(DeclarationScope { items });
    }

    scopes
}

/// Builds a [`DeclarationScope`] for direct methods of `class_def`.
fn build_class_declaration_scope<'a>(
    class_def: &StmtClassDef,
    future_annotations: bool,
    file: &'a ParsedFile,
) -> Option<DeclarationScope<'a>> {
    struct ClassMethodCallVisitor<'map, 'local> {
        class_name: &'map str,
        method_by_name: &'map HashMap<String, usize>,
        local_names: &'local HashSet<String>,
        has_self_param: bool,
        has_cls_param: bool,
        callees: &'local mut Vec<usize>,
    }

    impl SourceOrderVisitor<'_> for ClassMethodCallVisitor<'_, '_> {
        fn visit_stmt(&mut self, statement: &Stmt) {
            if matches!(statement, Stmt::ClassDef(_)) {
                return;
            }
            walk_stmt(self, statement);
        }

        fn visit_expr(&mut self, expr: &Expr) {
            if let Expr::Attribute(attr) = expr
                && attr.ctx.is_load()
                && let Expr::Name(recv) = attr.value.as_ref()
            {
                let recv_name = recv.id.as_str();
                let is_valid_receiver = (recv_name == SELF_RECEIVER
                    && (self.has_self_param || !self.local_names.contains(SELF_RECEIVER)))
                    || (recv_name == CLS_RECEIVER
                        && (self.has_cls_param || !self.local_names.contains(CLS_RECEIVER)))
                    || (recv_name == self.class_name && !self.local_names.contains(recv_name));
                if is_valid_receiver
                    && let Some(&idx) = self.method_by_name.get(attr.attr.id.as_str())
                    && !self.callees.contains(&idx)
                {
                    self.callees.push(idx);
                }
            }
            walk_expr(self, expr);
        }
    }

    let grouped = group_python_scope_items(&class_def.body, true, file);
    if grouped.len() < 2 {
        return None;
    }
    let method_by_name: HashMap<String, usize> = grouped
        .iter()
        .enumerate()
        .map(|(idx, item)| (item.name.clone(), idx))
        .collect();
    let class_name = class_def.name.id.as_str();
    let first_stmt_indices: Vec<usize> = grouped.iter().map(|item| item.first_stmt_idx).collect();

    let mut items: Vec<ScopeItem<'a>> = grouped
        .into_iter()
        .map(|group| {
            let mut callees = Vec::new();
            if let PythonScopeItem::Callable(defs) = &group.item {
                for def in defs {
                    let local_names = collect_local_bound_names(def, file);
                    let has_self_param = parameters_shadow_name(&def.parameters, SELF_RECEIVER);
                    let has_cls_param = parameters_shadow_name(&def.parameters, CLS_RECEIVER);
                    let mut visitor = ClassMethodCallVisitor {
                        class_name,
                        method_by_name: &method_by_name,
                        local_names: &local_names,
                        has_self_param,
                        has_cls_param,
                        callees: &mut callees,
                    };
                    visitor.visit_body(&def.body);
                }
            }
            let body_callees = callees.clone();
            ScopeItem {
                name_node: group.name_node,
                name: group.name,
                end_line: group.end_line,
                visibility: group.visibility,
                is_constructor: group.is_constructor,
                kind: ScopeItemKind::Callable,
                callees,
                param_callees: Vec::new(),
                return_callees: Vec::new(),
                body_callees,
                pinned_before: None,
            }
        })
        .collect();

    let is_callable = vec![true; items.len()];
    let pinned = compute_python_pinned_before(
        &class_def.body,
        0..class_def.body.len(),
        &first_stmt_indices,
        &method_by_name,
        &items,
        &is_callable,
        future_annotations,
    );
    for (item, pin) in items.iter_mut().zip(pinned) {
        item.pinned_before = pin;
    }

    Some(DeclarationScope { items })
}

struct GroupedPythonScopeItem<'a, 'ast> {
    name_node: AstNode<'a>,
    name: String,
    start_line: usize,
    end_line: usize,
    visibility: MethodVisibility,
    is_constructor: bool,
    first_stmt_idx: usize,
    item: PythonScopeItem<'ast>,
}

/// item (such as a class redefined later in the module), keyed by class name.
fn collect_external_class_callees<'body>(
    statements: &'body [Stmt],
    item_by_name: &HashMap<String, usize>,
    file: &ParsedFile,
) -> HashMap<&'body str, Vec<usize>> {
    let empty_classes = HashMap::new();
    let empty_locals = HashSet::new();
    let mut external_class_callees = HashMap::new();
    for statement in statements {
        if let Stmt::ClassDef(class_def) = statement
            && !item_by_name.contains_key(class_def.name.id.as_str())
        {
            let mut callees = Vec::new();
            collect_python_class_module_refs(
                class_def,
                item_by_name,
                &empty_classes,
                &empty_locals,
                file,
                &mut callees,
            );
            if !callees.is_empty() {
                external_class_callees.insert(class_def.name.id.as_str(), callees);
            }
        }
    }
    external_class_callees
}

/// Builds the [`ScopeItem`] for `group` from the sibling references in its signatures, bodies,
/// or class definition.
fn build_python_module_scope_item<'a>(
    group: &GroupedPythonScopeItem<'a, '_>,
    item_by_name: &HashMap<String, usize>,
    external_class_callees: &HashMap<&str, Vec<usize>>,
    file: &ParsedFile,
) -> ScopeItem<'a> {
    let empty_locals = HashSet::new();
    let mut callees = Vec::new();
    let mut param_callees = Vec::new();
    let mut return_callees = Vec::new();
    let mut body_callees = Vec::new();
    let kind = match &group.item {
        PythonScopeItem::Callable(defs) => {
            for def in defs {
                let mut param_visitor = ModuleRefVisitor {
                    fn_by_name: item_by_name,
                    class_callees: external_class_callees,
                    local_names: &empty_locals,
                    callees: &mut param_callees,
                };
                for dec in &def.decorator_list {
                    param_visitor.visit_decorator(dec);
                }
                param_visitor.visit_parameters(&def.parameters);
                if let Some(returns) = &def.returns {
                    let mut ret_visitor = ModuleRefVisitor {
                        fn_by_name: item_by_name,
                        class_callees: external_class_callees,
                        local_names: &empty_locals,
                        callees: &mut return_callees,
                    };
                    ret_visitor.visit_expr(returns);
                }
                let local_names = collect_local_bound_names(def, file);
                let mut body_visitor = ModuleRefVisitor {
                    fn_by_name: item_by_name,
                    class_callees: external_class_callees,
                    local_names: &local_names,
                    callees: &mut body_callees,
                };
                body_visitor.visit_body(&def.body);
            }
            for &idx in param_callees
                .iter()
                .chain(&return_callees)
                .chain(&body_callees)
            {
                if !callees.contains(&idx) {
                    callees.push(idx);
                }
            }
            ScopeItemKind::Callable
        }
        PythonScopeItem::Type(class_def) => {
            collect_python_class_module_refs(
                class_def,
                item_by_name,
                external_class_callees,
                &empty_locals,
                file,
                &mut callees,
            );
            body_callees.clone_from(&callees);
            ScopeItemKind::Type
        }
    };
    ScopeItem {
        name_node: group.name_node,
        name: group.name.clone(),
        end_line: group.end_line,
        visibility: group.visibility,
        is_constructor: group.is_constructor,
        kind,
        callees,
        param_callees,
        return_callees,
        body_callees,
        pinned_before: None,
    }
}

fn collect_python_class_module_refs<'map>(
    class_def: &StmtClassDef,
    item_by_name: &'map HashMap<String, usize>,
    external_class_callees: &'map HashMap<&'map str, Vec<usize>>,
    empty_locals: &HashSet<String>,
    file: &ParsedFile,
    callees: &mut Vec<usize>,
) {
    let mut header_visitor = ModuleRefVisitor {
        fn_by_name: item_by_name,
        class_callees: external_class_callees,
        local_names: empty_locals,
        callees,
    };
    for dec in &class_def.decorator_list {
        header_visitor.visit_decorator(dec);
    }
    if let Some(args) = &class_def.arguments {
        header_visitor.visit_arguments(args);
    }
    for class_statement in &class_def.body {
        if let Stmt::FunctionDef(method_def) = class_statement {
            let mut sig_visitor = ModuleRefVisitor {
                fn_by_name: item_by_name,
                class_callees: external_class_callees,
                local_names: empty_locals,
                callees,
            };
            for dec in &method_def.decorator_list {
                sig_visitor.visit_decorator(dec);
            }
            sig_visitor.visit_parameters(&method_def.parameters);
            if let Some(returns) = &method_def.returns {
                sig_visitor.visit_expr(returns);
            }
            let method_locals = collect_local_bound_names(method_def, file);
            let mut body_visitor = ModuleRefVisitor {
                fn_by_name: item_by_name,
                class_callees: external_class_callees,
                local_names: &method_locals,
                callees,
            };
            body_visitor.visit_body(&method_def.body);
        } else {
            let mut stmt_visitor = ModuleRefVisitor {
                fn_by_name: item_by_name,
                class_callees: external_class_callees,
                local_names: empty_locals,
                callees,
            };
            stmt_visitor.visit_stmt(class_statement);
        }
    }
}

struct ModuleRefVisitor<'map, 'local> {
    fn_by_name: &'map HashMap<String, usize>,
    class_callees: &'map HashMap<&'map str, Vec<usize>>,
    local_names: &'local HashSet<String>,
    callees: &'local mut Vec<usize>,
}

impl SourceOrderVisitor<'_> for ModuleRefVisitor<'_, '_> {
    fn visit_expr(&mut self, expr: &Expr) {
        if let Expr::Name(name) = expr
            && name.ctx.is_load()
        {
            let ident = name.id.as_str();
            if !self.local_names.contains(ident) {
                if let Some(&idx) = self.fn_by_name.get(ident)
                    && !self.callees.contains(&idx)
                {
                    self.callees.push(idx);
                }
                if let Some(bridged) = self.class_callees.get(ident) {
                    for &idx in bridged {
                        if !self.callees.contains(&idx) {
                            self.callees.push(idx);
                        }
                    }
                }
            }
        }
        walk_expr(self, expr);
    }
}

/// Computes `pinned_before` for each scope item in `items` by walking top-to-bottom runtime
/// evaluations in `scope_body[stmt_range]` (outside `Stmt::FunctionDef` bodies) and propagating
/// constraints through callable call edges.
fn compute_python_pinned_before(
    scope_body: &[Stmt],
    stmt_range: std::ops::Range<usize>,
    first_stmt_indices: &[usize],
    item_by_name: &HashMap<String, usize>,
    items: &[ScopeItem<'_>],
    is_callable: &[bool],
    future_annotations: bool,
) -> Vec<Option<usize>> {
    let count = items.len();
    let mut pinned: Vec<Option<usize>> = vec![None; count];

    for (stmt_idx, stmt) in scope_body
        .iter()
        .enumerate()
        .take(stmt_range.end)
        .skip(stmt_range.start)
    {
        let Some(bound_idx) = first_stmt_indices
            .iter()
            .position(|&decl_stmt_idx| decl_stmt_idx >= stmt_idx)
        else {
            break;
        };
        if bound_idx == 0 {
            continue;
        }
        let mut imm_refs = Vec::new();
        let mut visitor = ImmediateRefVisitor {
            item_by_name,
            future_annotations,
            refs: &mut imm_refs,
        };
        visitor.visit_stmt(stmt);
        for target_idx in imm_refs {
            if target_idx < bound_idx {
                pinned[target_idx] =
                    Some(pinned[target_idx].map_or(bound_idx, |prev| prev.min(bound_idx)));
            }
        }
    }

    // Propagate runtime pins through callable callees: if a function `u` is evaluated before
    // `pin`, any sibling item `v < pin` reachable from `u` must also be defined before `pin`.
    for start in 0..count {
        let Some(pin) = pinned[start] else {
            continue;
        };
        if !is_callable[start] {
            continue;
        }
        let mut stack = vec![start];
        let mut visited = vec![false; count];
        while let Some(cur) = stack.pop() {
            if visited[cur] {
                continue;
            }
            visited[cur] = true;
            for &callee in &items[cur].callees {
                if callee < pin {
                    pinned[callee] = Some(pinned[callee].map_or(pin, |prev| prev.min(pin)));
                    if is_callable[callee] {
                        stack.push(callee);
                    }
                }
            }
        }
    }

    pinned
}

/// Collects the scope items a statement references when the scope body is evaluated, skipping
/// function bodies and postponed annotations.
struct ImmediateRefVisitor<'map, 'out> {
    item_by_name: &'map HashMap<String, usize>,
    future_annotations: bool,
    refs: &'out mut Vec<usize>,
}

impl SourceOrderVisitor<'_> for ImmediateRefVisitor<'_, '_> {
    fn visit_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::FunctionDef(func_def) => {
                for dec in &func_def.decorator_list {
                    self.visit_decorator(dec);
                }
                let eval_annotations = !self.future_annotations && func_def.type_params.is_none();
                for param in func_def
                    .parameters
                    .posonlyargs
                    .iter()
                    .chain(&func_def.parameters.args)
                    .chain(&func_def.parameters.kwonlyargs)
                {
                    if eval_annotations && let Some(ann) = &param.parameter.annotation {
                        self.visit_expr(ann);
                    }
                    if let Some(default) = &param.default {
                        self.visit_expr(default);
                    }
                }
                if eval_annotations {
                    if let Some(vararg) = &func_def.parameters.vararg
                        && let Some(ann) = &vararg.annotation
                    {
                        self.visit_expr(ann);
                    }
                    if let Some(kwarg) = &func_def.parameters.kwarg
                        && let Some(ann) = &kwarg.annotation
                    {
                        self.visit_expr(ann);
                    }
                    if let Some(returns) = &func_def.returns {
                        self.visit_expr(returns);
                    }
                }
            }
            Stmt::ClassDef(class_def) => {
                for dec in &class_def.decorator_list {
                    self.visit_decorator(dec);
                }
                if let Some(args) = &class_def.arguments {
                    self.visit_arguments(args);
                }
                self.visit_body(&class_def.body);
            }
            Stmt::AnnAssign(ann_assign) => {
                self.visit_expr(&ann_assign.target);
                if !self.future_annotations {
                    self.visit_expr(&ann_assign.annotation);
                }
                if let Some(value) = &ann_assign.value {
                    self.visit_expr(value);
                }
            }
            Stmt::TypeAlias(_) => {}
            _ => walk_stmt(self, stmt),
        }
    }

    fn visit_expr(&mut self, expr: &Expr) {
        if let Expr::Name(name) = expr
            && name.ctx.is_load()
            && let Some(&idx) = self.item_by_name.get(name.id.as_str())
            && !self.refs.contains(&idx)
        {
            self.refs.push(idx);
        }
        walk_expr(self, expr);
    }
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
            && resolved_path_and_terminal_expr(&keyword.value, file)
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
    resolved_path_and_terminal_expr(base_expr, file).map(|name| name.path)
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

const OVERLOAD_DECORATOR_PATHS: &[&str] =
    &["overload", "typing.overload", "typing_extensions.overload"];

/// Collects direct functions and classes in `scope_body` in source order, grouping `@overload`
/// signatures and `@<prop>.setter` / `@<prop>.deleter` accessors with their first definition.
fn group_python_scope_items<'a, 'ast>(
    scope_body: &'ast [Stmt],
    is_class_scope: bool,
    file: &'a ParsedFile,
) -> Vec<GroupedPythonScopeItem<'a, 'ast>> {
    struct SeenCallable {
        index: usize,
        last_stmt_idx: usize,
        has_overload: bool,
        has_non_overload: bool,
    }

    let mut items: Vec<GroupedPythonScopeItem<'a, 'ast>> = Vec::new();
    let mut seen_by_name: HashMap<String, SeenCallable> = HashMap::new();

    for (stmt_idx, stmt) in scope_body.iter().enumerate() {
        match stmt {
            Stmt::FunctionDef(function_def) => {
                let name = function_def.name.id.to_string();
                let decorators = extract_decorators_from_slice(&function_def.decorator_list, file);
                let is_overload = decorators
                    .iter()
                    .any(|decorator| decorator.matches_any(OVERLOAD_DECORATOR_PATHS));
                let is_property_accessor = decorators.iter().any(|decorator| {
                    decorator.is_attribute_named(&["getter", "setter", "deleter"])
                });

                if let Some(seen) = seen_by_name.get_mut(&name) {
                    let continues_overload =
                        is_overload || (seen.has_overload && !seen.has_non_overload);
                    if is_property_accessor || continues_overload {
                        seen.has_overload |= is_overload;
                        seen.has_non_overload |= !is_overload;
                        if stmt_idx == seen.last_stmt_idx + 1 {
                            seen.last_stmt_idx = stmt_idx;
                            items[seen.index].end_line =
                                AstNode::from_span(file, span_from_ruff_range(function_def.range))
                                    .end_line();
                        }
                        if let PythonScopeItem::Callable(defs) = &mut items[seen.index].item {
                            defs.push(function_def);
                            continue;
                        }
                    }
                }

                let index = items.len();
                seen_by_name.insert(
                    name.clone(),
                    SeenCallable {
                        index,
                        last_stmt_idx: stmt_idx,
                        has_overload: is_overload,
                        has_non_overload: !is_overload,
                    },
                );
                let visibility = python_method_visibility(&name);
                let is_constructor =
                    is_class_scope && PYTHON_CONSTRUCTOR_NAMES.contains(&name.as_str());
                let def_node = AstNode::from_span(file, span_from_ruff_range(function_def.range));
                items.push(GroupedPythonScopeItem {
                    name_node: AstNode::from_span(
                        file,
                        span_from_ruff_range(function_def.name.range),
                    ),
                    name,
                    start_line: def_node.start_line(),
                    end_line: def_node.end_line(),
                    visibility,
                    is_constructor,
                    first_stmt_idx: stmt_idx,
                    item: PythonScopeItem::Callable(vec![function_def]),
                });
            }
            Stmt::ClassDef(class_def) if !is_class_scope => {
                let name = class_def.name.id.to_string();
                seen_by_name.remove(&name);
                let visibility = python_method_visibility(&name);
                let class_node = AstNode::from_span(file, span_from_ruff_range(class_def.range));
                items.push(GroupedPythonScopeItem {
                    name_node: AstNode::from_span(file, span_from_ruff_range(class_def.name.range)),
                    name,
                    start_line: class_node.start_line(),
                    end_line: class_node.end_line(),
                    visibility,
                    is_constructor: false,
                    first_stmt_idx: stmt_idx,
                    item: PythonScopeItem::Type(class_def),
                });
            }
            _ => {}
        }
    }

    items
}

enum PythonScopeItem<'ast> {
    Callable(Vec<&'ast StmtFunctionDef>),
    Type(&'ast StmtClassDef),
}

/// Classifies the visibility tier of a Python function or method identifier: public or
/// `__dunder__` names are `Public`; single-underscore (`_name`) and name-mangled (`__name`)
/// names are `Private`.
fn python_method_visibility(name: &str) -> MethodVisibility {
    let is_dunder = name.starts_with("__") && name.ends_with("__") && name.len() > 4;
    if is_dunder || !name.starts_with('_') {
        MethodVisibility::Public
    } else {
        MethodVisibility::Private
    }
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
    resolved_path_and_terminal_expr(base, file)
        .is_some_and(|name| INIT_VAR_PATHS.contains(&name.path.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Language;

    #[test]
    fn methods_are_direct_definitions_in_source_order() {
        let source = "class Outer:\n    def b(self): pass\n    x = 1\n    def a(self):\n        def nested(): pass\n    class Inner:\n        def inner(self): pass\n";
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
        let source = "from dataclasses import InitVar\nclass C:\n    a = b = 1\n    c, (d, *e) = f()\n    g: int\n    h: InitVar[int] = 0\n    a: str = ''\n    if x:\n        hidden = 1\n";
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
        let source = "class C:\n    def __init__(self, host, port):\n        self.host = host\n        self.port: int = int(port)\n        self.a, self.b = host, port\n        other.x = 1\n        if host:\n            self.flag = True\n        def bind():\n            self.leak = 1\n        f = lambda: setattr(self, 'y', 1)\n    def __post_init__(self, host):\n        self.later = host\n    def connect(self):\n        self.socket = 1\n";
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
        let source = "class C:\n    def __init__(self, x):\n        self.x = 0\n        self.x: int = x\n    x: int\n";
        assert_eq!(
            attribute_facts(source),
            owned_facts(&[("x", None, false, false)])
        );
    }

    #[test]
    fn constructor_skips_overloads_and_nested_definitions() {
        let source = "from typing import overload\nclass C:\n    @overload\n    def __init__(self, x: int) -> None: ...\n    def __init__(self, x, y=0):\n        pass\n    class Inner:\n        def __init__(self): pass\n";
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

    #[test]
    fn is_enum_resolves_direct_enum_bases() {
        let source = "import enum\nfrom enum import IntEnum as Code\nclass A(enum.Enum): pass\nclass B(Code): pass\nclass C(A): pass\nclass D: pass\n";
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
        let source = "class C:\n    @property\n    def area(self): pass\n    def m(self):\n        def inner(): pass\n";
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
        let source = "class Base: pass\nclass Base: pass\nclass Sub(Base, mod.Base, Later): pass\nclass Later: pass\n";
        let file = ParsedFile::new(source, Language::Python);
        assert_eq!(local_base_classes(&extract_classes(&file), 2), [1]);
    }

    #[test]
    fn ancestors_are_depth_first_left_to_right_without_repeats() {
        let source = "class Root: pass\nclass Left(Root): pass\nclass Right(Root): pass\nclass Sub(Left, Right): pass\n";
        let file = ParsedFile::new(source, Language::Python);
        assert_eq!(local_ancestors(&extract_classes(&file), 3), [1, 0, 2]);
    }
}
