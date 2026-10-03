//! AST helper predicates and structural extractors for Python.

// omni:disable-file [repeated-literal] -- Tree-sitter node kinds and field names (see ROADMAP)

use crate::code_lint::ast::{
    AstNode, LiteralOccurrence, LiteralRole, LiteralValue, ParsedFile, PositionalRead, RawNode,
    ScopePositionalReads, delimited_string_parts, parse_float_literal, parse_integer_literal,
};
use std::collections::{HashMap, HashSet};

/// Returns true for Python node kinds that hold statements as direct children.
///
/// `module` is the file root and `block` is an indented suite.
#[must_use]
pub(super) fn is_statement_container(kind: &str) -> bool {
    matches!(kind, "module" | "block")
}

/// Returns the `def` or `class` wrapped by a `decorated_definition` statement, so its header
/// spans from the first decorator to the end of the definition's own header.
#[must_use]
pub(super) fn decorated_definition<'a>(statement: &RawNode<'a>) -> Option<RawNode<'a>> {
    if statement.kind() == "decorated_definition" {
        statement.field("definition")
    } else {
        None
    }
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
                && matches!(
                    first_child.kind().as_ref(),
                    "identifier" | "list_splat_pattern" | "dictionary_splat_pattern"
                )
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

impl PythonBaseClass<'_> {
    /// Base class name with any generic type argument subscript (`[...]`) stripped.
    #[must_use]
    pub fn unsubscripted_name(&self) -> &str {
        self.name
            .split_once('[')
            .map_or(self.name.as_str(), |(base, _)| base.trim())
    }

    /// Returns true if this base class can represent a collaborator contract (an interface,
    /// domain `Protocol`, `ABC`, or concrete base class) rather than a structural marker
    /// (`object`, `Generic`, or `Protocol` itself).
    #[must_use]
    pub fn is_contract_base(&self) -> bool {
        !matches!(
            self.unsubscripted_name(),
            "object"
                | "builtins.object"
                | "Generic"
                | "typing.Generic"
                | "typing_extensions.Generic"
                | "Protocol"
                | "typing.Protocol"
                | "typing_extensions.Protocol"
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
            base.name == target
                || base
                    .name
                    .strip_suffix(target)
                    .is_some_and(|prefix| prefix.ends_with('.'))
        })
    }

    /// Returns true if the class name starts with the word `Fake` (after any leading `_`),
    /// such as `FakeClient`, `_FakeClient`, `Fake_Client`, `Fake2FA`, or `Fake`, but not
    /// words where `Fake` is followed by a lowercase letter (`Faker`, `Fakeable`).
    #[must_use]
    pub fn is_fake_class_name(&self) -> bool {
        self.name
            .trim_start_matches('_')
            .strip_prefix("Fake")
            .is_some_and(|rest| {
                rest.is_empty()
                    || !rest.starts_with(|character: char| character.is_ascii_lowercase())
            })
    }

    /// Returns true if the class declares at least one collaborator contract base class
    /// (excluding `object`, `Generic[...]`, and `Protocol[...]`).
    #[must_use]
    pub fn has_contract_base(&self) -> bool {
        self.bases.iter().any(PythonBaseClass::is_contract_base)
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
                if child.is_named()
                    && !child.is_extra()
                    && !matches!(
                        child.kind().as_ref(),
                        "keyword_argument" | "dictionary_splat"
                    )
                {
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

/// Controls how deeply [`collect_type_constructors`] traverses a Python type annotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnnotationTraversalDepth {
    /// Unwraps only transparent wrappers (`|`, `Optional`, `Union`, `Annotated[T, ...]`,
    /// `ClassVar[T]`, `Final[T]`, `Required[T]`, `NotRequired[T]`, `ReadOnly[T]`).
    TransparentWrappersOnly,
    /// Unwraps transparent wrappers and recurses into covariant type parameter positions of
    /// read-only containers (`Sequence[T]`, `Mapping[K, V]` value `V`, `tuple[...]`,
    /// `Awaitable[T]`, `Callable[[...], Ret]` return `Ret`, `Generator`/`Coroutine` yield and
    /// return). Invariant containers (`list`, `dict`, `set`, `Mutable*`) are not entered.
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
/// `set`, `List`, `Dict`, `Set`, `typing.List`, `typing.Dict`, `typing.Set`, etc.), or a
/// `collections` container (`defaultdict`, `deque`, `Counter`, `OrderedDict`) or its `typing`
/// alias (`DefaultDict`, `Deque`).
///
/// Qualified `collections.abc.Set` is excluded because it is the abstract set ABC, whereas
/// unqualified `Set` and `typing.Set` are flagged as concrete (per Ruff `PYI025`, `collections.abc.Set`
/// should be imported `as AbstractSet`).
fn is_concrete_collection_constructor(path: &str, terminal: &str) -> bool {
    match terminal {
        "list" | "List" | "dict" | "Dict" | "set" => is_std_type_constructor_prefix(path, terminal),
        "Set" => matches!(path, "Set" | "typing.Set" | "typing_extensions.Set"),
        "defaultdict" | "DefaultDict" | "deque" | "Deque" | "Counter" | "OrderedDict" => {
            path == terminal
                || path.strip_suffix(terminal).is_some_and(|prefix| {
                    matches!(prefix, "collections." | "typing." | "typing_extensions.")
                })
        }
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
                    "Mapping" | "Callable" => {
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

/// Abstract mutable collection constructors in `collections.abc` and `typing`.
const MUTABLE_COLLECTION_ABCS: &[&str] = &["MutableSequence", "MutableMapping", "MutableSet"];

/// Collects abstract mutable collection constructors (`MutableSequence`, `MutableMapping`,
/// `MutableSet`) from `type_node`, unwrapping only transparent wrappers.
///
/// Nested positions (`Sequence[MutableMapping[K, V]]`) are not collected: mutation is only
/// tracked on the annotated value itself, so a nested mutable type cannot be judged.
#[must_use]
pub fn collect_mutable_collection_types(type_node: &AstNode<'_>) -> Vec<String> {
    collect_type_constructors(
        type_node,
        AnnotationTraversalDepth::TransparentWrappersOnly,
        |full_path, terminal| is_std_type_constructor(full_path, terminal, MUTABLE_COLLECTION_ABCS),
    )
}

/// Collects specific read-only abstract collection constructors (`Sequence`, `Collection`)
/// from `type_node`, unwrapping only transparent wrappers.
#[must_use]
pub fn collect_specific_collection_types(type_node: &AstNode<'_>) -> Vec<String> {
    collect_type_constructors(
        type_node,
        AnnotationTraversalDepth::TransparentWrappersOnly,
        |full_path, terminal| {
            is_std_type_constructor(full_path, terminal, &["Sequence", "Collection"])
        },
    )
}

/// Formats deduplicated collection replacements for `type_paths`, mapping dictionary-like types
/// to `mapping`, set-like types to `set`, and sequence-like types to `sequence`.
fn format_collection_replacements(
    type_paths: &[String],
    mapping: &'static str,
    set: &'static str,
    sequence: &'static str,
) -> String {
    let mut replacements: Vec<&str> = Vec::new();
    for type_path in type_paths {
        let terminal = type_path.rsplit('.').next().unwrap_or(type_path);
        let replacement = match terminal {
            "dict" | "Dict" | "defaultdict" | "DefaultDict" | "Counter" | "OrderedDict"
            | "MutableMapping" => mapping,
            "set" | "Set" | "MutableSet" => set,
            _ => sequence,
        };
        if !replacements.contains(&replacement) {
            replacements.push(replacement);
        }
    }
    replacements.join(", ")
}

/// Returns the read-only `collections.abc` replacements of collection `type_paths`, joined with `", "`.
///
/// `list`, `deque`, and `MutableSequence` become `collections.abc.Sequence`; `dict`, `defaultdict`,
/// `Counter`, `OrderedDict`, and `MutableMapping` become `collections.abc.Mapping`; and `set` and
/// `MutableSet` become `collections.abc.Set`. Duplicates are removed.
#[must_use]
pub fn read_only_collection_replacements(type_paths: &[String]) -> String {
    format_collection_replacements(
        type_paths,
        "collections.abc.Mapping",
        "collections.abc.Set",
        "collections.abc.Sequence",
    )
}

/// Returns the immutable constant collection replacements for `type_paths`, joined with `", "`.
///
/// Sequence types (`list`, `deque`, `MutableSequence`) become `tuple`; set types (`set`,
/// `MutableSet`) become `frozenset`; and mapping types (`dict`, `defaultdict`, `Counter`,
/// `OrderedDict`, `MutableMapping`) become `frozendict`. Duplicates are removed.
#[must_use]
pub fn immutable_constant_collection_replacements(type_paths: &[String]) -> String {
    format_collection_replacements(type_paths, "frozendict", "frozenset", "tuple")
}

/// A Python module-level constant whose type annotation or initializer uses a mutable collection.
pub struct PythonMutableModuleConstant<'a> {
    /// Constant identifier name (e.g. `"ALLOWED"` or `"_PORTS"`).
    pub name: String,
    /// Matched mutable collection type constructors (e.g. `["list"]`, `["MutableSequence"]`).
    pub matched_types: Vec<String>,
    /// The AST node to highlight: the `type` annotation node when the annotation is mutable,
    /// or the RHS initializer expression node when the value is mutable.
    pub target_node: AstNode<'a>,
}

/// Returns true if `type_node` specifies a read-only `Mapping` contract (optionally wrapped in
/// transparent wrappers such as `Final[...]`, `Optional[...]`, or `Annotated[...]`).
fn has_read_only_mapping_annotation(type_node: &RawNode<'_>) -> bool {
    let mut matched = Vec::new();
    collect_type_constructors_raw(
        type_node,
        AnnotationTraversalDepth::TransparentWrappersOnly,
        &|full_path, terminal| is_std_type_constructor(full_path, terminal, &["Mapping"]),
        &mut matched,
    );
    !matched.is_empty()
}

/// Returns true if `(path, terminal)` is a runtime mutable collection constructor (`list`, `dict`,
/// `set`, or the `collections` containers `defaultdict`, `deque`, `Counter`, `OrderedDict`).
fn is_runtime_mutable_collection_constructor(path: &str, terminal: &str) -> bool {
    match terminal {
        "list" | "dict" | "set" => {
            path == terminal || path.strip_suffix(terminal) == Some("builtins.")
        }
        "defaultdict" | "deque" | "Counter" | "OrderedDict" => {
            path == terminal || path.strip_suffix(terminal) == Some("collections.")
        }
        _ => false,
    }
}

/// Returns the mutable collection constructor name if `right` is a mutable collection literal,
/// comprehension, or constructor call.
fn mutable_collection_initializer_type(
    right: &RawNode<'_>,
    is_read_only_mapping: bool,
) -> Option<String> {
    match right.kind().as_ref() {
        "list" | "list_comprehension" => Some("list".to_owned()),
        "set" | "set_comprehension" => Some("set".to_owned()),
        "dictionary" | "dictionary_comprehension" if !is_read_only_mapping => {
            Some("dict".to_owned())
        }
        "call" => {
            let callee = right.field("function")?;
            let callee_base = if callee.kind() == "subscript" {
                callee.field("value")?
            } else {
                callee
            };
            let (path, terminal) = resolve_path_and_terminal_raw(&callee_base);
            if is_read_only_mapping && terminal == "dict" {
                return None;
            }
            is_runtime_mutable_collection_constructor(&path, &terminal).then_some(path)
        }
        _ => None,
    }
}

/// Collects Python module-level constants whose type annotation or initializer value uses a
/// mutable collection.
#[must_use]
pub fn collect_mutable_module_constants(file: &ParsedFile) -> Vec<PythonMutableModuleConstant<'_>> {
    let abc_set_imported = has_unaliased_collections_abc_set_import(file);
    let mut out = Vec::new();
    for node in file.grep.root().dfs() {
        if node.kind() != "assignment" {
            continue;
        }
        let is_top_level = node
            .parent()
            .filter(|statement| statement.kind() == "expression_statement")
            .is_some_and(|statement| is_module_level(&statement));
        if !is_top_level {
            continue;
        }
        let Some(left) = node.field("left") else {
            continue;
        };
        if left.kind() != "identifier" {
            continue;
        }
        let name = left.text().into_owned();
        if name.starts_with("__") && name.ends_with("__") {
            continue;
        }
        let type_node = node.field("type");
        let is_final = type_node.as_ref().is_some_and(has_final_annotation);
        if !is_constant_name(&name) && !is_final {
            continue;
        }

        if let Some(ref annotation) = type_node {
            let mut matched = Vec::new();
            collect_type_constructors_raw(
                annotation,
                AnnotationTraversalDepth::CovariantPositions,
                &|full_path, terminal| {
                    if abc_set_imported && full_path == "Set" {
                        return false;
                    }
                    is_concrete_collection_constructor(full_path, terminal)
                        || is_std_type_constructor(full_path, terminal, MUTABLE_COLLECTION_ABCS)
                },
                &mut matched,
            );
            if !matched.is_empty() {
                out.push(PythonMutableModuleConstant {
                    name,
                    matched_types: matched,
                    target_node: AstNode::from_raw(annotation.clone()),
                });
                continue;
            }
        }

        let Some(right) = node.field("right").map(without_parentheses) else {
            continue;
        };
        let is_read_only_mapping = type_node
            .as_ref()
            .is_some_and(has_read_only_mapping_annotation);
        if let Some(matched_type) =
            mutable_collection_initializer_type(&right, is_read_only_mapping)
        {
            out.push(PythonMutableModuleConstant {
                name,
                matched_types: vec![matched_type],
                target_node: AstNode::from_raw(right),
            });
        }
    }
    out
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

/// Returns true if a Python `class_definition` inherits from `Protocol` or `ABC` or declares
/// `metaclass=ABCMeta`.
fn is_protocol_or_abc_class_raw(class_node: &RawNode<'_>) -> bool {
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
fn is_typed_dict_class_raw(class_node: &RawNode<'_>) -> bool {
    base_class_terminals_raw(class_node)
        .iter()
        .any(|terminal| terminal == "TypedDict")
}

/// Returns the terminal name of each positional base class of a Python `class_definition`,
/// unwrapping generic subscripts (`Protocol[T]` yields `Protocol`).
fn base_class_terminals_raw(class_node: &RawNode<'_>) -> Vec<String> {
    let Some(superclasses) = class_node.field("superclasses") else {
        return Vec::new();
    };
    superclasses
        .children()
        .filter(|child| child.is_named() && !child.is_extra() && child.kind() != "keyword_argument")
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
fn is_stub_function_body(func_node: &AstNode<'_>) -> bool {
    let Some(body) = func_node.raw.field("body") else {
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
/// Matches direct call mutations (`fn().append(...)`, `fn()[0] = 1`) and local bindings
/// (`buf = fn(); buf.append(...)`, `(buf := fn())`). A binding only counts in the function
/// (or module) scope that assigns it. Callees are matched by name only, so a mutated
/// `obj.get()` result also exempts an unrelated function named `get`.
#[must_use]
pub fn collect_locally_mutated_return_functions(file: &ParsedFile) -> HashSet<String> {
    // A binding is keyed by its enclosing function (`None` at module level) and its name.
    type ScopedName = (Option<usize>, String);
    let enclosing_function = |node: &RawNode<'_>| {
        node.ancestors()
            .find(|ancestor| ancestor.kind() == "function_definition")
            .map(|function_node| function_node.range().start)
    };
    let mut mutated_functions = HashSet::new();
    let mut bindings_to_callee: HashMap<ScopedName, String> = HashMap::new();
    let mut mutated_identifiers: HashSet<ScopedName> = HashSet::new();

    for node in file.grep.root().dfs() {
        let binding = match node.kind().as_ref() {
            "assignment" => node.field("left").zip(node.field("right")),
            "named_expression" => node.field("name").zip(node.field("value")),
            _ => None,
        };
        if let Some((target, value)) = binding
            && target.kind() == "identifier"
            && let Some(callee_name) = called_terminal_name(&value)
        {
            let scoped_name = (enclosing_function(&node), target.text().into_owned());
            bindings_to_callee.insert(scoped_name, callee_name);
        }

        if let Some(receiver) = in_place_mutated_receiver(&node) {
            if let Some(callee_name) = called_terminal_name(&receiver) {
                mutated_functions.insert(callee_name);
            } else if receiver.kind() == "identifier" {
                mutated_identifiers
                    .insert((enclosing_function(&receiver), receiver.text().into_owned()));
            }
        }
    }

    for scoped_name in &mutated_identifiers {
        if let Some(callee_name) = bindings_to_callee.get(scoped_name) {
            mutated_functions.insert(callee_name.clone());
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

/// Walks statements in `__init__` (without entering nested functions/classes/lambdas) and
/// collects public `self.<attr>: <type>` annotations.
fn collect_init_annotated_attrs_rec<'a>(
    node: &RawNode<'a>,
    class_name: &str,
    mutated_attrs: &HashSet<String>,
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
                is_typed_dict_key: false,
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
        let is_typed_dict = is_typed_dict_class_raw(&class_node);

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
                if !attr_name.starts_with('_') {
                    let is_mutated_in_class = mutated_attrs.contains(&attr_name);
                    out.push(PythonAnnotatedAttribute {
                        class_name: class_name.clone(),
                        name: attr_name,
                        type_node: AstNode::from_raw(type_node),
                        is_mutated_in_class,
                        is_typed_dict_key: is_typed_dict,
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
        // Defaults are evaluated in the enclosing scope; only the body is shadowed.
        return node.field("parameters").is_some_and(|params| {
            check_parameter_mutated_or_escaping_rec(&params, parameter_name)
        });
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
                // Defaults are evaluated in the enclosing scope; only the body is shadowed.
                if let Some(params) = node.field("parameters") {
                    analyze_capability_rec(&params, parameter_name, loop_or_closure_depth, tracker);
                }
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

/// Calls whose first argument is a name or type the language requires as a string
/// (`TypeVar("T")`, `cast("Node", value)`).
const TYPE_NAME_FIRST_ARGUMENT_CALLS: &[&str] = &[
    "TypeVar",
    "NewType",
    "ParamSpec",
    "TypeVarTuple",
    "NamedTuple",
    "TypedDict",
    "cast",
];

/// Returns the value of a non-interpolated `string`, `integer` or `float` node, or of a `-`
/// applied to a number; `None` for anything else, imaginary numbers and overflow.
fn literal_value(node: &RawNode<'_>) -> Option<LiteralValue> {
    match node.kind().as_ref() {
        "string" if !node.children().any(|child| child.kind() == "interpolation") => {
            let (opening, content) = delimited_string_parts(node);
            // Raw backslashes are literal: spell them as a plain string would (`r"\d"` is `"\\d"`).
            let content = if opening.contains(['r', 'R']) {
                content.replace('\\', "\\\\")
            } else {
                content
            };
            Some(if opening.contains(['b', 'B']) {
                LiteralValue::Bytes(content)
            } else {
                LiteralValue::Str(content)
            })
        }
        "integer" | "float" if node.text().ends_with(['j', 'J']) => None,
        "integer" => parse_integer_literal(&node.text()),
        "float" => parse_float_literal(&node.text()),
        "unary_operator" if node.field("operator")?.text() == "-" => {
            let argument = node.field("argument")?;
            if matches!(argument.kind().as_ref(), "integer" | "float") {
                literal_value(&argument)?.negated()
            } else {
                None
            }
        }
        // `case -4:` spells the negation as a `-` token and a number directly in the pattern.
        "case_pattern" => {
            let mut children = node.children();
            match (children.next(), children.next(), children.next()) {
                (Some(sign), Some(number), None)
                    if sign.text() == "-"
                        && matches!(number.kind().as_ref(), "integer" | "float") =>
                {
                    literal_value(&number)?.negated()
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Returns true if `name` is spelled as a constant (`MAX_RETRIES`, `_TIMEOUT_S`).
fn is_constant_name(name: &str) -> bool {
    name.chars().any(|character| character.is_ascii_uppercase())
        && name.chars().all(|character| {
            character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
        })
}

/// Statements whose bodies stay at the enclosing level for constants: a platform branch or an
/// `ImportError` fallback at module level still defines module constants.
const CONSTANT_TRANSPARENT_STATEMENTS: &[&str] = &[
    "if_statement",
    "elif_clause",
    "else_clause",
    "try_statement",
    "except_clause",
    "finally_clause",
    "with_statement",
];

/// Returns true if `statement` sits at module level, possibly inside the bodies of
/// [`CONSTANT_TRANSPARENT_STATEMENTS`].
fn is_module_level(statement: &RawNode<'_>) -> bool {
    let mut container = statement.parent();
    while let Some(node) = container {
        match node.kind().as_ref() {
            "module" => return true,
            "block" => {}
            kind if CONSTANT_TRANSPARENT_STATEMENTS.contains(&kind) => {}
            _ => return false,
        }
        container = node.parent();
    }
    false
}

/// Returns true if `statement` sits at module or class level, possibly inside the bodies of
/// [`CONSTANT_TRANSPARENT_STATEMENTS`].
fn is_module_or_class_level(statement: &RawNode<'_>) -> bool {
    let mut container = statement.parent();
    while let Some(node) = container {
        match node.kind().as_ref() {
            "module" => return true,
            "block" => {
                if node
                    .parent()
                    .is_some_and(|owner| owner.kind() == "class_definition")
                {
                    return true;
                }
            }
            kind if CONSTANT_TRANSPARENT_STATEMENTS.contains(&kind) => {}
            _ => return false,
        }
        container = node.parent();
    }
    false
}

/// Returns true if `type_node` is `Final` or `Final[...]` (qualified or unqualified, optionally
/// wrapped in `Annotated[..., ...]`).
fn has_final_annotation(type_node: &RawNode<'_>) -> bool {
    let mut current = type_node.clone();
    loop {
        while matches!(current.kind().as_ref(), "type" | "parenthesized_expression") {
            let Some(inner) = current
                .children()
                .find(|child| child.is_named() && !child.is_extra())
            else {
                return false;
            };
            current = inner;
        }
        if matches!(current.kind().as_ref(), "generic_type" | "subscript")
            && let Some((base_node, type_args)) = extract_generic_base_and_args(&current)
        {
            let (base_path, base_terminal) = resolve_path_and_terminal_raw(&base_node);
            if is_std_type_constructor(&base_path, &base_terminal, &["Annotated"])
                && let Some(first_arg) = type_args.into_iter().next()
            {
                current = first_arg;
                continue;
            }
            return base_terminal == "Final";
        }
        return resolve_path_and_terminal_raw(&current).1 == "Final";
    }
}

/// Returns true if an `assignment` defines a module- or class-level constant: an
/// `UPPER_SNAKE_CASE` name or a `Final` annotation.
fn is_constant_assignment(assignment: &RawNode<'_>) -> bool {
    let is_module_or_class_level = assignment
        .parent()
        .filter(|statement| statement.kind() == "expression_statement")
        .is_some_and(|statement| is_module_or_class_level(&statement));
    let is_final = assignment
        .field("type")
        .is_some_and(|type_node| has_final_annotation(&type_node));
    let is_constant_target = assignment
        .field("left")
        .is_some_and(|target| target.kind() == "identifier" && is_constant_name(&target.text()));
    is_module_or_class_level && (is_constant_target || is_final)
}

/// Returns the expression inside any enclosing parentheses (`("x")` is `"x"`).
fn without_parentheses(node: RawNode<'_>) -> RawNode<'_> {
    let mut node = node;
    while node.kind() == "parenthesized_expression" {
        let inner = node
            .children()
            .find(|child| child.is_named() && !child.is_extra());
        let Some(inner) = inner else { break };
        node = inner;
    }
    node
}

/// Walks `node`, pushing collectable literals to `out` (see [`super::collect_literal_occurrences`]).
fn collect_literal_occurrences_rec<'a>(node: &RawNode<'a>, out: &mut Vec<LiteralOccurrence<'a>>) {
    match node.kind().as_ref() {
        // Annotations and docstrings are not values.
        "type" => return,
        "string" if is_docstring_raw(node) => return,
        "assignment" if is_constant_assignment(node) => {
            // A scalar constant defines its value; a composite one (`URLS = ["a", "b"]`)
            // is a named value whose parts are not collected.
            if let Some(right) = node.field("right").map(without_parentheses)
                && let Some(value) = literal_value(&right)
            {
                out.push(LiteralOccurrence {
                    node: AstNode::from_raw(right),
                    value,
                    role: LiteralRole::ConstantDefinition,
                });
            }
            return;
        }
        "subscript"
            if node
                .field("value")
                .is_some_and(|value| resolve_path_and_terminal_raw(&value).1 == "Literal") =>
        {
            return;
        }
        "call" => {
            let is_type_name_call = node.field("function").is_some_and(|function| {
                TYPE_NAME_FIRST_ARGUMENT_CALLS
                    .contains(&resolve_path_and_terminal_raw(&function).1.as_str())
            });
            if is_type_name_call {
                let arguments = node.field("arguments");
                let first_argument = arguments
                    .as_ref()
                    .and_then(|arguments| arguments.children().find(RawNode::is_named));
                for argument in arguments.iter().flat_map(RawNode::children) {
                    let is_first = first_argument
                        .as_ref()
                        .is_some_and(|first| first.range() == argument.range());
                    if !is_first {
                        collect_literal_occurrences_rec(&argument, out);
                    }
                }
                return;
            }
        }
        _ => {
            if let Some(value) = literal_value(node) {
                out.push(LiteralOccurrence {
                    node: AstNode::from_raw(node.clone()),
                    value,
                    role: LiteralRole::Inline,
                });
                return;
            }
        }
    }
    // Interpolated f-strings are templates: their literal parts are not collected, but the
    // expressions inside `{...}` are walked like any other code.
    let is_pattern = node.kind().ends_with("_pattern");
    let mut follows_minus = false;
    for child in node.children() {
        // Inside a pattern, `-404` can be a bare `-` token and a number with no node spanning
        // both: skip the number rather than collect it as `404`.
        let is_signed_number =
            follows_minus && matches!(child.kind().as_ref(), "integer" | "float");
        follows_minus = is_pattern && child.kind() == "-";
        if !is_signed_number {
            collect_literal_occurrences_rec(&child, out);
        }
    }
}

/// Collects Python literal occurrences (see [`super::collect_literal_occurrences`]).
#[must_use]
pub fn collect_literal_occurrences(file: &ParsedFile) -> Vec<LiteralOccurrence<'_>> {
    let mut out = Vec::new();
    collect_literal_occurrences_rec(&file.grep.root(), &mut out);
    out
}

/// A public Python instance attribute annotated inline (`self.<name>: <type>`) inside an instance method.
#[derive(Clone)]
pub struct PythonInlinePublicAttributeAnnotation<'a> {
    /// Name of the enclosing class (`{class}`).
    pub class_name: String,
    /// Name of the enclosing instance method (`{function}`).
    pub method_name: String,
    /// Public attribute identifier (`{name}`, without `"self."`).
    pub name: String,
    /// Source text of the type annotation (`{expression}`, e.g. `"int"` or `"Final[int]"`).
    pub annotation_text: String,
    /// Full `assignment` AST node (`self.foo: int = 1` or `self.foo: int`).
    pub assignment_node: AstNode<'a>,
}

/// Returns true if `type_node` is an unparameterized `Final` qualifier (`Final`, `typing.Final`,
/// or `typing_extensions.Final`, optionally wrapped in `Annotated[Final, ...]`), which PEP 591
/// forbids in a class body without an initializer (`x: Final` is invalid; `x: Final[T]` is valid).
fn is_bare_final_annotation(type_node: &RawNode<'_>) -> bool {
    let mut current = type_node.clone();
    loop {
        while matches!(current.kind().as_ref(), "type" | "parenthesized_expression") {
            let Some(inner) = current
                .children()
                .find(|child| child.is_named() && !child.is_extra())
            else {
                return false;
            };
            current = inner;
        }
        if matches!(current.kind().as_ref(), "generic_type" | "subscript")
            && let Some((base_node, type_args)) = extract_generic_base_and_args(&current)
        {
            let (base_path, base_terminal) = resolve_path_and_terminal_raw(&base_node);
            if is_std_type_constructor(&base_path, &base_terminal, &["Annotated"])
                && let Some(first_arg) = type_args.into_iter().next()
            {
                current = first_arg;
                continue;
            }
            return false;
        }
        let (path, terminal) = resolve_path_and_terminal_raw(&current);
        return is_std_type_constructor(&path, &terminal, &["Final"]);
    }
}

/// Returns true if `function_node` is an instance method (not decorated with `@staticmethod` or
/// `@classmethod`) whose first parameter is the receiver `self`.
fn is_instance_method_with_self_receiver(function_node: &RawNode<'_>) -> bool {
    let has_non_instance_decorator =
        extract_decorators_raw(function_node)
            .iter()
            .any(|decorator| {
                matches!(
                    decorator.terminal_name.as_str(),
                    "staticmethod" | "classmethod"
                )
            });
    if has_non_instance_decorator {
        return false;
    }
    extract_parameters_raw(function_node)
        .first()
        .is_some_and(|parameter| {
            parameter.kind == PythonParameterKind::Receiver && parameter.name == "self"
        })
}

/// Walks statements inside an instance method body (without entering nested functions, classes,
/// or lambdas) and collects inline `self.<public_attr>: <type>` annotations.
fn collect_method_inline_public_attr_annotations_rec<'a>(
    node: &RawNode<'a>,
    class_name: &str,
    method_name: &str,
    out: &mut Vec<PythonInlinePublicAttributeAnnotation<'a>>,
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
        let attribute_name = attribute_node.text().into_owned();
        if !attribute_name.starts_with('_') && !is_bare_final_annotation(&type_node) {
            out.push(PythonInlinePublicAttributeAnnotation {
                class_name: class_name.to_owned(),
                method_name: method_name.to_owned(),
                name: attribute_name,
                annotation_text: type_node.text().into_owned(),
                assignment_node: AstNode::from_raw(node.clone()),
            });
        }
    }
    for child in node.children() {
        collect_method_inline_public_attr_annotations_rec(&child, class_name, method_name, out);
    }
}

/// Collects inline type annotations on public instance attributes (`self.<attr>: <type>`) inside
/// instance methods across `file`.
///
/// Only direct instance methods (not decorated with `@staticmethod` or `@classmethod`, and whose
/// first parameter is the receiver `self`) of a `class_definition` are inspected. Nested functions,
/// nested classes, and lambdas inside a method are not entered. Private attributes starting with
/// `_` and unparameterized `Final` annotations (`self.x: Final = 1`) are skipped.
#[must_use]
pub fn collect_inline_public_attribute_annotations(
    file: &ParsedFile,
) -> Vec<PythonInlinePublicAttributeAnnotation<'_>> {
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

        for child in body.children() {
            let function_candidate = if child.kind() == "decorated_definition" {
                child.field("definition")
            } else {
                Some(child)
            };
            if let Some(function_node) = function_candidate
                && function_node.kind() == "function_definition"
                && is_instance_method_with_self_receiver(&function_node)
                && let Some(method_name_node) = function_node.field("name")
                && let Some(method_body) = function_node.field("body")
            {
                let method_name = method_name_node.text().into_owned();
                for statement in method_body.children() {
                    collect_method_inline_public_attr_annotations_rec(
                        &statement,
                        &class_name,
                        &method_name,
                        &mut out,
                    );
                }
            }
        }
    }
    out
}

/// Abstract and immutable collection constructors in `collections.abc`, `typing`, and `builtins`
/// that have a natural empty value (`()`, `{}`, `frozenset()`).
const ABSTRACT_AND_IMMUTABLE_COLLECTION_CONSTRUCTORS: &[&str] = &[
    "Sequence",
    "MutableSequence",
    "Mapping",
    "MutableMapping",
    "Set",
    "AbstractSet",
    "MutableSet",
    "Collection",
    "Iterable",
    "Reversible",
    "frozenset",
    "FrozenSet",
];

/// Returns true if `(path, terminal)` is a concrete or abstract collection type constructor
/// (excluding `tuple` / `Tuple`, which requires variadic-vs-fixed arity inspection when subscripted).
fn is_non_tuple_collection_constructor(path: &str, terminal: &str) -> bool {
    is_concrete_collection_constructor(path, terminal)
        || is_std_type_constructor(
            path,
            terminal,
            ABSTRACT_AND_IMMUTABLE_COLLECTION_CONSTRUCTORS,
        )
}

/// Returns true if `node` (unwrapping `type` and `parenthesized_expression`) is an `ellipsis` (`...`).
fn is_ellipsis_type_arg(node: &RawNode<'_>) -> bool {
    let mut current = node.clone();
    while matches!(current.kind().as_ref(), "type" | "parenthesized_expression") {
        let Some(inner) = current
            .children()
            .find(|child| child.is_named() && !child.is_extra())
        else {
            return false;
        };
        current = inner;
    }
    current.kind() == "ellipsis"
}

/// Unwraps outer return-annotation envelopes (`type`, `parenthesized_expression`,
/// `Annotated[T, ...]`, `Awaitable[T]`, and `Coroutine[YieldT, SendT, ReturnT]`).
fn unwrap_return_envelope<'a>(node: &RawNode<'a>) -> RawNode<'a> {
    let mut current = node.clone();
    loop {
        if matches!(current.kind().as_ref(), "type" | "parenthesized_expression") {
            let Some(inner) = current
                .children()
                .find(|child| child.is_named() && !child.is_extra())
            else {
                return current;
            };
            current = inner;
            continue;
        }
        if matches!(current.kind().as_ref(), "generic_type" | "subscript")
            && let Some((base_node, type_args)) = extract_generic_base_and_args(&current)
        {
            let (base_path, base_terminal) = resolve_path_and_terminal_raw(&base_node);
            if is_std_type_constructor(&base_path, &base_terminal, &["Annotated", "Awaitable"])
                && let Some(first_arg) = type_args.first().cloned()
            {
                current = first_arg;
                continue;
            }
            if is_std_type_constructor(&base_path, &base_terminal, &["Coroutine"])
                && let Some(return_arg) = type_args.get(2).cloned()
            {
                current = return_arg;
                continue;
            }
        }
        return current;
    }
}

/// Flattens top-level union constructs (`|`, `Optional[...]`, `Union[...]`, and `Annotated[T, ...]`)
/// into `branches` and sets `*has_none = true` if `None` (`none`) or `Optional[...]` is part of the union.
fn collect_union_branches<'a>(
    node: &RawNode<'a>,
    has_none: &mut bool,
    branches: &mut Vec<RawNode<'a>>,
) {
    match node.kind().as_ref() {
        "type" | "parenthesized_expression" | "union_type" => {
            for child in node.children() {
                if child.is_named() && !child.is_extra() {
                    collect_union_branches(&child, has_none, branches);
                }
            }
        }
        "binary_operator"
            if node
                .field("operator")
                .is_some_and(|operator| operator.text() == "|") =>
        {
            if let Some(left) = node.field("left") {
                collect_union_branches(&left, has_none, branches);
            }
            if let Some(right) = node.field("right") {
                collect_union_branches(&right, has_none, branches);
            }
        }
        "none" => {
            *has_none = true;
        }
        "generic_type" | "subscript" => {
            let Some((base_node, type_args)) = extract_generic_base_and_args(node) else {
                branches.push(node.clone());
                return;
            };
            let (base_path, base_terminal) = resolve_path_and_terminal_raw(&base_node);
            if is_std_type_constructor(&base_path, &base_terminal, &["Optional"]) {
                *has_none = true;
                for argument in &type_args {
                    collect_union_branches(argument, has_none, branches);
                }
                return;
            }
            if is_std_type_constructor(&base_path, &base_terminal, &["Union"]) {
                for argument in &type_args {
                    collect_union_branches(argument, has_none, branches);
                }
                return;
            }
            if is_std_type_constructor(&base_path, &base_terminal, &["Annotated"]) {
                if let Some(first_arg) = type_args.first() {
                    collect_union_branches(first_arg, has_none, branches);
                }
                return;
            }
            branches.push(node.clone());
        }
        _ => {
            branches.push(node.clone());
        }
    }
}

/// Returns the collection type constructor path (e.g. `"Sequence"`, `"list"`, `"tuple"`) if
/// `branch` is a collection type, or `None` otherwise.
///
/// Bare `tuple`/`Tuple` and variadic `tuple[T, ...]`/`Tuple[T, ...]` are treated as collections;
/// fixed-length record tuples (`tuple[int, str]`) return `None`.
fn collection_branch_type_path(branch: &RawNode<'_>) -> Option<String> {
    match branch.kind().as_ref() {
        "identifier" | "attribute" => {
            let (path, terminal) = resolve_path_and_terminal_raw(branch);
            if is_non_tuple_collection_constructor(&path, &terminal)
                || is_std_type_constructor(&path, &terminal, &["tuple", "Tuple"])
            {
                Some(path)
            } else {
                None
            }
        }
        "generic_type" | "subscript" => {
            let (base_node, type_args) = extract_generic_base_and_args(branch)?;
            let (base_path, base_terminal) = resolve_path_and_terminal_raw(&base_node);
            let is_variadic_tuple =
                is_std_type_constructor(&base_path, &base_terminal, &["tuple", "Tuple"])
                    && type_args.len() == 2
                    && is_ellipsis_type_arg(&type_args[1]);
            if is_non_tuple_collection_constructor(&base_path, &base_terminal) || is_variadic_tuple
            {
                Some(base_path)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Collects collection type constructors from a nullable collection return annotation.
///
/// Unwraps `Annotated`, `Awaitable`, and `Coroutine` return envelopes and returns the collection
/// constructors (in source order, deduplicated) when `type_node` is a union containing `None`
/// in which all non-`None` branches are collection types.
#[must_use]
pub fn collect_nullable_collection_return_types(type_node: &AstNode<'_>) -> Vec<String> {
    let root = unwrap_return_envelope(&type_node.raw);
    let mut has_none = false;
    let mut branches = Vec::new();
    collect_union_branches(&root, &mut has_none, &mut branches);

    if !has_none || branches.is_empty() {
        return Vec::new();
    }

    let mut matched = Vec::new();
    for branch in &branches {
        let Some(type_path) = collection_branch_type_path(branch) else {
            return Vec::new();
        };
        if !matched.contains(&type_path) {
            matched.push(type_path);
        }
    }
    matched
}

/// A Python logger call with an unmatched named PEP 3101 placeholder and positional arguments.
#[derive(Clone)]
pub struct UnmatchedLoggerPlaceholder<'a> {
    /// The full `call` AST node (`logger.info("Order {order_id}", order_id)`).
    pub call_node: AstNode<'a>,
    /// Source text of the invoked logger method (`logger.info`, `self.logger.error`).
    pub callee: String,
    /// The first unmatched named placeholder root identifier (`order_id`).
    pub placeholder: String,
}

/// Method names recognized on logger objects and the `logging` module.
const LOGGER_METHODS: &[&str] = &[
    "trace",
    "debug",
    "info",
    "success",
    "warning",
    "warn",
    "error",
    "critical",
    "fatal",
    "exception",
    "log",
];

/// Bare variable/module names recognized as logger receivers (`logger.info`, `logging.error`).
const LOGGER_RECEIVERS: &[&str] = &["logger", "log", "logging"];

/// Attribute names recognized on compound logger receivers (`self.logger.info`, `cls.log.warn`).
const LOGGER_ATTRIBUTES: &[&str] = &["logger", "log"];

/// If `function` is a logger method access (`logger.info`, `self.logger.error`, `logging.log`),
/// returns the method name.
fn extract_logger_method(function: &RawNode<'_>) -> Option<String> {
    if function.kind() != "attribute" {
        return None;
    }
    let method = function.field("attribute")?.text().into_owned();
    if !LOGGER_METHODS.contains(&method.as_str()) {
        return None;
    }
    let receiver = function.field("object")?;
    let is_logger_receiver = match receiver.kind().as_ref() {
        "identifier" => LOGGER_RECEIVERS.contains(&receiver.text().as_ref()),
        "attribute" => receiver
            .field("attribute")
            .is_some_and(|attribute| LOGGER_ATTRIBUTES.contains(&attribute.text().as_ref())),
        _ => false,
    };
    is_logger_receiver.then_some(method)
}

/// Returns the inner text of a single plain Python `string` node, excluding f-strings, byte
/// strings, and template strings.
fn extract_plain_string_node(node: &RawNode<'_>) -> Option<String> {
    if node.kind() != "string" || node.children().any(|child| child.kind() == "interpolation") {
        return None;
    }
    let (opening, content) = delimited_string_parts(node);
    if opening.contains(['f', 'F', 'b', 'B', 't', 'T']) {
        return None;
    }
    Some(content)
}

/// Extracts the static text of a plain string literal or an implicit `concatenated_string` of
/// plain string literals.
fn extract_logger_message_literal(node: &RawNode<'_>) -> Option<String> {
    match node.kind().as_ref() {
        "string" => extract_plain_string_node(node),
        "concatenated_string" => {
            let mut combined = String::new();
            for child in node
                .children()
                .filter(|child| child.is_named() && !child.is_extra())
            {
                combined.push_str(&extract_plain_string_node(&child)?);
            }
            (!combined.is_empty()).then_some(combined)
        }
        _ => None,
    }
}

/// Returns true if `name` is a valid Python identifier (`order_id`, `_item2`).
fn is_python_identifier(name: &str) -> bool {
    let mut characters = name.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    let valid_start = first == '_' || first.is_alphabetic();
    valid_start
        && characters.all(|character| character == '_' || character.is_alphanumeric())
        && !first.is_ascii_digit()
}

/// Validates a PEP 3101 `field_name` (`arg_name("." attribute | "[" index "]")*`) and returns
/// its root `arg_name` slice.
fn extract_valid_field_root(field_name: &str) -> Option<&str> {
    let split_at = field_name.find(['.', '[']).unwrap_or(field_name.len());
    let root = &field_name[..split_at];
    let valid_root = root.is_empty()
        || root.chars().all(|character| character.is_ascii_digit())
        || is_python_identifier(root);
    if !valid_root {
        return None;
    }

    let mut tail = &field_name[split_at..];
    while !tail.is_empty() {
        if let Some(after_dot) = tail.strip_prefix('.') {
            let end = after_dot.find(['.', '[']).unwrap_or(after_dot.len());
            if !is_python_identifier(&after_dot[..end]) {
                return None;
            }
            tail = &after_dot[end..];
        } else if let Some(after_bracket) = tail.strip_prefix('[') {
            let close = after_bracket.find(']')?;
            if close == 0 {
                return None;
            }
            tail = &after_bracket[close + 1..];
        } else {
            return None;
        }
    }
    Some(root)
}

/// Parses the `:format_spec` portion of a PEP 3101 replacement field starting at `start`,
/// appending any nested `{nested_field}` root names to `roots` and returning the byte index
/// immediately after the outer closing `}`.
fn parse_format_spec_section<'a>(
    message: &'a str,
    start: usize,
    roots: &mut Vec<&'a str>,
) -> Option<usize> {
    let mut cursor = start;
    while cursor < message.len() {
        let rest = &message[cursor..];
        if rest.starts_with('}') {
            return Some(cursor + 1);
        }
        if rest.starts_with('{') {
            let after_open = &message[cursor + 1..];
            let close_offset = after_open.find('}')?;
            let nested_body = &after_open[..close_offset];
            if nested_body.contains('{') {
                return None;
            }
            let nested_field = nested_body
                .split_once('!')
                .map_or(nested_body, |(before, _)| before);
            if let Some(nested_root) = extract_valid_field_root(nested_field) {
                roots.push(nested_root);
            }
            cursor += 1 + close_offset + 1;
        } else {
            cursor += rest.chars().next()?.len_utf8();
        }
    }
    None
}

/// Parses one `{...}` replacement field whose body starts at `start` (immediately after `{`),
/// returning the byte index after the matching `}` and the root `arg_name`s found inside it.
fn parse_replacement_field(message: &str, start: usize) -> Option<(usize, Vec<&str>)> {
    let mut cursor = start;
    let mut in_brackets = false;
    let mut delimiter = None;

    while cursor < message.len() {
        let character = message[cursor..].chars().next()?;
        match character {
            '[' if !in_brackets => in_brackets = true,
            ']' if in_brackets => in_brackets = false,
            '{' if !in_brackets => return None,
            ':' | '}' if !in_brackets => {
                delimiter = Some((cursor, character));
                break;
            }
            _ => {}
        }
        cursor += character.len_utf8();
    }

    let (delimiter_index, delimiter_char) = delimiter?;
    let header = &message[start..delimiter_index];
    let field_name = match header.split_once('!') {
        Some((before, "r" | "s" | "a")) => Some(before),
        Some(_) => None,
        None => Some(header),
    };

    let mut roots = Vec::new();
    if let Some(root) = field_name.and_then(extract_valid_field_root) {
        roots.push(root);
    }

    if delimiter_char == '}' {
        return Some((delimiter_index + 1, roots));
    }
    let next_cursor = parse_format_spec_section(message, delimiter_index + 1, &mut roots)?;
    Some((next_cursor, roots))
}

/// Returns the first named PEP 3101 placeholder root identifier in `message` that is not
/// present in `keyword_names`, or `None` if `message` has unbalanced braces or all named
/// placeholders are satisfied.
fn first_unmatched_named_placeholder(
    message: &str,
    keyword_names: &HashSet<String>,
) -> Option<String> {
    let mut cursor = 0;
    let mut first_unmatched: Option<String> = None;

    while cursor < message.len() {
        let rest = &message[cursor..];
        if rest.starts_with("{{") || rest.starts_with("}}") {
            cursor += 2;
        } else if rest.starts_with('}') {
            return None;
        } else if rest.starts_with('{') {
            let (next_cursor, roots) = parse_replacement_field(message, cursor + 1)?;
            for root in roots {
                if first_unmatched.is_none()
                    && is_python_identifier(root)
                    && !keyword_names.contains(root)
                {
                    first_unmatched = Some(root.to_owned());
                }
            }
            cursor = next_cursor;
        } else {
            cursor += rest.chars().next()?.len_utf8();
        }
    }
    first_unmatched
}

/// Inspects a single `call` node and returns an [`UnmatchedLoggerPlaceholder`] if it is a
/// logger call passing positional format arguments to a message with an unmatched named
/// placeholder.
fn check_logger_call_node<'a>(call_node: &RawNode<'a>) -> Option<UnmatchedLoggerPlaceholder<'a>> {
    let function = call_node.field("function")?;
    let method = extract_logger_method(&function)?;
    let arguments = call_node.field("arguments")?;

    let mut positional_args = Vec::new();
    let mut keyword_names = HashSet::new();

    for child in arguments
        .children()
        .filter(|child| child.is_named() && !child.is_extra())
    {
        match child.kind().as_ref() {
            "dictionary_splat" => return None,
            "keyword_argument" => {
                if let Some(name_node) = child.field("name") {
                    keyword_names.insert(name_node.text().into_owned());
                }
            }
            _ => positional_args.push(child),
        }
    }

    let message_index = usize::from(method == "log");
    if positional_args.len() <= message_index + 1 {
        return None;
    }

    let message = extract_logger_message_literal(&positional_args[message_index])?;
    let placeholder = first_unmatched_named_placeholder(&message, &keyword_names)?;

    Some(UnmatchedLoggerPlaceholder {
        call_node: AstNode::from_raw(call_node.clone()),
        callee: function.text().into_owned(),
        placeholder,
    })
}

/// Collects logger calls in `file` that pass positional format arguments to a message literal
/// containing an unmatched named PEP 3101 placeholder.
#[must_use]
pub fn collect_unmatched_logger_placeholders(
    file: &ParsedFile,
) -> Vec<UnmatchedLoggerPlaceholder<'_>> {
    file.grep
        .root()
        .dfs()
        .filter(|node| node.kind() == "call")
        .filter_map(|node| check_logger_call_node(&node))
        .collect()
}

/// A call inside a Python function or method to a sibling function or method defined later in
/// the same module or class scope.
#[derive(Clone)]
pub struct ForwardCall<'a> {
    /// The `call` expression AST node (`callee(...)` or `self.callee(...)`).
    pub node: AstNode<'a>,
    /// Name of the enclosing function or method making the call.
    pub caller_name: String,
    /// Name of the called sibling function or method defined later in the scope.
    pub callee_name: String,
}

/// One syntactic `def` statement belonging to a [`LogicalFunction`].
struct FunctionDefPart<'a> {
    function_node: RawNode<'a>,
    is_staticmethod: bool,
}

/// A logical function or method in a scope declaration epoch (grouping `@overload` stubs with
/// their implementation and `@property` getters with `@<name>.setter` / `@<name>.deleter` at
/// their first declaration position).
struct LogicalFunction<'a> {
    name: String,
    definition_order: usize,
    has_overload: bool,
    has_non_overload_definition: bool,
    parts: Vec<FunctionDefPart<'a>>,
}

/// Extracts bound variable identifiers from an assignment or loop target pattern, stopping at
/// `attribute`, `subscript`, and `type` nodes so `obj.helper = 1` or `arr[helper] = 1` does not
/// record `helper` as a local variable binding.
fn extract_local_target_names(node: &RawNode<'_>, out: &mut HashSet<String>) {
    match node.kind().as_ref() {
        "attribute" | "subscript" | "type" => {}
        "identifier" => {
            let text = node.text();
            if text != "_" {
                out.insert(text.into_owned());
            }
        }
        "dotted_name" => {
            if !node.text().contains('.') {
                for child in node.children() {
                    extract_local_target_names(&child, out);
                }
            }
        }
        "as_pattern" => {
            if let Some(alias) = node.field("alias") {
                extract_local_target_names(&alias, out);
            }
        }
        _ => {
            for child in node.children() {
                extract_local_target_names(&child, out);
            }
        }
    }
}

/// Collects local variable, parameter, nested definition, and import bindings directly owned by
/// `scope_node` (`function_definition`, `lambda`, or comprehension expression), without
/// descending into nested `function_definition`, `lambda`, or `class_definition` bodies.
fn collect_local_scope_bindings(
    scope_node: &RawNode<'_>,
    excluded_parameter: Option<&str>,
) -> HashSet<String> {
    let mut bindings = HashSet::new();
    let mut globals = HashSet::new();

    if let Some(parameters) = scope_node
        .field("parameters")
        .or_else(|| scope_node.field("lambda_parameters"))
    {
        let mut parameter_nodes = Vec::new();
        for child in parameters.children() {
            if !matches!(child.kind().as_ref(), "(" | ")" | ",") {
                extract_from_pattern(&child, &mut parameter_nodes);
            }
        }
        for node in parameter_nodes {
            let text = node.text();
            if excluded_parameter != Some(text.as_ref()) {
                bindings.insert(text.into_owned());
            }
        }
    }

    let start_node = scope_node
        .field("body")
        .unwrap_or_else(|| scope_node.clone());
    collect_bindings_in_subtree(&start_node, &mut bindings, &mut globals);

    for global_name in globals {
        bindings.remove(&global_name);
    }
    bindings
}

/// Walks `node` to collect local bindings and `global` declarations within a single function or
/// expression scope, stopping at nested scope boundaries.
fn collect_bindings_in_subtree(
    node: &RawNode<'_>,
    bindings: &mut HashSet<String>,
    globals: &mut HashSet<String>,
) {
    let kind = node.kind();
    match kind.as_ref() {
        "assignment" | "augmented_assignment" => {
            if let Some(left) = node.field("left") {
                extract_local_target_names(&left, bindings);
            }
            if let Some(right) = node.field("right") {
                collect_bindings_in_subtree(&right, bindings, globals);
            }
            return;
        }
        "for_statement" | "for_in_clause" => {
            if let Some(left) = node.field("left") {
                extract_local_target_names(&left, bindings);
            }
        }
        "as_pattern" => {
            if let Some(alias) = node.field("alias") {
                extract_local_target_names(&alias, bindings);
            }
        }
        "named_expression" => {
            if let Some(name_node) = node.field("name") {
                extract_local_target_names(&name_node, bindings);
            }
        }
        "case_clause" => {
            for child in node.children() {
                if child.kind() == "case_pattern" {
                    let mut pattern_nodes = Vec::new();
                    extract_from_pattern(&child, &mut pattern_nodes);
                    for bound in pattern_nodes {
                        bindings.insert(bound.text().into_owned());
                    }
                } else {
                    collect_bindings_in_subtree(&child, bindings, globals);
                }
            }
            return;
        }
        "function_definition" | "class_definition" => {
            if let Some(name_node) = node.field("name") {
                bindings.insert(name_node.text().into_owned());
            }
            return;
        }
        "lambda"
        | "list_comprehension"
        | "set_comprehension"
        | "dictionary_comprehension"
        | "generator_expression" => {
            return;
        }
        "import_statement" | "import_from_statement" => {
            let mut imported = Vec::new();
            extract_from_import(node, &mut imported);
            for item in imported {
                bindings.insert(item.text().into_owned());
            }
            return;
        }
        "global_statement" => {
            for child in node.children() {
                if child.kind() == "identifier" {
                    globals.insert(child.text().into_owned());
                }
            }
            return;
        }
        _ => {}
    }

    for child in node.children() {
        collect_bindings_in_subtree(&child, bindings, globals);
    }
}

/// Extracts comprehension loop variable bindings (`for_in_clause`) owned directly by a
/// comprehension or generator expression node.
fn collect_comprehension_bindings(comprehension_node: &RawNode<'_>) -> HashSet<String> {
    let mut bindings = HashSet::new();
    for child in comprehension_node.children() {
        if child.kind() == "for_in_clause"
            && let Some(left) = child.field("left")
        {
            extract_local_target_names(&left, &mut bindings);
        }
    }
    bindings
}

/// Partitions direct function definitions in `scope_body` (`module` or class `block`) into
/// declaration epochs, grouping `@overload` stubs with their implementation and `@property`
/// getters with `@<name>.setter` / `@<name>.deleter` at their first declaration position.
fn partition_scope_epochs<'a>(scope_body: &RawNode<'a>) -> Vec<Vec<LogicalFunction<'a>>> {
    let mut epochs: Vec<Vec<LogicalFunction<'a>>> = Vec::new();
    let mut current_epoch: Vec<LogicalFunction<'a>> = Vec::new();
    let mut index_by_name: HashMap<String, usize> = HashMap::new();
    let mut next_order = 0usize;

    for statement in scope_body.children() {
        let function_node = if statement.kind() == "function_definition" {
            Some(statement.clone())
        } else {
            decorated_definition(&statement).filter(|inner| inner.kind() == "function_definition")
        };
        let Some(function_node) = function_node else {
            continue;
        };
        let Some(name_node) = function_node.field("name") else {
            continue;
        };
        let name = name_node.text().into_owned();
        let decorators = extract_decorators_raw(&statement);
        let is_overload = decorators
            .iter()
            .any(|decorator| decorator.terminal_name == "overload");
        let is_property_accessor = decorators
            .iter()
            .any(|decorator| matches!(decorator.terminal_name.as_str(), "setter" | "deleter"));
        let is_staticmethod = decorators
            .iter()
            .any(|decorator| decorator.terminal_name == "staticmethod");
        let part = FunctionDefPart {
            function_node,
            is_staticmethod,
        };

        if let Some(&existing_index) = index_by_name.get(&name) {
            let existing = &mut current_epoch[existing_index];
            let continues_overload =
                existing.has_overload && (!existing.has_non_overload_definition || is_overload);
            if is_property_accessor || continues_overload {
                existing.has_non_overload_definition |= !is_overload;
                existing.parts.push(part);
                continue;
            }
            epochs.push(std::mem::take(&mut current_epoch));
            index_by_name.clear();
        }

        let index = current_epoch.len();
        index_by_name.insert(name.clone(), index);
        current_epoch.push(LogicalFunction {
            name,
            definition_order: next_order,
            has_overload: is_overload,
            has_non_overload_definition: !is_overload,
            parts: vec![part],
        });
        next_order += 1;
    }

    if !current_epoch.is_empty() {
        epochs.push(current_epoch);
    }
    epochs
}

/// Returns the receiver parameter name (`"self"` or `"cls"`) for a class method, or `None` if
/// the method is a `@staticmethod` or has no receiver parameter.
fn method_receiver_name(part: &FunctionDefPart<'_>) -> Option<String> {
    if part.is_staticmethod {
        return None;
    }
    let parameters = extract_parameters_raw(&part.function_node);
    parameters
        .first()
        .filter(|first| first.kind == PythonParameterKind::Receiver)
        .map(|first| first.name.clone())
}

/// Context for walking a function or method body to collect sibling calls.
struct CallWalkContext<'a> {
    sibling_names: &'a HashSet<&'a str>,
    receiver_name: Option<&'a str>,
    is_class: bool,
}

/// Recursively walks `node` inside a function/method body, tracking scoped local bindings and
/// recording calls to sibling functions/methods in `out`.
fn collect_calls_in_body<'a>(
    node: &RawNode<'a>,
    context: &CallWalkContext<'_>,
    active_bindings: &HashSet<String>,
    out: &mut Vec<(String, AstNode<'a>)>,
) {
    let kind = node.kind();
    match kind.as_ref() {
        "class_definition" => return,
        "function_definition" | "lambda" => {
            let mut inner_bindings = active_bindings.clone();
            inner_bindings.extend(collect_local_scope_bindings(node, None));
            if let Some(body) = node.field("body") {
                collect_calls_in_body(&body, context, &inner_bindings, out);
            }
            return;
        }
        "list_comprehension"
        | "set_comprehension"
        | "dictionary_comprehension"
        | "generator_expression" => {
            let mut comprehension_bindings = active_bindings.clone();
            comprehension_bindings.extend(collect_comprehension_bindings(node));
            for child in node.children() {
                collect_calls_in_body(&child, context, &comprehension_bindings, out);
            }
            return;
        }
        "call" => {
            if let Some(callee_name) = match_sibling_call(node, context, active_bindings) {
                out.push((callee_name, AstNode::from_raw(node.clone())));
            }
        }
        _ => {}
    }

    for child in node.children() {
        collect_calls_in_body(&child, context, active_bindings, out);
    }
}

/// Checks whether `call_node` invokes a sibling function or method in `context.sibling_names`.
fn match_sibling_call(
    call_node: &RawNode<'_>,
    context: &CallWalkContext<'_>,
    active_bindings: &HashSet<String>,
) -> Option<String> {
    let function = call_node.field("function")?;
    if context.is_class {
        let receiver = context.receiver_name?;
        if active_bindings.contains(receiver) || function.kind() != "attribute" {
            return None;
        }
        let object = function.field("object")?;
        let attribute = function.field("attribute")?;
        let attribute_text = attribute.text();
        if object.kind() == "identifier"
            && object.text() == receiver
            && context.sibling_names.contains(attribute_text.as_ref())
        {
            return Some(attribute_text.into_owned());
        }
        None
    } else {
        if function.kind() != "identifier" {
            return None;
        }
        let name = function.text();
        if context.sibling_names.contains(name.as_ref()) && !active_bindings.contains(name.as_ref())
        {
            Some(name.into_owned())
        } else {
            None
        }
    }
}

/// Returns true if `start` can reach `target` in the directed `call_graph`.
fn can_reach<'a>(
    start: &'a str,
    target: &'a str,
    call_graph: &HashMap<&'a str, HashSet<&'a str>>,
) -> bool {
    let mut visited = HashSet::new();
    let mut stack = vec![start];
    while let Some(current) = stack.pop() {
        if current == target {
            return true;
        }
        if visited.insert(current)
            && let Some(neighbors) = call_graph.get(current)
        {
            stack.extend(neighbors.iter().copied());
        }
    }
    false
}

/// Evaluates a single declaration epoch of sibling functions or methods and appends any forward
/// calls to `out`.
fn evaluate_epoch<'a>(
    epoch: &[LogicalFunction<'a>],
    is_class: bool,
    out: &mut Vec<ForwardCall<'a>>,
) {
    let sibling_names: HashSet<&str> = epoch
        .iter()
        .map(|function| function.name.as_str())
        .collect();
    let order_by_name: HashMap<&str, usize> = epoch
        .iter()
        .map(|function| (function.name.as_str(), function.definition_order))
        .collect();

    let mut calls_by_function: Vec<Vec<(String, AstNode<'a>)>> = Vec::with_capacity(epoch.len());
    let mut call_graph: HashMap<&str, HashSet<&str>> = HashMap::new();

    for function in epoch {
        let mut function_calls = Vec::new();
        for part in &function.parts {
            let Some(body) = part.function_node.field("body") else {
                continue;
            };
            let receiver = if is_class {
                method_receiver_name(part)
            } else {
                None
            };
            let initial_bindings =
                collect_local_scope_bindings(&part.function_node, receiver.as_deref());
            let context = CallWalkContext {
                sibling_names: &sibling_names,
                receiver_name: receiver.as_deref(),
                is_class,
            };
            collect_calls_in_body(&body, &context, &initial_bindings, &mut function_calls);
        }
        let edges = call_graph.entry(function.name.as_str()).or_default();
        for (callee_name, _) in &function_calls {
            if let Some(&callee_ref) = sibling_names.get(callee_name.as_str()) {
                edges.insert(callee_ref);
            }
        }
        calls_by_function.push(function_calls);
    }

    for (function, function_calls) in epoch.iter().zip(calls_by_function) {
        if is_class
            && matches!(
                function.name.as_str(),
                "__init__" | "__new__" | "__post_init__"
            )
        {
            continue;
        }
        let mut reported_callees: HashSet<String> = HashSet::new();
        for (callee_name, call_node) in function_calls {
            let Some(&callee_order) = order_by_name.get(callee_name.as_str()) else {
                continue;
            };
            if callee_order <= function.definition_order {
                continue;
            }
            if can_reach(callee_name.as_str(), function.name.as_str(), &call_graph) {
                continue;
            }
            if reported_callees.insert(callee_name.clone()) {
                out.push(ForwardCall {
                    node: call_node,
                    caller_name: function.name.clone(),
                    callee_name,
                });
            }
        }
    }
}

/// Collects forward sibling calls in module and class scopes across `file`, in source order.
///
/// Exempts direct and mutual recursion (strongly connected components in the intra-scope call
/// graph), class constructor callers (`__init__`, `__new__`, `__post_init__`), locally shadowed
/// names, `@overload` / `@property` accessor groups, and non-call references. Reports at most
/// one [`ForwardCall`] per `(caller, callee)` pair per declaration epoch, anchored at the first
/// offending `call` node.
#[must_use]
pub fn collect_forward_calls(file: &ParsedFile) -> Vec<ForwardCall<'_>> {
    let mut results = Vec::new();
    let root = file.grep.root();

    for epoch in partition_scope_epochs(&root) {
        evaluate_epoch(&epoch, false, &mut results);
    }

    for node in root.dfs() {
        if node.kind() == "class_definition"
            && let Some(body) = node.field("body")
        {
            for epoch in partition_scope_epochs(&body) {
                evaluate_epoch(&epoch, true, &mut results);
            }
        }
    }

    results.sort_unstable_by_key(|call| call.node.span().start);
    results
}

/// A quote-wrapped format placeholder found in a Python f-string, `.format()` call,
/// `%`-formatted string, or multi-argument `logging` call.
#[derive(Clone)]
pub struct QuoteWrappedPlaceholder<'a> {
    /// The `string` AST node containing the quote-wrapped placeholder.
    pub node: AstNode<'a>,
    /// The quote-wrapped placeholder as written in source (such as `'{x}'`, `\"{}\"` , `'%s'`).
    pub expression: String,
    /// The canonical representation-formatted replacement (such as `{x!r}`, `{!r}`, `%r`).
    pub replacement: String,
}

