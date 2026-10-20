//! Python function signatures, parameter extraction, and method receivers.

use super::decorators::{DecoratorInfo, extract_decorators_from_slice};
use super::docstrings::{PythonDocstring, docstring_of};
use super::names::written_name;
use crate::code_lint::ast::{AstNode, ParsedFile, span_from_ruff_range};
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_stmt};
use ruff_python_ast::{
    Decorator, Expr, Parameter, ParameterWithDefault, Parameters, Stmt, StmtClassDef,
    StmtFunctionDef,
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
    /// Parameter identifier name.
    pub name: String,
    /// Parameter identifier AST node.
    pub name_node: AstNode<'a>,
    /// Type annotation AST node if present (e.g. `list[str]`).
    pub type_node: Option<AstNode<'a>>,
    /// Formatted type annotation text.
    pub type_text: Option<String>,
    /// Default value expression AST node if present (e.g. `None` in `x: int | None = None`).
    pub default_node: Option<AstNode<'a>>,
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

    /// The parameter name with its leading `*` or `**` when variadic.
    #[must_use]
    pub fn display_name(&self) -> String {
        match self.kind {
            PythonParameterKind::VarPositional => format!("*{}", self.name),
            PythonParameterKind::VarKeyword => format!("**{}", self.name),
            PythonParameterKind::Receiver
            | PythonParameterKind::Positional
            | PythonParameterKind::KeywordOnly => self.name.clone(),
        }
    }
}

/// Structured representation of a Python function signature.
#[derive(Clone)]
pub struct PythonFunctionSignature<'a> {
    /// Function identifier AST node.
    pub name_node: AstNode<'a>,
    /// Function identifier name.
    pub name: String,
    /// Parsed parameters in declaration order.
    pub parameters: Vec<PythonParameterInfo<'a>>,
    /// Return type annotation AST node (`-> <type>`), if present.
    pub return_type_node: Option<AstNode<'a>>,
    /// Parsed decorators on this function definition.
    decorators: Vec<DecoratorInfo>,
    /// True if the body is a stub ([`is_stub_body`]).
    pub(super) has_stub_body: bool,
    /// The `class` whose body contains this function when it is the nearest enclosing `def` or
    /// `class`.
    pub(super) enclosing_class: Option<&'a StmtClassDef>,
    /// The `def` whose body contains this one when it is the nearest enclosing `def` or `class`.
    pub(super) enclosing_function: Option<&'a StmtFunctionDef>,
    /// The parsed definition, for analyses of the body.
    pub(super) definition: &'a StmtFunctionDef,
}

