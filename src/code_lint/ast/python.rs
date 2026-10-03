//! AST helper predicates and structural extractors for Python.

use crate::code_lint::ast::{AstNode, ParsedFile, PositionalRead, RawNode, ScopePositionalReads};

/// Returns true for Python node kinds that hold statements as direct children.
///
/// `module` is the file root and `block` is an indented suite.
#[must_use]
pub(super) fn is_statement_container(kind: &str) -> bool {
    matches!(kind, "module" | "block")
}

/// Returns true for Python comment node kinds.
///
/// Python spells every comment `comment`, whether or not it is used as documentation.
#[must_use]
pub(super) fn is_comment_kind(kind: &str) -> bool {
    kind == "comment"
}

/// Returns true if a Python node of `parent_kind` makes a child identifier an import binding.
#[must_use]
pub(super) fn is_import_binding_parent(parent_kind: &str) -> bool {
    matches!(
        parent_kind,
        "import_statement" | "import_from_statement" | "aliased_import" | "dotted_name"
    )
}

/// Returns true if a Python node of `parent_kind` declares a structural definition name.
#[must_use]
pub(super) fn is_structural_definition_parent(parent_kind: &str) -> bool {
    matches!(parent_kind, "class_definition" | "function_definition")
}

/// Returns true if `kind` is a call expression in Python.
#[must_use]
pub(super) fn is_call_kind(kind: &str) -> bool {
    kind == "call"
}

/// If `function` is a method access (e.g. `obj.method`), returns the method identifier node.
#[must_use]
pub(super) fn extract_method_call_target<'a>(function: &RawNode<'a>) -> Option<RawNode<'a>> {
    if function.kind().as_ref() == "attribute" {
        function.field("attribute")
    } else {
        None
    }
}

/// Returns true if `item`, the definition owning a name, has that name mandated by a contract.
///
/// Python has no structural trait implementations, so the contract is an explicit
/// `@override` decorator on a method.
#[must_use]
pub fn is_trait_impl_member(item: &AstNode<'_>) -> bool {
    item.raw.kind().as_ref() == "function_definition" && has_override_decorator(item)
}

/// Recursively extracts binding identifiers from a pattern node.
fn extract_from_pattern<'a>(node: &RawNode<'a>, bindings: &mut Vec<AstNode<'a>>) {
    let kind = node.kind();
    match kind.as_ref() {
        "identifier" => {
            if node.text() != "_" {
                bindings.push(AstNode::from_raw(node.clone()));
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
fn extract_first_segment<'a>(node: &RawNode<'a>) -> RawNode<'a> {
    node.child(0).unwrap_or_else(|| node.clone())
}

/// Extracts bindings from Python import statements.
fn extract_from_import<'a>(node: &RawNode<'a>, bindings: &mut Vec<AstNode<'a>>) {
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
                bindings.push(AstNode::from_raw(alias));
            }
        }
        "dotted_name" | "identifier" => {
            let first_seg = extract_first_segment(node);
            if first_seg.text() != "_" {
                bindings.push(AstNode::from_raw(first_seg));
            }
        }
        _ => {}
    }
}

fn traverse_children_skipping<'a>(
    node: &RawNode<'a>,
    skip: Option<&RawNode<'a>>,
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

fn traverse_python<'a>(node: &RawNode<'a>, bindings: &mut Vec<AstNode<'a>>) {
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
                bindings.push(AstNode::from_raw(name.clone()));
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

/// Collects all binding definitions (variables, functions, classes, etc.) within a Python file.
#[must_use]
pub fn collect_bindings(file: &ParsedFile) -> Vec<AstNode<'_>> {
    let mut bindings = Vec::new();
    traverse_python(&file.grep.root(), &mut bindings);
    bindings
}

/// Returns true if a Python `function_definition` node is a test function (`test` or `test_*`).
fn is_test_function_raw(func_node: &RawNode<'_>) -> bool {
    func_node.field("name").is_some_and(|name_node| {
        let func_name = name_node.text();
        func_name == "test" || func_name.starts_with("test_")
    })
}

/// Returns true if a Python `call` node is a test assertion call
/// (`self.assert*()`, `pytest.raises(...)`, `raises(...)`, `pytest.warns(...)`, `self.fail(...)`).
fn is_assertion_call_raw(call_node: &RawNode<'_>) -> bool {
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
        match self.value_node.raw.kind().as_ref() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    }
}

/// Extracts all keyword arguments from any Python `call` or `argument_list` node.
fn extract_keyword_args_raw<'a>(call_or_args_node: &RawNode<'a>) -> Vec<KeywordArg<'a>> {
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
                name_node: AstNode::from_raw(name_n),
                value_node: AstNode::from_raw(val_n),
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
fn resolve_path_and_terminal_raw(expr: &RawNode<'_>) -> (String, String) {
    let path = expr.text().to_string();
    let terminal = expr.field("attribute").map_or_else(
        || path.rsplit('.').next().unwrap_or("").to_string(),
        |attr| attr.text().to_string(),
    );
    (path, terminal)
}

/// Extracts all decorators from a `decorated_definition` or a definition node inside one.
fn extract_decorators_raw<'a>(node: &RawNode<'a>) -> Vec<DecoratorInfo<'a>> {
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
                let kwargs = extract_keyword_args_raw(&call);
                (Some(AstNode::from_raw(call)), func, kwargs)
            } else {
                (None, expr, Vec::new())
            };

            let (path, terminal_name) = resolve_path_and_terminal_raw(&target_expr);

            decorators.push(DecoratorInfo {
                node: AstNode::from_raw(child),
                path,
                terminal_name,
                call_node,
                keyword_args,
            });
        }
    }
    decorators
}

/// Extracts all decorators from a `decorated_definition` or a definition node inside one.
#[must_use]
pub fn extract_decorators<'a>(node: &AstNode<'a>) -> Vec<DecoratorInfo<'a>> {
    extract_decorators_raw(&node.raw)
}

/// Returns true if a Python `function_definition` or `class_definition` has a decorator whose
/// terminal identifier or full path satisfies `predicate`.
#[must_use]
pub fn has_decorator(node: &AstNode<'_>, predicate: impl Fn(&str) -> bool) -> bool {
    extract_decorators(node)
        .into_iter()
        .any(|dec| predicate(&dec.terminal_name) || predicate(&dec.path))
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

/// Discovers and extracts all class definitions from a Python file.
#[must_use]
pub fn extract_classes(file: &ParsedFile) -> Vec<PythonClassInfo<'_>> {
    let mut classes = Vec::new();

    // Find all class_definition nodes
    for class_node in file.grep.root().dfs() {
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
                        node: AstNode::from_raw(child),
                    });
                }
            }
        }

        let decorators = extract_decorators_raw(&class_node);
        let body_node = class_node.field("body").map(AstNode::from_raw);

        // Use decorated_definition as node if present, otherwise class_node
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
    pub const fn is_positional(&self) -> bool {
        matches!(self.kind, PythonParameterKind::Positional)
    }

    /// Returns true if the parameter is a variadic (`*args` or `**kwargs`).
    #[must_use]
    pub const fn is_variadic(&self) -> bool {
        matches!(
            self.kind,
            PythonParameterKind::VarPositional | PythonParameterKind::VarKeyword
        )
    }
}

/// Structured representation of a Python function signature.
#[derive(Clone)]
pub struct PythonFunctionSignature<'a> {
    /// The `function_definition` AST node.
    pub node: AstNode<'a>,
    /// Function identifier AST node.
    pub name_node: AstNode<'a>,
    /// Function identifier name.
    pub name: String,
    /// Parsed parameters in declaration order.
    pub parameters: Vec<PythonParameterInfo<'a>>,
    /// Return type annotation AST node (`-> <type>`), if present.
    pub return_type_node: Option<AstNode<'a>>,
}

