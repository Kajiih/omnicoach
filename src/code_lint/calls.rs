//! Shared helpers for matching call expressions declaratively via `ast-grep-core`.
//!
//! Rules that flag banned function or method invocations
//! specify their targets via [`crate::core::FilterListDefaults`] and [`crate::core::DenyListConfig`].
//! Each callee entry (e.g. `"time.sleep"`, `"tokio::time::sleep"`, `"$LOOP($$$LOOP_ARGS).create_task"`)
//! is normalized into a `<callee>($$$ARGS)` pattern and matched structurally against the AST.

use crate::code_lint::{AstNode, SourceDoc};
use ast_grep_core::AstGrep;
use std::collections::HashSet;

/// A matched call expression together with its resolved callee string and argument nodes.
pub struct CallMatch<'a> {
    /// The matched call expression node (e.g. `time.sleep(1)`).
    pub node: AstNode<'a>,
    /// The source text of the invoked function/callee node (e.g. `time.sleep` or `asyncio.get_event_loop().create_task`).
    pub callee: String,
    /// The argument nodes captured by `$$$ARGS` (excluding punctuation).
    pub arguments: Vec<AstNode<'a>>,
}

/// Normalizes a callee entry into an `ast-grep` call pattern ending with `($$$ARGS)`.
///
/// If the entry already ends with `)` (a full call pattern), it is returned unchanged.
#[must_use]
pub fn to_call_pattern(entry: &str) -> String {
    let trimmed = entry.trim();
    if trimmed.ends_with(')') {
        trimmed.to_string()
    } else {
        format!("{trimmed}($$$ARGS)")
    }
}

/// Returns true if `entry` is a plain callee name that can be matched by text equality,
/// rather than an `ast-grep` structural pattern (`$LOOP($$$ARGS).create_task`, `foo()`).
fn is_literal_callee(entry: &str) -> bool {
    let trimmed = entry.trim();
    !trimmed.contains('$') && !trimmed.ends_with(')')
}

/// Returns the argument nodes of a call expression, excluding surrounding parens and separators.
fn call_argument_nodes<'a>(call_node: &AstNode<'a>) -> Vec<AstNode<'a>> {
    call_node.field("arguments").map_or_else(Vec::new, |args| {
        args.children()
            .filter(|child| !matches!(child.kind().as_ref(), "(" | ")" | ","))
            .collect()
    })
}

/// Recursively collects all call nodes whose callee text matches an entry in `literals`.
fn collect_literal_call_matches<'a, S: std::hash::BuildHasher>(
    node: &AstNode<'a>,
    literals: &HashSet<&str, S>,
    out: &mut Vec<CallMatch<'a>>,
) {
    if matches!(node.kind().as_ref(), "call_expression" | "call")
        && let Some(function) = node.field("function")
    {
        let callee_text = function.text();
        if literals.contains(callee_text.as_ref()) {
            out.push(CallMatch {
                node: node.clone(),
                callee: callee_text.to_string(),
                arguments: call_argument_nodes(node),
            });
        }
    }

    for child in node.children() {
        collect_literal_call_matches(&child, literals, out);
    }
}

/// Finds all call expressions in `grep` matching any of the `banned_callees` entries.
///
/// Literal callee names are evaluated in a single-pass AST traversal with $O(1)$ set lookups.
/// Entries containing metavariables (e.g. `"$LOOP($$$ARGS).create_task"`) or custom call
/// signatures fall back to structural `ast-grep` pattern matching.
///
/// Matches are sorted by byte span `(start, end)` and deduplicated so that overlapping
/// patterns or `HashSet` iteration order never produce non-deterministic or duplicate matches.
#[must_use]
pub fn find_banned_calls<'a, S: std::hash::BuildHasher>(
    grep: &'a AstGrep<SourceDoc>,
    banned_callees: &HashSet<String, S>,
) -> Vec<CallMatch<'a>> {
    let root = grep.root();
    let mut matches: Vec<CallMatch<'a>> = Vec::new();

    let mut literal_callees: HashSet<&str> = HashSet::new();
    let mut structural_entries: Vec<&str> = Vec::new();

    for entry in banned_callees {
        if is_literal_callee(entry) {
            literal_callees.insert(entry.as_str());
        } else {
            structural_entries.push(entry.as_str());
        }
    }

    if !literal_callees.is_empty() {
        collect_literal_call_matches(&root, &literal_callees, &mut matches);
    }

    for entry in structural_entries {
        let pattern = to_call_pattern(entry);
        for matched in root.find_all(pattern.as_str()) {
            let node: AstNode<'a> = matched.get_node().clone();
            let callee = node
                .field("function")
                .map_or_else(|| entry.to_string(), |func| func.text().to_string());
            let arguments = matched
                .get_env()
                .get_multiple_matches("ARGS")
                .into_iter()
                .collect();

            matches.push(CallMatch {
                node,
                callee,
                arguments,
            });
        }
    }

    matches.sort_by_key(|matched| (matched.node.range().start, matched.node.range().end));
    matches.dedup_by_key(|matched| (matched.node.range().start, matched.node.range().end));
    matches
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast_grep_language::SupportLang;

    #[test]
    fn test_python_call_matching_excludes_receiver_methods() {
        let source = r"
time.sleep(1)
sleep(0)
clock.sleep(1)
self.sleep(1)
asyncio.get_event_loop().create_task(work())
";
        let grep = AstGrep::new(source, SupportLang::Python);
        let banned: HashSet<String> = ["sleep", "time.sleep", "$LOOP($$$LOOP_ARGS).create_task"]
            .into_iter()
            .map(str::to_string)
            .collect();

        let matched = find_banned_calls(&grep, &banned);
        let callees: Vec<&str> = matched.iter().map(|item| item.callee.as_str()).collect();

        assert_eq!(
            callees,
            vec![
                "time.sleep",
                "sleep",
                "asyncio.get_event_loop().create_task"
            ]
        );
        assert_eq!(matched[0].arguments.len(), 1);
        assert_eq!(matched[0].arguments[0].text(), "1");
        assert_eq!(matched[1].arguments[0].text(), "0");
    }

    #[test]
    fn test_rust_call_matching_excludes_receiver_methods() {
        let source = r"
fn test_case() {
    std::thread::sleep(dur);
    tokio::time::sleep(Duration::ZERO).await;
    sleep(dur);
    clock.sleep(dur).await;
}
";
        let grep = AstGrep::new(source, SupportLang::Rust);
        let banned: HashSet<String> = ["sleep", "std::thread::sleep", "tokio::time::sleep"]
            .into_iter()
            .map(str::to_string)
            .collect();

        let matched = find_banned_calls(&grep, &banned);
        let callees: Vec<&str> = matched.iter().map(|item| item.callee.as_str()).collect();

        assert_eq!(
            callees,
            vec!["std::thread::sleep", "tokio::time::sleep", "sleep"]
        );
        assert_eq!(matched[1].arguments.len(), 1);
        assert_eq!(matched[1].arguments[0].text(), "Duration::ZERO");
    }
}
