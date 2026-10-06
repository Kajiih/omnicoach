//! Python function signatures, parameter extraction, method receivers, and direct scope definitions.

// omni:disable-file [repeated-literal] -- Tree-sitter node kinds and field names (see ROADMAP)

use super::{
    AstNode, ParsedFile, RawNode, decorated_definition, extract_decorators_raw, has_decorator,
    is_in_protocol_or_abc_class, resolve_path_and_terminal_raw,
};

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
    /// Returns true if the function's signature is imposed from outside: a data-model dunder
    /// method other than `__init__`, `__new__`, and `__call__`, or a function decorated with
    /// `@override`, `@overload`, `@abstractmethod`, `@fixture`, `@<function>.register`, or
    /// `@<property>.setter`.
    #[must_use]
    pub fn has_imposed_signature(&self) -> bool {
        is_exempt_dunder_method(&self.name) || has_exempt_signature_decorator(&self.node)
    }

    /// Returns true if the function is exempt from signature annotation rules: its signature
    /// is imposed ([`Self::has_imposed_signature`]) or it is a method of a `Protocol` or `ABC`
    /// class.
    #[must_use]
    pub fn is_exempt_from_signature_rules(&self) -> bool {
        self.has_imposed_signature() || is_in_protocol_or_abc_class(&self.node)
    }

    /// Returns true if the function is exempt from body-usage parameter rules (signature-exempt
    /// functions or stub bodies consisting only of `...`, `pass`, or `raise NotImplementedError`).
    #[must_use]
    pub fn is_exempt_from_body_usage_rules(&self) -> bool {
        self.is_exempt_from_signature_rules() || is_stub_function_body(&self.node)
    }
}

/// Internal helper struct for extracted parameter parts.
pub(super) struct ParsedParamParts<'a> {
    pub name_node: AstNode<'a>,
    pub name: String,
    pub type_node: Option<AstNode<'a>>,
    pub type_text: Option<String>,
    pub default_value_node: Option<AstNode<'a>>,
}

/// Extracts parameter name, name node, type annotation, and default value from a parameter node.
pub(super) fn parse_param_parts<'a>(node: &RawNode<'a>) -> Option<ParsedParamParts<'a>> {
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
            // The grammar gives `typed_parameter` no `name` field: the name is the leading
            // identifier, wrapped in a splat pattern for `*args: T` and `**kwargs: T`.
            let name_node = node.children().find_map(|child| {
                if child.kind() == "identifier" {
                    Some(child)
                } else if child.kind() == "list_splat_pattern"
                    || child.kind() == "dictionary_splat_pattern"
                {
                    child.children().find(|sub| sub.kind() == "identifier")
                } else {
                    None
                }
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
pub(super) fn extract_parameters_raw<'a>(
    func_or_params_node: &RawNode<'a>,
) -> Vec<PythonParameterInfo<'a>> {
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
    func_or_params_node
        .raw_opt()
        .map_or_else(Vec::new, extract_parameters_raw)
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

/// Returns `(statement_node, function_node)` for each direct function definition in `scope_body`
/// (`module` or class `block`), unwrapping `decorated_definition` wrappers.
pub(super) fn direct_function_definitions<'a>(
    scope_body: &RawNode<'a>,
) -> Vec<(RawNode<'a>, RawNode<'a>)> {
    let mut definitions = Vec::new();
    for statement in scope_body.children() {
        let function_node = if statement.kind() == "function_definition" {
            Some(statement.clone())
        } else {
            decorated_definition(&statement).filter(|inner| inner.kind() == "function_definition")
        };
        if let Some(function_node) = function_node {
            definitions.push((statement, function_node));
        }
    }
    definitions
}

/// Returns the receiver parameter name (`"self"`, or `"cls"` when `allow_classmethod_cls` is true)
/// for a method `function_node`, or `None` if decorated with `@staticmethod` (or `@classmethod`
/// when `!allow_classmethod_cls`) or if the first parameter is not a receiver.
pub(super) fn method_receiver_name(
    function_node: &RawNode<'_>,
    allow_classmethod_cls: bool,
) -> Option<String> {
    let decorators = extract_decorators_raw(function_node);
    let is_excluded_decorator = decorators.iter().any(|decorator| {
        decorator.terminal_name == "staticmethod"
            || (!allow_classmethod_cls && decorator.terminal_name == "classmethod")
    });
    if is_excluded_decorator {
        return None;
    }
    let first = extract_parameters_raw(function_node).into_iter().next()?;
    if first.kind != PythonParameterKind::Receiver {
        return None;
    }
    if !allow_classmethod_cls && first.name != "self" {
        return None;
    }
    Some(first.name)
}

/// Returns true if `func_name` is a Python Data Model dunder method with a fixed signature
/// (all `__*__` methods except constructors `__init__` and `__new__`, and `__call__`, whose
/// signature is designed by the class author).
fn is_exempt_dunder_method(func_name: &str) -> bool {
    func_name.starts_with("__")
        && func_name.ends_with("__")
        && func_name.len() > 4
        && !matches!(func_name, "__init__" | "__new__" | "__call__")
}

/// Returns true if `func_node` is decorated with `@override`, `@overload`, `@abstractmethod`,
/// `@fixture` (`@pytest.fixture`), `@<function>.register` (`functools.singledispatch`
/// implementations, which dispatch on their annotations), or `@<property>.setter` (whose value
/// type mirrors the getter's return type).
fn has_exempt_signature_decorator(func_node: &AstNode<'_>) -> bool {
    has_decorator(func_node, |terminal| {
        matches!(
            terminal,
            "override" | "overload" | "abstractmethod" | "fixture" | "register" | "setter"
        )
    })
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
            .find(|child| child.is_named() && !child.is_extra())
            .is_some_and(|operand| match operand.kind().as_ref() {
                "identifier" => operand.text() == "NotImplementedError",
                "call" => operand.field("function").is_some_and(|func| {
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
pub(super) fn is_stub_function_body(func_node: &AstNode<'_>) -> bool {
    let Some(body) = func_node.raw_opt().and_then(|raw| raw.field("body")) else {
        return false;
    };
    let statements: Vec<_> = body
        .children()
        .filter(|child| child.is_named() && !child.is_extra())
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

/// Returns true if `item`, the definition owning a name, has that name mandated by a contract.
///
/// Python has no structural trait implementations, so the contract is an explicit
/// `@override` decorator on a method.
#[must_use]
pub fn is_trait_impl_member(item: &AstNode<'_>) -> bool {
    item.raw_opt()
        .is_some_and(|raw| raw.kind().as_ref() == "function_definition")
        && has_override_decorator(item)
}

/// Returns true if the nearest enclosing `function_definition` or `class_definition` of a Python
/// `function_definition` is a `function_definition`. Methods of a class declared inside a function
/// are therefore not nested; a `def` inside such a method is.
fn is_nested_function_raw(func_node: &RawNode<'_>) -> bool {
    func_node
        .ancestors()
        .find(|ancestor| {
            matches!(
                ancestor.kind().as_ref(),
                "function_definition" | "class_definition"
            )
        })
        .is_some_and(|scope| scope.kind() == "function_definition")
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
