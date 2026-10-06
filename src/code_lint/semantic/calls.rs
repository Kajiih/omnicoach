//! Shared helpers for matching banned call expressions declaratively.
//!
//! Rules that flag banned function or method invocations specify their targets via a `ListOption`.
//! Each callee entry (e.g. `"time.sleep"`, `"tokio::time::sleep"`, `"*.assert_called_once"`,
//! `"*().create_task"`) is matched in a single pass over [`ast::collect_call_candidates`].

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

/// Finds all call expressions in `file` matching any of the `banned_callees` entries.
///
/// Supported entry forms:
/// - Literal callee names (e.g. `"time.sleep"`, `"tokio::time::sleep"`): matched by exact callee text.
/// - Any-receiver method patterns `"*.<method>"` (e.g. `"*.assert_called_once"`): matches any method call
///   whose terminal method name is `<method>`.
/// - Chained-call method patterns `"*().<method>"` (e.g. `"*().create_task"`): matches any method call
///   whose receiver is itself a call expression and whose terminal method name is `<method>`.
/// - Specific chained-call method patterns `"<receiver_callee>().<method>"`
///   (e.g. `"asyncio.get_running_loop().create_task"`): matches a method call whose receiver call has
///   callee `<receiver_callee>` and whose terminal method name is `<method>`.
///
/// All patterns are evaluated in a single pass over [`ast::collect_call_candidates`].
/// Matches are sorted by byte span `(start, end)` and deduplicated so that overlapping
/// patterns or `HashSet` iteration order never produce non-deterministic or duplicate matches.
#[must_use]
pub fn find_banned_calls<'a, S: std::hash::BuildHasher>(
    file: &'a ParsedFile,
    banned_callees: &HashSet<String, S>,
) -> Vec<CallMatch<'a>> {
    let mut literal_callees: HashSet<&str> = HashSet::new();
    let mut method_callees: HashSet<&str> = HashSet::new();
    let mut any_chained_methods: HashSet<&str> = HashSet::new();
    let mut specific_chained_methods: HashSet<(&str, &str)> = HashSet::new();

    for entry in banned_callees {
        let trimmed = entry.trim();
        let trimmed = trimmed.strip_suffix("()").unwrap_or(trimmed);
        if let Some(method_name) = trimmed.strip_prefix("*().") {
            if !method_name.is_empty() {
                any_chained_methods.insert(method_name);
            }
        } else if let Some(method_name) = trimmed.strip_prefix("*.") {
            if !method_name.is_empty() {
                method_callees.insert(method_name);
            }
        } else if let Some((receiver_callee, method_name)) = trimmed.split_once("().") {
            if !receiver_callee.is_empty() && !method_name.is_empty() {
                specific_chained_methods.insert((receiver_callee, method_name));
            }
        } else if !trimmed.is_empty() {
            literal_callees.insert(trimmed);
        }
    }

    if literal_callees.is_empty()
        && method_callees.is_empty()
        && any_chained_methods.is_empty()
        && specific_chained_methods.is_empty()
    {
        return Vec::new();
    }

    let mut matches: Vec<CallMatch<'a>> = Vec::new();
    for candidate in ast::collect_call_candidates(file) {
        let is_banned = literal_callees.contains(candidate.callee.as_str())
            || candidate.method_name.as_deref().is_some_and(|method| {
                method_callees.contains(method)
                    || (candidate.receiver_call_callee.is_some()
                        && any_chained_methods.contains(method))
                    || candidate
                        .receiver_call_callee
                        .as_deref()
                        .is_some_and(|receiver_callee| {
                            specific_chained_methods.contains(&(receiver_callee, method))
                        })
            });
        if is_banned {
            matches.push(CallMatch {
                node: candidate.node,
                callee: candidate.callee,
                arguments: candidate.arguments,
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
    use crate::diagnostic::Language;

    #[test]
    fn test_python_call_matching_excludes_receiver_methods() {
        let source = indoc::indoc! {r"
            time.sleep(1)
            sleep(0)
            clock.sleep(1)
            self.sleep(1)
            asyncio.get_event_loop().create_task(work())
        "};
        let file = ParsedFile::new(source, Language::Python);
        let banned: HashSet<String> = ["sleep", "time.sleep", "*().create_task"]
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
        let file = ParsedFile::new(source, Language::Rust);
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
    fn test_find_banned_calls_with_wildcard_method_syntax() {
        let source = indoc::indoc! {r"
            mock_service.assert_called_once()
            gateway.charge.assert_called_once_with(100)
            assert_called_once()
            self.assertEqual(1, 1)
        "};
        let file = ParsedFile::new(source, Language::Python);
        let banned: HashSet<String> = ["*.assert_called_once", "*.assert_called_once_with"]
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