impl PythonFunctionSignature<'_> {
    /// Returns true if the function is exempt from signature annotation rules (data-model dunder
    /// methods other than `__init__`/`__new__`, functions decorated with `@override`, `@overload`,
    /// `@abstractmethod`, or `@fixture`, or methods on `Protocol` or `ABC` classes).
    #[must_use]
    pub fn is_exempt_from_signature_rules(&self) -> bool {
        is_exempt_dunder_method(&self.name)
            || has_exempt_signature_decorator(&self.node)
            || is_in_protocol_or_abc_class(&self.node)
    }

    /// Returns true if the function is exempt from body-usage parameter rules (signature-exempt
    /// functions or stub bodies consisting only of `...`, `pass`, or `raise NotImplementedError`).
    #[must_use]
    pub fn is_exempt_from_body_usage_rules(&self) -> bool {
        self.is_exempt_from_signature_rules() || is_stub_function_body(&self.node)
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
fn parse_param_parts<'a>(node: &RawNode<'a>) -> Option<ParsedParamParts<'a>> {
    match node.kind().as_ref() {
        "identifier" => {
            let name = node.text().to_string();
            Some(ParsedParamParts {
                name_node: AstNode::from_raw(node.clone()),
                name,
                type_node: None,
                type_text: None,
                default_value_node: None,
            })
        }
        "default_parameter" => {
            let name_node = node.field("name")?;
            let name = name_node.text().to_string();
            let default_val = node.field("value").map(AstNode::from_raw);
            Some(ParsedParamParts {
                name_node: AstNode::from_raw(name_node),
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
                name_node: AstNode::from_raw(name_node),
                name,
                type_node: type_node.map(AstNode::from_raw),
                type_text,
                default_value_node: None,
            })
        }
        "typed_default_parameter" => {
            let name_node = node.field("name")?;
            let name = name_node.text().to_string();
            let type_node = node.field("type");
            let type_text = type_node.as_ref().map(|type_n| type_n.text().to_string());
            let default_val = node.field("value").map(AstNode::from_raw);
            Some(ParsedParamParts {
                name_node: AstNode::from_raw(name_node),
                name,
                type_node: type_node.map(AstNode::from_raw),
                type_text,
                default_value_node: default_val,
            })
        }
        "list_splat_pattern" | "dictionary_splat_pattern" => {
            let name_node = node.children().find(|child| child.kind() == "identifier")?;
            let name = name_node.text().to_string();
            Some(ParsedParamParts {
                name_node: AstNode::from_raw(name_node),
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
fn extract_parameters_raw<'a>(func_or_params_node: &RawNode<'a>) -> Vec<PythonParameterInfo<'a>> {
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
            node: AstNode::from_raw(child),
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

/// Extracts all parameters in order from a Python `parameters` or `function_definition` node.
#[must_use]
pub fn extract_parameters<'a>(func_or_params_node: &AstNode<'a>) -> Vec<PythonParameterInfo<'a>> {
    extract_parameters_raw(&func_or_params_node.raw)
}

/// Discovers and extracts all function signatures from a Python file.
#[must_use]
pub fn extract_function_signatures(file: &ParsedFile) -> Vec<PythonFunctionSignature<'_>> {
    let mut signatures = Vec::new();
    for node in file.grep.root().dfs() {
        if node.kind() != "function_definition" {
            continue;
        }
        let Some(name_node) = node.field("name") else {
            continue;
        };
        let Some(params_node) = node.field("parameters") else {
            continue;
        };
        let name = name_node.text().to_string();
        let parameters = extract_parameters_raw(&params_node);
        let return_type_node = node.field("return_type").map(AstNode::from_raw);
        signatures.push(PythonFunctionSignature {
            node: AstNode::from_raw(node),
            name_node: AstNode::from_raw(name_node),
            name,
            parameters,
            return_type_node,
        });
    }
    signatures
}

/// Controls how deeply [`collect_mutable_collection_types`] traverses a Python type annotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnotationTraversalDepth {
    /// Unwraps only transparent wrappers (`|`, `Optional`, `Union`, `Annotated[T, ...]`,
    /// `ClassVar[T]`, `Final[T]`, `Required[T]`, `NotRequired[T]`, `ReadOnly[T]`).
    TransparentWrappersOnly,
    /// Unwraps transparent wrappers and recurses into known covariant generic type parameter
    /// positions (`Sequence[T]`, `Mapping[K, V]` value `V`, `tuple[...]`, `Awaitable[T]`,
    /// `Callable[[...], Ret]` return `Ret`, `Generator`/`Coroutine` yield & return, etc.).
    CovariantPositions,
}

/// Returns true if `(path, terminal)` refers to an unqualified or standard-library (`typing`,
/// `typing_extensions`, `collections.abc`, `builtins`) type constructor.
fn is_std_type_constructor_prefix(path: &str, terminal: &str) -> bool {
    path == terminal
        || path.strip_suffix(terminal).is_some_and(|prefix| {
            matches!(
                prefix,
                "typing." | "typing_extensions." | "collections.abc." | "builtins."
            )
        })
}

/// Returns true if `(path, terminal)` matches one of `targets` in the standard typing namespaces.
fn is_std_type_constructor(path: &str, terminal: &str, targets: &[&str]) -> bool {
    targets.contains(&terminal) && is_std_type_constructor_prefix(path, terminal)
}

/// Returns true if `(path, terminal)` is a concrete mutable collection constructor (`list`, `dict`,
/// `set`, `List`, `Dict`, `Set`, `typing.List`, `typing.Dict`, `typing.Set`, etc.).
///
/// Qualified `collections.abc.Set` is excluded because it is the abstract set ABC, whereas
/// unqualified `Set` and `typing.Set` are flagged as concrete (per Ruff `PYI025`, `collections.abc.Set`
/// should be imported `as AbstractSet`).
fn is_concrete_collection_constructor(path: &str, terminal: &str) -> bool {
    match terminal {
        "list" | "List" | "dict" | "Dict" | "set" => is_std_type_constructor_prefix(path, terminal),
        "Set" => matches!(
            path,
            "Set" | "typing.Set" | "typing_extensions.Set" | "builtins.Set"
        ),
        _ => false,
    }
}

/// Returns true if `file` contains an unaliased `from collections.abc import Set` statement.
#[must_use]
pub fn has_unaliased_collections_abc_set_import(file: &ParsedFile) -> bool {
    file.grep.root().dfs().any(|node| {
        node.kind() == "import_from_statement"
            && node
                .field("module_name")
                .is_some_and(|module_node| module_node.text() == "collections.abc")
            && node
                .field_children("name")
                .any(|imported| imported.kind() == "dotted_name" && imported.text() == "Set")
    })
}

/// Extracts `(base_node, type_argument_nodes)` from a Python `generic_type` or expression-fallback
/// `subscript` node inside a type annotation.
fn extract_generic_base_and_args<'a>(
    node: &RawNode<'a>,
) -> Option<(RawNode<'a>, Vec<RawNode<'a>>)> {
    match node.kind().as_ref() {
        "generic_type" => {
            let mut base_node = None;
            let mut type_args = Vec::new();
            for child in node.children() {
                if !child.is_named() || child.is_extra() {
                    continue;
                }
                if child.kind() == "type_parameter" {
                    for param_child in child.children() {
                        if param_child.is_named() && !param_child.is_extra() {
                            type_args.push(param_child);
                        }
                    }
                } else if base_node.is_none() {
                    base_node = Some(child);
                }
            }
            Some((base_node?, type_args))
        }
        "subscript" => {
            let base_node = node.field("value")?;
            let raw_args: Vec<_> = node
                .field_children("subscript")
                .filter(|child| child.is_named() && !child.is_extra())
                .collect();
            let type_args = if raw_args.len() == 1 && raw_args[0].kind() == "tuple" {
                raw_args[0]
                    .children()
                    .filter(|child| child.is_named() && !child.is_extra())
                    .collect()
            } else {
                raw_args
            };
            Some((base_node, type_args))
        }
        _ => None,
    }
}

/// Transparent type wrappers whose all type arguments preserve the enclosing variance.
const TRANSPARENT_UNION_WRAPPERS: &[&str] = &["Optional", "Union"];

/// Transparent type qualifiers whose first type argument (`arg 0`) preserves the enclosing variance.
const TRANSPARENT_FIRST_ARG_WRAPPERS: &[&str] = &[
    "Annotated",
    "ClassVar",
    "Final",
    "Required",
    "NotRequired",
    "ReadOnly",
];

/// Single-parameter generic containers that are covariant in their element type (`arg 0`).
const SINGLE_ARG_COVARIANT_CONTAINERS: &[&str] = &[
    "Sequence",
    "Collection",
    "Iterable",
    "Iterator",
    "Reversible",
    "Container",
    "AsyncIterable",
    "AsyncIterator",
    "Awaitable",
    "AbstractSet",
    "Set",
    "frozenset",
    "FrozenSet",
    "list",
    "List",
    "set",
];

