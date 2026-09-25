//! Concrete syntax tree (CST) and grammar encapsulation.
//!
//! Encapsulates `ast_grep_core` (`AstGrep`, `StrDoc`, `Node`) behind [`ParsedFile`] and an opaque
//! [`AstNode`], and provides language-dispatched and language-specific CST extractors.
//! No module outside `crate::code_lint::ast` imports `ast_grep_core` or accesses raw Tree-sitter
//! node kinds, field names, or traversal iterators.

architecture_component!(CodeSyntaxAdapters);

/// Dispatches `$func(args...)` to `ast::python` or `ast::rust` by `$lang`, evaluating
/// `$fallback` for any other language.
macro_rules! dispatch_lang {
    ($lang:expr, $func:ident ( $($arg:expr),* $(,)? ), $fallback:expr) => {
        match $lang {
            ast_grep_language::SupportLang::Python => {
                $crate::code_lint::ast::python::$func($($arg),*)
            }
            ast_grep_language::SupportLang::Rust => {
                $crate::code_lint::ast::rust::$func($($arg),*)
            }
            _ => $fallback,
        }
    };
}
pub(crate) use dispatch_lang;

pub mod python;
pub mod rust;
pub mod statements;

use crate::diagnostic::{LineColumn, SourceLocation, SourceSpan};
use ast_grep_core::AstGrep;
use ast_grep_core::tree_sitter::StrDoc;
use ast_grep_language::SupportLang;
use std::path::PathBuf;

/// Type alias for an in-memory source document parsed by `ast-grep`.
pub(in crate::code_lint::ast) type SourceDoc = StrDoc<SupportLang>;

/// Internal type alias for a raw `ast-grep` syntax tree node.
pub(in crate::code_lint::ast) type RawNode<'a> = ast_grep_core::Node<'a, SourceDoc>;

/// A parsed source file encapsulating the language and syntax tree.
///
/// The inner `AstGrep` tree is restricted to `crate::code_lint::ast` so that higher layers
/// (semantic engines, rule traits, and lint rules) interact strictly through typed AST helpers.
pub struct ParsedFile {
    pub(in crate::code_lint::ast) grep: AstGrep<SourceDoc>,
}

impl ParsedFile {
    /// Parses `source` into a syntax tree for `lang`.
    #[must_use]
    pub fn new(source: &str, lang: SupportLang) -> Self {
        Self {
            grep: AstGrep::new(source, lang),
        }
    }

    /// Parses `source` into a Rust syntax tree.
    #[must_use]
    pub fn rust(source: &str) -> Self {
        Self::new(source, SupportLang::Rust)
    }

    /// Returns the programming language of this parsed file.
    #[must_use]
    pub fn lang(&self) -> SupportLang {
        *self.grep.lang()
    }

    /// Returns the full source text of the file.
    #[must_use]
    pub fn source_text(&self) -> std::borrow::Cow<'_, str> {
        self.grep.root().text()
    }
}

/// An opaque syntax tree node exposing source text and span coordinates without leaking
/// low-level Tree-sitter grammar vocabulary (`.kind()`, `.field()`, `.dfs()`, etc.).
#[derive(Clone)]
pub struct AstNode<'a> {
    pub(in crate::code_lint::ast) raw: RawNode<'a>,
}

impl<'a> AstNode<'a> {
    #[must_use]
    pub(in crate::code_lint::ast) const fn from_raw(raw: RawNode<'a>) -> Self {
        Self { raw }
    }

    /// Returns the source text slice spanned by this node.
    #[must_use]
    pub fn text(&self) -> std::borrow::Cow<'_, str> {
        self.raw.text()
    }

    /// Returns the programming language of the file containing this node.
    #[must_use]
    pub fn lang(&self) -> SupportLang {
        *self.raw.lang()
    }

    /// Returns the [`SourceSpan`] (byte range) of this node.
    #[must_use]
    pub fn span(&self) -> SourceSpan {
        SourceSpan::from_range(self.raw.range())
    }

    /// Returns the 1-indexed starting line number of this node.
    #[must_use]
    pub fn start_line(&self) -> usize {
        self.raw.start_pos().line() + 1
    }

    /// Returns the 1-indexed ending line number of this node.
    #[must_use]
    pub fn end_line(&self) -> usize {
        self.raw.end_pos().line() + 1
    }

    /// Resolves the 1-indexed start `(line, column)` coordinate of this node.
    #[must_use]
    pub fn start_coordinate(&self) -> LineColumn {
        let start_pos = self.raw.start_pos();
        LineColumn {
            line: start_pos.line() + 1,
            column: start_pos.column(&self.raw) + 1,
        }
    }

    /// Constructs a [`SourceLocation`] for this node inside the file at `path`.
    #[must_use]
    pub fn to_source_location(&self, path: impl Into<PathBuf>) -> SourceLocation {
        SourceLocation::file_span(path, self.span(), self.start_coordinate())
    }
}

