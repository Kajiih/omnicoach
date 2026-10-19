//! Python class model: class members, hierarchy, and class/instance attribute walkers.

use super::annotations::is_bare_final_annotation_expr;
use super::class_declarations::{
    PythonClassDeclaration, is_field_synthesizing_class, is_protocol_or_abc_class,
    is_typed_dict_class,
};
use super::decorators::extract_decorators_from_slice;
use super::docstrings::{DocstringSectionKind, PythonDocstring};
use super::effects::in_place_mutated_receiver_expr;
use super::functions::{
    CLS_RECEIVER, OVERLOAD_DECORATOR_PATHS, PythonFunctionSignature, SELF_RECEIVER,
    function_signature, method_receiver_name_ast,
};
use super::names::written_name;
use super::tree::direct_function_definitions;
use crate::code_lint::ast::{AstNode, ParsedFile, PlacementAnchor, span_from_ruff_range};
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

/// Structured representation of a Python class definition: its declaration plus its members and
/// hierarchy queries.
#[derive(Clone)]
pub struct PythonClassInfo<'a> {
    declaration: PythonClassDeclaration<'a>,
}

impl<'a> PythonClassInfo<'a> {
    /// Builds [`PythonClassInfo`] from a parsed `class` statement.
    pub(super) fn from_ast(class_def: &'a StmtClassDef, file: &'a ParsedFile) -> Self {
        Self::from_declaration(PythonClassDeclaration::from_ast(class_def, file))
    }

    /// Wraps a [`PythonClassDeclaration`] as a full [`PythonClassInfo`].
    #[must_use]
    pub const fn from_declaration(declaration: PythonClassDeclaration<'a>) -> Self {
        Self { declaration }
    }

