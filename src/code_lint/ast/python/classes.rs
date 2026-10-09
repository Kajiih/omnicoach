//! Python class definitions, base-class classification, and class/instance attribute walkers.

use super::scopes::{collect_local_bound_names, parameters_shadow_name};
use super::{
    AstNode, DecoratorInfo, ParsedFile, direct_function_definitions, extract_decorators_from_slice,
    in_place_mutated_receiver_expr, is_bare_final_annotation_expr, method_receiver_name_ast,
    resolve_path_and_terminal_expr,
};
use crate::code_lint::ast::{
    CallableItem, CallableScope, MethodVisibility, TypeMethod, TypeMethodScope,
    span_from_ruff_range,
};
use crate::diagnostic::SourceSpan;
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_expr, walk_stmt};
use ruff_python_ast::{Expr, Stmt, StmtAnnAssign, StmtClassDef, StmtFunctionDef};
use ruff_text_size::Ranged as _;
use std::collections::{HashMap, HashSet};

const PROTOCOL_CLASS: &str = "Protocol";
const ABC_CLASS: &str = "ABC";
const SELF_RECEIVER: &str = "self";
const CLS_RECEIVER: &str = "cls";
const DATACLASS_DECORATOR: &str = "dataclass";
const QUALIFIED_DATACLASS_DECORATOR: &str = "dataclasses.dataclass";
const PYTHON_CONSTRUCTOR_NAMES: &[&str] = &[
    "__prepare__",
    "__init_subclass__",
    "__new__",
    "__init__",
    "__post_init__",
    "__attrs_pre_init__",
    "__attrs_post_init__",
];

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
                    if function_def.name.id == "__init__"
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
                    | "s"
                    | "attrs"
                    | "attr.s"
                    | "attr.attrs"
                    | "attr.dataclass"
                    | "attrs.define"
                    | "attrs.frozen"
                    | "attrs.mutable"
                    | "attrs.s"
                    | "attrs.attrs"
                    | "attrs.dataclass"
            )
        });
    has_field_decorator
        || base_class_terminals(class_def, &file.source)
            .iter()
            .any(|terminal| terminal == "BaseModel")
}

/// A grouped Python function or method (merging `@overload` stubs and `@property` accessors).
struct GroupedPythonCallable<'a, 'ast> {
    name_node: AstNode<'a>,
    name: String,
    visibility: MethodVisibility,
    is_constructor: bool,
    defs: Vec<&'ast StmtFunctionDef>,
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
    group_python_callables(&class_def.body, true, file)
        .into_iter()
        .map(|callable| TypeMethod {
            name_node: callable.name_node,
            name: callable.name,
            visibility: callable.visibility,
            is_constructor: callable.is_constructor,
        })
        .collect()
}

/// Collects all module and `class` [`CallableScope`]s in a Python `file`.
#[must_use]
pub(in crate::code_lint::ast) fn collect_callable_scopes(
    file: &ParsedFile,
) -> Vec<CallableScope<'_>> {
    struct ScopeCollector<'a> {
        file: &'a ParsedFile,
        scopes: Vec<CallableScope<'a>>,
    }

    impl<'a> SourceOrderVisitor<'a> for ScopeCollector<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            if let Stmt::ClassDef(class_def) = statement
                && let Some(scope) = build_class_callable_scope(class_def, self.file)
            {
                self.scopes.push(scope);
            }
            walk_stmt(self, statement);
        }
    }

    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let scopes = build_module_callable_scopes(&parsed.syntax().body, file);
    let mut collector = ScopeCollector { file, scopes };
    collector.visit_body(&parsed.syntax().body);
    collector.scopes
}

/// Builds [`CallableScope`]s for top-level functions in `module_body`, bridging references
/// through top-level classes in the same module and starting a new scope segment whenever a
/// non-overload, non-property function name is redefined.
fn build_module_callable_scopes<'a>(
    module_body: &[Stmt],
    file: &'a ParsedFile,
) -> Vec<CallableScope<'a>> {
    let grouped = group_python_callables(module_body, false, file);
    if grouped.len() < 2 {
        return Vec::new();
    }

    let mut segments: Vec<Vec<GroupedPythonCallable<'a, '_>>> = Vec::new();
    let mut current_segment: Vec<GroupedPythonCallable<'a, '_>> = Vec::new();
    let mut seen_names: HashSet<String> = HashSet::new();
    for item in grouped {
        if !seen_names.insert(item.name.clone()) {
            segments.push(std::mem::take(&mut current_segment));
            seen_names.clear();
            seen_names.insert(item.name.clone());
        }
        current_segment.push(item);
    }
    if !current_segment.is_empty() {
        segments.push(current_segment);
    }

    let empty_classes = HashMap::new();
    let empty_locals = HashSet::new();
    let mut scopes = Vec::new();

    for segment in segments {
        if segment.len() < 2 {
            continue;
        }
        let fn_by_name: HashMap<String, usize> = segment
            .iter()
            .enumerate()
            .map(|(idx, item)| (item.name.clone(), idx))
            .collect();

        let mut class_callees: HashMap<&str, Vec<usize>> = HashMap::new();
        for statement in module_body {
            if let Stmt::ClassDef(class_def) = statement {
                let mut callees = Vec::new();
                for class_statement in &class_def.body {
                    let method_locals;
                    let local_names = if let Stmt::FunctionDef(method_def) = class_statement {
                        method_locals = collect_local_bound_names(method_def, file);
                        &method_locals
                    } else {
                        &empty_locals
                    };
                    let mut visitor = ModuleRefVisitor {
                        fn_by_name: &fn_by_name,
                        class_callees: &empty_classes,
                        local_names,
                        callees: &mut callees,
                    };
                    visitor.visit_stmt(class_statement);
                }
                if !callees.is_empty() {
                    class_callees.insert(class_def.name.id.as_str(), callees);
                }
            }
        }

        let callables = segment
            .into_iter()
            .map(|group| {
                let mut local_names = HashSet::new();
                for def in &group.defs {
                    local_names.extend(collect_local_bound_names(def, file));
                }
                let mut callees = Vec::new();
                let mut visitor = ModuleRefVisitor {
                    fn_by_name: &fn_by_name,
                    class_callees: &class_callees,
                    local_names: &local_names,
                    callees: &mut callees,
                };
                for def in &group.defs {
                    visitor.visit_body(&def.body);
                }
                CallableItem {
                    name_node: group.name_node,
                    name: group.name,
                    visibility: group.visibility,
                    is_constructor: group.is_constructor,
                    callees,
                }
            })
            .collect();

        scopes.push(CallableScope { callables });
    }

    scopes
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