/// Active Python string-formatting mechanism for a `string` literal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PythonFormatContext {
    /// An f-string literal (`f"..."` or `F"..."`).
    FString,
    /// Receiver of `.format(...)` / `.format_map(...)` or first argument of `str.format(...)`.
    StrFormat,
    /// Left operand of `%` or message argument of a multi-argument `logging` call.
    Printf,
}

/// Standard logger method names whose first positional argument (`index 0`) is the format string.
const LOGGER_MESSAGE_FIRST_METHODS: &[&str] = &[
    "debug",
    "info",
    "warning",
    "warn",
    "error",
    "exception",
    "critical",
    "fatal",
];

/// Common logger receiver paths recognized for printf-style logging calls.
const PRINTF_LOGGER_RECEIVERS: &[&str] = &[
    "logging",
    "logger",
    "log",
    "_logger",
    "_log",
    "self.logger",
    "self.log",
    "cls.logger",
    "cls.log",
];

/// SQL statement prefixes that mark a string as a SQL query rather than prose.
const SQL_STATEMENT_KEYWORDS: &[&str] = &[
    "SELECT", "INSERT", "UPDATE", "DELETE", "CREATE", "ALTER", "DROP", "WITH", "REPLACE", "MERGE",
    "PRAGMA", "EXPLAIN", "TRUNCATE", "GRANT", "REVOKE",
];