/// Returns true for node kinds that represent comments in `lang`.
fn is_comment_kind(kind: &str, lang: SupportLang) -> bool {
    dispatch_lang!(lang, is_comment_kind(kind), false)
}

/// Collects all Tree-sitter comment nodes in `file` in source order.
///
/// Matches on language-specific comment kinds rather than `Node::is_extra`, which would also
/// admit non-comment trivia such as Python's `line_continuation`.
#[must_use]
pub fn collect_comment_nodes(file: &ParsedFile) -> Vec<AstNode<'_>> {
    let lang = file.lang();
    file.grep
        .root()
        .dfs()
        .filter(|curr| is_comment_kind(curr.kind().as_ref(), lang))
        .map(AstNode::from_raw)
        .collect()
}

/// Returns the semantic argument nodes of a call expression, excluding unnamed punctuation
/// tokens (`(`, `)`, `,`, `::`, etc.) and extra trivia nodes (`comment`, `line_comment`,
/// `block_comment`, `line_continuation`).
fn call_argument_nodes<'a>(call_node: &RawNode<'a>) -> Vec<AstNode<'a>> {
    call_node.field("arguments").map_or_else(Vec::new, |args| {
        args.children()
            .filter(|child| child.is_named() && !child.is_extra())
            .map(AstNode::from_raw)
            .collect()
    })
}

/// Returns true for node kinds that represent call expressions in `lang`.
fn is_call_kind(kind: &str, lang: SupportLang) -> bool {
    dispatch_lang!(lang, is_call_kind(kind), false)
}

/// Returns the invoked method name node of a call's callee expression in `lang`, if the callee
/// is a method access rather than a plain function reference.
fn extract_method_call_target<'a>(
    function: &RawNode<'a>,
    lang: SupportLang,
) -> Option<RawNode<'a>> {
    dispatch_lang!(lang, extract_method_call_target(function), None)
}

/// Candidate call expression extracted from the syntax tree.
pub struct AstCallCandidate<'a> {
    /// The call expression AST node.
    pub node: AstNode<'a>,
    /// Full source text of the invoked function/callee expression.
    pub callee: String,
    /// Terminal method identifier text if the callee is a method access (e.g. `obj.method`).
    pub method_name: Option<String>,
    /// Semantic argument nodes passed to the call.
    pub arguments: Vec<AstNode<'a>>,
}

/// Collects all direct call expressions in `file` along with their callee text, optional method
/// target name, and semantic argument nodes.
#[must_use]
pub fn collect_call_candidates(file: &ParsedFile) -> Vec<AstCallCandidate<'_>> {
    let lang = file.lang();
    let mut out = Vec::new();
    for node in file
        .grep
        .root()
        .dfs()
        .filter(|call_node| is_call_kind(call_node.kind().as_ref(), lang))
    {
        if let Some(function) = node.field("function") {
            let callee = function.text().into_owned();
            let method_name = extract_method_call_target(&function, lang)
                .map(|target| target.text().into_owned());
            let arguments = call_argument_nodes(&node);
            out.push(AstCallCandidate {
                node: AstNode::from_raw(node),
                callee,
                method_name,
                arguments,
            });
        }
    }
    out
}

/// Matches an `ast-grep` call pattern containing `$ARGS` (e.g. `$LOOP($$$LOOP_ARGS).create_task($$$ARGS)`)
/// and returns `(call_node, callee_text, argument_nodes)` tuples.
#[must_use]
pub fn find_pattern_calls<'a>(
    file: &'a ParsedFile,
    pattern: &str,
    fallback_callee: &str,
) -> Vec<(AstNode<'a>, String, Vec<AstNode<'a>>)> {
    let mut out = Vec::new();
    for matched in file.grep.root().find_all(pattern) {
        let raw_node = matched.get_node().clone();
        let callee = raw_node.field("function").map_or_else(
            || fallback_callee.to_string(),
            |func| func.text().to_string(),
        );
        let arguments = matched
            .get_env()
            .get_multiple_matches("ARGS")
            .into_iter()
            .filter(|child| child.is_named() && !child.is_extra())
            .map(AstNode::from_raw)
            .collect();
        out.push((AstNode::from_raw(raw_node), callee, arguments));
    }
    out
}

/// Collects all binding definition nodes from a parsed file.
#[must_use]
pub fn collect_bindings(file: &ParsedFile) -> Vec<AstNode<'_>> {
    dispatch_lang!(file.lang(), collect_bindings(file), Vec::new())
}

/// Returns true if the node represents an import binding.
#[must_use]
pub fn is_import_binding(node: &AstNode<'_>, lang: SupportLang) -> bool {
    let Some(parent) = node.raw.parent() else {
        return false;
    };
    let parent_kind = parent.kind();
    dispatch_lang!(lang, is_import_binding_parent(parent_kind.as_ref()), false)
}