impl PythonFunctionSignature<'_> {
    /// Returns true if the function's signature is imposed from outside: a data-model dunder
    /// method other than `__init__`, `__new__`, and `__call__`, or a function decorated with
    /// `@override`, `@overload`, `@abstractmethod`, `@fixture` (`@pytest.fixture`),
    /// `@<function>.register` (`functools.singledispatch` implementations, which dispatch on
    /// their annotations), or `@<property>.setter` (whose value type mirrors the getter's return
    /// type).
    #[must_use]
    pub fn has_imposed_signature(&self) -> bool {
        is_data_model_dunder(&self.name)
            || self.decorators.iter().any(|decorator| {
                decorator.matches_any(IMPOSED_SIGNATURE_DECORATOR_PATHS)
                    || decorator.is_attribute_named(&["register", SETTER_ATTRIBUTE])
            })
    }

    /// Returns true if the function is decorated with `@override` (`typing.override` or
    /// `typing_extensions.override`), resolved through the file's imports.
    #[must_use]
    pub fn has_override_decorator(&self) -> bool {
        self.decorators
            .iter()
            .any(|decorator| decorator.matches_any(OVERRIDE_DECORATOR_PATHS))
    }

    /// Returns true if the function is decorated with `@overload` (`typing.overload` or
    /// `typing_extensions.overload`), resolved through the file's imports.
    #[must_use]
    pub(super) fn is_overload(&self) -> bool {
        self.decorators
            .iter()
            .any(|decorator| decorator.matches_any(OVERLOAD_DECORATOR_PATHS))
    }

    /// Returns true if the function is defined in another function's body (its nearest
    /// enclosing `def` or `class` is a `def`). Methods of a class defined in a function are not
    /// nested.
    #[must_use]
    pub const fn is_nested(&self) -> bool {
        self.enclosing_function.is_some()
    }

    /// Returns true if the function is a class constructor (`__init__` or `__new__`) defined
    /// directly in a class body.
    #[must_use]
    pub fn is_constructor(&self) -> bool {
        self.enclosing_class.is_some() && matches!(self.name.as_str(), INIT_METHOD | NEW_METHOD)
    }

    /// Returns true if the function body is a stub consisting only of an optional docstring
    /// and `...`, `pass`, or `raise NotImplementedError`.
    #[must_use]
    pub const fn has_stub_body(&self) -> bool {
        self.has_stub_body
    }

    /// Returns true if the function is documented elsewhere: a data-model dunder method other
    /// than `__init__`, `__new__`, and `__call__` (documented by the Python data model), an
    /// `@override` method (documented on the base method), or a `@<property>.setter` /
    /// `@<property>.deleter` accessor (documented on the getter).
    #[must_use]
    pub fn is_documented_elsewhere(&self) -> bool {
        is_data_model_dunder(&self.name)
            || self.decorators.iter().any(|decorator| {
                decorator.matches_any(OVERRIDE_DECORATOR_PATHS)
                    || decorator.is_attribute_named(&[SETTER_ATTRIBUTE, "deleter"])
            })
    }

    /// Returns true if the function is a property getter (`@property`,
    /// `@functools.cached_property`), documented like an attribute rather than a function.
    #[must_use]
    pub fn is_property_getter(&self) -> bool {
        self.decorators
            .iter()
            .any(|decorator| decorator.matches_any(PROPERTY_DECORATOR_PATHS))
    }
}

impl<'a> PythonFunctionSignature<'a> {
    /// The function's docstring, if its body starts with one.
    #[must_use]
    pub fn docstring(&self) -> Option<PythonDocstring<'a>> {
        docstring_of(&self.definition.body, self.name_node.file)
    }
}

pub(super) const SELF_RECEIVER: &str = "self";
pub(super) const CLS_RECEIVER: &str = "cls";
pub(super) const OVERLOAD_DECORATOR_PATHS: &[&str] =
    &["overload", "typing.overload", "typing_extensions.overload"];
const INIT_METHOD: &str = "__init__";
const NEW_METHOD: &str = "__new__";
const NOT_IMPLEMENTED_ERROR: &str = "NotImplementedError";
/// The attribute of a property setter decorator (`@<property>.setter`).
const SETTER_ATTRIBUTE: &str = "setter";
const PROPERTY_DECORATOR_PATHS: &[&str] = &[
    "property",
    "builtins.property",
    "functools.cached_property",
    "cached_property",
];
const OVERRIDE_DECORATOR_PATHS: &[&str] =
    &["override", "typing.override", "typing_extensions.override"];
const IMPOSED_SIGNATURE_DECORATOR_PATHS: &[&str] = &[
    "override",
    "typing.override",
    "typing_extensions.override",
    "overload",
    "typing.overload",
    "typing_extensions.overload",
    "abstractmethod",
    "abc.abstractmethod",
    "fixture",
    "pytest.fixture",
    "pytest_asyncio.fixture",
    "register",
];

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
        decorator.matches_any(&["staticmethod", "builtins.staticmethod"])
            || (!allow_classmethod_cls
                && decorator.matches_any(&["classmethod", "builtins.classmethod"]))
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
    if !allow_classmethod_cls && first.name != SELF_RECEIVER {
        return None;
    }
    Some(first.name)
}

