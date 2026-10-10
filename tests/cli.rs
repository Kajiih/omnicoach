//! End-to-end command-line interface integration tests.

// Workaround for rust-lang/rust-clippy#13981 so clippy.toml `allow-*-in-tests` applies to the whole file.
#![cfg(test)]

use assert_cmd::Command;
use indoc::indoc;
use std::fs;

fn create_temp_file(file_name: &str, content: &str) -> tempfile::TempDir {
    let temp_dir = tempfile::tempdir().unwrap();
    fs::write(temp_dir.path().join(file_name), content).unwrap();
    temp_dir
}

fn setup_temp_jj_repo() -> tempfile::TempDir {
    // Ensure jj binary is installed and executable
    let jj_check = std::process::Command::new("jj")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    assert!(
        matches!(jj_check, Ok(status) if status.success()),
        "Error: Jujutsu CLI ('jj') is not installed or not in PATH.\n\
         Please install Jujutsu to run VCS-related integration tests."
    );

    let temp = tempfile::tempdir().unwrap();

    // Initialize temporary repository
    let status = std::process::Command::new("jj")
        .arg("git")
        .arg("init")
        .current_dir(temp.path())
        .status()
        .expect("failed to run jj git init");
    assert!(status.success(), "jj git init failed");

    // Set configuration for user identity to prevent warnings/failures
    let status = std::process::Command::new("jj")
        .arg("config")
        .arg("set")
        .arg("--repo")
        .arg("user.name")
        .arg("Test User")
        .current_dir(temp.path())
        .status()
        .expect("failed to set jj user.name");
    assert!(status.success());

    let status = std::process::Command::new("jj")
        .arg("config")
        .arg("set")
        .arg("--repo")
        .arg("user.email")
        .arg("test@example.com")
        .current_dir(temp.path())
        .status()
        .expect("failed to set jj user.email");
    assert!(status.success());

    temp
}

fn run_cli(bin_name: &str, args: &[&str], current_dir: Option<&std::path::Path>) -> String {
    let mut cmd = Command::cargo_bin(bin_name).unwrap();
    cmd.args(args);
    if let Some(dir) = current_dir {
        cmd.current_dir(dir);
    }
    let output = cmd.output().unwrap();
    let status = output.status;
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    let combined = format!(
        "--- exit code ---\n{}\n--- stdout ---\n{}\n--- stderr ---\n{}",
        status.code().unwrap_or(-1),
        stdout,
        stderr
    );

    // Normalize Windows CRLF line endings and .exe binary suffix in Usage output
    combined.replace("\r\n", "\n").replace(
        &format!("{bin_name}{}", std::env::consts::EXE_SUFFIX),
        bin_name,
    )
}

#[test]
fn test_code_lint_help_flag() {
    let output = run_cli("omni-code-lint", &["--help"], None);
    insta::assert_snapshot!(output);
}

