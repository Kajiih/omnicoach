//! AST helper predicates for structural traversal in Python.

use crate::code_lint::AstNode;

/// Recursively extracts binding identifiers from a pattern node.
fn extract_from_pattern<'a>(node: &AstNode<'a>, bindings: &mut Vec<AstNode<'a>>) {
    let kind = node.kind();
    match kind.as_ref() {
        "identifier" => {
            if node.text() != "_" {
                bindings.push(node.clone());
            }
        }
        "dotted_name" => {
            // Dotted names containing '.' represent attribute lookups/assignments
            // (e.g., `self.x = 1`), which are attribute modifications rather than new local bindings.
            if !node.text().contains('.') {
                for child in node.children() {
                    extract_from_pattern(&child, bindings);
                }
            }
        }
        "class_pattern" => {
            // In a class match pattern like `case Point(x, y):`, the first child
            // is the class name identifier (`Point`), which is not a variable binding.
            let mut first = true;
            for child in node.children() {
                if first {
                    first = false;
                    continue;
                }
                extract_from_pattern(&child, bindings);
            }
        }
        "keyword_pattern" => {
            // In keyword match patterns like `case Point(x=z):`, the identifier
            // before the `=` (`x`) is the parameter name, and only the value (`z`) is the binding.
            let mut seen_equals = false;
            for child in node.children() {
                if child.kind() == "=" {
                    seen_equals = true;
                    continue;
                }
                if !seen_equals {
                    continue;
                }
                extract_from_pattern(&child, bindings);
            }
        }
        "typed_parameter" | "default_parameter" | "typed_default_parameter" => {
            if let Some(name_node) = node.field("name") {
                extract_from_pattern(&name_node, bindings);
            } else if let Some(first_child) = node.child(0)
                && first_child.kind() == "identifier"
            {
                extract_from_pattern(&first_child, bindings);
            }
        }
        "as_pattern" => {
            if let Some(alias) = node.field("alias") {
                extract_from_pattern(&alias, bindings);
            }
        }
        _ => {
            for child in node.children() {
                extract_from_pattern(&child, bindings);
            }
        }
    }
}

/// Helper to extract the first segment from a dotted name.
fn extract_first_segment<'a>(node: &AstNode<'a>) -> AstNode<'a> {
    node.child(0).unwrap_or_else(|| node.clone())
}

/// Extracts bindings from Python import statements.
fn extract_from_import<'a>(node: &AstNode<'a>, bindings: &mut Vec<AstNode<'a>>) {
    match node.kind().as_ref() {
        "import_statement" => {
            for child in node.children() {
                let kind = child.kind();
                if kind != "import" && kind != "," {
                    extract_from_import(&child, bindings);
                }
            }
        }
        "import_from_statement" => {
            let mut seen_import = false;
            for child in node.children() {
                if child.kind() == "import" {
                    seen_import = true;
                    continue;
                }
                if !seen_import {
                    continue;
                }
                let kind = child.kind();
                if kind != "," && kind != "(" && kind != ")" {
                    extract_from_import(&child, bindings);
                }
            }
        }
        "aliased_import" => {
            if let Some(alias) = node.field("alias")
                && alias.text() != "_"
            {
                bindings.push(alias);
            }
        }
        "dotted_name" | "identifier" => {
            let first_seg = extract_first_segment(node);
            if first_seg.text() != "_" {
                bindings.push(first_seg);
            }
        }
        _ => {}
    }
}

fn traverse_children_skipping<'a>(
    node: &AstNode<'a>,
    skip: Option<&AstNode<'a>>,
    bindings: &mut Vec<AstNode<'a>>,
) {
    for child in node.children() {
        if let Some(skip_node) = skip
            && child.range() == skip_node.range()
        {
            continue;
        }
        traverse_python(&child, bindings);
    }
}