/// Recursively collects matching type constructor paths from `node` according to `depth`.
fn collect_type_constructors_raw<F>(
    node: &RawNode<'_>,
    depth: AnnotationTraversalDepth,
    predicate: &F,
    out: &mut Vec<String>,
) where
    F: Fn(&str, &str) -> bool,
{
    match node.kind().as_ref() {
        "type" | "parenthesized_expression" | "union_type" => {
            for child in node.children() {
                if child.is_named() && !child.is_extra() {
                    collect_type_constructors_raw(&child, depth, predicate, out);
                }
            }
        }
        "binary_operator" => {
            if node.field("operator").is_some_and(|op| op.text() == "|") {
                if let Some(left) = node.field("left") {
                    collect_type_constructors_raw(&left, depth, predicate, out);
                }
                if let Some(right) = node.field("right") {
                    collect_type_constructors_raw(&right, depth, predicate, out);
                }
            }
        }
        "identifier" | "attribute" => {
            let (path, terminal) = resolve_path_and_terminal_raw(node);
            if predicate(&path, &terminal) && !out.contains(&path) {
                out.push(path);
            }
        }
        "generic_type" | "subscript" => {
            let Some((base_node, type_args)) = extract_generic_base_and_args(node) else {
                return;
            };
            let (base_path, base_terminal) = resolve_path_and_terminal_raw(&base_node);

            if is_std_type_constructor(&base_path, &base_terminal, TRANSPARENT_UNION_WRAPPERS) {
                for arg in &type_args {
                    collect_type_constructors_raw(arg, depth, predicate, out);
                }
                return;
            }

            if is_std_type_constructor(&base_path, &base_terminal, TRANSPARENT_FIRST_ARG_WRAPPERS) {
                if let Some(first_arg) = type_args.first() {
                    collect_type_constructors_raw(first_arg, depth, predicate, out);
                }
                return;
            }

            if predicate(&base_path, &base_terminal) && !out.contains(&base_path) {
                out.push(base_path.clone());
            }

            if depth == AnnotationTraversalDepth::CovariantPositions
                && is_std_type_constructor_prefix(&base_path, &base_terminal)
            {
                match base_terminal.as_str() {
                    terminal_name if SINGLE_ARG_COVARIANT_CONTAINERS.contains(&terminal_name) => {
                        if let Some(first_arg) = type_args.first() {
                            collect_type_constructors_raw(first_arg, depth, predicate, out);
                        }
                    }
                    "tuple" | "Tuple" => {
                        for arg in &type_args {
                            collect_type_constructors_raw(arg, depth, predicate, out);
                        }
                    }
                    "Mapping" | "dict" | "Dict" | "Callable" => {
                        if let Some(second_arg) = type_args.get(1) {
                            collect_type_constructors_raw(second_arg, depth, predicate, out);
                        }
                    }
                    "Generator" | "Coroutine" => {
                        if let Some(yield_arg) = type_args.first() {
                            collect_type_constructors_raw(yield_arg, depth, predicate, out);
                        }
                        if let Some(return_arg) = type_args.get(2) {
                            collect_type_constructors_raw(return_arg, depth, predicate, out);
                        }
                    }
                    "AsyncGenerator" => {
                        if let Some(yield_arg) = type_args.first() {
                            collect_type_constructors_raw(yield_arg, depth, predicate, out);
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

/// Collects matching type constructor strings (in source order, deduplicated) from a Python
/// type annotation node according to `depth` and `predicate(full_path, terminal_name)`.
fn collect_type_constructors<F>(
    type_node: &AstNode<'_>,
    depth: AnnotationTraversalDepth,
    predicate: F,
) -> Vec<String>
where
    F: Fn(&str, &str) -> bool,
{
    let mut out = Vec::new();
    collect_type_constructors_raw(&type_node.raw, depth, &predicate, &mut out);
    out
}

/// Collects concrete mutable collection constructors (`list`, `dict`, `set`, `Set`, etc.) from `type_node`.
///
/// If `abc_set_imported` is true (`from collections.abc import Set` is present in the file),
/// unqualified `Set` is treated as the abstract `collections.abc.Set` rather than concrete `typing.Set`.
#[must_use]
pub fn collect_concrete_collection_types(
    type_node: &AstNode<'_>,
    abc_set_imported: bool,
) -> Vec<String> {
    collect_type_constructors(
        type_node,
        AnnotationTraversalDepth::CovariantPositions,
        |full_path, terminal| {
            if abc_set_imported && full_path == "Set" {
                return false;
            }
            is_concrete_collection_constructor(full_path, terminal)
        },
    )
}

/// Collects abstract mutable collection constructors (`MutableSequence`, `MutableMapping`, `MutableSet`)
/// from `type_node` according to `depth`.
#[must_use]
pub fn collect_mutable_collection_types(
    type_node: &AstNode<'_>,
    depth: AnnotationTraversalDepth,
) -> Vec<String> {
    collect_type_constructors(type_node, depth, |full_path, terminal| {
        matches!(
            terminal,
            "MutableSequence" | "MutableMapping" | "MutableSet"
        ) && is_std_type_constructor_prefix(full_path, terminal)
    })
}

/// Collects specific read-only abstract collection constructors (`Sequence`, `Collection`)
/// from `type_node`, unwrapping only transparent wrappers (`AnnotationTraversalDepth::TransparentWrappersOnly`).
#[must_use]
pub fn collect_specific_collection_types(type_node: &AstNode<'_>) -> Vec<String> {
    collect_type_constructors(
        type_node,
        AnnotationTraversalDepth::TransparentWrappersOnly,
        |full_path, terminal| {
            matches!(terminal, "Sequence" | "Collection")
                && is_std_type_constructor_prefix(full_path, terminal)
        },
    )
}

/// Returns true if `func_name` is a Python Data Model dunder method with a fixed signature
/// (all `__*__` methods except constructors `__init__` and `__new__`).
fn is_exempt_dunder_method(func_name: &str) -> bool {
    func_name.starts_with("__")
        && func_name.ends_with("__")
        && func_name.len() > 4
        && !matches!(func_name, "__init__" | "__new__")
}

/// Returns true if `func_node` is decorated with `@override`, `@overload`, `@abstractmethod`,
/// or `@fixture` (`@pytest.fixture`).
fn has_exempt_signature_decorator(func_node: &AstNode<'_>) -> bool {
    has_decorator(func_node, |terminal| {
        matches!(
            terminal,
            "override" | "overload" | "abstractmethod" | "fixture"
        )
    })
}

/// Returns true if a Python `class_definition` inherits from `Protocol` or `ABC` or declares
/// `metaclass=ABCMeta`.
fn is_protocol_or_abc_class_raw(class_node: &RawNode<'_>) -> bool {
    let Some(superclasses) = class_node.field("superclasses") else {
        return false;
    };
    for child in superclasses.children() {
        if !child.is_named() || child.is_extra() {
            continue;
        }
        if child.kind() == "keyword_argument" {
            let is_abc_meta = child
                .field("name")
                .is_some_and(|name_node| name_node.text() == "metaclass")
                && child.field("value").is_some_and(|value_node| {
                    let (_, terminal) = resolve_path_and_terminal_raw(&value_node);
                    terminal == "ABCMeta"
                });
            if is_abc_meta {
                return true;
            }
            continue;
        }
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
        let (_, terminal) = resolve_path_and_terminal_raw(&base_expr);
        if matches!(terminal.as_str(), "Protocol" | "ABC") {
            return true;
        }
    }
    false
}

/// Returns true if `node` (a method `function_definition` or class attribute node) is directly
/// enclosed in a `Protocol` or `ABC` class definition.
fn is_in_protocol_or_abc_class(node: &AstNode<'_>) -> bool {
    for ancestor in node.raw.ancestors() {
        match ancestor.kind().as_ref() {
            "function_definition" | "lambda" => return false,
            "class_definition" => return is_protocol_or_abc_class_raw(&ancestor),
            _ => {}
        }
    }
    false
}

/// Returns true if `statement` is an `expression_statement` wrapping a string literal (docstring).
fn is_docstring_statement_raw(statement: &RawNode<'_>) -> bool {
    statement.kind() == "expression_statement"
        && statement
            .children()
            .find(|child| child.is_named() && !child.is_extra())
            .is_some_and(|child| child.kind() == "string")
}

/// Returns true if `statement` is `pass`, `...`, or `raise NotImplementedError` / `raise NotImplementedError(...)`.
fn is_stub_statement_raw(statement: &RawNode<'_>) -> bool {
    match statement.kind().as_ref() {
        "pass_statement" => true,
        "expression_statement" => statement
            .children()
            .find(|child| child.is_named() && !child.is_extra())
            .is_some_and(|child| child.kind() == "ellipsis"),
        "raise_statement" => statement
            .children()
            .any(|child| match child.kind().as_ref() {
                "identifier" => child.text() == "NotImplementedError",
                "call" => child.field("function").is_some_and(|func| {
                    let (_, terminal) = resolve_path_and_terminal_raw(&func);
                    terminal == "NotImplementedError"
                }),
                _ => false,
            }),
        _ => false,
    }
}

/// Returns true if `func_node` has a stub body consisting only of an optional docstring and
/// `...`, `pass`, or `raise NotImplementedError`.
fn is_stub_function_body(func_node: &AstNode<'_>) -> bool {
    let Some(body) = func_node.raw.field("body") else {
        return false;
    };
    let statements: Vec<_> = body
        .children()
        .filter(|child| {
            child.is_named() && !child.is_extra() && !is_comment_kind(child.kind().as_ref())
        })
        .collect();
    let remaining = if statements.first().is_some_and(is_docstring_statement_raw) {
        &statements[1..]
    } else {
        &statements[..]
    };
    remaining.is_empty() || (remaining.len() == 1 && is_stub_statement_raw(&remaining[0]))
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
    for ancestor in node.raw.ancestors() {
        match ancestor.kind().as_ref() {
            "with_item" => return Some(AstNode::from_raw(ancestor)),
            "parenthesized_expression" => {}
            _ => return None,
        }
    }
    None
}

/// Traverses upward from a node to find its nearest enclosing `with_statement`.
#[must_use]
pub fn find_enclosing_with_statement<'a>(node: &AstNode<'a>) -> Option<AstNode<'a>> {
    node.raw
        .ancestors()
        .find(|parent| parent.kind() == "with_statement")
        .map(AstNode::from_raw)
}

/// Returns true if `node` is invoked as a context manager inside a Python `with` statement header.
#[must_use]
pub fn is_with_context_manager(node: &AstNode<'_>) -> bool {
    find_enclosing_with_item(node).is_some() && find_enclosing_with_statement(node).is_some()
}

/// Returns true if a Python `function_definition` is nested inside another `function_definition`.
fn is_nested_function_raw(func_node: &RawNode<'_>) -> bool {
    func_node
        .ancestors()
        .any(|ancestor| ancestor.kind() == "function_definition")
}

/// Finds all nested Python function definitions (`def ...` inside another `def ...`) and returns
/// `(function_node, function_name)` pairs.
#[must_use]
pub fn find_nested_functions(file: &ParsedFile) -> Vec<(AstNode<'_>, String)> {
    file.grep
        .root()
        .dfs()
        .filter(|func| func.kind() == "function_definition" && is_nested_function_raw(func))
        .map(|func| {
            let func_name = func
                .field("name")
                .map(|name_node| name_node.text().to_string())
                .unwrap_or_default();
            (AstNode::from_raw(func), func_name)
        })
        .collect()
}

/// Returns true if `node` is enclosed inside an `except_clause` block within the same scope.
#[must_use]
pub fn is_inside_except_clause(node: &AstNode<'_>) -> bool {
    for ancestor in node.raw.ancestors() {
        match ancestor.kind().as_ref() {
            "except_clause" => return true,
            "function_definition" | "lambda" | "class_definition" => return false,
            _ => {}
        }
    }
    false
}

/// Returns true if `node` is a Python `tuple` or `list` consisting solely of `>= 2` boolean literals (`True` / `False`).
fn is_boolean_literal_collection_raw(node: &RawNode<'_>) -> bool {
    let kind = node.kind();
    if kind != "tuple" && kind != "list" {
        return false;
    }
    let items: Vec<_> = node
        .children()
        .filter(|child| !matches!(child.kind().as_ref(), "(" | ")" | "[" | "]" | ","))
        .collect();
    items.len() >= 2
        && items
            .iter()
            .all(|item| matches!(item.kind().as_ref(), "true" | "false"))
}

/// Collects all Python `assert_statement` nodes in `file`.
#[must_use]
pub fn collect_assert_statements(file: &ParsedFile) -> Vec<AstNode<'_>> {
    file.grep
        .root()
        .dfs()
        .filter(|node| node.kind() == "assert_statement")
        .map(AstNode::from_raw)
        .collect()
}

/// Returns true if a Python `assert_statement` node has a top-level `and` boolean operator.
#[must_use]
pub fn has_top_level_logical_and(assert_node: &AstNode<'_>) -> bool {
    assert_node
        .raw
        .children()
        .any(|c| c.kind() == "boolean_operator" && c.children().any(|op| op.kind() == "and"))
}

/// Returns true if a Python `assert_statement` node compares against a boolean literal tuple/list.
#[must_use]
pub fn has_boolean_literal_comparison(assert_node: &AstNode<'_>) -> bool {
    assert_node
        .raw
        .children()
        .find(|c| c.kind() == "comparison_operator")
        .is_some_and(|comp| {
            comp.children()
                .any(|c| is_boolean_literal_collection_raw(&c))
        })
}

/// Collects all outermost Python test function definitions (`def test` or `def test_*`).
#[must_use]
pub fn collect_outer_test_functions(file: &ParsedFile) -> Vec<AstNode<'_>> {
    let mut out = Vec::new();
    collect_outer_test_functions_rec(&file.grep.root(), &mut out);
    out
}

fn collect_outer_test_functions_rec<'a>(node: &RawNode<'a>, out: &mut Vec<AstNode<'a>>) {
    if node.kind() == "function_definition" {
        if is_test_function_raw(node) {
            out.push(AstNode::from_raw(node.clone()));
        }
        return;
    }
    for child in node.children() {
        collect_outer_test_functions_rec(&child, out);
    }
}

/// Recursively counts top-level assertion constructs in a Python test function body.
fn count_python_assertions(node: &RawNode<'_>) -> usize {
    let kind = node.kind();
    if matches!(kind.as_ref(), "function_definition" | "class_definition") {
        return 0;
    }
    if kind == "assert_statement" || (kind == "call" && is_assertion_call_raw(node)) {
        return 1;
    }
    node.children()
        .map(|child| count_python_assertions(&child))
        .sum()
}

/// Collects all outermost Python test functions along with their identifier node, name, and assertion count.
#[must_use]
pub fn collect_test_function_assertion_counts(
    file: &ParsedFile,
) -> Vec<(AstNode<'_>, String, usize)> {
    collect_outer_test_functions(file)
        .into_iter()
        .filter_map(|func_node| {
            let name_node = func_node.raw.field("name")?;
            let body_node = func_node.raw.field("body")?;
            let func_name = name_node.text().to_string();
            let count = count_python_assertions(&body_node);
            Some((AstNode::from_raw(name_node), func_name, count))
        })
        .collect()
}

/// If `node` is a Python `function_definition`, returns its name and whether it is declared at top-level `module` scope.
#[must_use]
pub(super) fn function_name_and_is_top_level<'a>(
    node: &RawNode<'a>,
) -> Option<(std::borrow::Cow<'a, str>, bool)> {
    if node.kind() != "function_definition" {
        return None;
    }
    let name_node = node.field("name")?;
    let is_top_level = node
        .parent()
        .is_some_and(|parent| parent.kind() == "module");
    Some((name_node.text(), is_top_level))
}

/// Returns the formatted subscript label (`"os.environ[...]"` or `"environ[...]"`) if `node`
/// indexes into Python's `os.environ` or `environ` mapping.
fn python_environ_subscript_label(node: &RawNode<'_>) -> Option<&'static str> {
    if node.kind() != "subscript" {
        return None;
    }
    let value = node.field("value")?;
    match value.kind().as_ref() {
        "identifier" if value.text() == "environ" => Some("environ[...]"),
        "attribute" => {
            let obj = value.field("object")?;
            let attr = value.field("attribute")?;
            (obj.text() == "os" && attr.text() == "environ").then_some("os.environ[...]")
        }
        _ => None,
    }
}

/// Collects all Python subscript expressions indexing into `os.environ` or `environ`.
#[must_use]
pub fn collect_environ_subscripts(file: &ParsedFile) -> Vec<(AstNode<'_>, &'static str)> {
    file.grep
        .root()
        .dfs()
        .filter_map(|node| {
            let label = python_environ_subscript_label(&node)?;
            Some((AstNode::from_raw(node), label))
        })
        .collect()
}

/// Returns true if a Python `string` node is triple-quoted (`"""` or `'''`).
/// In Python's grammar, only triple-quoted strings can contain literal newlines.
fn is_triple_quoted(node: &RawNode<'_>) -> bool {
    let text = node.text();
    let stripped = text.trim_start_matches(['r', 'R', 'f', 'F', 'b', 'B', 'u', 'U']);
    stripped.starts_with("\"\"\"") || stripped.starts_with("'''")
}

/// Returns true if `node` is a Python multiline triple-quoted string literal.
fn is_multiline_string_literal_raw(node: &RawNode<'_>) -> bool {
    node.kind() == "string"
        && node.end_pos().line() > node.start_pos().line()
        && is_triple_quoted(node)
}

/// Returns true if a Python `string` node is a standalone docstring statement.
fn is_docstring_raw(node: &RawNode<'_>) -> bool {
    node.parent()
        .is_some_and(|parent| parent.kind() == "expression_statement")
}

/// Returns true if `node` is enclosed in a Python `call` within the current scope whose
/// `(full_path, terminal_name)` satisfies `predicate`.
fn is_enclosed_in_call_raw(node: &RawNode<'_>, predicate: impl Fn(&str, &str) -> bool) -> bool {
    for ancestor in node.ancestors() {
        match ancestor.kind().as_ref() {
            "function_definition" | "class_definition" | "lambda" => break,
            "call" => {
                if let Some(func_node) = ancestor.field("function") {
                    let (path, terminal) = resolve_path_and_terminal_raw(&func_node);
                    if predicate(&path, &terminal) {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

/// Finds all multiline string literals in a Python file that are not docstrings
/// and not wrapped in an allowed call.
#[must_use]
pub fn find_unwrapped_multiline_strings(
    file: &ParsedFile,
    is_allowed_wrapper: impl Fn(&str, &str) -> bool,
) -> Vec<AstNode<'_>> {
    file.grep
        .root()
        .dfs()
        .filter(|node| {
            is_multiline_string_literal_raw(node)
                && !is_docstring_raw(node)
                && !is_enclosed_in_call_raw(node, &is_allowed_wrapper)
        })
        .map(AstNode::from_raw)
        .collect()
}

/// Returns the value of a Python decimal `integer` literal (not `0x1`, `1_000`, ...).
fn decimal_literal(node: &RawNode<'_>) -> Option<i64> {
    let text = node.text();
    if node.kind() != "integer" || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// Returns the position read by a Python `subscript` index: a decimal literal, or its negation
/// for end-relative reads (`xs[-1]`).
fn literal_position(index: &RawNode<'_>) -> Option<i64> {
    if index.kind() == "unary_operator" && index.field("operator")?.text() == "-" {
        return decimal_literal(&index.field("argument")?).map(std::ops::Neg::neg);
    }
    decimal_literal(index)
}

/// Returns true if `receiver` names a stable value: a name, attribute or subscript chain with no
/// call inside, so that identical text means the same value.
fn is_stable_receiver(receiver: &RawNode<'_>) -> bool {
    matches!(
        receiver.kind().as_ref(),
        "identifier" | "attribute" | "subscript"
    ) && !receiver.dfs().any(|node| node.kind() == "call")
}

/// Node kinds an assignment or deletion target can be nested in (`a[0], b = ...`).
const TARGET_CONTAINER_KINDS: &[&str] = &[
    "pattern_list",
    "tuple_pattern",
    "list_pattern",
    "expression_list",
    "parenthesized_expression",
    "list_splat_pattern",
];

/// Methods that mutate a `list`, `dict` or `set` in place.
const MUTATING_METHODS: &[&str] = &[
    "append",
    "extend",
    "insert",
    "pop",
    "remove",
    "clear",
    "sort",
    "reverse",
    "update",
    "setdefault",
    "popitem",
    "add",
    "discard",
    "intersection_update",
    "difference_update",
    "symmetric_difference_update",
];

/// Returns true if `node` is assigned to, augmented, deleted, or bound by a `for` loop.
fn is_write_target(node: &RawNode<'_>) -> bool {
    let mut target = node.clone();
    while let Some(parent) = target.parent() {
        match parent.kind().as_ref() {
            kind if TARGET_CONTAINER_KINDS.contains(&kind) => target = parent,
            "delete_statement" => return true,
            "assignment" | "augmented_assignment" | "for_statement" | "for_in_clause" => {
                return parent
                    .field("left")
                    .is_some_and(|left| left.range() == target.range());
            }
            _ => return false,
        }
    }
    false
}

/// If `node` mutates a collection receiver in place (`receiver.append(...)`, `receiver[k] = v`,
/// `del receiver[k]`, `receiver += ...`), returns that `receiver` node.
fn in_place_mutated_receiver<'a>(node: &RawNode<'a>) -> Option<RawNode<'a>> {
    match node.kind().as_ref() {
        "call" => {
            let function_node = node.field("function")?;
            if function_node.kind() != "attribute" {
                return None;
            }
            let method = function_node.field("attribute")?;
            if MUTATING_METHODS.contains(&method.text().as_ref()) {
                function_node.field("object")
            } else {
                None
            }
        }
        "subscript" if is_write_target(node) => node.field("value"),
        "augmented_assignment" => node.field("left"),
        _ => None,
    }
}

/// Extracts the terminal function/method name if `node` is a `call` expression.
fn called_terminal_name(node: &RawNode<'_>) -> Option<String> {
    if node.kind() != "call" {
        return None;
    }
    let function_node = node.field("function")?;
    let (_, terminal) = resolve_path_and_terminal_raw(&function_node);
    (!terminal.is_empty()).then_some(terminal)
}

/// Collects function or method names whose return values are mutated in place in `file`.
///
/// Matches direct call mutations (`fn().append(...)`, `fn()[0] = 1`) and local bindings (`buf = fn(); buf.append(...)`).
#[must_use]
pub fn collect_locally_mutated_return_functions(
    file: &ParsedFile,
) -> std::collections::HashSet<String> {
    let mut mutated_functions = std::collections::HashSet::new();
    let mut bindings_to_callee: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut mutated_identifiers: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    for node in file.grep.root().dfs() {
        if node.kind() == "assignment"
            && let (Some(left), Some(right)) = (node.field("left"), node.field("right"))
            && left.kind() == "identifier"
            && let Some(callee_name) = called_terminal_name(&right)
        {
            bindings_to_callee.insert(left.text().into_owned(), callee_name);
        }

        if let Some(receiver) = in_place_mutated_receiver(&node) {
            if let Some(callee_name) = called_terminal_name(&receiver) {
                mutated_functions.insert(callee_name);
            } else if receiver.kind() == "identifier" {
                mutated_identifiers.insert(receiver.text().into_owned());
            }
        }
    }

    for variable_name in mutated_identifiers {
        if let Some(callee_name) = bindings_to_callee.remove(&variable_name) {
            mutated_functions.insert(callee_name);
        }
    }

    mutated_functions
}

/// A public Python class or instance attribute carrying a type annotation.
pub struct PythonAnnotatedAttribute<'a> {
    /// Name of the enclosing class.
    pub class_name: String,
    /// Attribute identifier name (e.g. `"items"`).
    pub name: String,
    /// Type annotation AST node (`type`).
    pub type_node: AstNode<'a>,
    /// True if any method in the enclosing class mutates this attribute in place.
    pub is_mutated_in_class: bool,
}

/// Walks `node` (without entering nested `class_definition`s) and records attribute names
/// mutated in place on `self`, `cls`, or `class_name`.
fn collect_mutated_class_attr_names_rec(
    node: &RawNode<'_>,
    class_name: &str,
    out: &mut std::collections::HashSet<String>,
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

/// Walks statements in `__init__` (without entering nested functions/classes/lambdas) and
/// collects public `self.<attr>: <type>` annotations.
fn collect_init_annotated_attrs_rec<'a>(
    node: &RawNode<'a>,
    class_name: &str,
    mutated_attrs: &std::collections::HashSet<String>,
    out: &mut Vec<PythonAnnotatedAttribute<'a>>,
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
        && left
            .field("object")
            .is_some_and(|object_node| object_node.text() == "self")
        && let Some(attribute_node) = left.field("attribute")
    {
        let attr_name = attribute_node.text().into_owned();
        if !attr_name.starts_with('_') {
            let is_mutated_in_class = mutated_attrs.contains(&attr_name);
            out.push(PythonAnnotatedAttribute {
                class_name: class_name.to_owned(),
                name: attr_name,
                type_node: AstNode::from_raw(type_node),
                is_mutated_in_class,
            });
        }
    }
    for child in node.children() {
        collect_init_annotated_attrs_rec(&child, class_name, mutated_attrs, out);
    }
}

/// Collects public (`!name.starts_with('_')`) annotated class and `__init__` attributes,
/// recording whether each attribute is mutated in place within its class.
///
/// Skips `Protocol` and `ABC` classes.
#[must_use]
pub fn collect_public_class_attributes(file: &ParsedFile) -> Vec<PythonAnnotatedAttribute<'_>> {
    let mut out = Vec::new();
    for class_node in file.grep.root().dfs() {
        if class_node.kind() != "class_definition" || is_protocol_or_abc_class_raw(&class_node) {
            continue;
        }
        let Some(name_node) = class_node.field("name") else {
            continue;
        };
        let Some(body) = class_node.field("body") else {
            continue;
        };
        let class_name = name_node.text().into_owned();

        let mut mutated_attrs = std::collections::HashSet::new();
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
                if !attr_name.starts_with('_') {
                    let is_mutated_in_class = mutated_attrs.contains(&attr_name);
                    out.push(PythonAnnotatedAttribute {
                        class_name: class_name.clone(),
                        name: attr_name,
                        type_node: AstNode::from_raw(type_node),
                        is_mutated_in_class,
                    });
                }
            } else {
                let func_candidate = if child.kind() == "decorated_definition" {
                    child.field("definition")
                } else {
                    Some(child)
                };
                if let Some(function_node) = func_candidate
                    && function_node.kind() == "function_definition"
                    && function_node
                        .field("name")
                        .is_some_and(|method_name| method_name.text() == "__init__")
                    && let Some(init_body) = function_node.field("body")
                {
                    for statement in init_body.children() {
                        collect_init_annotated_attrs_rec(
                            &statement,
                            &class_name,
                            &mutated_attrs,
                            &mut out,
                        );
                    }
                }
            }
        }
    }
    out
}

/// Read-only methods available on `Sequence`, `Mapping`, or `Set` (`collections.abc`).
const READONLY_COLLECTION_METHODS: &[&str] = &[
    "count",
    "index",
    "get",
    "keys",
    "values",
    "items",
    "isdisjoint",
    "issubset",
    "issuperset",
    "copy",
    "__contains__",
    "__len__",
    "__iter__",
    "__getitem__",
    "__reversed__",
];

/// Builtin functions that read or iterate a collection without mutating it in place or
/// retaining a mutable alias to the outer container.
const SAFE_READONLY_BUILTINS: &[&str] = &[
    "len",
    "max",
    "min",
    "sum",
    "sorted",
    "list",
    "tuple",
    "set",
    "frozenset",
    "dict",
    "bool",
    "any",
    "all",
    "enumerate",
    "zip",
    "reversed",
    "iter",
    "map",
    "filter",
    "repr",
    "str",
    "hash",
    "id",
    "type",
    "isinstance",
    "issubclass",
    "print",
    "range",
    "next",
];

/// Builtin functions that consume an `Iterable` in a single pass.
const SINGLE_PASS_ITERABLE_BUILTINS: &[&str] = &[
    "sum",
    "min",
    "max",
    "any",
    "all",
    "sorted",
    "list",
    "tuple",
    "set",
    "frozenset",
    "dict",
    "enumerate",
    "zip",
    "iter",
    "map",
    "filter",
];

/// Returns true if a nested `function_definition` or `lambda` declares a parameter named `parameter_name`.
fn scope_shadows_parameter(scope_node: &RawNode<'_>, parameter_name: &str) -> bool {
    let Some(params) = scope_node.field("parameters") else {
        return false;
    };
    params
        .children()
        .filter_map(|child| parse_param_parts(&child))
        .any(|parts| parts.name == parameter_name)
}

/// Unwraps enclosing `parenthesized_expression` nodes around `node`, returning `(outermost_expr, parent)`.
fn unwrap_parenthesized_with_parent<'a>(node: &RawNode<'a>) -> Option<(RawNode<'a>, RawNode<'a>)> {
    let mut expr = node.clone();
    while let Some(parent) = expr.parent() {
        if parent.kind() == "parenthesized_expression" {
            expr = parent;
        } else {
            return Some((expr, parent));
        }
    }
    None
}

/// Returns true if `ident_node` (`identifier`) is a variable reference rather than an attribute
/// name (`obj.x`), keyword argument name (`f(x=1)`), or declaration name.
fn is_variable_reference(ident_node: &RawNode<'_>) -> bool {
    let Some(parent) = ident_node.parent() else {
        return false;
    };
    match parent.kind().as_ref() {
        "attribute" => !parent
            .field("attribute")
            .is_some_and(|attr_node| attr_node.range() == ident_node.range()),
        "keyword_argument" => !parent
            .field("name")
            .is_some_and(|name_node| name_node.range() == ident_node.range()),
        "function_definition" | "class_definition" => !parent
            .field("name")
            .is_some_and(|name_node| name_node.range() == ident_node.range()),
        "parameters"
        | "lambda_parameters"
        | "typed_parameter"
        | "default_parameter"
        | "typed_default_parameter"
        | "list_splat_pattern"
        | "dictionary_splat_pattern" => parent
            .field("value")
            .is_some_and(|value_node| value_node.range() == ident_node.range()),
        _ => true,
    }
}

/// Returns true if `call_node` is a direct call to a builtin in `SAFE_READONLY_BUILTINS`.
fn is_safe_readonly_builtin_call(call_node: &RawNode<'_>) -> bool {
    call_node.kind() == "call"
        && call_node.field("function").is_some_and(|function_node| {
            function_node.kind() == "identifier"
                && SAFE_READONLY_BUILTINS.contains(&function_node.text().as_ref())
        })
}

/// Returns true if `node` sits inside a boolean test position (`if`, `elif`, `while`, `assert`, or `bool(...)`).
fn is_in_boolean_context(node: &RawNode<'_>) -> bool {
    let mut current = node.clone();
    while let Some(parent) = current.parent() {
        match parent.kind().as_ref() {
            "parenthesized_expression" | "boolean_operator" | "not_operator" => {
                current = parent;
            }
            "if_statement" | "elif_clause" | "while_statement" => {
                return parent
                    .field("condition")
                    .is_some_and(|condition| condition.range() == current.range());
            }
            "assert_statement" => return true,
            "argument_list" => {
                return parent.parent().is_some_and(|call_node| {
                    call_node.kind() == "call"
                        && call_node
                            .field("function")
                            .is_some_and(|func_node| func_node.text() == "bool")
                });
            }
            _ => return false,
        }
    }
    false
}

/// Returns true if `expr` is the middle condition child of a Python `conditional_expression` (`a if cond else b`).
fn is_conditional_expression_condition(cond_expr: &RawNode<'_>, expr: &RawNode<'_>) -> bool {
    let mut named = cond_expr
        .children()
        .filter(|child| child.is_named() && !child.is_extra());
    let _consequence = named.next();
    named
        .next()
        .is_some_and(|condition| condition.range() == expr.range())
}

/// Returns true if `ident_node` is used in a strictly read-only, non-escaping position.
fn is_safe_readonly_parameter_reference(ident_node: &RawNode<'_>) -> bool {
    if is_write_target(ident_node) {
        return false;
    }
    let Some((expr, parent)) = unwrap_parenthesized_with_parent(ident_node) else {
        return false;
    };
    match parent.kind().as_ref() {
        "attribute" => {
            parent
                .field("object")
                .is_some_and(|object_node| object_node.range() == expr.range())
                && parent.field("attribute").is_some_and(|attr_node| {
                    READONLY_COLLECTION_METHODS.contains(&attr_node.text().as_ref())
                })
                && unwrap_parenthesized_with_parent(&parent).is_some_and(|(attr_expr, grand)| {
                    grand.kind() == "call"
                        && grand
                            .field("function")
                            .is_some_and(|func_node| func_node.range() == attr_expr.range())
                })
        }
        "subscript" => !is_write_target(&parent),
        "argument_list" => parent
            .parent()
            .is_some_and(|call_node| is_safe_readonly_builtin_call(&call_node)),
        "keyword_argument" => parent.parent().is_some_and(|arguments| {
            arguments.kind() == "argument_list"
                && arguments
                    .parent()
                    .is_some_and(|call_node| is_safe_readonly_builtin_call(&call_node))
        }),
        "list_splat" | "dictionary_splat" => {
            unwrap_parenthesized_with_parent(&parent).is_some_and(|(_, grand)| {
                matches!(
                    grand.kind().as_ref(),
                    "list" | "tuple" | "set" | "dictionary"
                ) || (grand.kind() == "argument_list"
                    && grand
                        .parent()
                        .is_some_and(|call_node| is_safe_readonly_builtin_call(&call_node)))
            })
        }
        "for_statement" | "for_in_clause" => parent
            .field("right")
            .is_some_and(|right| right.range() == expr.range()),
        "comparison_operator" | "not_operator" | "binary_operator" | "assert_statement" => true,
        "boolean_operator" => is_in_boolean_context(&parent),
        "if_statement" | "elif_clause" | "while_statement" => parent
            .field("condition")
            .is_some_and(|condition| condition.range() == expr.range()),
        "conditional_expression" => is_conditional_expression_condition(&parent, &expr),
        _ => false,
    }
}

fn check_parameter_mutated_or_escaping_rec(node: &RawNode<'_>, parameter_name: &str) -> bool {
    if node.kind() == "type" {
        return false;
    }
    if matches!(node.kind().as_ref(), "function_definition" | "lambda")
        && scope_shadows_parameter(node, parameter_name)
    {
        return false;
    }
    if node.kind() == "identifier"
        && node.text() == parameter_name
        && is_variable_reference(node)
        && !is_safe_readonly_parameter_reference(node)
    {
        return true;
    }
    node.children()
        .any(|child| check_parameter_mutated_or_escaping_rec(&child, parameter_name))
}

/// Returns true if `parameter_name` is mutated in place or escapes (aliased, returned, yielded,
/// or passed to an unknown function/method) anywhere in `func_node`'s body.
#[must_use]
pub fn is_parameter_mutated_or_escaping(func_node: &AstNode<'_>, parameter_name: &str) -> bool {
    let Some(body) = func_node.raw.field("body") else {
        return false;
    };
    check_parameter_mutated_or_escaping_rec(&body, parameter_name)
}

/// Minimum read-only `collections.abc` capability required by a parameter's usages inside
/// its function body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ParameterCollectionCapability {
    /// Parameter is never referenced in the function body.
    Unused,
    /// Parameter is only iterated once at top-level depth (`Iterable` suffices).
    Iterable,
    /// Parameter uses `len(x)`, `v in x`, truthiness (`if x:`), or multi-pass iteration (`Collection` suffices).
    Collection,
    /// Parameter uses indexing/slicing, `reversed(x)`, `.index()`/`.count()`, pattern matching, or escapes (`Sequence` required).
    Sequence,
}