/// Builds a [`CallableScope`] for direct methods of `class_def`.
fn build_class_callable_scope<'a>(
    class_def: &StmtClassDef,
    file: &'a ParsedFile,
) -> Option<CallableScope<'a>> {
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

    let grouped = group_python_callables(&class_def.body, true, file);
    if grouped.len() < 2 {
        return None;
    }
    let method_by_name: HashMap<String, usize> = grouped
        .iter()
        .enumerate()
        .map(|(idx, item)| (item.name.clone(), idx))
        .collect();
    let class_name = class_def.name.id.as_str();

    let callables = grouped
        .into_iter()
        .map(|group| {
            let mut callees = Vec::new();
            for def in &group.defs {
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
            CallableItem {
                name_node: group.name_node,
                name: group.name,
                visibility: group.visibility,
                is_constructor: group.is_constructor,
                callees,
            }
        })
        .collect();

    Some(CallableScope { callables })
}

/// A type-annotated class or instance attribute (`name: Type` or `name: Type = value`) declared
/// after a method definition in a Python class body.
pub struct PythonFieldAfterMethod<'a> {
    /// Name of the enclosing class.
    pub class_name: String,
    /// Attribute identifier name.
    pub name: String,
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
                let mut seen_method = false;
                for child in &class_def.body {
                    match child {
                        Stmt::FunctionDef(_) => {
                            seen_method = true;
                        }
                        Stmt::AnnAssign(ann) if seen_method => {
                            if let Expr::Name(target) = ann.target.as_ref() {
                                self.out.push(PythonFieldAfterMethod {
                                    class_name: class_name.clone(),
                                    name: target.id.to_string(),
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

/// Collects direct functions in `scope_body` in source order, grouping `@overload` signatures
/// and `@<prop>.setter` / `@<prop>.deleter` accessors with their first definition.
fn group_python_callables<'a, 'ast>(
    scope_body: &'ast [Stmt],
    is_class_scope: bool,
    file: &'a ParsedFile,
) -> Vec<GroupedPythonCallable<'a, 'ast>> {
    struct SeenCallable {
        index: usize,
        has_overload: bool,
        has_non_overload: bool,
    }

    let mut callables: Vec<GroupedPythonCallable<'a, 'ast>> = Vec::new();
    let mut seen_by_name: HashMap<String, SeenCallable> = HashMap::new();

    for function_def in direct_function_definitions(scope_body) {
        let name = function_def.name.id.to_string();
        let decorators = extract_decorators_from_slice(&function_def.decorator_list, file);
        let is_overload = decorators
            .iter()
            .any(|decorator| decorator.terminal_name == "overload");
        let is_property_accessor = decorators.iter().any(|decorator| {
            matches!(
                decorator.terminal_name.as_str(),
                "getter" | "setter" | "deleter"
            )
        });

        if let Some(seen) = seen_by_name.get_mut(&name) {
            let continues_overload = is_overload || (seen.has_overload && !seen.has_non_overload);
            if is_property_accessor || continues_overload {
                seen.has_overload |= is_overload;
                seen.has_non_overload |= !is_overload;
                callables[seen.index].defs.push(function_def);
                continue;
            }
        }

        let index = callables.len();
        seen_by_name.insert(
            name.clone(),
            SeenCallable {
                index,
                has_overload: is_overload,
                has_non_overload: !is_overload,
            },
        );
        let visibility = python_method_visibility(&name);
        let is_constructor = is_class_scope && PYTHON_CONSTRUCTOR_NAMES.contains(&name.as_str());
        callables.push(GroupedPythonCallable {
            name_node: AstNode::from_span(file, span_from_ruff_range(function_def.name.range)),
            name,
            visibility,
            is_constructor,
            defs: vec![function_def],
        });
    }

    callables
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