fn traverse_python<'a>(node: &AstNode<'a>, bindings: &mut Vec<AstNode<'a>>) {
    let kind = node.kind();
    match kind.as_ref() {
        "assignment" => {
            let left = node.field("left");
            if let Some(ref left_node) = left {
                extract_from_pattern(left_node, bindings);
            } else if let Some(first_child) = node.child(0) {
                extract_from_pattern(&first_child, bindings);
            }
            traverse_children_skipping(node, left.as_ref(), bindings);
        }
        "for_statement" | "for_in_clause" => {
            let left = node.field("left");
            if let Some(ref left_node) = left {
                extract_from_pattern(left_node, bindings);
            } else {
                let mut found_for = false;
                for child in node.children() {
                    if child.kind() == "for" {
                        found_for = true;
                        continue;
                    }
                    if found_for {
                        extract_from_pattern(&child, bindings);
                        break;
                    }
                }
            }
            traverse_children_skipping(node, left.as_ref(), bindings);
        }
        "as_pattern" => {
            let alias = node.field("alias");
            if let Some(ref alias_node) = alias {
                extract_from_pattern(alias_node, bindings);
            }
            traverse_children_skipping(node, alias.as_ref(), bindings);
        }
        "named_expression" => {
            let name_node = node.field("name");
            if let Some(ref name) = name_node {
                extract_from_pattern(name, bindings);
            }
            traverse_children_skipping(node, name_node.as_ref(), bindings);
        }
        "parameters" | "lambda_parameters" => {
            for child in node.children() {
                if child.kind() != "(" && child.kind() != ")" && child.kind() != "," {
                    extract_from_pattern(&child, bindings);
                }
            }
        }
        "case_clause" => {
            for child in node.children() {
                if child.kind() == "case_pattern" {
                    extract_from_pattern(&child, bindings);
                } else if child.kind() != "case" && child.kind() != ":" {
                    traverse_python(&child, bindings);
                }
            }
        }
        "function_definition" | "class_definition" => {
            let name_node = node.field("name");
            if let Some(ref name) = name_node {
                bindings.push(name.clone());
            }
            traverse_children_skipping(node, name_node.as_ref(), bindings);
        }
        "import_statement" | "import_from_statement" => {
            extract_from_import(node, bindings);
        }
        _ => {
            for child in node.children() {
                traverse_python(&child, bindings);
            }
        }
    }
}

/// Collects all binding definitions (variables, functions, classes, etc.) within a node.
#[must_use]
pub fn collect_bindings<'a>(root: &AstNode<'a>) -> Vec<AstNode<'a>> {
    let mut bindings = Vec::new();
    traverse_python(root, &mut bindings);
    bindings
}

/// Returns true if a Python `function_definition` node is a test function (`test` or `test_*`).
#[must_use]
pub fn is_test_function(func_node: &AstNode<'_>) -> bool {
    func_node.field("name").is_some_and(|name_node| {
        let func_name = name_node.text();
        func_name == "test" || func_name.starts_with("test_")
    })
}

/// Returns true if a Python `call` node is a test assertion call
/// (`self.assert*()`, `pytest.raises(...)`, `raises(...)`, `pytest.warns(...)`, `self.fail(...)`).
#[must_use]
pub fn is_assertion_call(call_node: &AstNode<'_>) -> bool {
    let Some(func) = call_node.field("function") else {
        return false;
    };
    match func.kind().as_ref() {
        "identifier" => func.text() == "raises",
        "attribute" => {
            let Some(attr) = func.field("attribute") else {
                return false;
            };
            let attr_name = attr.text();
            if attr_name.starts_with("assert") {
                return true;
            }
            let Some(obj) = func.field("object") else {
                return false;
            };
            let obj_text = obj.text();
            (obj_text == "pytest" && (attr_name == "raises" || attr_name == "warns"))
                || (obj_text == "self" && attr_name == "fail")
        }
        _ => false,
    }
}

/// Represents a keyword argument (`key=value`) in a Python call or argument list.
#[derive(Clone)]
pub struct KeywordArg<'a> {
    /// The keyword parameter name.
    pub name: String,
    /// AST node for the argument identifier name.
    pub name_node: AstNode<'a>,
    /// AST node for the argument value expression.
    pub value_node: AstNode<'a>,
}

impl KeywordArg<'_> {
    /// Evaluates literal boolean arguments (`True` / `False`).
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self.value_node.kind().as_ref() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    }
}

/// Extracts all keyword arguments from any Python `call` or `argument_list` node.
#[must_use]
pub fn extract_keyword_args<'a>(call_or_args_node: &AstNode<'a>) -> Vec<KeywordArg<'a>> {
    let args_node = if call_or_args_node.kind() == "argument_list" {
        Some(call_or_args_node.clone())
    } else {
        call_or_args_node.field("arguments")
    };

    let Some(args) = args_node else {
        return Vec::new();
    };

    let mut result = Vec::new();
    for child in args.children() {
        if child.kind() == "keyword_argument"
            && let (Some(name_n), Some(val_n)) = (child.field("name"), child.field("value"))
        {
            result.push(KeywordArg {
                name: name_n.text().to_string(),
                name_node: name_n,
                value_node: val_n,
            });
        }
    }
    result
}