#[derive(Default)]
struct CapabilityTracker {
    iteration_count: usize,
    needs_collection: bool,
    needs_sequence: bool,
}

/// Inspects a `comparison_operator` node containing `expr` and updates `tracker`.
fn record_comparison_capability(
    comp_node: &RawNode<'_>,
    expr: &RawNode<'_>,
    tracker: &mut CapabilityTracker,
) {
    let has_in_operator = comp_node
        .children()
        .any(|child| matches!(child.kind().as_ref(), "in" | "not in"));
    let is_identity_check = comp_node
        .children()
        .any(|child| matches!(child.kind().as_ref(), "is" | "is not"));
    let last_named = comp_node
        .children()
        .filter(|child| child.is_named() && !child.is_extra())
        .last();
    if has_in_operator && last_named.is_some_and(|last| last.range() == expr.range()) {
        tracker.needs_collection = true;
    } else if !is_identity_check {
        tracker.needs_sequence = true;
    }
}

/// Updates `tracker` for a single variable reference `ident_node` at `loop_or_closure_depth`.
fn record_reference_capability(
    ident_node: &RawNode<'_>,
    loop_or_closure_depth: usize,
    tracker: &mut CapabilityTracker,
) {
    if is_write_target(ident_node) {
        tracker.needs_sequence = true;
        return;
    }
    let Some((expr, parent)) = unwrap_parenthesized_with_parent(ident_node) else {
        tracker.needs_sequence = true;
        return;
    };
    match parent.kind().as_ref() {
        "for_statement" | "for_in_clause"
            if parent
                .field("right")
                .is_some_and(|right| right.range() == expr.range()) =>
        {
            tracker.iteration_count += 1;
            if loop_or_closure_depth > 0 {
                tracker.needs_collection = true;
            }
        }
        "list_splat" => {
            if let Some((_, grand)) = unwrap_parenthesized_with_parent(&parent)
                && matches!(grand.kind().as_ref(), "list" | "tuple" | "set")
            {
                tracker.iteration_count += 1;
                if loop_or_closure_depth > 0 {
                    tracker.needs_collection = true;
                }
            } else {
                tracker.needs_sequence = true;
            }
        }
        "argument_list" => {
            let Some(call_node) = parent.parent() else {
                tracker.needs_sequence = true;
                return;
            };
            let Some(func_node) = call_node.field("function") else {
                tracker.needs_sequence = true;
                return;
            };
            if func_node.kind() != "identifier" {
                tracker.needs_sequence = true;
                return;
            }
            match func_node.text().as_ref() {
                "len" | "bool" => tracker.needs_collection = true,
                name if SINGLE_PASS_ITERABLE_BUILTINS.contains(&name) => {
                    tracker.iteration_count += 1;
                    if loop_or_closure_depth > 0 {
                        tracker.needs_collection = true;
                    }
                }
                _ => tracker.needs_sequence = true,
            }
        }
        "comparison_operator" => record_comparison_capability(&parent, &expr, tracker),
        "not_operator" => tracker.needs_collection = true,
        "boolean_operator" => {
            if is_in_boolean_context(&parent) {
                tracker.needs_collection = true;
            } else {
                tracker.needs_sequence = true;
            }
        }
        "if_statement" | "elif_clause" | "while_statement"
            if parent
                .field("condition")
                .is_some_and(|condition| condition.range() == expr.range()) =>
        {
            tracker.needs_collection = true;
        }
        "conditional_expression" if is_conditional_expression_condition(&parent, &expr) => {
            tracker.needs_collection = true;
        }
        _ => tracker.needs_sequence = true,
    }
}