    /// The class's declaration header, base classes, decorators, and docstring.
    #[must_use]
    pub const fn declaration(&self) -> &PythonClassDeclaration<'a> {
        &self.declaration
    }

    /// The functions defined directly in the class body (not nested deeper), in source order.
    #[must_use]
    pub fn methods(&self) -> Vec<PythonFunctionSignature<'a>> {
        let file = self.declaration.name_node.file;
        let definition = self.declaration.definition;
        definition
            .body
            .iter()
            .filter_map(|statement| match statement {
                Stmt::FunctionDef(func_def) => {
                    Some(function_signature(func_def, file, Some(definition), None))
                }
                _ => None,
            })
            .collect()
    }

    /// The `__init__` defined directly in the class body, skipping `@overload` stubs; the last
    /// one when it is redefined.
    #[must_use]
    pub fn constructor(&self) -> Option<PythonFunctionSignature<'a>> {
        let file = self.declaration.name_node.file;
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
    /// method's `self` parameter. Nested functions, lambdas and classes are not read, and
    /// other methods are not read. Private and dunder names are included.
    #[must_use]
    pub fn attribute_definitions(&self) -> Vec<PythonAttributeDefinition<'a>> {
        let file = self.declaration.name_node.file;
        let mut collector = AttributeCollector {
            file,
            seen: HashSet::new(),
            definitions: Vec::new(),
        };
        for statement in &self.declaration.definition.body {
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
        direct_function_definitions(&self.declaration.definition.body)
            .iter()
            .any(|function_def| function_def.name.id == name)
    }

    /// Names of parameters of [`Self::constructor`] that are stored unchanged on `self`
    /// (`self.<name> = <name>` or `self.<name>: <T> = <name>`) and are not rebound in `__init__`.
    #[must_use]
    pub fn unchanged_constructor_parameters(&self) -> HashSet<String> {
        let Some(constructor) = self.constructor() else {
            return HashSet::new();
        };
        let Some(receiver) = method_receiver_name_ast(
            constructor.definition,
            false,
            self.declaration.name_node.file,
        ) else {
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
        let Some(docstring) = &self.declaration.docstring else {
            return HashSet::new();
        };
        let documented = docstring.section_entry_names(DocstringSectionKind::Attributes);
        self.unchanged_constructor_parameters()
            .into_iter()
            .filter(|name| documented.contains(name))
            .collect()
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
        .declaration()
        .bases
        .iter()
        .filter_map(|base| {
            let name = base.unsubscripted_name();
            classes[..index]
                .iter()
                .rposition(|candidate| candidate.declaration().name == name)
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
            .filter(|class| class.declaration().is_enum())
            .map(|class| class.declaration().name.clone())
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

    #[test]
    fn test_extract_classes_bases_metadata() {
        let source = indoc::indoc! {r"
            class FakeService(abc.ABC, Protocol):
                pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let classes = extract_classes(&file);
        let decl = classes[0].declaration();
        let base_names: Vec<&str> = decl.bases.iter().map(|base| base.name.as_str()).collect();
        assert_eq!(classes.len(), 1);
        assert_eq!(decl.name, "FakeService");
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

        let decl = classes[0].declaration();
        assert_eq!(decl.name, "Config");
        assert_eq!(decl.decorators[0].terminal_name, "dataclass");
        assert!(!decl.is_dataclass_missing_arg("frozen"));
        assert!(decl.is_dataclass_missing_arg("slots"));
    }

    #[rstest::rstest]
    #[case::class_level("class C:\n    items: list[int]", &[("items", false, false)])]
    #[case::private_included("class C:\n    _items: list[int]", &[("_items", false, false)])]
    #[case::init_attribute("class C:\n    def __init__(self):\n        self.items: list[int] = []", &[("items", false, false)])]
    #[case::self_mutation("class C:\n    items: list[int]\n    def add(self):\n        self.items.append(1)", &[("items", true, false)])]
    #[case::cls_mutation("class C:\n    items: list[int]\n    @classmethod\n    def add(cls):\n        cls.items.append(1)", &[("items", true, false)])]
    #[case::class_name_mutation("class C:\n    items: list[int]\n    def add(self):\n        C.items.append(1)", &[("items", true, false)])]
    #[case::subscript_delete("class C:\n    items: list[int]\n    def pop(self):\n        del self.items[0]", &[("items", true, false)])]
    #[case::protocol_member("class P(Protocol):\n    items: list[int]", &[("items", false, true)])]
    fn test_collect_class_attributes(
        #[case] source: &str,
        #[case] expected: &[(&str, bool, bool)],
    ) {
        let file = ParsedFile::new(source, Language::Python);
        let actual: Vec<(String, bool, bool)> = collect_class_attributes(&file)
            .into_iter()
            .map(|attribute| {
                (
                    attribute.name,
                    attribute.is_mutated_in_class,
                    attribute.is_in_protocol_or_abc,
                )
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
        assert!(classes[0].declaration().bases.is_empty());
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
            .declaration()
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
        "class C:\n    def __init__(self):\n        self.x: int = 1\n    async def reset(self):\n        if True:\n            self.y: str",
        &[("C", "__init__", "x", "int"), ("C", "reset", "y", "str")]
    )]
    #[case::private_collected_unannotated_skipped(
        "class C:\n    def __init__(self):\n        self._p: int = 1\n        self.pub = 2",
        &[("C", "__init__", "_p", "int")]
    )]
    #[case::staticmethod_classmethod_and_nested_func_skipped(
        "class C:\n    @staticmethod\n    def sm(self):\n        self.a: int = 1\n    @classmethod\n    def cm(cls):\n        cls.b: int = 2\n    def run(self):\n        def inner(self):\n            self.c: int = 3",
        &[]
    )]
    fn test_collect_instance_attribute_annotations(
        #[case] source: &str,
        #[case] expected: &[(&str, &str, &str, &str)],
    ) {
        let file = ParsedFile::new(source, Language::Python);
        let actual: Vec<(String, String, String, String)> =
            collect_instance_attribute_annotations(&file)
                .into_iter()
                .map(|attribute| {
                    (
                        attribute.class_name,
                        attribute.method_name,
                        attribute.name,
                        attribute.annotation.text().into_owned(),
                    )
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
        "class C:\n    def __init__(self):\n        self.a: Final = 1",
        (true, false)
    )]
    #[case::qualified_bare_final(
        "class C:\n    def __init__(self):\n        self.a: typing.Final = 1",
        (true, false)
    )]
    #[case::annotated_bare_final(
        "class C:\n    def __init__(self):\n        self.a: Annotated[Final, 'm'] = 1",
        (true, false)
    )]
    #[case::parameterized_final(
        "class C:\n    def __init__(self):\n        self.a: Final[int] = 1",
        (false, false)
    )]
    #[case::dataclass(
        "@dataclass\nclass C:\n    def __post_init__(self):\n        self.a: int = 1",
        (false, true)
    )]
    #[case::pydantic_model(
        "class C(BaseModel):\n    def model_post_init(self, context):\n        self.a: int = 1",
        (false, true)
    )]
    fn test_instance_attribute_annotation_facts(
        #[case] source: &str,
        #[case] expected: (bool, bool),
    ) {
        let file = ParsedFile::new(source, Language::Python);
        let actual: Vec<(bool, bool)> = collect_instance_attribute_annotations(&file)
            .iter()
            .map(|attribute| {
                (
                    attribute.is_bare_final(),
                    attribute.is_in_field_synthesizing_class,
                )
            })
            .collect();
        assert_eq!(actual, [expected]);
    }
}
