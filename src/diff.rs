//! Differential VCS diff parsing and filtering utilities.

use anyhow::{anyhow, Result};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The type of Version Control System detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VcsType {
    /// Git version control system.
    Git,
    /// Jujutsu (jj) version control system.
    Jujutsu,
}

/// Walks up from the current working directory to locate the repository root
/// and determine the active VCS type.
#[must_use]
pub fn find_repo_root() -> Option<(PathBuf, VcsType)> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join(".jj").is_dir() {
            return Some((dir, VcsType::Jujutsu));
        }
        if dir.join(".git").exists() {
            return Some((dir, VcsType::Git));
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

/// Runs the detected VCS diff command and parses the output into a map of absolute paths
/// mapped to their 1-indexed changed line numbers.
///
/// # Errors
///
/// Returns an error if no VCS repository is found, if subprocess execution fails,
/// or if command stderr is non-empty.
pub fn detect_vcs_diff(
    custom_rev: Option<&str>,
) -> Result<(VcsType, String, HashMap<PathBuf, HashSet<usize>>)> {
    let (repo_root, vcs_type) = find_repo_root().ok_or_else(|| {
        anyhow!("--diff requires a Git or Jujutsu repository, but none was found")
    })?;
    let repo_root = repo_root.canonicalize()?;

    let resolved_rev = match vcs_type {
        VcsType::Jujutsu => custom_rev.unwrap_or("immutable().."),
        VcsType::Git => custom_rev.unwrap_or("HEAD"),
    };

    let diff_content = match vcs_type {
        VcsType::Jujutsu => get_jj_diff(&repo_root, resolved_rev)?,
        VcsType::Git => get_git_diff(&repo_root, resolved_rev)?,
    };

    let parsed_diff = parse_git_diff(&diff_content);

    // Convert relative repository paths to absolute paths
    let mut absolute_diff = HashMap::new();
    for (rel_path, lines) in parsed_diff {
        let abs_path = repo_root.join(rel_path);
        absolute_diff.insert(abs_path, lines);
    }

    Ok((vcs_type, resolved_rev.to_string(), absolute_diff))
}

fn is_complex_jj_revset(rev: &str) -> bool {
    rev.contains("..")
        || rev.contains("::")
        || rev.contains('(')
        || rev.contains(')')
        || rev.chars().any(|c| matches!(c, '|' | '&' | '~' | ' '))
}

fn get_jj_diff(repo_root: &Path, rev: &str) -> Result<String> {
    let mut cmd = Command::new("jj");
    cmd.current_dir(repo_root);
    cmd.arg("diff").arg("--git");
    if is_complex_jj_revset(rev) {
        cmd.arg("-r").arg(rev);
    } else {
        cmd.arg("--from").arg(rev);
    }
    let output = cmd.output().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            anyhow!("Jujutsu CLI ('jj') was not found in PATH. Please install it or ensure it is accessible.")
        } else {
            anyhow!("Failed to run Jujutsu CLI: {error}")
        }
    })?;

    if !output.status.success() {
        let error_message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(anyhow!("Jujutsu diff failed: {error_message}"));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn get_git_diff(repo_root: &Path, rev: &str) -> Result<String> {
    let output = Command::new("git")
        .current_dir(repo_root)
        .args([
            "-c",
            "core.quotepath=false",
            "diff",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            rev,
        ])
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                anyhow!("Git CLI ('git') was not found in PATH. Please install it or ensure it is accessible.")
            } else {
                anyhow!("Failed to run Git CLI: {error}")
            }
        })?;

    if !output.status.success() {
        let error_message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(anyhow!("Git diff failed: {error_message}"));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Parses a unified git-compatible diff output and returns a mapping from relative file paths
/// to their 1-indexed changed line numbers in the new file.
#[must_use]
pub fn parse_git_diff(content: &str) -> HashMap<PathBuf, HashSet<usize>> {
    let mut changed_lines = HashMap::new();
    let mut current_file: Option<(PathBuf, HashSet<usize>)> = None;
    let mut line_counter = 0;
    let mut is_deleted = false;
    let mut expecting_file_header = true;

    for line in content.lines() {
        if line.starts_with("diff --git") {
            // Commit the previous file if it was not deleted and has changes
            if let Some((path, lines)) = current_file.take() {
                if !is_deleted && !lines.is_empty() {
                    changed_lines.insert(path, lines);
                }
            }
            is_deleted = false;
            line_counter = 0;
            expecting_file_header = true;
        } else if line.starts_with("+++ ") && expecting_file_header {
            let path_part = &line[4..];
            if path_part == "/dev/null" {
                is_deleted = true;
            } else {
                let parsed_path = match path_part.split_once('\t') {
                    Some((path, _)) => path,
                    None => path_part,
                };
                let parsed_path = parsed_path.trim_matches('"');
                let parsed_path = parsed_path.strip_prefix("b/").unwrap_or(parsed_path);
                let parsed_path = parsed_path.trim_matches('"');
                current_file = Some((PathBuf::from(parsed_path), HashSet::new()));
            }
            expecting_file_header = false;
        } else if line.starts_with("@@ ") {
            if let Some(new_start) = parse_hunk_header(line) {
                line_counter = new_start;
            } else {
                line_counter = 0;
            }
        } else if let Some((_, lines)) = current_file.as_mut() {
            if is_deleted || line_counter == 0 {
                continue;
            }
            if line.starts_with('+') {
                lines.insert(line_counter);
                line_counter += 1;
            } else if line.starts_with('-') {
                // Deleted line: exists only in old file, do not increment new line counter
            } else if line.starts_with(' ') || line.is_empty() {
                // Context line: exists in new file, increment counter
                line_counter += 1;
            }
        }
    }

    // Commit the last file
    if let Some((path, lines)) = current_file {
        if !is_deleted && !lines.is_empty() {
            changed_lines.insert(path, lines);
        }
    }

    changed_lines
}

fn parse_hunk_header(line: &str) -> Option<usize> {
    // Example: @@ -1,3 +4,5 @@
    // We want to extract '4' from '+4,5'
    let parts: Vec<&str> = line.split("@@").collect();
    let header = parts.get(1)?;
    let new_file_part = header.split('+').nth(1)?; // "4,5 "
    let new_start_text = new_file_part.split(',').next()?.trim();
    new_start_text.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_diff() {
        let diff = r#"diff --git a/src/lib.rs b/src/lib.rs
index 123456..789101 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,4 @@
 fn foo() {
-    let x = 1;
+    let y = 2;
+    let z = 3;
 }
"#;
        let changed_lines = parse_git_diff(diff);
        let path = PathBuf::from("src/lib.rs");
        assert!(changed_lines.contains_key(&path));
        let lines = &changed_lines[&path];
        assert_eq!(lines.len(), 2);
        assert!(lines.contains(&2));
        assert!(lines.contains(&3));
    }

    #[test]
    fn test_parse_single_line_hunk() {
        let diff = r#"diff --git a/test.py b/test.py
index 111..222 100644
--- a/test.py
+++ b/test.py
@@ -10 +10 @@
-old
+new
"#;
        let changed_lines = parse_git_diff(diff);
        let path = PathBuf::from("test.py");
        let lines = &changed_lines[&path];
        assert_eq!(lines.len(), 1);
        assert!(lines.contains(&10));
    }

    #[test]
    fn test_parse_deleted_file() {
        let diff = r#"diff --git a/test.py b/test.py
deleted file mode 100644
--- a/test.py
+++ /dev/null
@@ -1,2 +0,0 @@
-def foo():
-    pass
"#;
        let changed_lines = parse_git_diff(diff);
        assert!(changed_lines.is_empty());
    }

    #[test]
    fn test_parse_quoted_paths_with_spaces() {
        let diff = r#"diff --git a/"some file.rs" b/"some file.rs"
--- a/"some file.rs"
+++ b/"some file.rs"
@@ -1 +1,2 @@
+// added comment
 "#;
        let changed_lines = parse_git_diff(diff);
        let path = PathBuf::from("some file.rs");
        assert!(changed_lines.contains_key(&path));
        assert!(changed_lines[&path].contains(&1));
    }

    #[test]
    fn test_parse_no_newline_warning() {
        let diff = r#"diff --git a/test.py b/test.py
--- a/test.py
+++ b/test.py
@@ -1 +1,2 @@
+def foo(): pass
\ No newline at end of file
 "#;
        let changed_lines = parse_git_diff(diff);
        let path = PathBuf::from("test.py");
        assert!(changed_lines[&path].contains(&1));
    }

    #[test]
    fn test_parse_path_containing_b_slash() {
        let diff = r#"diff --git a/some b/file.rs b/some b/file.rs
--- a/some b/file.rs
+++ b/some b/file.rs
@@ -1 +1,2 @@
+// comment
 "#;
        let changed_lines = parse_git_diff(diff);
        let path = PathBuf::from("some b/file.rs");
        assert!(changed_lines.contains_key(&path));
        assert!(changed_lines[&path].contains(&1));
    }

    #[test]
    fn test_parse_diff_with_added_line_starting_with_plus_plus() {
        let diff = r#"diff --git a/test.py b/test.py
--- a/test.py
+++ b/test.py
@@ -1,2 +1,3 @@
 def foo():
    pass
+++ this looks like a file header but is actually an added line starting with ++
"#;
        let changed_lines = parse_git_diff(diff);
        let path = PathBuf::from("test.py");
        assert!(changed_lines.contains_key(&path));
        assert!(changed_lines[&path].contains(&3));
    }

    #[test]
    fn test_is_complex_jj_revset() {
        assert!(is_complex_jj_revset("immutable().."));
        assert!(is_complex_jj_revset("@-..@"));
        assert!(is_complex_jj_revset("main..@"));
        assert!(is_complex_jj_revset("root()..@"));
        assert!(is_complex_jj_revset("all()"));
        assert!(is_complex_jj_revset("@-::@"));
        assert!(is_complex_jj_revset("a | b"));
        assert!(is_complex_jj_revset("a & b"));
        assert!(is_complex_jj_revset("~a"));

        assert!(!is_complex_jj_revset("@-"));
        assert!(!is_complex_jj_revset("@"));
        assert!(!is_complex_jj_revset("main"));
        assert!(!is_complex_jj_revset("v1.0"));
        assert!(!is_complex_jj_revset("cad67343"));
    }
}
