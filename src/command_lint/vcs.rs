//! VCS context abstractions and client implementations for Jujutsu (jj).

architecture_component!(CommandLintVcs);

use std::path::{Path, PathBuf};

/// Metadata about a Jujutsu revision queried from the repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitInfo {
    /// Commit description text.
    pub description: String,
    /// Whether the revision currently has unresolved conflicts.
    pub conflict: bool,
}

/// Interface representing a client to query Jujutsu repository details.
pub trait JjClient {
    /// Queries the commit description and conflict status for the given revision.
    ///
    /// # Errors
    ///
    /// Returns an error if the VCS client fails to retrieve commit metadata.
    fn get_commit_info(&self, revision: &str) -> Result<CommitInfo, String>;

    /// Queries the description of the given revision.
    ///
    /// # Errors
    ///
    /// Returns an error if the VCS client fails to retrieve the description.
    fn get_commit_description(&self, revision: &str) -> Result<String, String> {
        self.get_commit_info(revision)
            .map(|commit_info| commit_info.description)
    }
}

/// Command-line client executing `jj` commands as subprocesses.
#[derive(Debug, Clone, Default)]
pub struct JjCliClient {
    working_dir: Option<PathBuf>,
}

impl JjCliClient {
    /// Creates a new CLI client operating in the current working directory.
    #[must_use]
    pub const fn new() -> Self {
        Self { working_dir: None }
    }

    /// Creates a new CLI client operating in the specified working directory.
    #[must_use]
    pub fn in_dir(working_dir: impl Into<PathBuf>) -> Self {
        Self {
            working_dir: Some(working_dir.into()),
        }
    }
}

impl JjClient for JjCliClient {
    /// Queries the description and conflict status using the `jj` CLI command.
    ///
    /// # Errors
    ///
    /// Returns an error if the `jj` command fails to run or returns a non-zero exit status.
    fn get_commit_info(&self, revision: &str) -> Result<CommitInfo, String> {
        let mut command = std::process::Command::new("jj");
        command.args([
            "log",
            "--no-graph",
            "--ignore-working-copy",
            "--limit",
            "1",
            "-r",
            revision,
            "-T",
            r#"self.conflict() ++ "\t" ++ description"#,
        ]);
        if let Some(directory) = &self.working_dir
            && let Some(existing_directory) = existing_working_directory(directory)
        {
            command.current_dir(existing_directory);
        }

        let output = command.output().map_err(|error| error.to_string())?;

        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).to_string());
        }

        let text = String::from_utf8_lossy(&output.stdout);
        Ok(parse_commit_info_output(&text))
    }
}

/// Finds the nearest ancestor directory (including `directory` itself) that contains a `.jj`
/// repository root.
#[must_use]
pub fn find_jj_repo_root(directory: &Path) -> Option<PathBuf> {
    let resolved = resolve_search_directory(directory);
    resolved
        .ancestors()
        .find(|ancestor| !ancestor.as_os_str().is_empty() && ancestor.join(".jj").exists())
        .map(Path::to_path_buf)
}

/// Checks whether `directory` or any of its parent directories contains a `.jj` repository root.
#[must_use]
pub fn has_jj_repo_ancestor(directory: &Path) -> bool {
    find_jj_repo_root(directory).is_some()
}

fn existing_working_directory(directory: &Path) -> Option<PathBuf> {
    let resolved = resolve_search_directory(directory);
    resolved
        .ancestors()
        .find(|ancestor| !ancestor.as_os_str().is_empty() && ancestor.is_dir())
        .map(Path::to_path_buf)
}

fn parse_commit_info_output(stdout: &str) -> CommitInfo {
    if let Some((conflict_flag, description_text)) = stdout.split_once('\t') {
        CommitInfo {
            conflict: conflict_flag.trim() == "true",
            description: description_text.trim().to_owned(),
        }
    } else {
        CommitInfo {
            conflict: false,
            description: stdout.trim().to_owned(),
        }
    }
}

fn resolve_search_directory(directory: &Path) -> PathBuf {
    if directory.as_os_str().is_empty() || directory.is_relative() {
        std::env::current_dir().map_or_else(
            |_| PathBuf::from("."),
            |current_dir| current_dir.join(directory),
        )
    } else {
        directory.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[rstest::rstest]
    #[case::empty("false\t\n", "", false)]
    #[case::conflicted(
        "true\tfeat: resolve merge conflict\n",
        "feat: resolve merge conflict",
        true
    )]
    #[case::multiline_tabbed(
        indoc::indoc! {"
            false\tfeat: title

            \tbody with \t tab
        "},
        indoc::indoc! {"
            feat: title

            \tbody with \t tab"},
        false
    )]
    fn test_parse_commit_info_output(
        #[case] raw_output: &str,
        #[case] expected_description: &str,
        #[case] expected_conflict: bool,
    ) {
        let commit_info = parse_commit_info_output(raw_output);
        assert_eq!(commit_info.description, expected_description);
        assert_eq!(commit_info.conflict, expected_conflict);
    }

    #[test]
    fn find_jj_repo_root_detects_nested_directory_and_non_existent_children() {
        let temp_dir = tempfile::tempdir().expect("tempdir creation should succeed");
        let repo_root = temp_dir.path().join("repo");
        let nested_dir = repo_root.join("nested").join("subfolder");
        let non_existent_dir = repo_root.join("uncreated").join("future_file.rs");
        fs::create_dir_all(repo_root.join(".jj")).expect("create .jj dir");
        fs::create_dir_all(&nested_dir).expect("create nested dirs");

        assert_eq!(find_jj_repo_root(&nested_dir), Some(repo_root.clone()));
        assert_eq!(find_jj_repo_root(&non_existent_dir), Some(repo_root));
        assert_eq!(find_jj_repo_root(temp_dir.path()), None);
    }
}
