//! Python class definitions, base-class classification, and class/instance attribute walkers.

use super::{
    AstNode, DecoratorInfo, ParsedFile, direct_function_definitions, extract_decorators_from_slice,
    find_expr_at_span, in_place_mutated_receiver_expr, is_bare_final_annotation_expr,
    method_receiver_name_ast, resolve_path_and_terminal_expr,
};
use crate::code_lint::ast::span_from_ruff_range;
use crate::diagnostic::SourceSpan;
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_expr, walk_stmt};
use ruff_python_ast::{Expr, Stmt, StmtAnnAssign, StmtClassDef};
use ruff_text_size::Ranged as _;
use std::collections::HashSet;

const PROTOCOL_CLASS: &str = "Protocol";
const ABC_CLASS: &str = "ABC";
const SELF_RECEIVER: &str = "self";
const DATACLASS_DECORATOR: &str = "dataclass";
const QUALIFIED_DATACLASS_DECORATOR: &str = "dataclasses.dataclass";

/// Represents a base class expression in a Python class definition.
#[derive(Clone)]
pub struct PythonBaseClass<'a> {
    /// AST node for the base class expression.
    pub node: AstNode<'a>,
    /// Base class identifier or dotted path text (e.g. `"Protocol"`, `"abc.ABC"`).
    pub name: String,
}

impl PythonBaseClass<'_> {
    /// Base class name with any generic type argument subscript (`[...]`) stripped.
    #[must_use]
    pub fn unsubscripted_name(&self) -> &str {
        self.name
            .split_once('[')
            .map_or(self.name.as_str(), |(base, _)| base.trim())
    }

    /// Returns true if this base class is a structural marker rather than a supertype:
    /// `object`, `Generic`, `Protocol`, or `ABC` itself (optionally subscripted or qualified).
    #[must_use]
    pub fn is_structural_marker(&self) -> bool {
        matches!(
            self.unsubscripted_name(),
            "object"
                | "builtins.object"
                | "Generic"
                | "typing.Generic"
                | "typing_extensions.Generic"
                | PROTOCOL_CLASS
                | "typing.Protocol"
                | "typing_extensions.Protocol"
                | ABC_CLASS
                | "abc.ABC"
        )
    }
}

/// Structured representation of a Python class definition.
#[derive(Clone)]
pub struct PythonClassInfo<'a> {
    /// The `class_definition` AST node (including any decorators).
    pub node: AstNode<'a>,
    /// Class name identifier text.
    pub name: String,
    /// Class name AST node.
    pub name_node: AstNode<'a>,
    /// Base classes from `class Foo(Base1, Base2):`.
    pub bases: Vec<PythonBaseClass<'a>>,
    /// Parsed decorators on this class definition.
    pub decorators: Vec<DecoratorInfo<'a>>,
    /// The class body `block` node.
    pub body_node: Option<AstNode<'a>>,
}

impl<'a> PythonClassInfo<'a> {
    /// Returns true if the class inherits from any base whose terminal name matches `target`.
    #[must_use]
    pub fn inherits_from(&self, target: &str) -> bool {
        self.bases.iter().any(|base| {
            let unsubscripted = base.unsubscripted_name();
            unsubscripted == target
                || unsubscripted
                    .strip_suffix(target)
                    .is_some_and(|prefix| prefix.ends_with('.'))
        })
    }

    /// The `@dataclass` or `@dataclasses.dataclass` decorator, matched by name rather than by
    /// import, if the class carries one.
    #[must_use]
    pub fn dataclass_decorator(&self) -> Option<&DecoratorInfo<'a>> {
        self.decorators.iter().find(|decorator| {
            matches!(
                decorator.path.as_str(),
                DATACLASS_DECORATOR | QUALIFIED_DATACLASS_DECORATOR
            )
        })
    }

    /// Returns true if the class carries a `@dataclass` or `@dataclasses.dataclass` decorator
    /// that does not pass the keyword argument `key`.
    #[must_use]
    pub fn is_dataclass_missing_arg(&self, key: &str) -> bool {
        self.dataclass_decorator()
            .is_some_and(|decorator| !decorator.has_arg(key))
    }
}