/// Structured metadata for a Python decorator.
#[derive(Clone)]
pub struct DecoratorInfo<'a> {
    /// Full decorator AST node (including `@`).
    pub node: AstNode<'a>,
    /// Full path string (e.g. `"dataclasses.dataclass"`, `"pytest.mark.parametrize"`).
    pub path: String,
    /// Terminal identifier (e.g. `"dataclass"`, `"parametrize"`).
    pub terminal_name: String,
    /// Call node if the decorator was invoked with parentheses `@dec(...)`.
    pub call_node: Option<AstNode<'a>>,
    /// Parsed keyword arguments if invoked as a call.
    pub keyword_args: Vec<KeywordArg<'a>>,
}

impl<'a> DecoratorInfo<'a> {
    /// Looks up a keyword argument by name.
    #[must_use]
    pub fn get_arg(&self, key: &str) -> Option<&KeywordArg<'a>> {
        self.keyword_args.iter().find(|kw| kw.name == key)
    }

    /// Returns true if a keyword argument with `key` was explicitly passed.
    #[must_use]
    pub fn has_arg(&self, key: &str) -> bool {
        self.get_arg(key).is_some()
    }
}

/// Helper to resolve the dotted expression path and terminal identifier.
fn resolve_path_and_terminal(expr: &AstNode<'_>) -> (String, String) {
    let path = expr.text().to_string();
    let terminal = expr.field("attribute").map_or_else(
        || path.rsplit('.').next().unwrap_or("").to_string(),
        |attr| attr.text().to_string(),
    );
    (path, terminal)
}

/// Extracts all decorators from a `decorated_definition` or a definition node inside one.
#[must_use]
pub fn extract_decorators<'a>(node: &AstNode<'a>) -> Vec<DecoratorInfo<'a>> {
    let parent = if node.kind() == "decorated_definition" {
        Some(node.clone())
    } else {
        node.parent()
    };

    let Some(dec_def) = parent else {
        return Vec::new();
    };
    if dec_def.kind() != "decorated_definition" {
        return Vec::new();
    }

    let mut decorators = Vec::new();
    for child in dec_def.children() {
        if child.kind() == "decorator" {
            // decorator children: "@" and expression (call or identifier/attribute)
            let expr_node = child.children().find(|c| c.kind() != "@");
            let Some(expr) = expr_node else {
                continue;
            };

            let (call_node, target_expr, keyword_args) = if expr.kind() == "call" {
                let call = expr.clone();
                let func = call.field("function").unwrap_or_else(|| call.clone());
                let kwargs = extract_keyword_args(&call);
                (Some(call), func, kwargs)
            } else {
                (None, expr, Vec::new())
            };

            let (path, terminal_name) = resolve_path_and_terminal(&target_expr);

            decorators.push(DecoratorInfo {
                node: child,
                path,
                terminal_name,
                call_node,
                keyword_args,
            });
        }
    }
    decorators
}

/// Returns true if a Python `function_definition` or `class_definition` has a decorator whose
/// terminal identifier or full path satisfies `predicate`.
#[must_use]
pub fn has_decorator(node: &AstNode<'_>, predicate: impl Fn(&str) -> bool) -> bool {
    let parent = if node.kind() == "decorated_definition" {
        Some(node.clone())
    } else {
        node.parent()
    };
    let Some(dec_def) = parent else {
        return false;
    };
    if dec_def.kind() != "decorated_definition" {
        return false;
    }
    for child in dec_def.children() {
        if child.kind() == "decorator" {
            let text = child.text();
            let trimmed = text.trim().trim_start_matches('@').trim();
            let base_path = trimmed.split('(').next().unwrap_or("").trim();
            let terminal = base_path.rsplit('.').next().unwrap_or("").trim();
            if predicate(terminal) || predicate(base_path) {
                return true;
            }
        }
    }
    false
}

