//! Python function signatures, parameter extraction, method receivers, and direct scope definitions.

#[cfg(test)]
use super::find_parameters_at_span;
use super::{
    AstNode, DecoratorInfo, ParsedFile, extract_decorators_from_slice, is_protocol_or_abc_class,
    resolve_path_and_terminal_expr,
};
use crate::code_lint::ast::span_from_ruff_range;
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_stmt};
use ruff_python_ast::{
    Decorator, Expr, Parameter, ParameterWithDefault, Parameters, Stmt, StmtFunctionDef,
};
use ruff_text_size::Ranged as _;

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
    /// True if the function is decorated with an exempt signature decorator
    /// ([`has_exempt_signature_decorator`]).
    pub(super) has_exempt_signature_decorator: bool,
    /// True if the function is directly enclosed in a `Protocol` or `ABC` class definition.
    pub(super) is_in_protocol_or_abc_class: bool,
    /// True if the body is a stub ([`is_stub_body`]).
    pub(super) has_stub_body: bool,
}

impl PythonFunctionSignature<'_> {
    /// Returns true if the function's signature is imposed from outside: a data-model dunder
    /// method other than `__init__`, `__new__`, and `__call__`, or a function decorated with
    /// `@override`, `@overload`, `@abstractmethod`, `@fixture`, `@<function>.register`, or
    /// `@<property>.setter`.
    #[must_use]
    pub fn has_imposed_signature(&self) -> bool {
        is_exempt_dunder_method(&self.name) || self.has_exempt_signature_decorator
    }

    /// Returns true if the function is exempt from signature annotation rules: its signature
    /// is imposed ([`Self::has_imposed_signature`]) or it is a method of a `Protocol` or `ABC`
    /// class.
    #[must_use]
    pub fn is_exempt_from_signature_rules(&self) -> bool {
        self.has_imposed_signature() || self.is_in_protocol_or_abc_class
    }

    /// Returns true if the function is exempt from body-usage parameter rules (signature-exempt
    /// functions or stub bodies consisting only of `...`, `pass`, or `raise NotImplementedError`).
    #[must_use]
    pub fn is_exempt_from_body_usage_rules(&self) -> bool {
        self.is_exempt_from_signature_rules() || self.has_stub_body
    }
}

fn build_parameter_with_default<'a>(
    param_with_default: &ParameterWithDefault,
    kind: PythonParameterKind,
    file: &'a ParsedFile,
) -> PythonParameterInfo<'a> {
    let parameter = &param_with_default.parameter;
    let type_span = parameter
        .annotation
        .as_ref()
        .map(|ann| span_from_ruff_range(ann.range()));
    let type_text = type_span.map(|span| file.source[span.start..span.end].to_string());
    let type_node = type_span.map(|span| AstNode::from_span(file, span));
    let default_value_node = param_with_default
        .default
        .as_ref()
        .map(|def| AstNode::from_span(file, span_from_ruff_range(def.range())));

    PythonParameterInfo {
        node: AstNode::from_span(file, span_from_ruff_range(param_with_default.range)),
        name: parameter.name.id.to_string(),
        name_node: AstNode::from_span(file, span_from_ruff_range(parameter.name.range)),
        type_node,
        type_text,
        default_value_node,
        kind,
    }
}

fn build_variadic_parameter<'a>(
    parameter: &Parameter,
    kind: PythonParameterKind,
    file: &'a ParsedFile,
) -> PythonParameterInfo<'a> {
    let type_span = parameter
        .annotation
        .as_ref()
        .map(|ann| span_from_ruff_range(ann.range()));
    let type_text = type_span.map(|span| file.source[span.start..span.end].to_string());
    let type_node = type_span.map(|span| AstNode::from_span(file, span));

    PythonParameterInfo {
        node: AstNode::from_span(file, span_from_ruff_range(parameter.range)),
        name: parameter.name.id.to_string(),
        name_node: AstNode::from_span(file, span_from_ruff_range(parameter.name.range)),
        type_node,
        type_text,
        default_value_node: None,
        kind,
    }
}

const SELF_PARAMETER: &str = "self";
const NOT_IMPLEMENTED_ERROR: &str = "NotImplementedError";
const OVERRIDE_DECORATOR: &str = "override";