/// SQL clause or operator tokens that immediately precede a quoted SQL literal.
const SQL_OPERATOR_KEYWORDS: &[&str] = &[
    "VALUES", "LIKE", "ILIKE", "RLIKE", "REGEXP", "GLOB", "IN", "WHERE", "SET", "TABLE", "INTO",
    "FROM", "JOIN",
];

/// Splits the prefix flags (`f`, `r`, `b`, `u`) from a `string_start` delimiter token.
fn string_prefix_flags(opening_delimiter: &str) -> &str {
    opening_delimiter
        .find(['\'', '"'])
        .map_or("", |quote_index| &opening_delimiter[..quote_index])
}

/// Walks upward from a `string` node through enclosing `concatenated_string` and
/// `parenthesized_expression` nodes to find the expression node bound to its surrounding context.
fn outermost_string_expression<'a>(string_node: &RawNode<'a>) -> RawNode<'a> {
    let mut current = string_node.clone();
    while let Some(parent) = current.parent() {
        let is_wrapper = matches!(
            parent.kind().as_ref(),
            "concatenated_string" | "parenthesized_expression"
        );
        if !is_wrapper {
            break;
        }
        current = parent;
    }
    current
}

/// Returns the positional (non-keyword, non-splat) argument nodes of an `argument_list`.
fn positional_call_arguments<'a>(arguments: &RawNode<'a>) -> Vec<RawNode<'a>> {
    arguments
        .children()
        .filter(|child| {
            child.is_named()
                && !child.is_extra()
                && !matches!(
                    child.kind().as_ref(),
                    "keyword_argument" | "list_splat" | "dictionary_splat"
                )
        })
        .collect()
}