/// Discovers and extracts all function signatures from a Python file.
#[must_use]
pub fn extract_function_signatures(file: &ParsedFile) -> Vec<PythonFunctionSignature<'_>> {
    struct SignatureVisitor<'a> {
        file: &'a ParsedFile,
        /// The `class` being walked while it is the nearest enclosing `def` or `class`.
        enclosing_class: Option<&'a StmtClassDef>,
        /// The `def` being walked while it is the nearest enclosing `def` or `class`.
        enclosing_function: Option<&'a StmtFunctionDef>,
        signatures: Vec<PythonFunctionSignature<'a>>,
    }

    impl<'a> SourceOrderVisitor<'a> for SignatureVisitor<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            let enclosing_class = self.enclosing_class;
            let enclosing_function = self.enclosing_function;
            match statement {
                Stmt::FunctionDef(func_def) => {
                    self.signatures.push(function_signature(
                        func_def,
                        self.file,
                        enclosing_class,
                        enclosing_function,
                    ));
                    self.enclosing_class = None;
                    self.enclosing_function = Some(func_def);
                }
                Stmt::ClassDef(class_def) => {
                    self.enclosing_class = Some(class_def);
                    self.enclosing_function = None;
                }
                _ => {}
            }
            walk_stmt(self, statement);
            self.enclosing_class = enclosing_class;
            self.enclosing_function = enclosing_function;
        }
    }

    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut visitor = SignatureVisitor {
        file,
        enclosing_class: None,
        enclosing_function: None,
        signatures: Vec::new(),
    };
    visitor.visit_body(&parsed.syntax().body);
    visitor.signatures
}

/// Builds the signature of `func_def`; `enclosing_class` and `enclosing_function` name the
/// nearest enclosing `class` or `def` whose body contains `func_def`.
pub(super) fn function_signature<'a>(
    func_def: &'a StmtFunctionDef,
    file: &'a ParsedFile,
    enclosing_class: Option<&'a StmtClassDef>,
    enclosing_function: Option<&'a StmtFunctionDef>,
) -> PythonFunctionSignature<'a> {
    PythonFunctionSignature {
        name_node: AstNode::from_span(file, span_from_ruff_range(func_def.name.range)),
        name: func_def.name.id.to_string(),
        parameters: extract_parameters_from_ast(&func_def.parameters, file),
        return_type_node: func_def
            .returns
            .as_ref()
            .map(|ret| AstNode::from_span(file, span_from_ruff_range(ret.range()))),
        decorators: extract_decorators_from_slice(&func_def.decorator_list, file),
        has_stub_body: is_stub_body(&func_def.body, &file.source),
        enclosing_class,
        enclosing_function,
        definition: func_def,
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
                        written_name(&call.func, source).terminal == NOT_IMPLEMENTED_ERROR
                    }
                    _ => false,
                })
        }
        _ => false,
    }
}

/// Extracts all parameters in order from a `ruff_python_ast::Parameters` node.
pub(super) fn extract_parameters_from_ast<'a>(
    params: &Parameters,
    file: &'a ParsedFile,
) -> Vec<PythonParameterInfo<'a>> {
    let mut result = Vec::new();
    let mut is_first_param = true;

    for param_with_default in params.posonlyargs.iter().chain(params.args.iter()) {
        let name = param_with_default.parameter.name.id.as_str();
        let kind = if is_first_param && matches!(name, SELF_RECEIVER | CLS_RECEIVER) {
            PythonParameterKind::Receiver
        } else {
            PythonParameterKind::Positional
        };
        is_first_param = false;
        result.push(build_parameter_with_default(param_with_default, kind, file));
    }

    if let Some(vararg) = &params.vararg {
        result.push(build_variadic_parameter(
            vararg,
            PythonParameterKind::VarPositional,
            file,
        ));
    }

    for param_with_default in &params.kwonlyargs {
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
    let default_node = param_with_default
        .default
        .as_ref()
        .map(|default| AstNode::from_span(file, span_from_ruff_range(default.range())));

    PythonParameterInfo {
        name: parameter.name.id.to_string(),
        name_node: AstNode::from_span(file, span_from_ruff_range(parameter.name.range)),
        type_node,
        type_text,
        default_node,
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
        name: parameter.name.id.to_string(),
        name_node: AstNode::from_span(file, span_from_ruff_range(parameter.name.range)),
        type_node,
        type_text,
        default_node: None,
        kind,
    }
}