/// Extracts all parameters in order from a `ruff_python_ast::Parameters` node.
pub(super) fn extract_parameters_from_ast<'a>(
    params: &Parameters,
    file: &'a ParsedFile,
) -> Vec<PythonParameterInfo<'a>> {
    let mut result = Vec::new();
    let mut is_first_param = true;

    for param_with_default in params.posonlyargs.iter().chain(params.args.iter()) {
        let name = param_with_default.parameter.name.id.as_str();
        let kind = if is_first_param && matches!(name, SELF_PARAMETER | "cls") {
            PythonParameterKind::Receiver
        } else {
            PythonParameterKind::Positional
        };
        is_first_param = false;
        result.push(build_parameter_with_default(param_with_default, kind, file));
    }

    if let Some(vararg) = &params.vararg {
        is_first_param = false;
        result.push(build_variadic_parameter(
            vararg,
            PythonParameterKind::VarPositional,
            file,
        ));
    }

    for param_with_default in &params.kwonlyargs {
        let _ = is_first_param;
        result.push(build_parameter_with_default(
            param_with_default,
            PythonParameterKind::KeywordOnly,
            file,
        ));
    }

    if let Some(kwarg) = &params.kwarg {
        result.push(build_variadic_parameter(
            kwarg,
            PythonParameterKind::VarKeyword,
            file,
        ));
    }

    result
}

/// Extracts all parameters in order from a Python `parameters` or `function_definition` node.
#[cfg(test)]
#[must_use]
pub(super) fn extract_parameters<'a>(
    func_or_params_node: &AstNode<'a>,
) -> Vec<PythonParameterInfo<'a>> {
    let Some(parsed) = func_or_params_node.file.py_module() else {
        return Vec::new();
    };
    let Some(params) = find_parameters_at_span(parsed.syntax(), func_or_params_node.span()) else {
        return Vec::new();
    };
    extract_parameters_from_ast(params, func_or_params_node.file)
}

/// Discovers and extracts all function signatures from a Python file.
#[must_use]
pub fn extract_function_signatures(file: &ParsedFile) -> Vec<PythonFunctionSignature<'_>> {
    struct SignatureVisitor<'a> {
        file: &'a ParsedFile,
        /// True while directly inside a `Protocol` or `ABC` class body (not a nested `def`).
        in_protocol_or_abc_class: bool,
        signatures: Vec<PythonFunctionSignature<'a>>,
    }

    impl<'a> SourceOrderVisitor<'a> for SignatureVisitor<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            let enclosing = self.in_protocol_or_abc_class;
            match statement {
                Stmt::FunctionDef(func_def) => {
                    let parameters = extract_parameters_from_ast(&func_def.parameters, self.file);
                    let return_type_node = func_def.returns.as_ref().map(|ret| {
                        AstNode::from_span(self.file, span_from_ruff_range(ret.range()))
                    });
                    let decorators =
                        extract_decorators_from_slice(&func_def.decorator_list, self.file);
                    self.signatures.push(PythonFunctionSignature {
                        node: AstNode::from_span(self.file, span_from_ruff_range(func_def.range)),
                        name_node: AstNode::from_span(
                            self.file,
                            span_from_ruff_range(func_def.name.range),
                        ),
                        name: func_def.name.id.to_string(),
                        parameters,
                        return_type_node,
                        has_exempt_signature_decorator: has_exempt_signature_decorator(&decorators),
                        is_in_protocol_or_abc_class: enclosing,
                        has_stub_body: is_stub_body(&func_def.body, &self.file.source),
                    });
                    self.in_protocol_or_abc_class = false;
                }
                Stmt::ClassDef(class_def) => {
                    self.in_protocol_or_abc_class =
                        is_protocol_or_abc_class(class_def, &self.file.source);
                }
                _ => {}
            }
            walk_stmt(self, statement);
            self.in_protocol_or_abc_class = enclosing;
        }
    }

    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut visitor = SignatureVisitor {
        file,
        in_protocol_or_abc_class: false,
        signatures: Vec::new(),
    };
    visitor.visit_body(&parsed.syntax().body);
    visitor.signatures
}

/// Returns each direct `StmtFunctionDef` in `scope_body` (`module` or class `body`).
pub(super) fn direct_function_definitions(scope_body: &[Stmt]) -> Vec<&StmtFunctionDef> {
    scope_body
        .iter()
        .filter_map(|statement| match statement {
            Stmt::FunctionDef(func_def) => Some(func_def),
            _ => None,
        })
        .collect()
}