fn analyze_capability_rec(
    node: &RawNode<'_>,
    parameter_name: &str,
    loop_or_closure_depth: usize,
    tracker: &mut CapabilityTracker,
) {
    if tracker.needs_sequence || node.kind() == "type" {
        return;
    }
    match node.kind().as_ref() {
        "function_definition" | "lambda" => {
            if scope_shadows_parameter(node, parameter_name) {
                return;
            }
            for child in node.children() {
                analyze_capability_rec(&child, parameter_name, loop_or_closure_depth + 1, tracker);
            }
            return;
        }
        "for_statement" => {
            let right_range = node.field("right").map(|right| right.range());
            for child in node.children() {
                let child_depth = if right_range.as_ref() == Some(&child.range()) {
                    loop_or_closure_depth
                } else {
                    loop_or_closure_depth + 1
                };
                analyze_capability_rec(&child, parameter_name, child_depth, tracker);
            }
            return;
        }
        "while_statement" => {
            for child in node.children() {
                analyze_capability_rec(&child, parameter_name, loop_or_closure_depth + 1, tracker);
            }
            return;
        }
        "list_comprehension"
        | "set_comprehension"
        | "dictionary_comprehension"
        | "generator_expression" => {
            let mut seen_first_for_clause = false;
            for child in node.children() {
                if child.kind() == "for_in_clause" && !seen_first_for_clause {
                    seen_first_for_clause = true;
                    let right_range = child.field("right").map(|right| right.range());
                    for clause_child in child.children() {
                        let clause_depth = if right_range.as_ref() == Some(&clause_child.range()) {
                            loop_or_closure_depth
                        } else {
                            loop_or_closure_depth + 1
                        };
                        analyze_capability_rec(
                            &clause_child,
                            parameter_name,
                            clause_depth,
                            tracker,
                        );
                    }
                } else {
                    analyze_capability_rec(
                        &child,
                        parameter_name,
                        loop_or_closure_depth + 1,
                        tracker,
                    );
                }
            }
            return;
        }
        "identifier" if node.text() == parameter_name && is_variable_reference(node) => {
            record_reference_capability(node, loop_or_closure_depth, tracker);
            return;
        }
        _ => {}
    }

    for child in node.children() {
        analyze_capability_rec(&child, parameter_name, loop_or_closure_depth, tracker);
    }
}