/// Returns true if `decorators` include `@override`, which makes the decorated method's name
/// mandated by a contract.
///
/// Python has no structural trait implementations, so the contract is an explicit
/// `@override` decorator on a method.
pub(super) fn has_override_decorator(decorators: &[Decorator], file: &ParsedFile) -> bool {
    extract_decorators_from_slice(decorators, file)
        .iter()
        .any(|decorator| decorator.matches_any(OVERRIDE_DECORATOR_PATHS))
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

/// Finds all Python class definitions inside a function or method body (`class ...` inside `def ...`)
/// and returns `(class_node, class_name)` pairs.
#[must_use]
pub fn find_nested_classes(file: &ParsedFile) -> Vec<(AstNode<'_>, String)> {
    struct NestedClassVisitor<'a> {
        file: &'a ParsedFile,
        in_function: bool,
        out: Vec<(AstNode<'a>, String)>,
    }

    impl<'a> SourceOrderVisitor<'a> for NestedClassVisitor<'a> {
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            match statement {
                Stmt::FunctionDef(_) => {
                    let prev = self.in_function;
                    self.in_function = true;
                    walk_stmt(self, statement);
                    self.in_function = prev;
                }
                Stmt::ClassDef(class_def) => {
                    if self.in_function {
                        self.out.push((
                            AstNode::from_span(self.file, span_from_ruff_range(class_def.range)),
                            class_def.name.id.to_string(),
                        ));
                    }
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
    let mut visitor = NestedClassVisitor {
        file,
        in_function: false,
        out: Vec::new(),
    };
    visitor.visit_body(&parsed.syntax().body);
    visitor.out
}

/// Returns true if `func_name` is a Python Data Model dunder method with a fixed signature
/// (all `__*__` methods except constructors `__init__` and `__new__`, and `__call__`, whose
/// signature is designed by the class author).
fn is_data_model_dunder(func_name: &str) -> bool {
    func_name.starts_with("__")
        && func_name.ends_with("__")
        && func_name.len() > 4
        && !matches!(func_name, INIT_METHOD | NEW_METHOD | "__call__")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Language;

    #[rstest::rstest]
    #[case::other_decorator("@pytest.mark.parametrize(\"x\", [1, 2])", false)]
    #[case::bare_override("@override", true)]
    #[case::qualified_override("@typing_extensions.override", true)]
    fn test_has_override_decorator(#[case] decorator: &str, #[case] expected: bool) {
        let source = format!("{decorator}\ndef foo():\n    pass\n");
        let file = ParsedFile::new(&source, Language::Python);
        let parsed = file.py_module().expect("source should parse");
        let Some(Stmt::FunctionDef(func_def)) = parsed.syntax().body.first() else {
            unreachable!("source should start with a function definition");
        };
        assert_eq!(
            has_override_decorator(&func_def.decorator_list, &file),
            expected
        );
    }

    #[test]
    fn test_extract_parameters_metadata() {
        use PythonParameterKind::{KeywordOnly, Positional, Receiver, VarKeyword};

        let source = indoc::indoc! {r"
            def handler(self, a: int, b: str = 'hello', *, c: bool, **kwargs):
                pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let signatures = extract_function_signatures(&file);
        let actual: Vec<(&str, PythonParameterKind, Option<&str>)> = signatures[0]
            .parameters
            .iter()
            .map(|param| (param.name.as_str(), param.kind, param.type_text.as_deref()))
            .collect();

        assert_eq!(
            actual,
            vec![
                ("self", Receiver, None),
                ("a", Positional, Some("int")),
                ("b", Positional, Some("str")),
                ("c", KeywordOnly, Some("bool")),
                ("kwargs", VarKeyword, None),
            ]
        );
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
    #[case::raise_from_not_implemented_cause(
        "def f(x: list[int]) -> None:\n    raise ValueError() from NotImplementedError",
        false
    )]
    fn test_has_stub_body(#[case] source: &str, #[case] expected: bool) {
        let file = ParsedFile::new(source, Language::Python);
        let sigs = extract_function_signatures(&file);
        assert_eq!(sigs[0].has_stub_body, expected);
    }
}