/// Represents a base class expression in a Python class definition.
#[derive(Clone)]
pub struct PythonBaseClass<'a> {
    /// AST node for the base class expression.
    pub node: AstNode<'a>,
    /// Base class identifier or dotted path text (e.g. `"Protocol"`, `"abc.ABC"`).
    pub name: String,
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
    /// Returns the `@dataclass` or `@dataclasses.dataclass` decorator if present on this class.
    #[must_use]
    pub fn dataclass_decorator(&self) -> Option<&DecoratorInfo<'a>> {
        self.decorators
            .iter()
            .find(|dec| matches!(dec.path.as_str(), "dataclass" | "dataclasses.dataclass"))
    }

    /// Returns true if the class is decorated with `@dataclass` or `@dataclasses.dataclass`.
    #[must_use]
    pub fn is_dataclass(&self) -> bool {
        self.dataclass_decorator().is_some()
    }

    /// Returns true if the class inherits from any base whose terminal name matches `target`.
    #[must_use]
    pub fn inherits_from(&self, target: &str) -> bool {
        self.bases.iter().any(|base| {
            base.name == target
                || base
                    .name
                    .strip_suffix(target)
                    .is_some_and(|prefix| prefix.ends_with('.'))
        })
    }
}

/// Discovers and extracts all class definitions from a Python AST document.
#[must_use]
pub fn extract_classes<'a>(root: &AstNode<'a>) -> Vec<PythonClassInfo<'a>> {
    let mut classes = Vec::new();

    // Find all class_definition nodes
    for class_node in root.dfs() {
        if class_node.kind() != "class_definition" {
            continue;
        }

        let Some(name_node) = class_node.field("name") else {
            continue;
        };
        let name = name_node.text().to_string();

        let mut bases = Vec::new();
        if let Some(superclasses) = class_node.field("superclasses") {
            for child in superclasses.children() {
                let kind = child.kind();
                if kind != "(" && kind != ")" && kind != "," && kind != "keyword_argument" {
                    bases.push(PythonBaseClass {
                        name: child.text().to_string(),
                        node: child,
                    });
                }
            }
        }

        let decorators = extract_decorators(&class_node);
        let body_node = class_node.field("body");

        // Use decorated_definition as node if present, otherwise class_node
        let effective_node = if let Some(parent) = class_node.parent()
            && parent.kind() == "decorated_definition"
        {
            parent
        } else {
            class_node
        };

        classes.push(PythonClassInfo {
            node: effective_node,
            name,
            name_node,
            bases,
            decorators,
            body_node,
        });
    }

    classes
}

/// Parameter classification in a Python function signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PythonParameterKind {
    /// Method receiver (`self` or `cls`).
    Receiver,
    /// Standard positional or positional-or-keyword parameter.
    Positional,
    /// Keyword-only parameter declared after `*` or `*args`.
    KeywordOnly,
    /// Positional variadic (`*args`).
    VarPositional,
    /// Keyword variadic (`**kwargs`).
    VarKeyword,
}

/// Structured metadata for a Python parameter.
#[derive(Clone)]
pub struct PythonParameterInfo<'a> {
    /// Full parameter AST node (`typed_parameter`, `default_parameter`, or `identifier`).
    pub node: AstNode<'a>,
    /// Parameter identifier name.
    pub name: String,
    /// Parameter identifier AST node.
    pub name_node: AstNode<'a>,
    /// Type annotation AST node if present (e.g. `list[str]`).
    pub type_node: Option<AstNode<'a>>,
    /// Formatted type annotation text.
    pub type_text: Option<String>,
    /// Default value expression AST node if present.
    pub default_value_node: Option<AstNode<'a>>,
    /// Parameter classification kind.
    pub kind: PythonParameterKind,
}

impl PythonParameterInfo<'_> {
    /// Returns true if the parameter is a standard positional or positional-or-keyword parameter.
    #[must_use]
    pub fn is_positional(&self) -> bool {
        self.kind == PythonParameterKind::Positional
    }

    /// Returns true if the parameter is a variadic (`*args` or `**kwargs`).
    #[must_use]
    pub fn is_variadic(&self) -> bool {
        matches!(
            self.kind,
            PythonParameterKind::VarPositional | PythonParameterKind::VarKeyword
        )
    }
}

/// Internal helper struct for extracted parameter parts.
struct ParsedParamParts<'a> {
    name_node: AstNode<'a>,
    name: String,
    type_node: Option<AstNode<'a>>,
    type_text: Option<String>,
    default_value_node: Option<AstNode<'a>>,
}