/// Returns the receiver parameter name (`"self"`, or `"cls"` when `allow_classmethod_cls` is true)
/// for a method `function_def`, or `None` if decorated with `@staticmethod` (or `@classmethod`
/// when `!allow_classmethod_cls`) or if the first parameter is not a receiver.
pub(super) fn method_receiver_name_ast(
    function_def: &StmtFunctionDef,
    allow_classmethod_cls: bool,
    file: &ParsedFile,
) -> Option<String> {
    let decorators = extract_decorators_from_slice(&function_def.decorator_list, file);
    let is_excluded_decorator = decorators.iter().any(|decorator| {
        decorator.terminal_name == "staticmethod"
            || (!allow_classmethod_cls && decorator.terminal_name == "classmethod")
    });
    if is_excluded_decorator {
        return None;
    }
    let first = extract_parameters_from_ast(&function_def.parameters, file)
        .into_iter()
        .next()?;
    if first.kind != PythonParameterKind::Receiver {
        return None;
    }
    if !allow_classmethod_cls && first.name != SELF_PARAMETER {
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

/// Returns true if `decorators` include `@override`, `@overload`, `@abstractmethod`,
/// `@fixture` (`@pytest.fixture`), `@<function>.register` (`functools.singledispatch`
/// implementations, which dispatch on their annotations), or `@<property>.setter` (whose value
/// type mirrors the getter's return type).
fn has_exempt_signature_decorator(decorators: &[DecoratorInfo<'_>]) -> bool {
    let is_exempt = |name: &str| {
        matches!(
            name,
            OVERRIDE_DECORATOR | "overload" | "abstractmethod" | "fixture" | "register" | "setter"
        )
    };
    decorators
        .iter()
        .any(|decorator| is_exempt(&decorator.terminal_name) || is_exempt(&decorator.path))
}

/// Returns true if `statement` is an expression statement wrapping a string literal (docstring).
fn is_docstring_statement(statement: &Stmt) -> bool {
    matches!(
        statement,
        Stmt::Expr(expr_statement)
            if matches!(
                expr_statement.value.as_ref(),
                Expr::StringLiteral(_) | Expr::BytesLiteral(_) | Expr::FString(_)
            )
    )
}

/// Returns true if `statement` is `pass`, `...`, or `raise NotImplementedError` / `raise NotImplementedError(...)`.
fn is_stub_statement(statement: &Stmt, source: &str) -> bool {
    match statement {
        Stmt::Pass(_) => true,
        Stmt::Expr(expr_statement) => {
            matches!(expr_statement.value.as_ref(), Expr::EllipsisLiteral(_))
        }
        Stmt::Raise(raise_statement) => {
            raise_statement.cause.is_none()
                && raise_statement.exc.as_deref().is_some_and(|exc| match exc {
                    Expr::Name(name) => name.id == NOT_IMPLEMENTED_ERROR,
                    Expr::Call(call) => {
                        resolve_path_and_terminal_expr(&call.func, source).1
                            == NOT_IMPLEMENTED_ERROR
                    }
                    _ => false,
                })
        }
        _ => false,
    }
}

/// Returns true if the function body `statements` is a stub consisting only of an optional
/// docstring and `...`, `pass`, or `raise NotImplementedError`.
fn is_stub_body(statements: &[Stmt], source: &str) -> bool {
    let remaining = if statements.first().is_some_and(is_docstring_statement) {
        &statements[1..]
    } else {
        statements
    };
    remaining.is_empty() || (remaining.len() == 1 && is_stub_statement(&remaining[0], source))
}

/// Returns true if `decorators` include `@override`, which makes the decorated method's name
/// mandated by a contract.
///
/// Python has no structural trait implementations, so the contract is an explicit
/// `@override` decorator on a method.
pub(super) fn has_override_decorator(decorators: &[Decorator], file: &ParsedFile) -> bool {
    extract_decorators_from_slice(decorators, file)
        .iter()
        .any(|decorator| {
            decorator.terminal_name == OVERRIDE_DECORATOR || decorator.path == OVERRIDE_DECORATOR
        })
}

/// Finds all nested Python function definitions (`def ...` inside another `def ...`) and returns
/// `(function_node, function_name)` pairs.
#[must_use]
pub fn find_nested_functions(file: &ParsedFile) -> Vec<(AstNode<'_>, String)> {
    struct NestedFunctionVisitor<'a> {
        file: &'a ParsedFile,
        in_function: bool,
        out: Vec<(AstNode<'a>, String)>,
    }

    impl<'a> SourceOrderVisitor<'a> for NestedFunctionVisitor<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            match statement {
                Stmt::FunctionDef(func_def) => {
                    if self.in_function {
                        self.out.push((
                            AstNode::from_span(self.file, span_from_ruff_range(func_def.range)),
                            func_def.name.id.to_string(),
                        ));
                    }
                    let prev = self.in_function;
                    self.in_function = true;
                    walk_stmt(self, statement);
                    self.in_function = prev;
                }
                Stmt::ClassDef(_) => {
                    let prev = self.in_function;
                    self.in_function = false;
                    walk_stmt(self, statement);
                    self.in_function = prev;
                }
                _ => walk_stmt(self, statement),
            }
        }
    }

    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut visitor = NestedFunctionVisitor {
        file,
        in_function: false,
        out: Vec::new(),
    };
    visitor.visit_body(&parsed.syntax().body);
    visitor.out
}