/// Determines the minimum read-only collection capability (`Iterable`, `Collection`, or `Sequence`)
/// required by `parameter_name` across `func_node`'s body.
#[must_use]
pub fn analyze_parameter_collection_capability(
    func_node: &AstNode<'_>,
    parameter_name: &str,
) -> ParameterCollectionCapability {
    let Some(body) = func_node.raw.field("body") else {
        return ParameterCollectionCapability::Unused;
    };
    let mut tracker = CapabilityTracker::default();
    analyze_capability_rec(&body, parameter_name, 0, &mut tracker);

    if tracker.needs_sequence {
        ParameterCollectionCapability::Sequence
    } else if tracker.needs_collection || tracker.iteration_count > 1 {
        ParameterCollectionCapability::Collection
    } else if tracker.iteration_count == 1 {
        ParameterCollectionCapability::Iterable
    } else {
        ParameterCollectionCapability::Unused
    }
}

/// Returns the position a `subscript` reads, if its index is a single literal.
fn literal_index(subscript: &RawNode<'_>) -> Option<i64> {
    let mut indices = subscript.field_children("subscript");
    match (indices.next(), indices.next()) {
        (Some(index), None) => literal_position(&index),
        _ => None,
    }
}

/// Records a `subscript` as a positional read, or its receiver as an exempt receiver when it
/// is written to or indexed by anything but a single literal.
fn record_subscript<'a>(subscript: &RawNode<'a>, scope: &mut ScopePositionalReads<'a>) {
    let Some(receiver) = subscript.field("value") else {
        return;
    };
    match literal_index(subscript) {
        Some(position) if !is_write_target(subscript) => {
            if is_stable_receiver(&receiver) {
                scope.reads.push(PositionalRead {
                    node: AstNode::from_raw(subscript.clone()),
                    receiver: receiver.text().into_owned(),
                    position,
                });
            }
        }
        _ => {
            scope.exempt_receivers.insert(receiver.text().into_owned());
        }
    }
}