/// Returns the terminal name of each positional base class of a Python `StmtClassDef`,
/// unwrapping generic subscripts (`Protocol[T]` yields `Protocol`).
fn base_class_terminals(class_def: &StmtClassDef, source: &str) -> Vec<String> {
    class_def
        .bases()
        .iter()
        .map(|base| {
            let base_expr = if let Expr::Subscript(subscript) = base {
                subscript.value.as_ref()
            } else {
                base
            };
            resolve_path_and_terminal_expr(base_expr, source).1
        })
        .collect()
}

/// Returns true if a Python `StmtClassDef` inherits from `Protocol` or `ABC` or declares
/// `metaclass=ABCMeta`.
pub(super) fn is_protocol_or_abc_class(class_def: &StmtClassDef, source: &str) -> bool {
    let has_abc_metaclass = class_def.keywords().iter().any(|keyword| {
        keyword
            .arg
            .as_ref()
            .is_some_and(|arg| arg.id == "metaclass")
            && resolve_path_and_terminal_expr(&keyword.value, source).1 == "ABCMeta"
    });
    has_abc_metaclass
        || base_class_terminals(class_def, source)
            .iter()
            .any(|terminal| matches!(terminal.as_str(), PROTOCOL_CLASS | ABC_CLASS))
}

/// Returns true if a Python `StmtClassDef` inherits from `TypedDict`.
pub(super) fn is_typed_dict_class(class_def: &StmtClassDef, source: &str) -> bool {
    base_class_terminals(class_def, source)
        .iter()
        .any(|terminal| terminal == "TypedDict")
}

/// Returns true if `class_def` synthesizes constructor fields from class-body annotations
/// (`@dataclass`, `attrs` decorators, or Pydantic `BaseModel` subclasses).
fn is_field_synthesizing_class(class_def: &StmtClassDef, file: &ParsedFile) -> bool {
    let has_field_decorator = extract_decorators_from_slice(&class_def.decorator_list, file)
        .iter()
        .any(|decorator| {
            matches!(
                decorator.path.as_str(),
                DATACLASS_DECORATOR
                    | QUALIFIED_DATACLASS_DECORATOR
                    | "define"
                    | "frozen"
                    | "mutable"
                    | "attr.s"
                    | "attr.attrs"
                    | "attr.dataclass"
                    | "attrs.define"
                    | "attrs.frozen"
                    | "attrs.mutable"
            )
        });
    has_field_decorator
        || base_class_terminals(class_def, &file.source)
            .iter()
            .any(|terminal| terminal == "BaseModel")
}

/// Returns true if `node` (a method `function_definition` or class attribute node) is directly
/// enclosed in a `Protocol` or `ABC` class definition.
pub(super) fn is_in_protocol_or_abc_class(node: &AstNode<'_>) -> bool {
    struct EnclosingScopeFinder<'a> {
        target_span: SourceSpan,
        source: &'a str,
        in_protocol_or_abc: bool,
        matched: Option<bool>,
    }

    impl<'a> SourceOrderVisitor<'a> for EnclosingScopeFinder<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if self.matched.is_some() {
                return;
            }
            let span = span_from_ruff_range(statement.range());
            if !(span.start <= self.target_span.start && self.target_span.end <= span.end) {
                return;
            }
            if span == self.target_span {
                self.matched = Some(self.in_protocol_or_abc);
                return;
            }
            let prev = self.in_protocol_or_abc;
            match statement {
                Stmt::ClassDef(class_def) => {
                    self.in_protocol_or_abc = is_protocol_or_abc_class(class_def, self.source);
                }
                Stmt::FunctionDef(_) => {
                    self.in_protocol_or_abc = false;
                }
                _ => {}
            }
            walk_stmt(self, statement);
            self.in_protocol_or_abc = prev;
        }

        fn visit_expr(&mut self, expr: &'a Expr) {
            if self.matched.is_some() {
                return;
            }
            let span = span_from_ruff_range(expr.range());
            if !(span.start <= self.target_span.start && self.target_span.end <= span.end) {
                return;
            }
            let prev = self.in_protocol_or_abc;
            if matches!(expr, Expr::Lambda(_)) {
                self.in_protocol_or_abc = false;
            }
            walk_expr(self, expr);
            self.in_protocol_or_abc = prev;
        }
    }

    let Some(parsed) = node.file.py_module() else {
        return false;
    };
    let mut finder = EnclosingScopeFinder {
        target_span: node.span(),
        source: &node.file.source,
        in_protocol_or_abc: false,
        matched: None,
    };
    finder.visit_body(&parsed.syntax().body);
    finder.matched.unwrap_or(false)
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
                let bases = class_def
                    .bases()
                    .iter()
                    .map(|base| {
                        let base_span = span_from_ruff_range(base.range());
                        PythonBaseClass {
                            name: self.file.source[base_span.start..base_span.end].to_string(),
                            node: AstNode::from_span(self.file, base_span),
                        }
                    })
                    .collect();
                let decorators =
                    extract_decorators_from_slice(&class_def.decorator_list, self.file);
                let body_node =
                    class_def
                        .body
                        .first()
                        .zip(class_def.body.last())
                        .map(|(first, last)| {
                            AstNode::from_span(
                                self.file,
                                SourceSpan {
                                    start: usize::from(first.range().start()),
                                    end: usize::from(last.range().end()),
                                },
                            )
                        });
                self.classes.push(PythonClassInfo {
                    node: AstNode::from_span(self.file, span_from_ruff_range(class_def.range)),
                    name: class_def.name.id.to_string(),
                    name_node: AstNode::from_span(
                        self.file,
                        span_from_ruff_range(class_def.name.range),
                    ),
                    bases,
                    decorators,
                    body_node,
                });
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

