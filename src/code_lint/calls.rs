//! Shared helpers for matching banned call expressions declaratively.
//!
//! Rules that flag banned function or method invocations
//! specify their targets via [`crate::core::FilterListDefaults`] and [`crate::core::DenyListConfig`].
//! Each callee entry (e.g. `"time.sleep"`, `"tokio::time::sleep"`, `"$LOOP($$$LOOP_ARGS).create_task"`)
//! is either matched by callee text or normalized into a `<callee>($$$ARGS)` structural pattern.

use crate::code_lint::ast::{self, AstNode, ParsedFile};
use std::collections::HashSet;

/// A matched call expression together with its resolved callee string and argument nodes.
pub struct CallMatch<'a> {
    /// The matched call expression node (e.g. `time.sleep(1)`).
    pub node: AstNode<'a>,
    /// The source text of the invoked function/callee node (e.g. `time.sleep` or `asyncio.get_event_loop().create_task`).
    pub callee: String,
    /// The semantic argument nodes (excluding punctuation and comments).
    pub arguments: Vec<AstNode<'a>>,
}

/// Normalizes a callee entry into an `ast-grep` call pattern ending with `($$$ARGS)`.
///
/// If the entry already ends with `)` (a full call pattern), it is returned unchanged.
#[must_use]
fn to_call_pattern(entry: &str) -> String {
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

/// Finds all call expressions in `file` matching any of the `banned_callees` entries.
///
/// Literal callee names (e.g. `"time.sleep"`) and any-receiver method patterns starting with `$OBJ.`
/// (e.g. `"$OBJ.assert_called_once"`) are evaluated in a single-pass AST traversal with O(1) set lookups.
/// Entries containing general metavariables (e.g. `"$LOOP($$$ARGS).create_task"`) or custom call
/// signatures fall back to structural `ast-grep` pattern matching.
///
/// Matches are sorted by byte span `(start, end)` and deduplicated so that overlapping
/// patterns or `HashSet` iteration order never produce non-deterministic or duplicate matches.
#[must_use]
pub fn find_banned_calls<'a, S: std::hash::BuildHasher>(
    file: &'a ParsedFile,
    banned_callees: &HashSet<String, S>,
) -> Vec<CallMatch<'a>> {
    let mut matches: Vec<CallMatch<'a>> = Vec::new();

    let mut literal_callees: HashSet<&str> = HashSet::new();
    let mut method_callees: HashSet<&str> = HashSet::new();
    let mut structural_entries: Vec<&str> = Vec::new();

    for entry in banned_callees {
        let trimmed = entry.trim();
        if let Some(method_name) = trimmed.strip_prefix("$OBJ.")
            && !method_name.is_empty()
            && is_literal_callee(method_name)
        {
            method_callees.insert(method_name);
            continue;
        }
        if is_literal_callee(trimmed) {
            literal_callees.insert(trimmed);
        } else {
            structural_entries.push(trimmed);
        }
    }

    if !literal_callees.is_empty() || !method_callees.is_empty() {
        for candidate in ast::collect_call_candidates(file) {
            let is_banned = literal_callees.contains(candidate.callee.as_str())
                || candidate
                    .method_name
                    .as_deref()
                    .is_some_and(|method| method_callees.contains(method));
            if is_banned {
                matches.push(CallMatch {
                    node: candidate.node,
                    callee: candidate.callee,
                    arguments: candidate.arguments,
                });
            }
        }
    }

    for entry in structural_entries {
        let pattern = to_call_pattern(entry);
        for (node, callee, arguments) in ast::find_pattern_calls(file, &pattern, entry) {
            matches.push(CallMatch {
                node,
                callee,
                arguments,
            });
        }
    }

    matches.sort_by_key(|matched| matched.node.span());
    matches.dedup_by_key(|matched| matched.node.span());
    matches
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast_grep_language::SupportLang;

    #[test]
    fn test_python_call_matching_excludes_receiver_methods() {
        let source = indoc::indoc! {r"
            time.sleep(1)
            sleep(0)
            clock.sleep(1)
            self.sleep(1)
            asyncio.get_event_loop().create_task(work())
        "};
        let file = ParsedFile::new(source, SupportLang::Python);
        let banned: HashSet<String> = ["sleep", "time.sleep", "$LOOP($$$LOOP_ARGS).create_task"]
            .into_iter()
            .map(str::to_string)
            .collect();

        let matched = find_banned_calls(&file, &banned);
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
        let source = indoc::indoc! {r"
            fn test_case() {
                std::thread::sleep(dur);
                tokio::time::sleep(Duration::ZERO).await;
                sleep(dur);
                clock.sleep(dur).await;
            }
        "};
        let file = ParsedFile::new(source, SupportLang::Rust);
        let banned: HashSet<String> = ["sleep", "std::thread::sleep", "tokio::time::sleep"]
            .into_iter()
            .map(str::to_string)
            .collect();

        let matched = find_banned_calls(&file, &banned);
        let callees: Vec<&str> = matched.iter().map(|item| item.callee.as_str()).collect();

        assert_eq!(
            callees,
            vec!["std::thread::sleep", "tokio::time::sleep", "sleep"]
        );
        assert_eq!(matched[1].arguments.len(), 1);
        assert_eq!(matched[1].arguments[0].text(), "Duration::ZERO");
    }

    #[test]
    fn test_find_banned_calls_with_obj_method_syntax() {
        let source = indoc::indoc! {r"
            mock_service.assert_called_once()
            gateway.charge.assert_called_once_with(100)
            assert_called_once()
            self.assertEqual(1, 1)
        "};
        let file = ParsedFile::new(source, SupportLang::Python);
        let banned: HashSet<String> = ["$OBJ.assert_called_once", "$OBJ.assert_called_once_with"]
            .into_iter()
            .map(str::to_string)
            .collect();

        let matched = find_banned_calls(&file, &banned);
        let callees: Vec<&str> = matched.iter().map(|item| item.callee.as_str()).collect();

        assert_eq!(
            callees,
            vec![
                "mock_service.assert_called_once",
                "gateway.charge.assert_called_once_with"
            ]
        );
        assert_eq!(matched[1].arguments.len(), 1);
        assert_eq!(matched[1].arguments[0].text(), "100");
    }
}
