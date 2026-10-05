//! Python class definitions, base-class classification, and class/instance attribute walkers.

// omni:disable-file [repeated-literal] -- Tree-sitter node kinds and field names (see ROADMAP)

use super::{
    AstNode, DecoratorInfo, ParsedFile, RawNode, direct_function_definitions,
    extract_decorators_raw, in_place_mutated_receiver, is_bare_final_annotation,
    method_receiver_name, resolve_path_and_terminal_raw,
};
use std::collections::HashSet;

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
                | "Protocol"
                | "typing.Protocol"
                | "typing_extensions.Protocol"
                | "ABC"
                | "abc.ABC"
        )
    }
}

/// Structured representation of a Python class definition.
#[derive(Clone)]
pub struct PythonClassInfo<'a> {
    /// The `class_definition` AST node (or enclosing `decorated_definition`).
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
                "dataclass" | "dataclasses.dataclass"
            )
        })
    }
}

/// Returns the positional base-class expression nodes from a `class_definition` node,
/// skipping `keyword_argument` (`metaclass=...`) and `dictionary_splat` (`**kwargs`).
fn positional_base_class_nodes<'a>(class_node: &RawNode<'a>) -> Vec<RawNode<'a>> {
    let Some(superclasses) = class_node.field("superclasses") else {
        return Vec::new();
    };
    superclasses
        .children()
        .filter(|child| {
            child.is_named()
                && !child.is_extra()
                && !matches!(
                    child.kind().as_ref(),
                    "keyword_argument" | "dictionary_splat"
                )
        })
        .collect()
}

/// Returns the terminal name of each positional base class of a Python `class_definition`,
/// unwrapping generic subscripts (`Protocol[T]` yields `Protocol`).
fn base_class_terminals_raw(class_node: &RawNode<'_>) -> Vec<String> {
    positional_base_class_nodes(class_node)
        .into_iter()
        .map(|child| {
            let base_expr = if matches!(child.kind().as_ref(), "subscript" | "generic_type") {
                child
                    .field("value")
                    .or_else(|| {
                        child
                            .children()
                            .find(|inner| inner.is_named() && !inner.is_extra())
                    })
                    .unwrap_or(child)
            } else {
                child
            };
            resolve_path_and_terminal_raw(&base_expr).1
        })
        .collect()
}

/// Returns true if a Python `class_definition` inherits from `Protocol` or `ABC` or declares
/// `metaclass=ABCMeta`.
pub(super) fn is_protocol_or_abc_class_raw(class_node: &RawNode<'_>) -> bool {
    let has_abc_metaclass = class_node
        .field("superclasses")
        .is_some_and(|superclasses| {
            superclasses.children().any(|child| {
                child.kind() == "keyword_argument"
                    && child
                        .field("name")
                        .is_some_and(|name_node| name_node.text() == "metaclass")
                    && child.field("value").is_some_and(|value_node| {
                        let (_, terminal) = resolve_path_and_terminal_raw(&value_node);
                        terminal == "ABCMeta"
                    })
            })
        });
    has_abc_metaclass
        || base_class_terminals_raw(class_node)
            .iter()
            .any(|terminal| matches!(terminal.as_str(), "Protocol" | "ABC"))
}

/// Returns true if a Python `class_definition` inherits from `TypedDict`.
pub(super) fn is_typed_dict_class_raw(class_node: &RawNode<'_>) -> bool {
    base_class_terminals_raw(class_node)
        .iter()
        .any(|terminal| terminal == "TypedDict")
}