/// Builtins that iterate or size their positional arguments (`len(xs)`, `enumerate(xs)`,
/// `zip(xs, ys)`).
const COLLECTION_BUILTINS: &[&str] = &["len", "enumerate", "zip", "reversed", "sorted"];

/// Returns the collections a `call` iterates, sizes (`len(xs)`, `zip(xs, ys)`) or mutates
/// (`xs.append(...)`).
fn called_collections<'a>(call: &RawNode<'a>) -> Vec<RawNode<'a>> {
    let Some(function) = call.field("function") else {
        return Vec::new();
    };
    match function.kind().as_ref() {
        "identifier" if COLLECTION_BUILTINS.contains(&function.text().as_ref()) => {
            call.field("arguments").map_or_else(Vec::new, |arguments| {
                arguments
                    .children()
                    .filter(|child| {
                        child.is_named() && !child.is_extra() && child.kind() != "keyword_argument"
                    })
                    .collect()
            })
        }
        "attribute" => {
            let is_mutating = function
                .field("attribute")
                .is_some_and(|method| MUTATING_METHODS.contains(&method.text().as_ref()));
            if is_mutating {
                function.field("object").into_iter().collect()
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    }
}

/// Returns the collections a node iterates (`for x in xs`), sizes or mutates.
fn collection_uses<'a>(node: &RawNode<'a>) -> Vec<RawNode<'a>> {
    match node.kind().as_ref() {
        "for_statement" | "for_in_clause" => node.field("right").into_iter().collect(),
        "call" => called_collections(node),
        _ => Vec::new(),
    }
}

/// Walks `node`, recording reads and collection uses into `scope` (none in class bodies) and
/// pushing each finished function scope to `out`. Lambdas are skipped.
fn collect_positional_reads_rec<'a>(
    node: &RawNode<'a>,
    mut scope: Option<&mut ScopePositionalReads<'a>>,
    out: &mut Vec<ScopePositionalReads<'a>>,
) {
    match node.kind().as_ref() {
        "lambda" => return,
        "function_definition" => {
            // Default values and annotations are evaluated in the enclosing scope.
            let body = node.field("body");
            for child in node.children() {
                if body
                    .as_ref()
                    .is_some_and(|body| body.range() == child.range())
                {
                    let mut function_scope = ScopePositionalReads::default();
                    collect_positional_reads_rec(&child, Some(&mut function_scope), out);
                    out.push(function_scope);
                } else {
                    collect_positional_reads_rec(&child, scope.as_deref_mut(), out);
                }
            }
            return;
        }
        "class_definition" => scope = None,
        _ => {}
    }
    if let Some(scope) = scope.as_deref_mut() {
        if node.kind() == "subscript" {
            record_subscript(node, scope);
        } else {
            for collection in collection_uses(node) {
                scope
                    .exempt_receivers
                    .insert(collection.text().into_owned());
            }
        }
    }
    for child in node.children() {
        collect_positional_reads_rec(&child, scope.as_deref_mut(), out);
    }
}