/// Checks if `receiver` is `self.<attr>`, `cls.<attr>`, or `<class_name>.<attr>`, and inserts
/// `<attr>` into `out`.
fn record_mutated_class_receiver(receiver: &Expr, class_name: &str, out: &mut HashSet<String>) {
    if let Expr::Attribute(attr) = receiver
        && let Expr::Name(object) = attr.value.as_ref()
        && (matches!(object.id.as_str(), SELF_RECEIVER | "cls") || object.id.as_str() == class_name)
    {
        out.insert(attr.attr.to_string());
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
                    let receiver = if let Expr::Subscript(sub) = aug.target.as_ref() {
                        sub.value.as_ref()
                    } else {
                        aug.target.as_ref()
                    };
                    record_mutated_class_receiver(receiver, self.class_name, &mut self.out);
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
                let is_typed_dict = is_typed_dict_class(class_def, &self.file.source);
                let is_in_protocol_or_abc = is_protocol_or_abc_class(class_def, &self.file.source);
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
                    if function_def.name.id == "__init__" {
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
    /// Attribute identifier, without `"self."`.
    pub name: String,
    /// Type annotation AST node (`int` or `Final[int]`).
    pub annotation: AstNode<'a>,
    /// Full `assignment` AST node (`self.foo: int = 1` or `self.foo: int`).
    pub assignment_node: AstNode<'a>,
}

impl PythonInstanceAttributeAnnotation<'_> {
    /// Returns true if the annotation is an unparameterized `Final` qualifier (`Final`,
    /// `typing.Final`, `Annotated[Final, ...]`), which PEP 591 forbids in a class body without
    /// an initializer.
    #[must_use]
    pub fn is_bare_final(&self) -> bool {
        let Some(parsed) = self.annotation.file.py_module() else {
            return false;
        };
        let Some(expr) = find_expr_at_span(parsed.syntax(), self.annotation.span()) else {
            return false;
        };
        is_bare_final_annotation_expr(expr, &self.annotation.file.source)
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
                let class_name = class_def.name.id.to_string();
                let is_in_field_synthesizing_class =
                    is_field_synthesizing_class(class_def, self.file);

                for function_def in direct_function_definitions(&class_def.body) {
                    if method_receiver_name_ast(function_def, false, self.file).is_some() {
                        let method_name = function_def.name.id.to_string();
                        for (ann_assign, attribute_name, type_expr) in
                            collect_self_annotated_assignments(&function_def.body)
                        {
                            self.out.push(PythonInstanceAttributeAnnotation {
                                class_name: class_name.clone(),
                                is_in_field_synthesizing_class,
                                method_name: method_name.clone(),
                                name: attribute_name,
                                annotation: AstNode::from_span(
                                    self.file,
                                    span_from_ruff_range(type_expr.range()),
                                ),
                                assignment_node: AstNode::from_span(
                                    self.file,
                                    span_from_ruff_range(ann_assign.range),
                                ),
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
