//! VCS context abstractions and client implementations for Jujutsu (jj).

architecture_component!(CommandLintVcs);

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Metadata about a Jujutsu revision queried from the repository.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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
}

const COMMIT_INFO_TEMPLATE: &str = r#""{\"conflict\":" ++ json(self.conflict()) ++ ",\"description\":" ++ json(description) ++ "}""#;

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
            COMMIT_INFO_TEMPLATE,
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

        serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())
    }
}

fn existing_working_directory(directory: &Path) -> Option<&Path> {
    directory
        .ancestors()
        .find(|ancestor| !ancestor.as_os_str().is_empty() && ancestor.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    #[case::empty(r#"{"conflict":false,"description":""}"#, "", false)]
    #[case::conflicted(
        r#"{"conflict":true,"description":"feat: resolve merge conflict"}"#,
        "feat: resolve merge conflict",
        true
    )]
    #[case::multiline_with_quotes(
        indoc::indoc! {r#"
            {"conflict":false,"description":"feat: title\n\nbody with \"quotes\" and \ttabs"}
        "#},
        indoc::indoc! {"
            feat: title

            body with \"quotes\" and \ttabs"},
        false
    )]
    fn test_deserialize_commit_info(
        #[case] json_output: &str,
        #[case] expected_description: &str,
        #[case] expected_conflict: bool,
    ) {
        let commit_info: CommitInfo =
            serde_json::from_str(json_output).expect("deserialize should succeed");
        assert_eq!(commit_info.description, expected_description);
        assert_eq!(commit_info.conflict, expected_conflict);
    }
}