/// Extracts parameter name, name node, type annotation, and default value from a parameter node.
fn parse_param_parts<'a>(node: &AstNode<'a>) -> Option<ParsedParamParts<'a>> {
    match node.kind().as_ref() {
        "identifier" => {
            let name = node.text().to_string();
            Some(ParsedParamParts {
                name_node: node.clone(),
                name,
                type_node: None,
                type_text: None,
                default_value_node: None,
            })
        }
        "default_parameter" => {
            let name_node = node.field("name")?;
            let name = name_node.text().to_string();
            let default_val = node.field("value");
            Some(ParsedParamParts {
                name_node,
                name,
                type_node: None,
                type_text: None,
                default_value_node: default_val,
            })
        }
        "typed_parameter" => {
            let name_node = node.field("name").or_else(|| {
                node.children().find_map(|child| {
                    if child.kind() == "identifier" {
                        Some(child)
                    } else if child.kind() == "list_splat_pattern"
                        || child.kind() == "dictionary_splat_pattern"
                    {
                        child.children().find(|sub| sub.kind() == "identifier")
                    } else {
                        None
                    }
                })
            })?;
            let name = name_node.text().to_string();
            let type_node = node.field("type");
            let type_text = type_node.as_ref().map(|type_n| type_n.text().to_string());
            Some(ParsedParamParts {
                name_node,
                name,
                type_node,
                type_text,
                default_value_node: None,
            })
        }
        "typed_default_parameter" => {
            let name_node = node.field("name")?;
            let name = name_node.text().to_string();
            let type_node = node.field("type");
            let type_text = type_node.as_ref().map(|type_n| type_n.text().to_string());
            let default_val = node.field("value");
            Some(ParsedParamParts {
                name_node,
                name,
                type_node,
                type_text,
                default_value_node: default_val,
            })
        }
        "list_splat_pattern" => {
            let name_node = node.children().find(|child| child.kind() == "identifier")?;
            let name = name_node.text().to_string();
            Some(ParsedParamParts {
                name_node,
                name,
                type_node: None,
                type_text: None,
                default_value_node: None,
            })
        }
        "dictionary_splat_pattern" => {
            let name_node = node.children().find(|child| child.kind() == "identifier")?;
            let name = name_node.text().to_string();
            Some(ParsedParamParts {
                name_node,
                name,
                type_node: None,
                type_text: None,
                default_value_node: None,
            })
        }
        _ => None,
    }
}

/// Extracts all parameters in order from a Python `parameters` or `function_definition` node.
#[must_use]
pub fn extract_parameters<'a>(func_or_params_node: &AstNode<'a>) -> Vec<PythonParameterInfo<'a>> {
    let params_node = if func_or_params_node.kind() == "parameters" {
        Some(func_or_params_node.clone())
    } else if let Some(params) = func_or_params_node.field("parameters") {
        Some(params)
    } else {
        func_or_params_node
            .dfs()
            .find(|target_node| target_node.kind() == "parameters")
    };

    let Some(params) = params_node else {
        return Vec::new();
    };

    let mut result = Vec::new();
    let mut seen_keyword_boundary = false;
    let mut is_first_param = true;

    for child in params.children() {
        let child_kind = child.kind();
        if child_kind == "(" || child_kind == ")" || child_kind == "," {
            continue;
        }

        if child_kind == "keyword_separator" {
            seen_keyword_boundary = true;
            continue;
        }

        let is_var_positional = child_kind == "list_splat_pattern"
            || (child_kind == "typed_parameter"
                && child
                    .children()
                    .any(|child_node| child_node.kind() == "list_splat_pattern"));

        let is_var_keyword = child_kind == "dictionary_splat_pattern"
            || (child_kind == "typed_parameter"
                && child
                    .children()
                    .any(|child_node| child_node.kind() == "dictionary_splat_pattern"));

        let Some(parts) = parse_param_parts(&child) else {
            continue;
        };

        let kind = if is_var_positional {
            PythonParameterKind::VarPositional
        } else if is_var_keyword {
            PythonParameterKind::VarKeyword
        } else if is_first_param && matches!(parts.name.as_str(), "self" | "cls") {
            PythonParameterKind::Receiver
        } else if seen_keyword_boundary {
            PythonParameterKind::KeywordOnly
        } else {
            PythonParameterKind::Positional
        };

        if is_var_positional {
            seen_keyword_boundary = true;
        }

        is_first_param = false;

        result.push(PythonParameterInfo {
            node: child,
            name: parts.name,
            name_node: parts.name_node,
            type_node: parts.type_node,
            type_text: parts.type_text,
            default_value_node: parts.default_value_node,
            kind,
        });
    }

    result
}