#[test]
fn test_code_lint_no_violations() {
    let clean_code = indoc! {r"
        def calculate_sum(num1, num2):
            return num1 + num2
    "};
    let temp_dir = create_temp_file("test.py", clean_code);
    let output = run_cli("omni-code-lint", &["test.py"], Some(temp_dir.path()));
    insta::assert_snapshot!(output);
}

#[test]
fn test_code_lint_with_violations() {
    let violating_code = indoc! {r"
        def outer():
            def inner():
                pass
    "};
    let temp_dir = create_temp_file("test.py", violating_code);
    let output = run_cli("omni-code-lint", &["test.py"], Some(temp_dir.path()));
    insta::assert_snapshot!(output);
}

#[test]
fn test_code_lint_invalid_config() {
    let temp_dir = tempfile::tempdir().unwrap();
    fs::write(
        temp_dir.path().join(omni::config::CONFIG_FILE_NAME),
        "select = [invalid syntax]",
    )
    .unwrap();
    let output = run_cli("omni-code-lint", &[], Some(temp_dir.path()));
    insta::assert_snapshot!(output);
}

#[rstest::rstest]
#[case::list_rules("omni-code-lint", &["--list-rules"], "list_rules")]
#[case::list_tags("omni-code-lint", &["--list-tags"], "list_tags")]
#[case::tag_typo("omni-code-lint", &["--list-rules", "--tag", "tesing"], "tag_typo")]
#[case::tag_without_list("omni-code-lint", &["--tag", "testing"], "tag_without_list")]
#[case::explain("omni-code-lint", &["--explain", "sleep-in-tests"], "explain_sleep_in_tests")]
#[case::explain_typo("omni-command-lint", &["--explain", "sleep-in-test"], "explain_typo")]
#[case::format_typo("omni-code-lint", &["--format", "jsno", "."], "format_typo")]
#[case::command_lint_needs_cmd("omni-command-lint", &[], "command_lint_needs_cmd")]
fn test_discovery_and_cli_errors(
    #[case] binary: &str,
    #[case] args: &[&str],
    #[case] snapshot: &str,
) {
    let output = run_cli(binary, args, None);
    insta::assert_snapshot!(snapshot, output.replace(binary, "<binary>"));
}

/// Both binaries serve the same rule catalog.
#[rstest::rstest]
#[case::list_rules(&["--list-rules"])]
#[case::list_tags(&["--list-tags"])]
#[case::explain(&["--explain", "edit-of-described-commit"])]
fn test_discovery_is_identical_across_binaries(#[case] args: &[&str]) {
    let code_lint = run_cli("omni-code-lint", args, None);
    let command_lint = run_cli("omni-command-lint", args, None);
    assert!(code_lint.contains("--- exit code ---\n0\n"), "{code_lint}");
    assert_eq!(
        code_lint.replace("omni-code-lint", "<binary>"),
        command_lint.replace("omni-command-lint", "<binary>")
    );
}

#[test]
fn test_command_lint_described_commit_blocked() {
    let temp_repo = setup_temp_jj_repo();

    let status = std::process::Command::new("jj")
        .arg("describe")
        .arg("-m")
        .arg("wip commit")
        .current_dir(temp_repo.path())
        .status()
        .expect("failed to describe commit");
    assert!(status.success());

    let output = run_cli(
        "omni-command-lint",
        &["--cmd", "jj edit @"],
        Some(temp_repo.path()),
    );
    insta::assert_snapshot!(output);
}

#[test]
fn test_command_lint_empty_commit_allowed() {
    let temp_repo = setup_temp_jj_repo();
    let output = run_cli(
        "omni-command-lint",
        &["--cmd", "jj edit @"],
        Some(temp_repo.path()),
    );
    insta::assert_snapshot!(output);
}

#[test]
fn test_code_lint_diff_flag_jj() {
    let temp_repo = setup_temp_jj_repo();
    let repo_path = temp_repo.path();

    // 1. Create a file with a violation
    let violating_code_1 = indoc! {r"
        def outer():
            def inner():
                pass
    "};
    let file_path = repo_path.join("test.py");
    fs::write(&file_path, violating_code_1).unwrap();

    // 2. Commit it to draft state by creating a new change on top of it
    let status = std::process::Command::new("jj")
        .arg("new")
        .current_dir(repo_path)
        .status()
        .expect("failed to run jj new");
    assert!(status.success());

    // 3. Now the first violation is in parent `@-`.
    // Let's add a second violation in the working copy `@`
    let violating_code_2 = indoc! {r"
        def outer():
            def inner():
                pass
        def hello():
            def world():
                pass
    "};
    fs::write(&file_path, violating_code_2).unwrap();

    // 4. Run full linter (should report BOTH violations: `inner` and `world`)
    let output_full = run_cli("omni-code-lint", &["test.py"], Some(repo_path));
    insta::assert_snapshot!("full_baseline", output_full);

    // 5. Run with --diff-rev @- (compares @ relative to parent @-)
    // It should ONLY report `world` because `inner` was already in the parent commit!
    let output_diff = run_cli(
        "omni-code-lint",
        &["--diff-rev", "@-", "test.py"],
        Some(repo_path),
    );
    insta::assert_snapshot!("diff_only", output_diff);
}

#[test]
fn test_code_lint_non_existent_file_fails() {
    let output = run_cli("omni-code-lint", &["non_existent_file.rs"], None);
    assert!(output.contains("Path does not exist: non_existent_file.rs"));
    assert!(output.contains("exit code ---\n2"));
}

#[test]
fn test_code_lint_unreadable_file_reports_error_and_lints_the_rest() {
    let temp_dir = tempfile::tempdir().unwrap();
    fs::write(temp_dir.path().join("latin1.py"), b"name = '\xe9'\n").unwrap();
    fs::write(
        temp_dir.path().join("nested.py"),
        "def outer():\n    def inner():\n        pass\n",
    )
    .unwrap();

    let output = run_cli(
        "omni-code-lint",
        &["latin1.py", "nested.py"],
        Some(temp_dir.path()),
    );

    assert!(output.contains("exit code ---\n2"), "{output}");
    assert!(output.contains("nested.py:2:"), "{output}");
    assert!(
        output.contains("Error: Failed to read file 'latin1.py'"),
        "{output}"
    );
}

#[test]
fn test_code_lint_diff_complex_revset_jj() {
    let temp_repo = setup_temp_jj_repo();
    let repo_path = temp_repo.path();

    let violating_code = indoc! {r"
        def outer():
            def inner():
                pass
    "};
    let file_path = repo_path.join("test.py");
    fs::write(&file_path, violating_code).unwrap();

    let output = run_cli(
        "omni-code-lint",
        &["--diff-rev", "immutable()..", "test.py"],
        Some(repo_path),
    );
    assert!(output.contains("Function `inner` is defined inside another function."));
}

#[test]
fn test_code_lint_diff_range_revset_jj() {
    let temp_repo = setup_temp_jj_repo();
    let repo_path = temp_repo.path();

    let violating_code = indoc! {r"
        def outer():
            def inner():
                pass
    "};
    let file_path = repo_path.join("test.py");
    fs::write(&file_path, violating_code).unwrap();

    let output = run_cli(
        "omni-code-lint",
        &["--diff-rev", "@-..@", "test.py"],
        Some(repo_path),
    );
    assert!(output.contains("Function `inner` is defined inside another function."));
}

#[test]
fn test_self_dogfooding_code_lint() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::cargo_bin("omni-code-lint")
        .unwrap()
        .args(["src", "tests"])
        .current_dir(&manifest_dir)
        .output()
        .expect("failed to execute omni-code-lint on workspace");

    assert!(
        output.status.success(),
        "omni-code-lint reported violations on its own repository:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_code_lint_json_format_deterministic() {
    let violating_code = indoc! {r"
        def outer():
            def inner():
                pass
    "};
    let temp_dir = create_temp_file("test.py", violating_code);
    let output = run_cli(
        "omni-code-lint",
        &["--format", "json", "test.py"],
        Some(temp_dir.path()),
    );
    insta::assert_snapshot!(output);
}

/// `suppressed-exception` defaults to `require-explanation` and `nested-function` to `ban`, so
/// only the `suppressed-exception` finding carries the explanation hint.
const EXPLANATION_HINT_CODE: &str = indoc! {r"
    import contextlib

    def outer():
        def inner():
            pass
        with contextlib.suppress(OSError):
            inner()
"};

#[rstest::rstest]
#[case::plain("plain")]
#[case::json("json")]
fn test_code_lint_explanation_hint(#[case] format: &str) {
    let temp_dir = create_temp_file("test.py", EXPLANATION_HINT_CODE);
    let output = run_cli(
        "omni-code-lint",
        &["--format", format, "test.py"],
        Some(temp_dir.path()),
    );
    insta::assert_snapshot!(format!("explanation_hint_{format}"), output);
}
