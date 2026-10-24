//! Pure in-memory file patching and modified line tracking for tool edits.

use std::collections::HashSet;

use similar::{ChangeTag, TextDiff};

use super::event::ToolCallPayload;

/// Errors encountered while applying virtual tool edits to file content.
#[derive(Debug, thiserror::Error)]
pub(super) enum VirtualEditError {
    /// Target file does not exist on disk for a relative or selective edit.
    #[error("original file does not exist for editing")]
    MissingOriginalFile,
    /// Target string not found in original file content.
    #[error("target pattern not found in file content")]
    TargetNotFound,
    /// Tool call payload is not a file modification.
    #[error("not a file editing tool payload")]
    NotAFileEdit,
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
    payload: &ToolCallPayload,
) -> Result<VirtualFile, VirtualEditError> {
    let existing = original_content.unwrap_or_default();
    let content = match payload {
        ToolCallPayload::WriteFile {
            content: new_content,
            append: true,
            ..
        } => format!("{existing}{new_content}"),
        ToolCallPayload::WriteFile {
            content: new_content,
            append: false,
            ..
        } => new_content.clone(),
        ToolCallPayload::EditFile {
            target,
            replacement,
            ..
        } => {
            let original = original_content.ok_or(VirtualEditError::MissingOriginalFile)?;
            if target.is_empty() || !original.contains(target.as_str()) {
                return Err(VirtualEditError::TargetNotFound);
            }
            original.replace(target.as_str(), replacement)
        }
        ToolCallPayload::RunCommand { .. } | ToolCallPayload::Other { .. } => {
            return Err(VirtualEditError::NotAFileEdit);
        }
    };

    let modified_lines = changed_line_numbers(existing, &content);

    Ok(VirtualFile {
        content,
        modified_lines,
    })
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
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn apply_write_file_overwrites_completely() {
        let new_content = indoc::indoc! {"
            fn test() {}
            fn second() {}
        "};
        let payload = ToolCallPayload::WriteFile {
            path: PathBuf::from("foo.rs"),
            content: new_content.to_owned(),
            append: false,
        };
        let virtual_file =
            apply_virtual_edit(Some("old content"), &payload).expect("virtual edit should succeed");
        assert_eq!(virtual_file.content, new_content);
        assert_eq!(virtual_file.modified_lines, HashSet::from([1, 2]));
    }

    #[test]
    fn apply_write_file_empty_returns_no_modified_lines() {
        let payload = ToolCallPayload::WriteFile {
            path: PathBuf::from("foo.rs"),
            content: String::new(),
            append: true,
        };
        let virtual_file =
            apply_virtual_edit(Some("existing\n"), &payload).expect("virtual edit should succeed");
        assert_eq!(virtual_file.content, "existing\n");
        assert!(virtual_file.modified_lines.is_empty());
    }

    #[test]
    fn apply_write_file_appends_to_existing() {
        let payload = ToolCallPayload::WriteFile {
            path: PathBuf::from("foo.rs"),
            content: "fn appended() {}\n".to_owned(),
            append: true,
        };
        let expected = indoc::indoc! {"
            fn first() {}
            fn appended() {}
        "};
        let virtual_file =
            apply_virtual_edit(Some("fn first() {}\n"), &payload).expect("append should succeed");
        assert_eq!(virtual_file.content, expected);
        assert_eq!(virtual_file.modified_lines, HashSet::from([2]));
    }

    #[test]
    fn apply_edit_file_replaces_target_and_tracks_lines() {
        let payload = ToolCallPayload::EditFile {
            path: PathBuf::from("foo.rs"),
            target: "OLD_VALUE".to_owned(),
            replacement: "NEW_VALUE".to_owned(),
        };
        let original = indoc::indoc! {r#"
            const A: u32 = 1;
            const B: &str = "OLD_VALUE";
        "#};
        let expected = indoc::indoc! {r#"
            const A: u32 = 1;
            const B: &str = "NEW_VALUE";
        "#};
        let virtual_file =
            apply_virtual_edit(Some(original), &payload).expect("edit should succeed");
        assert_eq!(virtual_file.content, expected);
        assert_eq!(virtual_file.modified_lines, HashSet::from([2]));
    }

    #[test]
    fn apply_edit_file_trailing_newline_does_not_mark_next_line() {
        let payload = ToolCallPayload::EditFile {
            path: PathBuf::from("foo.rs"),
            target: "line_two\n".to_owned(),
            replacement: "replacement_two\n".to_owned(),
        };
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
        let virtual_file =
            apply_virtual_edit(Some(original), &payload).expect("edit should succeed");
        assert_eq!(virtual_file.content, expected);
        assert_eq!(virtual_file.modified_lines, HashSet::from([2]));
    }

    #[test]
    fn apply_edit_file_replaces_multiple_target_matches() {
        let payload = ToolCallPayload::EditFile {
            path: PathBuf::from("foo.rs"),
            target: "foo".to_owned(),
            replacement: "bar".to_owned(),
        };
        let original = "let foo = 1;\nlet foo = 2;\n";
        let expected = "let bar = 1;\nlet bar = 2;\n";
        let virtual_file =
            apply_virtual_edit(Some(original), &payload).expect("edit should succeed");
        assert_eq!(virtual_file.content, expected);
        assert_eq!(virtual_file.modified_lines, HashSet::from([1, 2]));
    }

    #[test]
    fn apply_edit_file_fails_when_target_missing() {
        let payload = ToolCallPayload::EditFile {
            path: PathBuf::from("foo.rs"),
            target: "MISSING".to_owned(),
            replacement: "NEW".to_owned(),
        };
        let error = apply_virtual_edit(Some("existing text"), &payload).unwrap_err();
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
        let payload = ToolCallPayload::EditFile {
            path: PathBuf::from("foo.rs"),
            target: target.to_owned(),
            replacement: replacement.to_owned(),
        };
        let virtual_file =
            apply_virtual_edit(Some(original), &payload).expect("edit should succeed");
        assert_eq!(virtual_file.modified_lines, HashSet::from([2]));
    }
}