/// Returns true if `class_node` synthesizes constructor fields from class-body annotations
/// (`@dataclass`, `attrs` decorators, or Pydantic `BaseModel` subclasses).
fn is_field_synthesizing_class_raw(class_node: &RawNode<'_>) -> bool {
    let has_field_decorator = extract_decorators_raw(class_node).iter().any(|decorator| {
        matches!(
            decorator.path.as_str(),
            "dataclass"
                | "dataclasses.dataclass"
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
        || base_class_terminals_raw(class_node)
            .iter()
            .any(|terminal| terminal == "BaseModel")
}

/// Returns true if `node` (a method `function_definition` or class attribute node) is directly
/// enclosed in a `Protocol` or `ABC` class definition.
pub(super) fn is_in_protocol_or_abc_class(node: &AstNode<'_>) -> bool {
    for ancestor in node.raw.ancestors() {
        match ancestor.kind().as_ref() {
            "function_definition" | "lambda" => return false,
            "class_definition" => return is_protocol_or_abc_class_raw(&ancestor),
            _ => {}
        }
    }
    false
}

/// Discovers and extracts all class definitions from a Python file.
#[must_use]
pub fn extract_classes(file: &ParsedFile) -> Vec<PythonClassInfo<'_>> {
    let mut classes = Vec::new();

    for class_node in file.grep.root().dfs() {
        if class_node.kind() != "class_definition" {
            continue;
        }

        let Some(name_node) = class_node.field("name") else {
            continue;
        };
        let name = name_node.text().to_string();

        let bases = positional_base_class_nodes(&class_node)
            .into_iter()
            .map(|child| PythonBaseClass {
                name: child.text().to_string(),
                node: AstNode::from_raw(child),
            })
            .collect();

        let decorators = extract_decorators_raw(&class_node);
        let body_node = class_node.field("body").map(AstNode::from_raw);

        let effective_node = if let Some(parent) = class_node.parent()
            && parent.kind() == "decorated_definition"
        {
            parent
        } else {
            class_node
        };

        classes.push(PythonClassInfo {
            node: AstNode::from_raw(effective_node),
            name,
            name_node: AstNode::from_raw(name_node),
            bases,
            decorators,
            body_node,
        });
    }

    classes
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

/// Walks `node` (without entering nested `class_definition`s) and records attribute names
/// mutated in place on `self`, `cls`, or `class_name`.
fn collect_mutated_class_attr_names_rec(
    node: &RawNode<'_>,
    class_name: &str,
    out: &mut HashSet<String>,
) {
    if node.kind() == "class_definition" {
        return;
    }
    if let Some(receiver) = in_place_mutated_receiver(node)
        && receiver.kind() == "attribute"
        && let (Some(object_node), Some(attribute_node)) =
            (receiver.field("object"), receiver.field("attribute"))
    {
        let object_text = object_node.text();
        if matches!(object_text.as_ref(), "self" | "cls") || object_text == class_name {
            out.insert(attribute_node.text().into_owned());
        }
    }
    for child in node.children() {
        collect_mutated_class_attr_names_rec(&child, class_name, out);
    }
}

/// Walks statements inside a method body (without entering nested functions, classes, or lambdas)
/// and collects `(assignment_node, attr_name, type_node)` for `self.<attr>: <type>` annotations.
fn collect_self_annotated_assignments_rec<'a>(
    node: &RawNode<'a>,
    out: &mut Vec<(RawNode<'a>, String, RawNode<'a>)>,
) {
    if matches!(
        node.kind().as_ref(),
        "function_definition" | "class_definition" | "lambda"
    ) {
        return;
    }
    if node.kind() == "assignment"
        && let (Some(left), Some(type_node)) = (node.field("left"), node.field("type"))
        && left.kind() == "attribute"
        && left.field("object").is_some_and(|object_node| {
            object_node.kind() == "identifier" && object_node.text() == "self"
        })
        && let Some(attribute_node) = left.field("attribute")
    {
        out.push((node.clone(), attribute_node.text().into_owned(), type_node));
    }
    for child in node.children() {
        collect_self_annotated_assignments_rec(&child, out);
    }
}

/// Collects annotated class and `__init__` attributes, recording whether each attribute is
/// mutated in place within its class.
#[must_use]
pub fn collect_class_attributes(file: &ParsedFile) -> Vec<PythonAnnotatedAttribute<'_>> {
    let mut out = Vec::new();
    for class_node in file.grep.root().dfs() {
        if class_node.kind() != "class_definition" {
            continue;
        }
        let Some(name_node) = class_node.field("name") else {
            continue;
        };
        let Some(body) = class_node.field("body") else {
            continue;
        };
        let class_name = name_node.text().into_owned();
        let is_typed_dict = is_typed_dict_class_raw(&class_node);
        let is_in_protocol_or_abc = is_protocol_or_abc_class_raw(&class_node);

        let mut mutated_attrs = HashSet::new();
        for child in body.children() {
            collect_mutated_class_attr_names_rec(&child, &class_name, &mut mutated_attrs);
        }

        for child in body.children() {
            if child.kind() == "expression_statement"
                && let Some(assign) = child
                    .children()
                    .find(|inner| inner.is_named() && !inner.is_extra())
                && assign.kind() == "assignment"
                && let (Some(left), Some(type_node)) = (assign.field("left"), assign.field("type"))
                && left.kind() == "identifier"
            {
                let attr_name = left.text().into_owned();
                let is_mutated_in_class = mutated_attrs.contains(&attr_name);
                out.push(PythonAnnotatedAttribute {
                    class_name: class_name.clone(),
                    is_in_protocol_or_abc,
                    name: attr_name,
                    type_node: AstNode::from_raw(type_node),
                    is_mutated_in_class,
                    is_typed_dict_key: is_typed_dict,
                });
            }
        }

        for (_, function_node) in direct_function_definitions(&body) {
            if function_node
                .field("name")
                .is_some_and(|method_name| method_name.text() == "__init__")
                && let Some(init_body) = function_node.field("body")
            {
                let mut annotated = Vec::new();
                for statement in init_body.children() {
                    collect_self_annotated_assignments_rec(&statement, &mut annotated);
                }
                for (_, attr_name, type_node) in annotated {
                    let is_mutated_in_class = mutated_attrs.contains(&attr_name);
                    out.push(PythonAnnotatedAttribute {
                        class_name: class_name.clone(),
                        is_in_protocol_or_abc,
                        name: attr_name,
                        type_node: AstNode::from_raw(type_node),
                        is_mutated_in_class,
                        is_typed_dict_key: false,
                    });
                }
            }
        }
    }
    out
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
        is_bare_final_annotation(&self.annotation.raw)
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
    let mut out = Vec::new();
    for class_node in file.grep.root().dfs() {
        if class_node.kind() != "class_definition" {
            continue;
        }
        let Some(name_node) = class_node.field("name") else {
            continue;
        };
        let Some(body) = class_node.field("body") else {
            continue;
        };
        let class_name = name_node.text().into_owned();
        let is_in_field_synthesizing_class = is_field_synthesizing_class_raw(&class_node);

        for (_, function_node) in direct_function_definitions(&body) {
            if method_receiver_name(&function_node, false).is_some()
                && let Some(method_name_node) = function_node.field("name")
                && let Some(method_body) = function_node.field("body")
            {
                let method_name = method_name_node.text().into_owned();
                let mut annotated = Vec::new();
                for statement in method_body.children() {
                    collect_self_annotated_assignments_rec(&statement, &mut annotated);
                }
                for (assignment_node, attribute_name, type_node) in annotated {
                    out.push(PythonInstanceAttributeAnnotation {
                        class_name: class_name.clone(),
                        is_in_field_synthesizing_class,
                        method_name: method_name.clone(),
                        name: attribute_name,
                        annotation: AstNode::from_raw(type_node),
                        assignment_node: AstNode::from_raw(assignment_node),
                    });
                }
            }
        }
    }
    out
}
