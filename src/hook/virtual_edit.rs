//! Pure in-memory file patching and modified line tracking for tool edits.

use std::collections::HashSet;
use std::ops::Range;

use similar::{ChangeTag, TextDiff};

use super::event::{FileEdit, TextReplacement};

/// Errors encountered while applying virtual tool edits to file content.
#[derive(Debug, thiserror::Error)]
pub(super) enum VirtualEditError {
    /// Target file does not exist on disk for a relative or selective edit.
    #[error("original file does not exist for editing")]
    MissingOriginalFile,
    /// Target string not found in original file content.
    #[error("target pattern not found in file content")]
    TargetNotFound,
    /// Target string matched multiple times when `allow_multiple` is false.
    #[error("target pattern matched multiple times with allow_multiple = false")]
    AmbiguousTarget,
}

/// Result of applying a proposed tool edit to a file in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct VirtualFile {
    /// Complete file content after the proposed edit is applied.
    pub(super) content: String,
    /// Set of 1-based line numbers in `content` that were added or modified by the edit.
    pub(super) modified_lines: HashSet<usize>,
}

pub(super) fn apply_virtual_edit(
    original_content: Option<&str>,
    edit: &FileEdit,
) -> Result<VirtualFile, VirtualEditError> {
    let existing = original_content.unwrap_or_default();
    let content = match edit {
        FileEdit::Overwrite(new_content) => new_content.clone(),
        FileEdit::Append(new_content) => format!("{existing}{new_content}"),
        FileEdit::Replace(replacements) => apply_replacements(original_content, replacements)?,
    };

    let modified_lines = changed_line_numbers(existing, &content);

    Ok(VirtualFile {
        content,
        modified_lines,
    })
}

fn apply_replacements(
    original_content: Option<&str>,
    replacements: &[TextReplacement],
) -> Result<String, VirtualEditError> {
    let original = original_content.ok_or(VirtualEditError::MissingOriginalFile)?;
    if replacements.is_empty() {
        return Err(VirtualEditError::TargetNotFound);
    }
    let mut current = original.to_owned();
    for chunk in replacements {
        current = apply_single_replacement(&current, chunk)?;
    }
    Ok(current)
}

fn apply_single_replacement(
    content: &str,
    chunk: &TextReplacement,
) -> Result<String, VirtualEditError> {
    if chunk.target.is_empty() {
        return Err(VirtualEditError::TargetNotFound);
    }

    if let Some(line_range) = &chunk.line_range
        && let Some(byte_range) =
            line_range_byte_bounds(content, *line_range.start(), *line_range.end())
    {
        let scoped = &content[byte_range.clone()];
        let scoped_matches = scoped.matches(chunk.target.as_str()).count();
        if scoped_matches > 0 {
            if scoped_matches > 1 && !chunk.allow_multiple {
                return Err(VirtualEditError::AmbiguousTarget);
            }
            let replaced = scoped.replace(chunk.target.as_str(), &chunk.replacement);
            return Ok(format!(
                "{}{replaced}{}",
                &content[..byte_range.start],
                &content[byte_range.end..]
            ));
        }
        // Matches `apply_edit.go`: when a line range is given and the target is not found
        // locally, fall back to whole-file replacement only if the target is unique globally.
        if content.matches(chunk.target.as_str()).count() == 1 {
            return Ok(content.replacen(chunk.target.as_str(), &chunk.replacement, 1));
        }
        return Err(VirtualEditError::TargetNotFound);
    }

    let global_matches = content.matches(chunk.target.as_str()).count();
    if global_matches == 0 {
        return Err(VirtualEditError::TargetNotFound);
    }
    if global_matches > 1 && !chunk.allow_multiple {
        return Err(VirtualEditError::AmbiguousTarget);
    }
    Ok(content.replace(chunk.target.as_str(), &chunk.replacement))
}

fn line_range_byte_bounds(
    content: &str,
    start_line: usize,
    end_line: usize,
) -> Option<Range<usize>> {
    if start_line == 0 || end_line < start_line || content.is_empty() {
        return None;
    }
    let mut current_line = 1;
    let mut range_start = (start_line == 1).then_some(0);
    for (byte_index, character) in content.char_indices() {
        if character == '\n' {
            if current_line == end_line {
                return range_start.map(|start| start..byte_index + 1);
            }
            current_line += 1;
            if current_line == start_line {
                range_start = Some(byte_index + 1);
            }
        }
    }
    range_start
        .filter(|&start| start <= content.len())
        .map(|start| start..content.len())
}