/// Returns true if `context_root` is the receiver of `.format(...)` / `.format_map(...)` or the
/// first positional argument of `str.format(...)`.
fn is_str_format_target(context_root: &RawNode<'_>) -> bool {
    let Some(parent) = context_root.parent() else {
        return false;
    };
    if parent.kind() == "attribute"
        && parent
            .field("object")
            .is_some_and(|object| object.range() == context_root.range())
        && parent
            .field("attribute")
            .is_some_and(|attribute| matches!(attribute.text().as_ref(), "format" | "format_map"))
        && parent.parent().is_some_and(|grandparent| {
            grandparent.kind() == "call"
                && grandparent
                    .field("function")
                    .is_some_and(|function| function.range() == parent.range())
        })
    {
        return true;
    }

    if parent.kind() == "argument_list"
        && let Some(call_node) = parent.parent()
        && call_node.kind() == "call"
        && call_node
            .field("function")
            .is_some_and(|function| function.text() == "str.format")
    {
        return positional_call_arguments(&parent)
            .first()
            .is_some_and(|first| first.range() == context_root.range());
    }

    false
}

/// Returns true if `context_root` is the left operand of `%` or the message argument of a
/// `logging` call that passes at least one trailing format argument.
fn is_printf_format_target(context_root: &RawNode<'_>) -> bool {
    let Some(parent) = context_root.parent() else {
        return false;
    };
    if parent.kind() == "binary_operator"
        && parent
            .field("operator")
            .is_some_and(|operator| operator.text() == "%")
        && parent
            .field("left")
            .is_some_and(|left| left.range() == context_root.range())
    {
        return true;
    }

    if parent.kind() != "argument_list" {
        return false;
    }
    let Some(call_node) = parent.parent().filter(|node| node.kind() == "call") else {
        return false;
    };
    let Some(function) = call_node
        .field("function")
        .filter(|node| node.kind() == "attribute")
    else {
        return false;
    };
    let Some(receiver) = function.field("object") else {
        return false;
    };
    let Some(method) = function.field("attribute") else {
        return false;
    };
    if !PRINTF_LOGGER_RECEIVERS.contains(&receiver.text().trim()) {
        return false;
    }

    let positional_arguments = positional_call_arguments(&parent);
    let method_name = method.text();
    if LOGGER_MESSAGE_FIRST_METHODS.contains(&method_name.as_ref()) {
        positional_arguments.len() >= 2 && positional_arguments[0].range() == context_root.range()
    } else if method_name == "log" {
        positional_arguments.len() >= 3 && positional_arguments[1].range() == context_root.range()
    } else {
        false
    }
}

