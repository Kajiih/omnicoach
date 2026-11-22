//! Read-only Jujutsu (`jj`) repository state query client and per-run memoization cache.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;

const COMMIT_SUMMARY_JSONL_TEMPLATE: &str = concat!(
    r#""{\"change_id\":" ++ json(change_id.short(8))"#,
    r#" ++ ",\"description\":" ++ json(description)"#,
    r#" ++ ",\"conflict\":" ++ json(self.conflict())"#,
    r#" ++ ",\"divergent\":" ++ json(self.divergent())"#,
    r#" ++ ",\"in_other_workspace\":" ++ json(!self.immutable() && (self.contained_in("::(working_copies() ~ present(@))") || self.working_copies().len() > 1))"#,
    r#" ++ "}\n""#,
);

const MAX_QUERIED_REVISIONS: &str = "256";

type RevisionQueryResult = Result<Vec<CommitSummary>, String>;
type RevisionQueryCache = HashMap<(PathBuf, String), RevisionQueryResult>;

/// Summary of a single Jujutsu commit returned by a read-only `jj log` query.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommitSummary {
    /// Short 8-character change ID (`change_id.short(8)`).
    pub change_id: String,
    /// Commit description text.
    pub description: String,
    /// Whether the revision currently has unresolved conflicts (`self.conflict()`).
    pub conflict: bool,
    /// Whether multiple visible commits share this change ID (`self.divergent()`).
    pub divergent: bool,
    /// Whether this commit is mutable and checked out by or an ancestor of another workspace's `@`.
    pub in_other_workspace: bool,
}

/// Interface for read-only Jujutsu repository queries.
pub trait JjClient {
    /// Evaluates `revset` in `repo_dir` with `--ignore-working-copy` and returns up to the 256 most
    /// recent matching commits.
    ///
    /// # Errors
    ///
    /// Returns an error if the VCS client fails to spawn `jj` or evaluate `revset`.
    fn query_revisions(&self, repo_dir: &Path, revset: &str) -> Result<Vec<CommitSummary>, String>;
}

/// Command-line client executing read-only `jj log` subprocesses.
#[derive(Debug, Clone, Copy, Default)]
pub struct JjCliClient;

impl JjClient for JjCliClient {
    fn query_revisions(&self, repo_dir: &Path, revset: &str) -> Result<Vec<CommitSummary>, String> {
        let mut command = Command::new("jj");
        let rev_arg = format!("-r={revset}");
        command.stdin(Stdio::null()).args([
            "log",
            "--no-graph",
            "--color=never",
            "--no-pager",
            "--ignore-working-copy",
            "--limit",
            MAX_QUERIED_REVISIONS,
            &rev_arg,
            "-T",
            COMMIT_SUMMARY_JSONL_TEMPLATE,
        ]);
        let existing_directory = existing_working_directory(repo_dir)
            .ok_or_else(|| format!("working directory does not exist: {}", repo_dir.display()))?;
        command.current_dir(existing_directory);

        let output = command.output().map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).to_string());
        }

        parse_commit_summaries_jsonl(&output.stdout)
    }
}

/// Per-command-lint memoization wrapper around [`JjClient`] deduplicating `(repo_dir, revset)`
/// queries across rules evaluated in the same run.
pub struct CachedJjSession<'a> {
    client: &'a dyn JjClient,
    cache: RefCell<RevisionQueryCache>,
}

impl<'a> CachedJjSession<'a> {
    /// Wraps `client` with an empty query cache.
    #[must_use]
    pub fn new(client: &'a dyn JjClient) -> Self {
        Self {
            client,
            cache: RefCell::new(HashMap::new()),
        }
    }

    /// Queries `(repo_dir, revset)`, returning a cached result if already evaluated in this session.
    ///
    /// # Errors
    ///
    /// Returns the underlying [`JjClient::query_revisions`] error on failure.
    pub fn query_revisions(&self, repo_dir: &Path, revset: &str) -> RevisionQueryResult {
        let key = (repo_dir.to_path_buf(), revset.to_owned());
        if let Some(cached) = self.cache.borrow().get(&key) {
            return cached.clone();
        }
        let fetched = self.client.query_revisions(repo_dir, revset);
        self.cache.borrow_mut().insert(key, fetched.clone());
        fetched
    }
}

fn existing_working_directory(directory: &Path) -> Option<&Path> {
    directory
        .ancestors()
        .find(|ancestor| !ancestor.as_os_str().is_empty() && ancestor.is_dir())
}