fn changed_line_numbers(old: &str, new: &str) -> HashSet<usize> {
    TextDiff::from_lines(old, new)
        .iter_all_changes()
        .filter(|change| change.tag() == ChangeTag::Insert)
        .filter_map(|change| change.new_index().map(|index| index + 1))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_write_file_overwrites_completely() {
        let new_content = indoc::indoc! {"
            fn test() {}
            fn second() {}
        "};
        let edit = FileEdit::Overwrite(new_content.to_owned());
        let virtual_file =
            apply_virtual_edit(Some("old content"), &edit).expect("virtual edit should succeed");
        assert_eq!(virtual_file.content, new_content);
        assert_eq!(virtual_file.modified_lines, HashSet::from([1, 2]));
    }

    #[test]
    fn apply_append_empty_returns_no_modified_lines() {
        let edit = FileEdit::Append(String::new());
        let virtual_file =
            apply_virtual_edit(Some("existing\n"), &edit).expect("virtual edit should succeed");
        assert_eq!(virtual_file.content, "existing\n");
        assert!(virtual_file.modified_lines.is_empty());
    }

    #[test]
    fn apply_write_file_appends_to_existing() {
        let edit = FileEdit::Append("fn appended() {}\n".to_owned());
        let expected = indoc::indoc! {"
            fn first() {}
            fn appended() {}
        "};
        let virtual_file =
            apply_virtual_edit(Some("fn first() {}\n"), &edit).expect("append should succeed");
        assert_eq!(virtual_file.content, expected);
        assert_eq!(virtual_file.modified_lines, HashSet::from([2]));
    }

    #[test]
    fn apply_edit_file_replaces_target_and_tracks_lines() {
        let edit = FileEdit::Replace(vec![TextReplacement {
            target: "OLD_VALUE".to_owned(),
            replacement: "NEW_VALUE".to_owned(),
            line_range: None,
            allow_multiple: true,
        }]);
        let original = indoc::indoc! {r#"
            const A: u32 = 1;
            const B: &str = "OLD_VALUE";
        "#};
        let expected = indoc::indoc! {r#"
            const A: u32 = 1;
            const B: &str = "NEW_VALUE";
        "#};
        let virtual_file = apply_virtual_edit(Some(original), &edit).expect("edit should succeed");
        assert_eq!(virtual_file.content, expected);
        assert_eq!(virtual_file.modified_lines, HashSet::from([2]));
    }

    #[test]
    fn apply_edit_file_trailing_newline_does_not_mark_next_line() {
        let edit = FileEdit::Replace(vec![TextReplacement {
            target: "line_two\n".to_owned(),
            replacement: "replacement_two\n".to_owned(),
            line_range: None,
            allow_multiple: true,
        }]);
        let original = indoc::indoc! {"
            line_one
            line_two
            line_three
        "};
        let expected = indoc::indoc! {"
            line_one
            replacement_two
            line_three
        "};
        let virtual_file = apply_virtual_edit(Some(original), &edit).expect("edit should succeed");
        assert_eq!(virtual_file.content, expected);
        assert_eq!(virtual_file.modified_lines, HashSet::from([2]));
    }

    #[test]
    fn apply_edit_file_replaces_multiple_target_matches() {
        let edit = FileEdit::Replace(vec![TextReplacement {
            target: "foo".to_owned(),
            replacement: "bar".to_owned(),
            line_range: None,
            allow_multiple: true,
        }]);
        let original = "let foo = 1;\nlet foo = 2;\n";
        let expected = "let bar = 1;\nlet bar = 2;\n";
        let virtual_file = apply_virtual_edit(Some(original), &edit).expect("edit should succeed");
        assert_eq!(virtual_file.content, expected);
        assert_eq!(virtual_file.modified_lines, HashSet::from([1, 2]));
    }

    #[test]
    fn apply_edit_file_fails_when_target_missing() {
        let edit = FileEdit::Replace(vec![TextReplacement {
            target: "MISSING".to_owned(),
            replacement: "NEW".to_owned(),
            line_range: None,
            allow_multiple: true,
        }]);
        let error = apply_virtual_edit(Some("existing text"), &edit).unwrap_err();
        assert!(matches!(error, VirtualEditError::TargetNotFound));
    }

    #[test]
    fn apply_edit_file_ignores_unchanged_context_lines_in_target() {
        let original = indoc::indoc! {"
            fn example() {
                let old = 1;
                let keep = 2;
            }
        "};
        let target = indoc::indoc! {"
            fn example() {
                let old = 1;
                let keep = 2;
            }
        "};
        let replacement = indoc::indoc! {"
            fn example() {
                let new = 1;
                let keep = 2;
            }
        "};
        let edit = FileEdit::Replace(vec![TextReplacement {
            target: target.to_owned(),
            replacement: replacement.to_owned(),
            line_range: None,
            allow_multiple: true,
        }]);
        let virtual_file = apply_virtual_edit(Some(original), &edit).expect("edit should succeed");
        assert_eq!(virtual_file.modified_lines, HashSet::from([2]));
    }

    #[test]
    fn apply_replace_on_missing_file_returns_missing_original_file() {
        let edit = FileEdit::Replace(vec![TextReplacement {
            target: "target".to_owned(),
            replacement: "replacement".to_owned(),
            line_range: None,
            allow_multiple: true,
        }]);
        let error = apply_virtual_edit(None, &edit).unwrap_err();
        assert!(matches!(error, VirtualEditError::MissingOriginalFile));
    }

    #[test]
    fn apply_replace_applies_multiple_sequential_replacements() {
        let original = indoc::indoc! {"
            fn compute() {
                let first = 1;
                let second = 2;
            }
        "};
        let expected = indoc::indoc! {"
            fn compute() {
                let alpha = 1;
                let beta = 2;
            }
        "};
        let edit = FileEdit::Replace(vec![
            TextReplacement {
                target: "first".to_owned(),
                replacement: "alpha".to_owned(),
                line_range: None,
                allow_multiple: true,
            },
            TextReplacement {
                target: "second".to_owned(),
                replacement: "beta".to_owned(),
                line_range: None,
                allow_multiple: true,
            },
        ]);
        let virtual_file =
            apply_virtual_edit(Some(original), &edit).expect("multi replacement should succeed");
        assert_eq!(virtual_file.content, expected);
        assert_eq!(virtual_file.modified_lines, HashSet::from([2, 3]));
    }

    #[test]
    fn apply_replace_scopes_to_line_range_when_present() {
        let original = indoc::indoc! {"
            let target = 1;
            let target = 2;
            let target = 3;
        "};
        let expected = indoc::indoc! {"
            let target = 1;
            let updated = 2;
            let target = 3;
        "};
        let edit = FileEdit::Replace(vec![TextReplacement {
            target: "target".to_owned(),
            replacement: "updated".to_owned(),
            line_range: Some(2..=2),
            allow_multiple: false,
        }]);
        let virtual_file =
            apply_virtual_edit(Some(original), &edit).expect("scoped replacement should succeed");
        assert_eq!(virtual_file.content, expected);
        assert_eq!(virtual_file.modified_lines, HashSet::from([2]));
    }

    #[test]
    fn apply_replace_falls_back_to_unique_global_match_when_missing_in_range() {
        let original = indoc::indoc! {"
            let first = 1;
            let second = 2;
            let unique_needle = 3;
        "};
        let expected = indoc::indoc! {"
            let first = 1;
            let second = 2;
            let replaced_needle = 3;
        "};
        let edit = FileEdit::Replace(vec![TextReplacement {
            target: "unique_needle".to_owned(),
            replacement: "replaced_needle".to_owned(),
            line_range: Some(1..=1),
            allow_multiple: false,
        }]);
        let virtual_file =
            apply_virtual_edit(Some(original), &edit).expect("global fallback should succeed");
        assert_eq!(virtual_file.content, expected);
        assert_eq!(virtual_file.modified_lines, HashSet::from([3]));
    }

    #[test]
    fn apply_replace_rejects_multiple_matches_when_allow_multiple_is_false() {
        let original = indoc::indoc! {"
            let duplicated = 1;
            let duplicated = 2;
        "};
        let edit = FileEdit::Replace(vec![TextReplacement {
            target: "duplicated".to_owned(),
            replacement: "replaced".to_owned(),
            line_range: None,
            allow_multiple: false,
        }]);
        let error = apply_virtual_edit(Some(original), &edit).unwrap_err();
        assert!(matches!(error, VirtualEditError::AmbiguousTarget));
    }
}