/// Determines the active formatting context of a Python `string` node, returning `None` for
/// raw strings, byte strings, or unformatted string literals.
fn classify_string_format_context(string_node: &RawNode<'_>) -> Option<PythonFormatContext> {
    let opening = string_node.child(0)?;
    let opening_text = opening.text();
    let prefix = string_prefix_flags(&opening_text);
    if prefix.contains(['r', 'R', 'b', 'B']) {
        return None;
    }
    if prefix.contains(['f', 'F']) {
        return Some(PythonFormatContext::FString);
    }
    let context_root = outermost_string_expression(string_node);
    if is_str_format_target(&context_root) {
        Some(PythonFormatContext::StrFormat)
    } else if is_printf_format_target(&context_root) {
        Some(PythonFormatContext::Printf)
    } else {
        None
    }
}

/// Appends the literal text segments (excluding `{...}` interpolations) of a single `string` node
/// to `buffer`.
fn append_string_literal_segments(string_node: &RawNode<'_>, source: &str, buffer: &mut String) {
    let Some(opening) = string_node.child(0) else {
        return;
    };
    let Some(closing) = string_node.children().last() else {
        return;
    };
    let content_start = opening.range().end;
    let content_end = closing.range().start;
    if content_start >= content_end {
        return;
    }
    let mut cursor = content_start;
    for child in string_node
        .children()
        .filter(|child| child.kind() == "interpolation")
    {
        let range = child.range();
        buffer.push_str(source.get(cursor..range.start).unwrap_or_default());
        cursor = range.end;
    }
    buffer.push_str(source.get(cursor..content_end).unwrap_or_default());
}

/// Strips `.format()` (`{field}`) or `printf` (`%(key)s`) placeholder bodies from `text` so
/// placeholder identifiers are not mistaken for prose words.
fn strip_non_fstring_placeholders(text: &str, format_context: PythonFormatContext) -> String {
    match format_context {
        PythonFormatContext::FString => text.to_owned(),
        PythonFormatContext::StrFormat => {
            let bytes = text.as_bytes();
            let mut stripped = String::with_capacity(text.len());
            let mut cursor = 0;
            let mut index = 0;
            while index < bytes.len() {
                let current = bytes[index];
                let is_escaped = (current == b'{' && bytes.get(index + 1) == Some(&b'{'))
                    || (current == b'}' && bytes.get(index + 1) == Some(&b'}'));
                if is_escaped {
                    index += 2;
                    continue;
                }
                if current == b'{'
                    && let Some(close_index) = find_str_format_closing_brace(bytes, index)
                {
                    stripped.push_str(&text[cursor..index]);
                    index = close_index + 1;
                    cursor = index;
                    continue;
                }
                index += 1;
            }
            stripped.push_str(&text[cursor..]);
            stripped
        }
        PythonFormatContext::Printf => {
            let bytes = text.as_bytes();
            let mut stripped = String::with_capacity(text.len());
            let mut cursor = 0;
            let mut index = 0;
            while index < bytes.len() {
                let current = bytes[index];
                if current == b'%' && bytes.get(index + 1) == Some(&b'%') {
                    index += 2;
                    continue;
                }
                if current == b'%'
                    && bytes.get(index + 1) == Some(&b'(')
                    && let Some(close_offset) = text[index + 2..].find(')')
                {
                    stripped.push_str(&text[cursor..index]);
                    let after_paren = index + 2 + close_offset + 1;
                    let next_index = text[after_paren..]
                        .chars()
                        .next()
                        .map_or(after_paren, |conv| after_paren + conv.len_utf8());
                    index = next_index;
                    cursor = index;
                    continue;
                }
                index += 1;
            }
            stripped.push_str(&text[cursor..]);
            stripped
        }
    }
}

/// Returns the combined literal text of all sibling `string` nodes preceding `string_node` in an
/// enclosing `concatenated_string`.
fn preceding_concatenated_literal_text(string_node: &RawNode<'_>, source: &str) -> String {
    let mut prefix = String::new();
    if let Some(parent) = string_node
        .parent()
        .filter(|node| node.kind() == "concatenated_string")
    {
        for child in parent.children().filter(|child| child.kind() == "string") {
            if child.range().start >= string_node.range().start {
                break;
            }
            append_string_literal_segments(&child, source, &mut prefix);
        }
    }
    prefix
}

/// Returns the combined literal text of `string_node` (or all sibling `string` parts when
/// enclosed in a `concatenated_string`), with format placeholders excluded.
fn combined_message_literal_text(
    string_node: &RawNode<'_>,
    source: &str,
    format_context: PythonFormatContext,
) -> String {
    let mut combined = String::new();
    if let Some(parent) = string_node
        .parent()
        .filter(|node| node.kind() == "concatenated_string")
    {
        for child in parent.children().filter(|child| child.kind() == "string") {
            append_string_literal_segments(&child, source, &mut combined);
        }
    } else {
        append_string_literal_segments(string_node, source, &mut combined);
    }
    strip_non_fstring_placeholders(&combined, format_context)
}

/// Replaces escape sequences (`\n`, `\t`, `\"`, etc.) with spaces so escape letters are not
/// mistaken for prose words.
fn strip_escape_sequences(text: &str) -> String {
    let mut cleaned = String::with_capacity(text.len());
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character == '\\' {
            characters.next();
            cleaned.push(' ');
        } else {
            cleaned.push(character);
        }
    }
    cleaned
}

/// Returns true if `literal_text` contains at least one prose word ($\ge 2$ consecutive ASCII
/// letters) and does not start with a SQL statement keyword.
fn is_prose_message_text(literal_text: &str) -> bool {
    let cleaned = strip_escape_sequences(literal_text);
    let has_word = cleaned
        .split(|character: char| !character.is_ascii_alphabetic())
        .any(|word| word.len() >= 2);
    if !has_word {
        return false;
    }
    let first_word = cleaned
        .trim_start_matches(|character: char| character.is_ascii_whitespace() || character == '(')
        .split(|character: char| !character.is_ascii_alphabetic())
        .next()
        .unwrap_or_default();
    !SQL_STATEMENT_KEYWORDS
        .iter()
        .any(|keyword| first_word.eq_ignore_ascii_case(keyword))
}

/// Surrounding literal text slices for a single placeholder candidate.
struct PlaceholderNeighbors<'a> {
    /// Immediate literal text slice preceding the placeholder.
    before: &'a str,
    /// Cumulative literal text from the start of the string up to the placeholder.
    full_before: &'a str,
    /// Immediate literal text slice following the placeholder.
    after: &'a str,
    /// True when `after` is immediately followed by another f-string interpolation.
    followed_by_interpolation: bool,
}