/// Collects Python positional reads grouped by scope (see [`super::collect_positional_reads`]).
#[must_use]
pub fn collect_positional_reads(file: &ParsedFile) -> Vec<ScopePositionalReads<'_>> {
    let mut module_scope = ScopePositionalReads::default();
    let mut out = Vec::new();
    collect_positional_reads_rec(&file.grep.root(), Some(&mut module_scope), &mut out);
    out.push(module_scope);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast_grep_language::SupportLang;

    #[test]
    fn test_collect_bindings_python() {
        let source = indoc::indoc! {r"
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
        "};
        let file = ParsedFile::new(source, SupportLang::Python);
        let bindings = collect_bindings(&file);
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
        let source = indoc::indoc! {r"
            match val:
                case Point(x, y=z):
                    pass
                case [a, b]:
                    pass
        "};
        let file = ParsedFile::new(source, SupportLang::Python);
        let bindings = collect_bindings(&file);
        let names: Vec<String> = bindings
            .iter()
            .map(|node| node.text().to_string())
            .collect();
        assert_eq!(names, vec!["x", "z", "a", "b"]);
    }

    #[test]
    fn test_find_enclosing_with_helpers() {
        let source = indoc::indoc! {r"
            with suppress(FileNotFoundError):
                pass
            x = suppress(KeyError)
        "};
        let file = ParsedFile::new(source, SupportLang::Python);
        let calls: Vec<_> = file
            .grep
            .root()
            .find_all("suppress($$$ARGS)")
            .map(|matched| AstNode::from_raw(matched.get_node().clone()))
            .collect();
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
        let source = indoc::indoc! {r#"
            @dataclass(frozen=True, slots=False)
            @pytest.mark.parametrize("x", [1, 2])
            @custom
            def foo():
                pass
        "#};
        let file = ParsedFile::new(source, SupportLang::Python);
        let func = AstNode::from_raw(
            file.grep
                .root()
                .find("def foo(): $$$BODY")
                .unwrap()
                .get_node()
                .clone(),
        );
        let decorators = extract_decorators(&func);
        assert_eq!(decorators.len(), 3);
    }

    #[test]
    fn test_extract_decorator_keyword_args() {
        let source = indoc::indoc! {r"
            @dataclass(frozen=True, slots=False)
            def foo():
                pass
        "};
        let file = ParsedFile::new(source, SupportLang::Python);
        let func = AstNode::from_raw(
            file.grep
                .root()
                .find("def foo(): $$$BODY")
                .unwrap()
                .get_node()
                .clone(),
        );
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
        let source = indoc::indoc! {r#"
            @pytest.mark.parametrize("x", [1, 2])
            @custom
            def foo():
                pass
        "#};
        let file = ParsedFile::new(source, SupportLang::Python);
        let func = AstNode::from_raw(
            file.grep
                .root()
                .find("def foo(): $$$BODY")
                .unwrap()
                .get_node()
                .clone(),
        );
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
        let source = indoc::indoc! {r#"
            @pytest.mark.parametrize("x", [1, 2])
            def foo():
                pass
        "#};
        let file = ParsedFile::new(source, SupportLang::Python);
        let func = AstNode::from_raw(
            file.grep
                .root()
                .find("def foo(): $$$BODY")
                .unwrap()
                .get_node()
                .clone(),
        );

        assert!(has_decorator(&func, |name| name == "parametrize"));
        assert!(has_decorator(&func, |path| path == "pytest.mark.parametrize"));
        assert!(!has_decorator(&func, |name| name == "override"));
    }

    #[test]
    fn test_extract_classes_and_inheritance() {
        let source = indoc::indoc! {r"
            class FakeService(abc.ABC, Protocol):
                pass
        "};
        let file = ParsedFile::new(source, SupportLang::Python);
        let classes = extract_classes(&file);
        assert_eq!(classes.len(), 1);

        let cls = &classes[0];
        assert_eq!(cls.name, "FakeService");
        assert!(cls.inherits_from("Protocol"));
        assert!(cls.inherits_from("ABC"));
    }

    #[test]
    fn test_extract_classes_bases_metadata() {
        let source = indoc::indoc! {r"
            class FakeService(abc.ABC, Protocol):
                pass
        "};
        let file = ParsedFile::new(source, SupportLang::Python);
        let classes = extract_classes(&file);
        let cls = &classes[0];
        assert_eq!(cls.bases.len(), 2);
        assert_eq!(cls.bases[0].name, "abc.ABC");
        assert_eq!(cls.bases[1].name, "Protocol");
    }

    #[test]
    fn test_extract_classes_dataclass() {
        let source = indoc::indoc! {r"
            @dataclass(frozen=True)
            class Config:
                pass
        "};
        let file = ParsedFile::new(source, SupportLang::Python);
        let classes = extract_classes(&file);
        assert_eq!(classes.len(), 1);

        let cls = &classes[0];
        assert_eq!(cls.name, "Config");
        assert_eq!(cls.decorators[0].terminal_name, "dataclass");
        assert!(!cls.inherits_from("Protocol"));
    }

    #[test]
    fn test_extract_parameters_kinds() {
        let source = indoc::indoc! {r"
            def handler(self, a: int, b: str = 'hello', *, c: bool, **kwargs):
                pass
        "};
        let file = ParsedFile::new(source, SupportLang::Python);
        let root = AstNode::from_raw(file.grep.root());
        let params = extract_parameters(&root);

        assert_eq!(params[0].kind, PythonParameterKind::Receiver);
        assert_eq!(params[1].kind, PythonParameterKind::Positional);
        assert_eq!(params[3].kind, PythonParameterKind::KeywordOnly);
        assert_eq!(params[4].kind, PythonParameterKind::VarKeyword);
    }

    #[test]
    fn test_extract_parameters_type_annotations() {
        let source = indoc::indoc! {r"
            def handler(self, a: int, b: str = 'hello', *, c: bool, **kwargs):
                pass
        "};
        let file = ParsedFile::new(source, SupportLang::Python);
        let root = AstNode::from_raw(file.grep.root());
        let params = extract_parameters(&root);

        assert_eq!(params[1].name, "a");
        assert_eq!(params[1].type_text.as_deref(), Some("int"));
        assert_eq!(params[2].name, "b");
        assert_eq!(params[2].type_text.as_deref(), Some("str"));
    }

    /// Module-level reads form their own scope. `rule_test!` cannot cover this: repeating a
    /// fail case in one file merges both copies into the same module scope.
    #[test]
    fn test_collect_positional_reads_module_scope() {
        let source = indoc::indoc! {r"
            src, dst = sys.argv[1], sys.argv[2]

            def main(argv):
                return argv[1]
        "};
        let file = ParsedFile::new(source, SupportLang::Python);
        let reads_by_scope: Vec<Vec<(String, i64)>> = collect_positional_reads(&file)
            .into_iter()
            .map(|scope| {
                scope
                    .reads
                    .into_iter()
                    .map(|read| (read.receiver, read.position))
                    .collect()
            })
            .collect();

        let module_reads = vec![("sys.argv".to_string(), 1), ("sys.argv".to_string(), 2)];
        assert!(
            reads_by_scope.contains(&module_reads),
            "no module scope holding exactly the top-level reads: {reads_by_scope:?}"
        );
    }

    #[rstest::rstest]
    #[case::a1_bare_concrete("def f(x: list, y: dict, z: typing.List) -> None: pass", &[&["list"][..], &["dict"][..], &["typing.List"][..]])]
    #[case::a2_pep585_generic("def f(x: list[int], y: dict[str, int], z: set[str]) -> None: pass", &[&["list"][..], &["dict"][..], &["set"][..]])]
    #[case::a3_pep484_qualified_and_unqualified("def f(x: typing.List[int], y: typing.Dict[str, int], z: typing.Set[str], w: Set[str]) -> None: pass", &[&["typing.List"][..], &["typing.Dict"][..], &["typing.Set"][..], &["Set"][..]])]
    #[case::a4_pep604_union("def f(x: list[int] | None, y: int | list[str] | set[int]) -> None: pass", &[&["list"][..], &["list", "set"][..]])]
    #[case::a5_qualified_pep604_union("def f(x: typing.List[int] | None) -> None: pass", &[&["typing.List"][..]])]
    #[case::a6_optional_and_union("def f(x: Optional[list[int]], y: Union[list[int], str, None]) -> None: pass", &[&["list"][..], &["list"][..]])]
    #[case::a7_qualified_optional_and_union("def f(x: typing.Optional[typing.Dict[str, int]], y: typing.Union[list[int], None]) -> None: pass", &[&["typing.Dict"][..], &["list"][..]])]
    #[case::a8_annotated_unwraps_first_arg("def f(x: Annotated[list[int], 'meta'], y: typing.Annotated[set[str], Doc('x')]) -> None: pass", &[&["list"][..], &["set"][..]])]
    #[case::a9_annotated_ignores_metadata("def f(x: Annotated[Sequence[int], list]) -> None: pass", &[&[][..]])]
    #[case::a10_qualifiers("def f(x: ClassVar[list[str]], y: Final[dict[str, int]], z: Required[set[str]]) -> None: pass", &[&["list"][..], &["dict"][..], &["set"][..]])]
    #[case::a11_covariant_single_and_tuple_containers("def f(a: Sequence[list[int]], b: Collection[set[str]], c: Awaitable[list[int]], d: tuple[str, dict[str, int]], e: tuple[list[int], ...]) -> None: pass", &[&["list"][..], &["set"][..], &["list"][..], &["dict"][..], &["list"][..]])]
    #[case::a12_mapping_covariant_value("def f(x: Mapping[str, list[int]]) -> None: pass", &[&["list"][..]])]
    #[case::a13_callable_covariant_return("def f(cb: Callable[[int], list[str]]) -> None: pass", &[&["list"][..]])]
    #[case::a14_callable_contravariant_param_ignored("def f(cb: Callable[[list[int]], None]) -> None: pass", &[&[][..]])]
    #[case::a15_invariant_mutable_outer_ignored("def f(x: MutableMapping[str, list[int]], y: MutableSequence[list[int]]) -> None: pass", &[&[][..], &[][..]])]
    #[case::a16_unknown_generic_ignored("def f(x: CustomBox[list[int]]) -> None: pass", &[&[][..]])]
    #[case::a17_abstract_collections_pass("def f(a: Sequence[int], b: Mapping[str, int], c: AbstractSet[str], d: collections.abc.Set[str]) -> None: pass", &[&[][..], &[][..], &[][..], &[][..]])]
    #[case::a17b_unaliased_collections_abc_set_import("from collections.abc import Set\ndef f(a: Set[str], b: typing.Set[str]) -> None: pass", &[&[][..], &["typing.Set"][..]])]
    #[case::a18_immutable_builtins_pass("def f(a: tuple[int, ...], b: frozenset[str], c: bytes, d: str) -> None: pass", &[&[][..], &[][..], &[][..], &[][..]])]
    fn test_collect_concrete_collection_types_matrix_a(
        #[case] source: &str,
        #[case] expected_per_param: &[&[&str]],
    ) {
        let file = ParsedFile::new(source, SupportLang::Python);
        let abc_set_imported = has_unaliased_collections_abc_set_import(&file);
        let sigs = extract_function_signatures(&file);
        assert_eq!(sigs.len(), 1);
        let actual: Vec<Vec<String>> = sigs[0]
            .parameters
            .iter()
            .map(|parameter| {
                let type_node = parameter.type_node.as_ref().expect("param should be typed");
                collect_concrete_collection_types(type_node, abc_set_imported)
            })
            .collect();
        let expected: Vec<Vec<String>> = expected_per_param
            .iter()
            .map(|slice| {
                slice
                    .iter()
                    .map(|expected_name| (*expected_name).to_string())
                    .collect()
            })
            .collect();
        assert_eq!(actual, expected);
    }

    #[rstest::rstest]
    #[case::ellipsis_stub("def f(x: list[int]) -> None: ...", true)]
    #[case::pass_stub("def f(x: list[int]) -> None:\n    pass", true)]
    #[case::docstring_and_ellipsis(
        "def f(x: list[int]) -> None:\n    \"\"\"Doc.\"\"\"\n    ...",
        true
    )]
    #[case::raise_not_implemented_bare(
        "def f(x: list[int]) -> None:\n    raise NotImplementedError",
        true
    )]
    #[case::raise_not_implemented_call(
        "def f(x: list[int]) -> None:\n    raise NotImplementedError('todo')",
        true
    )]
    #[case::real_body("def f(x: list[int]) -> int:\n    return len(x)", false)]
    fn test_is_stub_function_body(#[case] source: &str, #[case] expected: bool) {
        let file = ParsedFile::new(source, SupportLang::Python);
        let sigs = extract_function_signatures(&file);
        assert_eq!(is_stub_function_body(&sigs[0].node), expected);
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
    fn test_is_in_protocol_or_abc_class(#[case] source: &str, #[case] expected: bool) {
        let file = ParsedFile::new(source, SupportLang::Python);
        let sigs = extract_function_signatures(&file);
        assert_eq!(is_in_protocol_or_abc_class(&sigs[0].node), expected);
    }
}
