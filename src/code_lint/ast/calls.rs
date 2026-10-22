//! Call expression candidates, call-site syntactic context, and enclosing function chains.

use super::tree::{AstNode, ParsedFile};
use crate::diagnostic::SourceSpan;
use std::sync::Arc;

/// A function enclosing a collected node, recorded by the walk that collected the node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::code_lint) struct EnclosingFunction {
    /// The function's name.
    pub(in crate::code_lint) name: String,
    /// Byte span of the function's declared name identifier.
    pub(in crate::code_lint) name_span: SourceSpan,
    /// True if the function is defined at file scope: a statement of the Python module body or
    /// an item of the Rust source file (not nested in a class, `impl`, `mod`, or block).
    pub(in crate::code_lint) is_top_level: bool,
    /// Name of the immediately enclosing top-level Python `class`, if this function is a direct
    /// method of that class. `None` for module-level functions, nested functions, methods of
    /// nested classes, and all Rust functions.
    pub(in crate::code_lint) enclosing_class: Option<String>,
}

/// Returns `outer` (innermost first) with `function` prepended as the new innermost function.
pub(super) fn with_innermost_function(
    function: EnclosingFunction,
    outer: &[EnclosingFunction],
) -> Arc<[EnclosingFunction]> {
    std::iter::once(function)
        .chain(outer.iter().cloned())
        .collect()
}

/// Syntactic context surrounding a call expression.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CallSiteContext {
    /// True if the call is the context-manager expression of a Python `with` item (`with
    /// suppress(KeyError):`). Always false in Rust.
    pub(in crate::code_lint) is_with_context_manager: bool,
    /// True if the call is inside a Python `except` handler without an intervening `def`,
    /// `class`, or `lambda` boundary. Always false in Rust.
    pub(in crate::code_lint) is_in_except_clause: bool,
    /// True if the call is inside the condition or body of a `for`, `while`, or `loop`
    /// statement without an intervening function, lambda, or closure boundary.
    pub(in crate::code_lint) is_in_loop: bool,
    /// The functions enclosing the call, innermost first. A Python function encloses its
    /// decorators, parameters, and body; lambdas and Rust closures are not functions.
    pub(in crate::code_lint) enclosing_functions: Arc<[EnclosingFunction]>,
}

/// Cached call expression metadata without lifetime ties to `ParsedFile`.
pub(super) struct CachedCallCandidate {
    pub(super) span: SourceSpan,
    pub(super) callee: String,
    pub(super) method_name: Option<String>,
    pub(super) receiver_call_callee: Option<String>,
    pub(super) argument_spans: Vec<SourceSpan>,
    pub(super) context: CallSiteContext,
}

/// Candidate call expression extracted from the syntax tree.
pub struct AstCallCandidate<'a> {
    /// The call expression AST node.
    pub(in crate::code_lint) node: AstNode<'a>,
    /// Full source text of the invoked function/callee expression.
    pub(in crate::code_lint) callee: String,
    /// Terminal method identifier text if the callee is a method access (e.g. `obj.method`).
    pub(in crate::code_lint) method_name: Option<String>,
    /// If the receiver of a method call is itself a call expression (e.g. `get_loop().create_task(coro)`),
    /// the callee text of that receiver call (e.g. `"get_loop"` or `"asyncio.get_running_loop"`).
    pub(in crate::code_lint) receiver_call_callee: Option<String>,
    /// Semantic argument nodes passed to the call.
    pub(in crate::code_lint) arguments: Vec<AstNode<'a>>,
    /// Syntactic context surrounding the call expression.
    pub(in crate::code_lint) context: CallSiteContext,
}

impl std::ops::Deref for AstCallCandidate<'_> {
    type Target = CallSiteContext;

    fn deref(&self) -> &Self::Target {
        &self.context
    }
}

/// Collects all direct call expressions in `file` along with their callee text, optional method
/// target name, and semantic argument nodes.
#[must_use]
pub fn collect_call_candidates(file: &ParsedFile) -> Vec<AstCallCandidate<'_>> {
    let cached = file
        .call_candidates
        .get_or_init(|| dispatch_lang!(file.lang(), collect_call_candidates(file)));
    cached
        .iter()
        .map(|entry| AstCallCandidate {
            node: AstNode::from_span(file, entry.span),
            callee: entry.callee.clone(),
            method_name: entry.method_name.clone(),
            receiver_call_callee: entry.receiver_call_callee.clone(),
            arguments: entry
                .argument_spans
                .iter()
                .map(|&span| AstNode::from_span(file, span))
                .collect(),
            context: entry.context.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Language;

    /// Call arguments exclude punctuation and trivia such as comments.
    #[rstest::rstest]
    #[case::python(
        indoc::indoc! {r"
            sleep(
                # comment before arg
                0,  # inline comment
            )
        "},
        Language::Python,
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
        Language::Rust,
        "Duration::ZERO"
    )]
    fn test_call_argument_nodes_excludes_comments_and_trivia(
        #[case] source: &str,
        #[case] lang: Language,
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