/// Extracted quote pair surrounding a placeholder, together with the text before the opening
/// quote and after the closing quote.
struct MatchedQuotePair<'a> {
    opening: &'a str,
    closing: &'a str,
    prefix_before: &'a str,
    suffix_after: &'a str,
}

/// Checks whether `before` ends with a single or double quote (unescaped or single-backslash
/// escaped) and `after` starts with the matching quote character.
fn extract_matching_quote_pair<'a>(
    before: &'a str,
    after: &'a str,
) -> Option<MatchedQuotePair<'a>> {
    let quote = *before.as_bytes().last()?;
    if quote != b'\'' && quote != b'"' {
        return None;
    }
    let quote_char = char::from(quote);
    let without_quote = &before[..before.len() - 1];
    let trailing_backslashes = without_quote
        .bytes()
        .rev()
        .take_while(|&byte| byte == b'\\')
        .count();
    let (opening, prefix_before) = match trailing_backslashes {
        0 => (&before[before.len() - 1..], without_quote),
        1 => (
            &before[before.len() - 2..],
            &without_quote[..without_quote.len() - 1],
        ),
        _ => return None,
    };
    if prefix_before.ends_with(quote_char) {
        return None;
    }

    let (closing, suffix_after) = if after.starts_with(quote_char) {
        (&after[..1], &after[1..])
    } else if after.as_bytes().first() == Some(&b'\\') && after.as_bytes().get(1) == Some(&quote) {
        (&after[..2], &after[2..])
    } else {
        return None;
    };
    if suffix_after.starts_with(quote_char) {
        return None;
    }

    Some(MatchedQuotePair {
        opening,
        closing,
        prefix_before,
        suffix_after,
    })
}

/// Returns true if `text` has any unclosed single `{` or `[` delimiter (ignoring `{{` and `}}`).
fn has_unclosed_structured_delimiter(text: &str) -> bool {
    let mut brace_depth = 0_i32;
    let mut bracket_depth = 0_i32;
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let current = bytes[index];
        let is_escaped_brace = (current == b'{' && bytes.get(index + 1) == Some(&b'{'))
            || (current == b'}' && bytes.get(index + 1) == Some(&b'}'));
        if is_escaped_brace {
            index += 2;
            continue;
        }
        match current {
            b'{' => brace_depth += 1,
            b'}' => brace_depth = (brace_depth - 1).max(0),
            b'[' => bracket_depth += 1,
            b']' => bracket_depth = (bracket_depth - 1).max(0),
            _ => {}
        }
        index += 1;
    }
    brace_depth > 0 || bracket_depth > 0
}

/// Returns true if `prefix_before_quote` and `full_prefix_before_quote` satisfy the left prose
/// boundary rules (`C4.2`).
fn has_valid_left_prose_boundary(
    prefix_before_quote: &str,
    full_prefix_before_quote: &str,
) -> bool {
    let backtick_count = full_prefix_before_quote
        .bytes()
        .filter(|&byte| byte == b'`')
        .count();
    if backtick_count % 2 != 0 || has_unclosed_structured_delimiter(full_prefix_before_quote) {
        return false;
    }

    let starts_at_beginning = prefix_before_quote.is_empty() && full_prefix_before_quote.is_empty();
    let preceded_by_space_or_paren = prefix_before_quote.ends_with([' ', '\t', '\n', '\r', '('])
        || prefix_before_quote.ends_with("\\n")
        || prefix_before_quote.ends_with("\\t")
        || prefix_before_quote.ends_with("\\r");
    if !starts_at_beginning && !preceded_by_space_or_paren {
        return false;
    }

    let cleaned_prefix = strip_escape_sequences(full_prefix_before_quote);
    let trimmed = cleaned_prefix.trim_end();
    let Some(last_char) = trimmed.chars().next_back() else {
        return true;
    };
    if !last_char.is_ascii_alphanumeric() && !matches!(last_char, ':' | ',' | '(' | '.' | '!' | '?')
    {
        return false;
    }

    let before_paren = trimmed.trim_end_matches('(').trim_end();
    let last_word = before_paren
        .rsplit(|character: char| !character.is_ascii_alphabetic())
        .next()
        .unwrap_or_default();
    !SQL_OPERATOR_KEYWORDS
        .iter()
        .any(|keyword| last_word.eq_ignore_ascii_case(keyword))
}

/// Returns true if `character` is a sentence punctuation mark allowed immediately after a closing
/// prose quote.
const fn is_prose_punctuation(character: char) -> bool {
    matches!(character, '.' | ',' | ';' | ':' | '!' | '?' | ')')
}

/// Returns true if `suffix_after_quote` satisfies the right prose boundary rules (`C4.3`).
fn has_valid_right_prose_boundary(
    suffix_after_quote: &str,
    followed_by_interpolation: bool,
) -> bool {
    if suffix_after_quote.is_empty() {
        return !followed_by_interpolation;
    }
    if suffix_after_quote.starts_with("\\n")
        || suffix_after_quote.starts_with("\\t")
        || suffix_after_quote.starts_with("\\r")
    {
        return true;
    }
    let mut characters = suffix_after_quote.chars();
    let Some(first_char) = characters.next() else {
        return !followed_by_interpolation;
    };
    if first_char.is_ascii_whitespace() {
        return true;
    }
    if !is_prose_punctuation(first_char) {
        return false;
    }
    characters
        .next()
        .map_or(!followed_by_interpolation, |second_char| {
            second_char.is_ascii_whitespace()
                || is_prose_punctuation(second_char)
                || matches!(second_char, '\'' | '"')
        })
}

/// Checks whether a candidate placeholder surrounded by `neighbors` is quote-wrapped in a prose
/// context (`C3` and `C4.2`–`C4.3`), returning the quote-wrapped expression if so.
fn match_prose_quoted_placeholder(
    neighbors: &PlaceholderNeighbors<'_>,
    placeholder_body: &str,
) -> Option<String> {
    let matched = extract_matching_quote_pair(neighbors.before, neighbors.after)?;
    let full_prefix_before_quote =
        &neighbors.full_before[..neighbors.full_before.len() - matched.opening.len()];
    if !has_valid_left_prose_boundary(matched.prefix_before, full_prefix_before_quote) {
        return None;
    }
    if !has_valid_right_prose_boundary(matched.suffix_after, neighbors.followed_by_interpolation) {
        return None;
    }
    Some(format!(
        "{open}{placeholder_body}{close}",
        open = matched.opening,
        close = matched.closing,
    ))
}

/// Returns the bare expression text inside an f-string `interpolation` node if it has no
/// `type_conversion` (`!r`, `!s`, `!a`), no `format_specifier` (`:...`), no debug `=`, and is not
/// a nested string literal.
fn bare_fstring_interpolation_expression(interpolation: &RawNode<'_>) -> Option<String> {
    if interpolation.field("type_conversion").is_some()
        || interpolation.field("format_specifier").is_some()
        || interpolation.children().any(|child| child.kind() == "=")
    {
        return None;
    }
    let expression = interpolation.field("expression")?;
    if expression.kind() == "string" {
        return None;
    }
    let text = expression.text().trim().to_owned();
    (!text.is_empty()).then_some(text)
}

/// Collects quote-wrapped placeholders inside an f-string `string` node.
fn collect_fstring_quote_wrapped<'a>(
    string_node: &RawNode<'a>,
    source: &str,
    out: &mut Vec<QuoteWrappedPlaceholder<'a>>,
) {
    let Some(opening) = string_node.child(0) else {
        return;
    };
    let Some(closing) = string_node.children().last() else {
        return;
    };
    let interpolations: Vec<RawNode<'a>> = string_node
        .children()
        .filter(|child| child.kind() == "interpolation")
        .collect();
    if interpolations.is_empty() {
        return;
    }

    let content_start = opening.range().end;
    let content_end = closing.range().start;
    let mut segments: Vec<&str> = Vec::with_capacity(interpolations.len() + 1);
    let mut cursor = content_start;
    for interpolation in &interpolations {
        let range = interpolation.range();
        segments.push(source.get(cursor..range.start).unwrap_or_default());
        cursor = range.end;
    }
    segments.push(source.get(cursor..content_end).unwrap_or_default());

    let mut cumulative_before = preceding_concatenated_literal_text(string_node, source);
    for (index, interpolation) in interpolations.iter().enumerate() {
        let segment_before = segments[index];
        let after = segments[index + 1];
        cumulative_before.push_str(segment_before);
        let before = if index == 0 {
            cumulative_before.as_str()
        } else {
            segment_before
        };
        let Some(inner_expression) = bare_fstring_interpolation_expression(interpolation) else {
            continue;
        };
        let neighbors = PlaceholderNeighbors {
            before,
            full_before: &cumulative_before,
            after,
            followed_by_interpolation: index + 1 < interpolations.len(),
        };
        let placeholder_body = format!("{{{inner_expression}}}");
        if let Some(expression) = match_prose_quoted_placeholder(&neighbors, &placeholder_body) {
            out.push(QuoteWrappedPlaceholder {
                node: AstNode::from_raw(string_node.clone()),
                expression,
                replacement: format!("{{{inner_expression}!r}}"),
            });
        }
    }
}

/// Returns true if `field` is a valid `.format()` field name (empty `""`, positional digits, or
/// an identifier/attribute/index path without operators, whitespace, or conversion specifiers).
fn is_valid_str_format_field(field: &str) -> bool {
    if field.is_empty() {
        return true;
    }
    let Some(first_char) = field.chars().next() else {
        return false;
    };
    if !first_char.is_ascii_alphanumeric() && first_char != '_' {
        return false;
    }
    field.chars().all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '[' | ']')
    })
}

/// Finds the closing `}` byte offset of an unescaped `.format()` placeholder starting at
/// `open_index`, returning `None` if an inner `{` or `}}` is encountered first.
fn find_str_format_closing_brace(bytes: &[u8], open_index: usize) -> Option<usize> {
    let mut scan = open_index + 1;
    while scan < bytes.len() {
        let current = bytes[scan];
        if current == b'}' && bytes.get(scan + 1) != Some(&b'}') {
            return Some(scan);
        }
        if current == b'{' {
            return None;
        }
        scan += 1;
    }
    None
}

/// Evaluates a single `{...}` span inside a `.format()` string and returns a
/// [`QuoteWrappedPlaceholder`] when it is a bare field wrapped in prose quotes.
fn evaluate_str_format_brace_span<'a>(
    string_node: &RawNode<'a>,
    content: &str,
    open_index: usize,
    close_index: usize,
) -> Option<QuoteWrappedPlaceholder<'a>> {
    let raw_field = &content[open_index + 1..close_index];
    if raw_field.contains(['!', ':', '{', '}']) {
        return None;
    }
    let field = raw_field.trim();
    if !is_valid_str_format_field(field) {
        return None;
    }
    let before = &content[..open_index];
    let after = &content[close_index + 1..];
    let neighbors = PlaceholderNeighbors {
        before,
        full_before: before,
        after,
        followed_by_interpolation: false,
    };
    let placeholder_body = format!("{{{field}}}");
    let expression = match_prose_quoted_placeholder(&neighbors, &placeholder_body)?;
    Some(QuoteWrappedPlaceholder {
        node: AstNode::from_raw(string_node.clone()),
        expression,
        replacement: format!("{{{field}!r}}"),
    })
}

/// Collects quote-wrapped placeholders inside a `.format()` or `.format_map()` string literal.
fn collect_str_format_quote_wrapped<'a>(
    string_node: &RawNode<'a>,
    content: &str,
    out: &mut Vec<QuoteWrappedPlaceholder<'a>>,
) {
    let bytes = content.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let current = bytes[index];
        let is_escaped = (current == b'{' && bytes.get(index + 1) == Some(&b'{'))
            || (current == b'}' && bytes.get(index + 1) == Some(&b'}'));
        if is_escaped {
            index += 2;
            continue;
        }
        if current == b'{'
            && let Some(close_index) = find_str_format_closing_brace(bytes, index)
        {
            if let Some(finding) =
                evaluate_str_format_brace_span(string_node, content, index, close_index)
            {
                out.push(finding);
            }
            index = close_index + 1;
            continue;
        }
        index += 1;
    }
}

/// Parses a bare `%s` or `%(name)s` printf placeholder starting at `&content[percent_index..]`,
/// returning `(end_index, raw_specifier, replacement)`.
fn parse_bare_printf_s_placeholder(
    content: &str,
    percent_index: usize,
) -> Option<(usize, &str, String)> {
    let rest = &content[percent_index + 1..];
    if rest.starts_with('s') {
        let end_index = percent_index + 2;
        return Some((
            end_index,
            &content[percent_index..end_index],
            "%r".to_owned(),
        ));
    }
    let after_open_paren = rest.strip_prefix('(')?;
    let close_offset = after_open_paren.find(")s")?;
    let key = &after_open_paren[..close_offset];
    let is_valid_key = !key.is_empty()
        && key
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_');
    if !is_valid_key {
        return None;
    }
    let end_index = percent_index + 2 + close_offset + 2;
    let raw_specifier = &content[percent_index..end_index];
    Some((end_index, raw_specifier, format!("%({key})r")))
}

/// Collects quote-wrapped `%s` and `%(name)s` placeholders inside a printf-formatted string.
fn collect_printf_quote_wrapped<'a>(
    string_node: &RawNode<'a>,
    content: &str,
    out: &mut Vec<QuoteWrappedPlaceholder<'a>>,
) {
    let bytes = content.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let current = bytes[index];
        if current == b'%' && bytes.get(index + 1) == Some(&b'%') {
            index += 2;
            continue;
        }
        if current == b'%'
            && let Some((end_index, raw_specifier, replacement)) =
                parse_bare_printf_s_placeholder(content, index)
        {
            let before = &content[..index];
            let after = &content[end_index..];
            let neighbors = PlaceholderNeighbors {
                before,
                full_before: before,
                after,
                followed_by_interpolation: false,
            };
            if let Some(expression) = match_prose_quoted_placeholder(&neighbors, raw_specifier) {
                out.push(QuoteWrappedPlaceholder {
                    node: AstNode::from_raw(string_node.clone()),
                    expression,
                    replacement,
                });
            }
            index = end_index;
            continue;
        }
        index += 1;
    }
}