/// Returns true if the node represents an unaliased import binding (an external symbol
/// imported directly without a local `as` alias).
#[must_use]
pub fn is_unaliased_import_binding(node: &AstNode<'_>, lang: SupportLang) -> bool {
    let Some(parent) = node.raw.parent() else {
        return false;
    };
    let parent_kind = parent.kind();
    dispatch_lang!(
        lang,
        is_unaliased_import_binding_parent(parent_kind.as_ref()),
        false
    )
}

/// Returns true if the node represents a structural type, class, or function definition name.
#[must_use]
pub fn is_structural_definition(node: &AstNode<'_>, lang: SupportLang) -> bool {
    let Some(parent) = node.raw.parent() else {
        return false;
    };
    let parent_kind = parent.kind();
    dispatch_lang!(
        lang,
        is_structural_definition_parent(parent_kind.as_ref()),
        false
    )
}

/// Returns true if the node is the name of a member defined inside a trait implementation
/// (`impl Trait for Type` in Rust or `@override` method in Python), i.e. a name mandated by a contract.
#[must_use]
pub fn is_trait_impl_member(node: &AstNode<'_>, lang: SupportLang) -> bool {
    let Some(raw_item) = node.raw.parent() else {
        return false;
    };
    let item = AstNode::from_raw(raw_item);
    dispatch_lang!(lang, is_trait_impl_member(&item), false)
}

/// Collects all outermost test functions in `file` along with their identifier node, name, and assertion count.
#[must_use]
pub fn collect_test_function_assertion_counts(
    file: &ParsedFile,
) -> Vec<(AstNode<'_>, String, usize)> {
    dispatch_lang!(
        file.lang(),
        collect_test_function_assertion_counts(file),
        Vec::new()
    )
}

/// Finds all multiline string literals in `file` that are not docstrings/doc-attributes/snapshots
/// and are not wrapped in an allowed dedent helper.
#[must_use]
pub fn find_unwrapped_multiline_strings(
    file: &ParsedFile,
    is_allowed_wrapper: impl Fn(&str, &str) -> bool,
) -> Vec<AstNode<'_>> {
    dispatch_lang!(
        file.lang(),
        find_unwrapped_multiline_strings(file, is_allowed_wrapper),
        Vec::new()
    )
}

/// Returns the nearest enclosing function name if `node` is inside a function and no enclosing
/// function satisfies `is_exempt(func_name, is_top_level)`.
///
/// The exemption is *inherited*: a nested function, closure, or lambda declared inside an exempt
/// boundary function is part of that boundary's implementation. The reported name is the *nearest*
/// enclosing function, so diagnostics point at the innermost context even though exemptions
/// consider every ancestor.
#[must_use]
pub fn enclosing_non_exempt_function_name(
    node: &AstNode<'_>,
    lang: SupportLang,
    is_exempt: impl Fn(&str, bool) -> bool,
) -> Option<String> {
    let mut nearest_function_name: Option<String> = None;

    for ancestor in node.raw.ancestors() {
        let func_info = dispatch_lang!(lang, function_name_and_is_top_level(&ancestor), None);
        if let Some((func_name, is_top_level)) = func_info {
            if is_exempt(&func_name, is_top_level) {
                return None;
            }
            if nearest_function_name.is_none() {
                nearest_function_name = Some(func_name.into_owned());
            }
        }
    }

    nearest_function_name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_location_from_node() {
        let source = "fn main() {\n    let value = 42;\n}\n";
        let file = ParsedFile::new(source, SupportLang::Rust);
        let matched = file
            .grep
            .root()
            .find("let $VAR = $VAL")
            .expect("let statement node should match pattern");
        let node = AstNode::from_raw(matched.get_node().clone());
        assert_eq!(
            node.to_source_location("src/main.rs"),
            SourceLocation::file_span(
                "src/main.rs",
                SourceSpan::new(16, 31),
                LineColumn { line: 2, column: 5 },
            )
        );
    }

    /// Call arguments exclude punctuation and trivia such as comments.
    #[rstest::rstest]
    #[case::python(
        indoc::indoc! {r"
            sleep(
                # comment before arg
                0,  # inline comment
            )
        "},
        SupportLang::Python,
        "0"
    )]
    #[case::rust(
        indoc::indoc! {r"
            fn f() {
                sleep(
                    /* block comment */
                    Duration::ZERO, // line comment
                );
            }
        "},
        SupportLang::Rust,
        "Duration::ZERO"
    )]
    fn test_call_argument_nodes_excludes_comments_and_trivia(
        #[case] source: &str,
        #[case] lang: SupportLang,
        #[case] expected_argument: &str,
    ) {
        let file = ParsedFile::new(source, lang);
        let argument_texts: Vec<Vec<String>> = collect_call_candidates(&file)
            .iter()
            .map(|call| {
                call.arguments
                    .iter()
                    .map(|argument| argument.text().into_owned())
                    .collect()
            })
            .collect();
        assert_eq!(argument_texts, vec![vec![expected_argument.to_owned()]]);
    }
}