/// Returns true if a Python `function_definition` is decorated with `@override`.
#[must_use]
pub fn has_override_decorator(func_node: &AstNode<'_>) -> bool {
    has_decorator(func_node, |terminal| terminal == "override")
}

/// Traverses upward from an expression to find if it is enclosed in a `with_item`.
/// Transparently handles expressions wrapped in `parenthesized_expression`.
#[must_use]
pub fn find_enclosing_with_item<'a>(node: &AstNode<'a>) -> Option<AstNode<'a>> {
    for ancestor in node.ancestors() {
        match ancestor.kind().as_ref() {
            "with_item" => return Some(ancestor),
            "parenthesized_expression" => {}
            _ => return None,
        }
    }
    None
}

/// Traverses upward from a node to find its nearest enclosing `with_statement`.
#[must_use]
pub fn find_enclosing_with_statement<'a>(node: &AstNode<'a>) -> Option<AstNode<'a>> {
    node.ancestors()
        .find(|parent| parent.kind() == "with_statement")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast_grep_core::AstGrep;
    use ast_grep_language::SupportLang;

    #[test]
    fn test_collect_bindings_python() {
        let source = r"
import os
import os.path
import numpy as np
from sys import stderr as err
from os import path
import sys, re
from os import * # wildcard
c = 2
d, e = 3, 4
for x in range(10): pass
[y for y in range(10)]
def foo(b: int = 1):
    pass
try:
    pass
except Exception as g:
    pass
(v := 1)
class MyClass:
    def my_method(self):
        pass
        ";
        let grep = AstGrep::new(source, SupportLang::Python);
        let bindings = collect_bindings(&grep.root());
        let names: Vec<String> = bindings
            .iter()
            .map(|node| node.text().to_string())
            .collect();
        assert_eq!(
            names,
            vec![
                "os",
                "os",
                "np",
                "err",
                "path",
                "sys",
                "re",
                "c",
                "d",
                "e",
                "x",
                "y",
                "foo",
                "b",
                "g",
                "v",
                "MyClass",
                "my_method",
                "self",
            ]
        );
    }

    #[test]
    fn test_collect_bindings_python_match_case() {
        let source = r"
match val:
    case Point(x, y=z):
        pass
    case [a, b]:
        pass
        ";
        let grep = AstGrep::new(source, SupportLang::Python);
        let bindings = collect_bindings(&grep.root());
        let names: Vec<String> = bindings
            .iter()
            .map(|node| node.text().to_string())
            .collect();
        assert_eq!(names, vec!["x", "z", "a", "b"]);
    }

    #[test]
    fn test_find_enclosing_with_helpers() {
        let source = r"
with suppress(FileNotFoundError):
    pass
x = suppress(KeyError)
        ";
        let grep = AstGrep::new(source, SupportLang::Python);
        let calls: Vec<_> = grep.root().find_all("suppress($$$ARGS)").collect();
        assert_eq!(calls.len(), 2);

        // First call is inside a with_statement
        let with_item = find_enclosing_with_item(&calls[0]);
        assert!(with_item.is_some());
        assert!(find_enclosing_with_statement(&calls[0]).is_some());

        // Second call is outside a with_statement
        assert!(find_enclosing_with_item(&calls[1]).is_none());
    }

    #[test]
    fn test_extract_decorators_count() {
        let source = r#"
@dataclass(frozen=True, slots=False)
@pytest.mark.parametrize("x", [1, 2])
@custom
def foo():
    pass
        "#;
        let grep = AstGrep::new(source, SupportLang::Python);
        let func = grep.root().find("def foo(): $$$BODY").unwrap();
        let decorators = extract_decorators(&func);
        assert_eq!(decorators.len(), 3);
    }

    #[test]
    fn test_extract_decorator_keyword_args() {
        let source = r"
@dataclass(frozen=True, slots=False)
def foo():
    pass
        ";
        let grep = AstGrep::new(source, SupportLang::Python);
        let func = grep.root().find("def foo(): $$$BODY").unwrap();
        let decorators = extract_decorators(&func);

        let dec0 = &decorators[0];
        assert_eq!(dec0.terminal_name, "dataclass");
        assert_eq!(
            dec0.get_arg("frozen").and_then(KeywordArg::as_bool),
            Some(true)
        );
        assert_eq!(
            dec0.get_arg("slots").and_then(KeywordArg::as_bool),
            Some(false)
        );
        assert!(dec0.get_arg("unknown").is_none());
    }

    #[test]
    fn test_extract_decorators_metadata_and_path() {
        let source = r#"
@pytest.mark.parametrize("x", [1, 2])
@custom
def foo():
    pass
        "#;
        let grep = AstGrep::new(source, SupportLang::Python);
        let func = grep.root().find("def foo(): $$$BODY").unwrap();
        let decorators = extract_decorators(&func);

        let dec0 = &decorators[0];
        assert_eq!(dec0.terminal_name, "parametrize");
        assert_eq!(dec0.path, "pytest.mark.parametrize");

        let dec1 = &decorators[1];
        assert_eq!(dec1.terminal_name, "custom");
        assert!(dec1.call_node.is_none());
    }

    #[test]
    fn test_has_decorator_predicate_matching() {
        let source = r#"
@pytest.mark.parametrize("x", [1, 2])
def foo():
    pass
        "#;
        let grep = AstGrep::new(source, SupportLang::Python);
        let func = grep.root().find("def foo(): $$$BODY").unwrap();

        assert!(has_decorator(&func, |name| name == "parametrize"));
        assert!(has_decorator(&func, |path| path == "pytest.mark.parametrize"));
        assert!(!has_decorator(&func, |name| name == "override"));
    }

    #[test]
    fn test_extract_classes_and_inheritance() {
        let source = r"
class FakeService(abc.ABC, Protocol):
    pass
        ";
        let grep = AstGrep::new(source, SupportLang::Python);
        let classes = extract_classes(&grep.root());
        assert_eq!(classes.len(), 1);

        let cls = &classes[0];
        assert_eq!(cls.name, "FakeService");
        assert!(cls.inherits_from("Protocol"));
        assert!(cls.inherits_from("ABC"));
    }

    #[test]
    fn test_extract_classes_bases_metadata() {
        let source = r"
class FakeService(abc.ABC, Protocol):
    pass
        ";
        let grep = AstGrep::new(source, SupportLang::Python);
        let classes = extract_classes(&grep.root());
        let cls = &classes[0];
        assert_eq!(cls.bases.len(), 2);
        assert_eq!(cls.bases[0].name, "abc.ABC");
        assert_eq!(cls.bases[1].name, "Protocol");
    }

    #[test]
    fn test_extract_classes_dataclass() {
        let source = r"
@dataclass(frozen=True)
class Config:
    pass
        ";
        let grep = AstGrep::new(source, SupportLang::Python);
        let classes = extract_classes(&grep.root());
        assert_eq!(classes.len(), 1);

        let cls = &classes[0];
        assert_eq!(cls.name, "Config");
        assert!(cls.is_dataclass());
        assert!(!cls.inherits_from("Protocol"));
    }

    #[test]
    fn test_extract_parameters_kinds() {
        let source = r"
def handler(self, a: int, b: str = 'hello', *, c: bool, **kwargs):
    pass
        ";
        let grep = AstGrep::new(source, SupportLang::Python);
        let params = extract_parameters(&grep.root());

        assert_eq!(params[0].kind, PythonParameterKind::Receiver);
        assert_eq!(params[1].kind, PythonParameterKind::Positional);
        assert_eq!(params[3].kind, PythonParameterKind::KeywordOnly);
        assert_eq!(params[4].kind, PythonParameterKind::VarKeyword);
    }

    #[test]
    fn test_extract_parameters_type_annotations() {
        let source = r"
def handler(self, a: int, b: str = 'hello', *, c: bool, **kwargs):
    pass
        ";
        let grep = AstGrep::new(source, SupportLang::Python);
        let params = extract_parameters(&grep.root());

        assert_eq!(params[1].name, "a");
        assert_eq!(params[1].type_text.as_deref(), Some("int"));
        assert_eq!(params[2].name, "b");
        assert_eq!(params[2].type_text.as_deref(), Some("str"));
    }
}