/// Collects quote-wrapped placeholders in Python f-strings, `.format()` / `.format_map()`
/// calls, `%`-formatted strings, and multi-argument `logging` calls in `file`, in source order.
#[must_use]
pub fn collect_quote_wrapped_placeholders(file: &ParsedFile) -> Vec<QuoteWrappedPlaceholder<'_>> {
    let source = file.source_text();
    let mut out = Vec::new();
    for node in file.grep.root().dfs() {
        if node.kind() != "string" {
            continue;
        }
        let Some(format_context) = classify_string_format_context(&node) else {
            continue;
        };
        let combined_literal = combined_message_literal_text(&node, &source, format_context);
        if !is_prose_message_text(&combined_literal) {
            continue;
        }
        match format_context {
            PythonFormatContext::FString => {
                collect_fstring_quote_wrapped(&node, &source, &mut out);
            }
            PythonFormatContext::StrFormat => {
                let (_, content) = delimited_string_parts(&node);
                collect_str_format_quote_wrapped(&node, &content, &mut out);
            }
            PythonFormatContext::Printf => {
                let (_, content) = delimited_string_parts(&node);
                collect_printf_quote_wrapped(&node, &content, &mut out);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use LiteralRole::{ConstantDefinition, Inline};
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
    #[case::a19_invariant_concrete_outer_not_recursed("def f(x: list[set[str]], y: dict[str, list[int]]) -> None: pass", &[&["list"][..], &["dict"][..]])]
    #[case::a20_typed_dict_qualifiers("def f(x: NotRequired[list[int]], y: ReadOnly[dict[str, int]]) -> None: pass", &[&["list"][..], &["dict"][..]])]
    #[case::a21_generator_yield_and_return_positions("def f(a: Generator[list[int], set[str], dict[str, int]], b: Coroutine[None, set[str], list[int]], c: AsyncGenerator[list[int], set[str]]) -> None: pass", &[&["list", "dict"][..], &["list"][..], &["list"][..]])]
    #[case::a22_iterator_and_abstract_set_covariant("def f(a: Iterable[list[int]], b: Iterator[dict[str, int]], c: AbstractSet[frozenset[int]]) -> None: pass", &[&["list"][..], &["dict"][..], &[][..]])]
    #[case::a23_qualified_covariant_outer("def f(x: collections.abc.Sequence[list[int]], y: typing.Mapping[str, set[str]]) -> None: pass", &[&["list"][..], &["set"][..]])]
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
    #[case::raise_from_not_implemented_cause(
        "def f(x: list[int]) -> None:\n    raise ValueError() from NotImplementedError",
        false
    )]
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

    /// Parses `source` and applies `collect` to the annotation of its first function's first parameter.
    fn collect_from_first_annotation(
        source: &str,
        collect: fn(&AstNode<'_>) -> Vec<String>,
    ) -> Vec<String> {
        let file = ParsedFile::new(source, SupportLang::Python);
        let sigs = extract_function_signatures(&file);
        let type_node = sigs[0].parameters[0]
            .type_node
            .as_ref()
            .expect("param should be typed");
        collect(type_node)
    }

    #[rstest::rstest]
    #[case::unqualified("def f(x: MutableSequence[int]): pass", &["MutableSequence"])]
    #[case::qualified("def f(x: collections.abc.MutableMapping[str, int]): pass", &["collections.abc.MutableMapping"])]
    #[case::optional_unwrapped("def f(x: typing.MutableSet[int] | None): pass", &["typing.MutableSet"])]
    #[case::nested_not_collected("def f(x: Sequence[MutableSequence[int]]): pass", &[])]
    #[case::unknown_module_ignored("def f(x: mylib.MutableSequence[int]): pass", &[])]
    fn test_collect_mutable_collection_types(#[case] source: &str, #[case] expected: &[&str]) {
        assert_eq!(
            collect_from_first_annotation(source, collect_mutable_collection_types),
            expected
        );
    }

    #[rstest::rstest]
    #[case::sequence("def f(x: Sequence[int]): pass", &["Sequence"])]
    #[case::qualified_collection("def f(x: typing.Collection[int]): pass", &["typing.Collection"])]
    #[case::optional_unwrapped("def f(x: Optional[collections.abc.Sequence[int]]): pass", &["collections.abc.Sequence"])]
    #[case::nested_not_collected("def f(x: Mapping[str, Sequence[int]]): pass", &[])]
    #[case::iterable_not_collected("def f(x: Iterable[int]): pass", &[])]
    fn test_collect_specific_collection_types(#[case] source: &str, #[case] expected: &[&str]) {
        assert_eq!(
            collect_from_first_annotation(source, collect_specific_collection_types),
            expected
        );
    }

    #[rstest::rstest]
    #[case::sequence_kinds(&["list", "typing.List", "MutableSequence"], "collections.abc.Sequence")]
    #[case::mapping_kinds(&["dict", "Dict", "collections.abc.MutableMapping"], "collections.abc.Mapping")]
    #[case::set_kinds(&["set", "typing.Set", "MutableSet"], "collections.abc.Set")]
    #[case::collections_mappings(&["defaultdict", "typing.DefaultDict", "collections.Counter", "OrderedDict"], "collections.abc.Mapping")]
    #[case::collections_deque(&["collections.deque", "Deque"], "collections.abc.Sequence")]
    #[case::mixed_in_order(&["dict", "list", "dict"], "collections.abc.Mapping, collections.abc.Sequence")]
    fn test_read_only_collection_replacements(#[case] type_paths: &[&str], #[case] expected: &str) {
        let type_paths: Vec<String> = type_paths.iter().map(|path| (*path).to_string()).collect();
        assert_eq!(read_only_collection_replacements(&type_paths), expected);
    }

    #[rstest::rstest]
    #[case::sequence_kinds(&["list", "typing.List", "collections.deque", "Deque", "MutableSequence"], "tuple")]
    #[case::mapping_kinds(
        &["dict", "Dict", "defaultdict", "typing.DefaultDict", "collections.Counter", "OrderedDict", "MutableMapping"],
        "frozendict"
    )]
    #[case::set_kinds(&["set", "typing.Set", "MutableSet"], "frozenset")]
    #[case::mixed_in_order(&["set", "list", "set"], "frozenset, tuple")]
    fn test_immutable_constant_collection_replacements(
        #[case] type_paths: &[&str],
        #[case] expected: &str,
    ) {
        let type_paths: Vec<String> = type_paths.iter().map(|path| (*path).to_string()).collect();
        assert_eq!(
            immutable_constant_collection_replacements(&type_paths),
            expected
        );
    }

    #[rstest::rstest]
    #[case::annotated_list(
        "ALLOWED: list[str] = (\"a\",)",
        &[("ALLOWED", &["list"][..], "list[str]")]
    )]
    #[case::bare_final_dict(
        "from typing import Final\nPORTS: Final = {\"http\": 80}",
        &[("PORTS", &["dict"][..], "{\"http\": 80}")]
    )]
    #[case::mapping_exempts_dict_literal(
        "from collections.abc import Mapping\nPORTS: Mapping[str, int] = {\"http\": 80}",
        &[]
    )]
    #[case::mapping_does_not_exempt_defaultdict(
        "from collections import defaultdict\nfrom collections.abc import Mapping\nCOUNTS: Mapping[str, int] = defaultdict(int)",
        &[("COUNTS", &["defaultdict"][..], "defaultdict(int)")]
    )]
    #[case::multiline_parenthesized_with_comment(
        "ALLOWED = (\n    # Default roles\n    [\"admin\"]\n)",
        &[("ALLOWED", &["list"][..], "[\"admin\"]")]
    )]
    #[case::annotated_wrapped_final_on_lowercase(
        "from typing import Annotated, Final\nallowed: Annotated[Final[list[str]], \"doc\"] = [\"a\"]",
        &[("allowed", &["list"][..], "Annotated[Final[list[str]], \"doc\"]")]
    )]
    fn test_collect_mutable_module_constants(
        #[case] source: &str,
        #[case] expected: &[(&str, &[&str], &str)],
    ) {
        let file = ParsedFile::new(source, SupportLang::Python);
        let collected: Vec<_> = collect_mutable_module_constants(&file)
            .into_iter()
            .map(|constant| {
                (
                    constant.name,
                    constant.matched_types,
                    constant.target_node.text().into_owned(),
                )
            })
            .collect();
        let expected: Vec<(String, Vec<String>, String)> = expected
            .iter()
            .map(|(name, types, span)| {
                (
                    (*name).to_string(),
                    types.iter().map(|item| (*item).to_string()).collect(),
                    (*span).to_string(),
                )
            })
            .collect();
        assert_eq!(collected, expected);
    }

    #[rstest::rstest]
    #[case::unused("def f(x):\n    return 1", ParameterCollectionCapability::Unused)]
    #[case::single_for(
        "def f(x):\n    for i in x:\n        print(i)",
        ParameterCollectionCapability::Iterable
    )]
    #[case::single_comprehension(
        "def f(x):\n    return [i for i in x]",
        ParameterCollectionCapability::Iterable
    )]
    #[case::single_consuming_builtin(
        "def f(x):\n    return sum(x)",
        ParameterCollectionCapability::Iterable
    )]
    #[case::len(
        "def f(x):\n    return len(x)",
        ParameterCollectionCapability::Collection
    )]
    #[case::membership(
        "def f(x):\n    return 1 in x",
        ParameterCollectionCapability::Collection
    )]
    #[case::truthiness(
        "def f(x):\n    if x:\n        return 1",
        ParameterCollectionCapability::Collection
    )]
    #[case::negation(
        "def f(x):\n    return not x",
        ParameterCollectionCapability::Collection
    )]
    #[case::two_passes(
        "def f(x):\n    return sum(x) + max(x)",
        ParameterCollectionCapability::Collection
    )]
    #[case::iteration_inside_loop(
        "def f(x):\n    for _ in range(3):\n        for i in x:\n            print(i)",
        ParameterCollectionCapability::Collection
    )]
    #[case::index("def f(x):\n    return x[0]", ParameterCollectionCapability::Sequence)]
    #[case::reversed(
        "def f(x):\n    return list(reversed(x))",
        ParameterCollectionCapability::Sequence
    )]
    #[case::index_method(
        "def f(x):\n    return x.index(1)",
        ParameterCollectionCapability::Sequence
    )]
    #[case::pattern_matching(
        "def f(x):\n    match x:\n        case [a]:\n            return a",
        ParameterCollectionCapability::Sequence
    )]
    #[case::passed_to_unknown_function(
        "def f(x):\n    return helper(x)",
        ParameterCollectionCapability::Sequence
    )]
    #[case::shadowing_nested_def_default(
        "def f(x):\n    def g(x=x[0]):\n        return x\n    return g",
        ParameterCollectionCapability::Sequence
    )]
    fn test_analyze_parameter_collection_capability(
        #[case] source: &str,
        #[case] expected: ParameterCollectionCapability,
    ) {
        let file = ParsedFile::new(source, SupportLang::Python);
        let sigs = extract_function_signatures(&file);
        assert_eq!(
            analyze_parameter_collection_capability(&sigs[0].node, "x"),
            expected
        );
    }

    #[rstest::rstest]
    #[case::method_mutation("def f(x):\n    x.append(1)", true)]
    #[case::subscript_write("def f(x):\n    x[0] = 1", true)]
    #[case::subscript_delete("def f(x):\n    del x[0]", true)]
    #[case::augmented_assignment("def f(x):\n    x += [1]", true)]
    #[case::alias("def f(x):\n    y = x\n    return len(y)", true)]
    #[case::stored_in_container("def f(x):\n    y = [x]\n    return len(y)", true)]
    #[case::returned("def f(x):\n    return x", true)]
    #[case::yielded("def f(x):\n    yield x", true)]
    #[case::passed_to_unknown_function("def f(x):\n    helper(x)", true)]
    #[case::passed_to_method("def f(x):\n    registry.register(x)", true)]
    #[case::mutated_in_closure("def f(x):\n    def g():\n        x.append(1)\n    g()", true)]
    #[case::shadowing_nested_def_default(
        "def f(x):\n    def g(x=x):\n        x.append(1)\n    g()",
        true
    )]
    #[case::read_only_iteration("def f(x):\n    for i in x:\n        print(i)", false)]
    #[case::read_only_len("def f(x):\n    return len(x)", false)]
    #[case::shadowed_by_nested_def(
        "def f(x):\n    def g(x):\n        x.append(1)\n    g([])",
        false
    )]
    #[case::shadowed_by_lambda(
        "def f(x):\n    g = lambda x: x.append(1)\n    return len(x)",
        false
    )]
    fn test_is_parameter_mutated_or_escaping(#[case] source: &str, #[case] expected: bool) {
        let file = ParsedFile::new(source, SupportLang::Python);
        let sigs = extract_function_signatures(&file);
        assert_eq!(
            is_parameter_mutated_or_escaping(&sigs[0].node, "x"),
            expected
        );
    }

    #[rstest::rstest]
    #[case::direct_call_mutation("make().append(1)", &["make"])]
    #[case::assigned_then_mutated("def use():\n    items = make()\n    items.append(1)", &["make"])]
    #[case::walrus_then_mutated("def use():\n    if (items := make()):\n        items.append(1)", &["make"])]
    #[case::subscript_write("def use():\n    items = make()\n    items[0] = 1", &["make"])]
    #[case::read_only_use("def use():\n    items = make()\n    return len(items)", &[])]
    #[case::binding_in_other_function("def a():\n    items = make()\ndef b():\n    items.append(1)", &[])]
    fn test_collect_locally_mutated_return_functions(
        #[case] usage: &str,
        #[case] expected: &[&str],
    ) {
        let source = format!("def make():\n    return []\n{usage}");
        let file = ParsedFile::new(&source, SupportLang::Python);
        let expected: HashSet<String> = expected.iter().map(|name| (*name).to_string()).collect();
        assert_eq!(collect_locally_mutated_return_functions(&file), expected);
    }

    #[rstest::rstest]
    #[case::class_level("class C:\n    items: list[int]", &[("items", false)])]
    #[case::private_skipped("class C:\n    _items: list[int]", &[])]
    #[case::init_attribute("class C:\n    def __init__(self):\n        self.items: list[int] = []", &[("items", false)])]
    #[case::self_mutation("class C:\n    items: list[int]\n    def add(self):\n        self.items.append(1)", &[("items", true)])]
    #[case::cls_mutation("class C:\n    items: list[int]\n    @classmethod\n    def add(cls):\n        cls.items.append(1)", &[("items", true)])]
    #[case::class_name_mutation("class C:\n    items: list[int]\n    def add(self):\n        C.items.append(1)", &[("items", true)])]
    #[case::subscript_delete("class C:\n    items: list[int]\n    def pop(self):\n        del self.items[0]", &[("items", true)])]
    #[case::protocol_skipped("class P(Protocol):\n    items: list[int]", &[])]
    fn test_collect_public_class_attributes(
        #[case] source: &str,
        #[case] expected: &[(&str, bool)],
    ) {
        let file = ParsedFile::new(source, SupportLang::Python);
        let actual: Vec<(String, bool)> = collect_public_class_attributes(&file)
            .into_iter()
            .map(|attribute| (attribute.name, attribute.is_mutated_in_class))
            .collect();
        let expected: Vec<(String, bool)> = expected
            .iter()
            .map(|(name, mutated)| ((*name).to_string(), *mutated))
            .collect();
        assert_eq!(actual, expected);
    }

    #[rstest::rstest]
    #[case::module_upper_name_defines("MAX = 30", &[("30", ConstantDefinition)])]
    #[case::class_final_lowercase_defines("class C:\n    limit: Final[int] = -5", &[("-5", ConstantDefinition)])]
    #[case::aliased_final_defines("timeout: t.Final = 30", &[("30", ConstantDefinition)])]
    #[case::class_lowercase_is_inline("class C:\n    name = 'cc'", &[("'cc'", Inline)])]
    #[case::function_level_upper_name_is_inline("def f():\n    MAX = 30", &[("30", Inline)])]
    #[case::module_if_body_defines("if WIN:\n    RETRIES = 3\nelse:\n    RETRIES = 5", &[("3", ConstantDefinition), ("5", ConstantDefinition)])]
    #[case::module_except_body_defines("try:\n    import x\nexcept ImportError:\n    LIMIT = 9", &[("9", ConstantDefinition)])]
    #[case::function_if_body_is_inline("def f():\n    if a:\n        MAX = 30", &[("30", Inline)])]
    #[case::parenthesized_constant_defines("MSG = (\n    'refused'\n)", &[("'refused'", ConstantDefinition)])]
    #[case::composite_constant_not_collected("URLS = ['u1', 'u2']\nPAIR = 'aa' 'bb'", &[])]
    #[case::negation_anchored_on_operator("f(-42, 7 - 42)", &[("-42", Inline), ("7", Inline), ("42", Inline)])]
    #[case::negative_case_pattern("match m:\n    case [-42, 'xx']:\n        pass", &[("-42", Inline), ("'xx'", Inline)])]
    #[case::signed_numbers_in_patterns_skipped("match m:\n    case {-404: _} | Resp(code=-404) | -7:\n        pass", &[])]
    #[case::docstring_skipped("def f():\n    '''Doc.'''\n    return 'rv'", &[("'rv'", Inline)])]
    #[case::annotations_skipped("def f(a: 'T' = 'dv') -> 'R':\n    v: 'V' = 'vv'", &[("'dv'", Inline), ("'vv'", Inline)])]
    #[case::literal_type_skipped("v = Literal['y']", &[])]
    #[case::interpolated_fstring_walked("print(f\"{row['st']} and\", f'plain')", &[("'st'", Inline), ("f'plain'", Inline)])]
    #[case::imaginary_skipped("z = 2j", &[])]
    fn test_collect_literal_occurrences_python(
        #[case] source: &str,
        #[case] expected: &[(&str, LiteralRole)],
    ) {
        let file = ParsedFile::new(source, SupportLang::Python);
        let actual: Vec<(String, LiteralRole)> = collect_literal_occurrences(&file)
            .into_iter()
            .map(|occurrence| (occurrence.node.text().into_owned(), occurrence.role))
            .collect();
        let expected: Vec<(String, LiteralRole)> = expected
            .iter()
            .map(|(text, role)| ((*text).to_string(), *role))
            .collect();
        assert_eq!(actual, expected);
    }

    #[rstest::rstest]
    fn test_collect_literal_occurrences_python_skips_type_name_argument(
        #[values(
            "TypeVar",
            "NewType",
            "ParamSpec",
            "TypeVarTuple",
            "NamedTuple",
            "TypedDict",
            "typing.cast"
        )]
        callee: &str,
    ) {
        let file = ParsedFile::new(&format!("t = {callee}('Nm', 'vv')"), SupportLang::Python);
        let texts: Vec<String> = collect_literal_occurrences(&file)
            .into_iter()
            .map(|occurrence| occurrence.node.text().into_owned())
            .collect();
        assert_eq!(texts, vec!["'vv'"]);
    }

    #[rstest::rstest]
    #[case::quote_style_ignored("'ab'", LiteralValue::Str("ab".to_string()))]
    #[case::triple_quoted("'''ab'''", LiteralValue::Str("ab".to_string()))]
    #[case::unicode_prefix("u'ab'", LiteralValue::Str("ab".to_string()))]
    #[case::bytes("b\"ab\"", LiteralValue::Bytes("ab".to_string()))]
    #[case::raw_bytes("Rb'ab'", LiteralValue::Bytes("ab".to_string()))]
    #[case::escapes_kept("'a\\nb'", LiteralValue::Str("a\\nb".to_string()))]
    #[case::raw_backslash_spelled_plain("r'a\\nb'", LiteralValue::Str("a\\\\nb".to_string()))]
    #[case::hex("0x1F", LiteralValue::Int(31))]
    #[case::separators("1_000", LiteralValue::Int(1000))]
    #[case::exponent("1e3", LiteralValue::Float(1000.0_f64.to_bits()))]
    #[case::leading_dot(".5", LiteralValue::Float(0.5_f64.to_bits()))]
    #[case::trailing_dot("3.", LiteralValue::Float(3.0_f64.to_bits()))]
    #[case::negative_float("-2.5", LiteralValue::Float((-2.5_f64).to_bits()))]
    #[case::negative_zero("-0.0", LiteralValue::Float(0.0_f64.to_bits()))]
    fn test_collect_literal_occurrences_python_values(
        #[case] literal: &str,
        #[case] expected: LiteralValue,
    ) {
        let file = ParsedFile::new(&format!("value = {literal}"), SupportLang::Python);
        let values: Vec<LiteralValue> = collect_literal_occurrences(&file)
            .into_iter()
            .map(|occurrence| occurrence.value)
            .collect();
        assert_eq!(values, vec![expected]);
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
        let file = ParsedFile::new(source, SupportLang::Python);
        let classes = extract_classes(&file);
        assert_eq!(classes.len(), 1);
        assert!(classes[0].bases.is_empty());
        assert!(!classes[0].has_contract_base());
    }

    #[test]
    fn test_python_class_info_fake_name_and_contract_base() {
        let source = indoc::indoc! {r"
            class FakeClient(HttpClient):
                pass

            class _FakeGateway(object):
                pass

            class Fake_Repo(Generic[T]):
                pass

            class Fake2FA(Protocol):
                pass

            class FakeExtGeneric(typing_extensions.Generic[T]):
                pass

            class FakeExtProtocol(typing_extensions.Protocol, **kwargs):
                pass

            class Fake(ABC):
                pass

            class Faker:
                pass

            class Fakeable:
                pass
        "};
        let file = ParsedFile::new(source, SupportLang::Python);
        let classes = extract_classes(&file);
        let summary: Vec<(&str, bool, bool)> = classes
            .iter()
            .map(|class_info| {
                (
                    class_info.name.as_str(),
                    class_info.is_fake_class_name(),
                    class_info.has_contract_base(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                ("FakeClient", true, true),
                ("_FakeGateway", true, false),
                ("Fake_Repo", true, false),
                ("Fake2FA", true, false),
                ("FakeExtGeneric", true, false),
                ("FakeExtProtocol", true, false),
                ("Fake", true, true),
                ("Faker", false, false),
                ("Fakeable", false, false),
            ]
        );
    }

    #[rstest::rstest]
    #[case::init_and_method(
        "class C:\n    def __init__(self):\n        self.x: int = 1\n    async def reset(self):\n        if True:\n            self.y: str",
        &[("C", "__init__", "x", "int"), ("C", "reset", "y", "str")]
    )]
    #[case::parameterized_final_collected_bare_final_skipped(
        "class C:\n    def __init__(self):\n        self.a: Final = 1\n        self.b: typing.Final = 2\n        self.c: Annotated[Final, 'm'] = 3\n        self.d: Final[int] = 4",
        &[("C", "__init__", "d", "Final[int]")]
    )]
    #[case::private_and_unannotated_skipped(
        "class C:\n    def __init__(self):\n        self._p: int = 1\n        self.pub = 2",
        &[]
    )]
    #[case::staticmethod_classmethod_and_nested_func_skipped(
        "class C:\n    @staticmethod\n    def sm(self):\n        self.a: int = 1\n    @classmethod\n    def cm(cls):\n        cls.b: int = 2\n    def run(self):\n        def inner(self):\n            self.c: int = 3",
        &[]
    )]
    fn test_collect_inline_public_attribute_annotations(
        #[case] source: &str,
        #[case] expected: &[(&str, &str, &str, &str)],
    ) {
        let file = ParsedFile::new(source, SupportLang::Python);
        let actual: Vec<(String, String, String, String)> =
            collect_inline_public_attribute_annotations(&file)
                .into_iter()
                .map(|attribute| {
                    (
                        attribute.class_name,
                        attribute.method_name,
                        attribute.name,
                        attribute.annotation_text,
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
    #[case::pep604_sequence("def f() -> Sequence[int] | None: pass", &["Sequence"])]
    #[case::optional_list("def f() -> Optional[list[int]]: pass", &["list"])]
    #[case::union_mapping("def f() -> typing.Union[typing.Mapping[str, int], None]: pass", &["typing.Mapping"])]
    #[case::deduplicated_branches("def f() -> list[int] | list[str] | None: pass", &["list"])]
    #[case::multiple_collection_branches("def f() -> list[int] | set[str] | None: pass", &["list", "set"])]
    #[case::variadic_tuple("def f() -> tuple[int, ...] | None: pass", &["tuple"])]
    #[case::bare_tuple("def f() -> tuple | None: pass", &["tuple"])]
    #[case::fixed_pair_tuple_ignored("def f() -> tuple[int, str] | None: pass", &[])]
    #[case::single_element_tuple_ignored("def f() -> tuple[int] | None: pass", &[])]
    #[case::mixed_scalar_and_collection_ignored("def f() -> str | Sequence[str] | None: pass", &[])]
    #[case::inner_nullable_element_ignored("def f() -> Sequence[int | None]: pass", &[])]
    #[case::annotated_branch_unwrapped("def f() -> Annotated[Sequence[int], 'meta'] | None: pass", &["Sequence"])]
    #[case::awaitable_coroutine_envelopes_unwrapped(
        "def f() -> Awaitable[Coroutine[Any, Any, MutableMapping[str, int] | None]]: pass",
        &["MutableMapping"]
    )]
    fn test_collect_nullable_collection_return_types(
        #[case] source: &str,
        #[case] expected: &[&str],
    ) {
        let file = ParsedFile::new(source, SupportLang::Python);
        let signatures = extract_function_signatures(&file);
        let return_type_node = signatures[0]
            .return_type_node
            .as_ref()
            .expect("function should have return annotation");
        assert_eq!(
            collect_nullable_collection_return_types(return_type_node),
            expected
        );
    }

    #[rstest::rstest]
    #[case::simple_unmatched("Order {order_id} filled", &[], Some("order_id"))]
    #[case::simple_matched("Order {order_id} filled", &["order_id"], None)]
    #[case::compound_attribute("Order {order.id} filled", &[], Some("order"))]
    #[case::compound_subscript("Order {order[id]} filled", &[], Some("order"))]
    #[case::conversion_and_spec("Order {order_id!r} {amount:.2f}", &["order_id"], Some("amount"))]
    #[case::positional_empty_and_numbered("Order {} and {0} and {0.id} and {1[key]}", &[], None)]
    #[case::escaped_double_braces("Literal {{order_id}} value {}", &[], None)]
    #[case::triple_braces_captures_inner("Literal {{{order_id}}}", &[], Some("order_id"))]
    #[case::nested_format_spec("Value {:>{width}}", &[], Some("width"))]
    #[case::non_identifier_braces("Payload {\"order_id\": 1} and {a, b}", &[], None)]
    #[case::unclosed_opening_brace("Malformed {order_id in input", &[], None)]
    #[case::unmatched_closing_brace("Malformed {order_id} stray }", &[], None)]
    fn test_first_unmatched_named_placeholder_parsing(
        #[case] message: &str,
        #[case] provided_kwargs: &[&str],
        #[case] expected: Option<&str>,
    ) {
        let keyword_names: HashSet<String> = provided_kwargs
            .iter()
            .map(|name| (*name).to_owned())
            .collect();
        let actual = first_unmatched_named_placeholder(message, &keyword_names);
        assert_eq!(actual.as_deref(), expected);
    }

    #[test]
    fn test_collect_unmatched_logger_placeholders_extracts_callee_and_placeholder() {
        let source = indoc::indoc! {r#"
            logger.info("Order {order_id} filled", order_id)
            self.log.error("Peer {peer.id} failed", peer)
            logging.log(20, "User {user_id} action {action}", action, user_id=1)
            logger.info("Order {order_id} filled", order_id=1)
        "#};
        let file = ParsedFile::new(source, SupportLang::Python);
        let findings: Vec<(String, String)> = collect_unmatched_logger_placeholders(&file)
            .into_iter()
            .map(|item| (item.callee, item.placeholder))
            .collect();
        assert_eq!(
            findings,
            vec![
                ("logger.info".to_owned(), "order_id".to_owned()),
                ("self.log.error".to_owned(), "peer".to_owned()),
                ("logging.log".to_owned(), "action".to_owned()),
            ]
        );
    }

    #[test]
    fn test_collect_forward_calls_multiple_callees_and_cycles() {
        let source = indoc::indoc! {r"
            def orchestrate(x: int) -> int:
                first = step_one(x)
                second = step_two(first)
                return second + is_even(second)

            def step_one(x: int) -> int:
                return x + 1

            def step_two(x: int) -> int:
                return x * 2

            def is_even(n: int) -> bool:
                return True if n == 0 else is_odd(n - 1)

            def is_odd(n: int) -> bool:
                return False if n == 0 else is_even(n - 1)
        "};
        let file = ParsedFile::new(source, SupportLang::Python);
        let forward = collect_forward_calls(&file);
        let summary: Vec<(&str, &str, String)> = forward
            .iter()
            .map(|call| {
                (
                    call.caller_name.as_str(),
                    call.callee_name.as_str(),
                    call.node.text().into_owned(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                ("orchestrate", "step_one", "step_one(x)".to_string()),
                ("orchestrate", "step_two", "step_two(first)".to_string()),
                ("orchestrate", "is_even", "is_even(second)".to_string()),
            ]
        );
    }

    #[rstest::rstest]
    #[case::fstring_multiple_placeholders(
        "msg = f\"Copied '{source}' to '{target}'\"",
        &[("'{source}'", "{source!r}"), ("'{target}'", "{target!r}")]
    )]
    #[case::str_format_empty_and_named(
        "msg = \"Copied '{}' to '{target}'\".format(source, target=dest)",
        &[("'{}'", "{!r}"), ("'{target}'", "{target!r}")]
    )]
    #[case::printf_positional_and_named(
        "msg = \"Invalid '%s' and '%(key)s'\" % (a, b)",
        &[("'%s'", "%r"), ("'%(key)s'", "%(key)r")]
    )]
    #[case::concatenated_fstring_shares_prose_context(
        "msg = (\n    \"Failed to load configuration for \"\n    f\"'{service_name}'\"\n)",
        &[("'{service_name}'", "{service_name!r}")]
    )]
    #[case::isolated_str_format_and_printf_ignored(
        "a = \"'{value}'\".format(value=x)\nb = \"'%(name)s'\" % {\"name\": x}",
        &[]
    )]
    #[case::concatenated_flag_and_backtick_ignored(
        "a = \"Pass --output=\" f\"'{output_path}'\"\nb = \"Run `mode = \" f\"'{mode}'` in config\"",
        &[]
    )]
    fn test_collect_quote_wrapped_placeholders_python(
        #[case] python_code: &str,
        #[case] expected: &[(&str, &str)],
    ) {
        let file = ParsedFile::new(python_code, SupportLang::Python);
        let actual: Vec<(String, String)> = collect_quote_wrapped_placeholders(&file)
            .into_iter()
            .map(|item| (item.expression, item.replacement))
            .collect();
        let expected: Vec<(String, String)> = expected
            .iter()
            .map(|(expression, replacement)| {
                ((*expression).to_string(), (*replacement).to_string())
            })
            .collect();
        assert_eq!(actual, expected);
    }
}