fn parse_commit_summaries_jsonl(stdout: &[u8]) -> Result<Vec<CommitSummary>, String> {
    let text = std::str::from_utf8(stdout).map_err(|error| error.to_string())?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<CommitSummary>(line).map_err(|error| error.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    struct CountingClient {
        calls: Cell<usize>,
    }

    impl JjClient for CountingClient {
        fn query_revisions(
            &self,
            repo_dir: &Path,
            revset: &str,
        ) -> Result<Vec<CommitSummary>, String> {
            self.calls.set(self.calls.get() + 1);
            if revset == "missing" {
                return Err("unknown revision".to_owned());
            }
            Ok(vec![CommitSummary {
                change_id: format!("{}:{revset}", repo_dir.display()),
                ..CommitSummary::default()
            }])
        }
    }

    #[rstest::rstest]
    #[case::empty_stdout(b"", vec![])]
    #[case::multiple_commits(
        indoc::indoc! {br#"
            {"change_id":"kpqxywon","description":"feat: title\n\nbody with \"quotes\"","conflict":false,"divergent":true,"in_other_workspace":false}
            {"change_id":"zyultkup","description":"","conflict":true,"divergent":false,"in_other_workspace":true}
        "#},
        vec![
            CommitSummary {
                change_id: "kpqxywon".to_owned(),
                description: indoc::indoc! {r#"
                    feat: title

                    body with "quotes""#}
                .to_owned(),
                conflict: false,
                divergent: true,
                in_other_workspace: false,
            },
            CommitSummary {
                change_id: "zyultkup".to_owned(),
                description: String::new(),
                conflict: true,
                divergent: false,
                in_other_workspace: true,
            },
        ]
    )]
    fn test_parse_commit_summaries_jsonl(
        #[case] raw_stdout: &[u8],
        #[case] expected: Vec<CommitSummary>,
    ) {
        let parsed = parse_commit_summaries_jsonl(raw_stdout).expect("valid JSONL");
        assert_eq!(parsed, expected);
    }

    #[rstest::rstest]
    #[case::malformed_json(b"{not-json}\n")]
    #[case::missing_required_field(br#"{"change_id":"kpqxywon"}"#)]
    #[case::unknown_field(br#"{"change_id":"kpqxywon","description":"","conflict":false,"divergent":false,"in_other_workspace":false,"extra":1}"#)]
    fn test_parse_commit_summaries_jsonl_rejects_invalid_records(#[case] raw_stdout: &[u8]) {
        assert!(parse_commit_summaries_jsonl(raw_stdout).is_err());
    }

    #[test]
    fn test_cached_jj_session_deduplicates_identical_repo_and_revset() {
        let backing = CountingClient {
            calls: Cell::new(0),
        };
        let session = CachedJjSession::new(&backing);

        let first = session.query_revisions(Path::new("/ws1"), "@").unwrap();
        let duplicate = session.query_revisions(Path::new("/ws1"), "@").unwrap();
        let other_repo = session.query_revisions(Path::new("/ws2"), "@").unwrap();
        let other_rev = session.query_revisions(Path::new("/ws1"), "@-").unwrap();

        assert_eq!(first, duplicate);
        assert_eq!(other_repo[0].change_id, "/ws2:@");
        assert_eq!(other_rev[0].change_id, "/ws1:@-");
        assert_eq!(backing.calls.get(), 3);
    }

    #[test]
    fn test_cached_jj_session_memoizes_query_errors() {
        let backing = CountingClient {
            calls: Cell::new(0),
        };
        let session = CachedJjSession::new(&backing);

        let first_error = session.query_revisions(Path::new("/ws1"), "missing");
        let second_error = session.query_revisions(Path::new("/ws1"), "missing");

        assert!(first_error.is_err());
        assert_eq!(first_error, second_error);
        assert_eq!(backing.calls.get(), 1);
    }

    #[test]
    fn test_jj_cli_client_queries_real_repo_and_ancestor_of_uncreated_subdir() {
        let temp_repo = init_temp_jj_repo();
        let nested_uncreated = temp_repo.path().join("uncreated_subdir");

        let direct_summaries = JjCliClient
            .query_revisions(temp_repo.path(), "@")
            .expect("repo root resolves");
        let nested_summaries = JjCliClient
            .query_revisions(&nested_uncreated, "@")
            .expect("ancestor repo directory resolves");

        assert_eq!(direct_summaries.len(), 1);
        assert_eq!(direct_summaries[0].change_id.len(), 8);
        assert_eq!(nested_summaries, direct_summaries);
    }

    #[test]
    fn test_jj_cli_client_returns_error_on_unknown_revision_or_missing_directory() {
        let temp_repo = init_temp_jj_repo();

        let bad_rev_result = JjCliClient.query_revisions(temp_repo.path(), "nonexistent_rev_xyz");
        let missing_dir_result =
            JjCliClient.query_revisions(Path::new("nonexistent_relative_root_xyz"), "@");
        let empty_dir_result = JjCliClient.query_revisions(Path::new(""), "@");

        assert!(bad_rev_result.is_err());
        assert!(missing_dir_result.is_err());
        assert!(empty_dir_result.is_err());
    }

    fn init_temp_jj_repo() -> tempfile::TempDir {
        let temp_repo = tempfile::tempdir().unwrap();
        let isolated_xdg = temp_repo.path().join(".isolated_xdg");
        let status = Command::new("jj")
            .args([
                "git",
                "init",
                "--config",
                "user.name=\"Test\"",
                "--config",
                "user.email=\"test@example.com\"",
            ])
            .env("XDG_CONFIG_HOME", &isolated_xdg)
            .stdin(Stdio::null())
            .current_dir(temp_repo.path())
            .output()
            .expect("jj git init")
            .status;
        assert!(status.success());
        temp_repo
    }
}
