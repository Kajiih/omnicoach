#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used)]

use assert_cmd::Command;
use indoc::indoc;
use std::fs;
use std::io::Write;

fn create_temp_file(suffix: &str, content: &str) -> tempfile::NamedTempFile {
    let mut file = tempfile::Builder::new()
        .prefix("omni_test_")
        .suffix(suffix)
        .tempfile()
        .unwrap();
    write!(file, "{content}").unwrap();
    file
}

fn setup_temp_jj_repo() -> tempfile::TempDir {
    // Ensure jj binary is installed and executable
    let jj_check = std::process::Command::new("jj")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    if jj_check.is_err() || !jj_check.unwrap().success() {
        panic!(
            "Error: Jujutsu CLI ('jj') is not installed or not in PATH.\n\
             Please install Jujutsu to run VCS-related integration tests."
        );
    }

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

fn run_and_sanitize_cli(
    bin_name: &str,
    args: &[&str],
    current_dir: Option<&std::path::Path>,
    temp_paths: &[&std::path::Path],
) -> String {
    let mut cmd = Command::cargo_bin(bin_name).unwrap();
    cmd.args(args);
    if let Some(dir) = current_dir {
        cmd.current_dir(dir);
    }
    let output = cmd.output().unwrap();
    let status = output.status;
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    let mut combined = format!(
        "--- exit code ---\n{}\n--- stdout ---\n{}\n--- stderr ---\n{}",
        status.code().unwrap_or(-1),
        stdout,
        stderr
    );

    // Sanitize any dynamic temporary paths
    for path in temp_paths {
        combined = combined.replace(&*path.to_string_lossy(), "[TEMP_PATH]");
    }

    // Normalize windows CRLF to LF
    combined.replace("\r\n", "\n")
}

#[test]
fn test_code_lint_help_flag() {
    let output = run_and_sanitize_cli("omni-code-lint", &["--help"], None, &[]);
    insta::assert_snapshot!(output);
}

#[test]
fn test_code_lint_no_violations() {
    let clean_code = indoc! {r"
        def calculate_sum(num1, num2):
            return num1 + num2
    "};
    let temp_file = create_temp_file(".py", clean_code);
    let output = run_and_sanitize_cli(
        "omni-code-lint",
        &[temp_file.path().to_str().unwrap()],
        None,
        &[temp_file.path()],
    );
    insta::assert_snapshot!(output);
}

#[test]
fn test_code_lint_with_violations() {
    let violating_code = indoc! {r"
        def outer():
            def inner():
                pass
    "};
    let temp_file = create_temp_file(".py", violating_code);
    let output = run_and_sanitize_cli(
        "omni-code-lint",
        &[temp_file.path().to_str().unwrap()],
        None,
        &[temp_file.path()],
    );
    insta::assert_snapshot!(output);
}

#[test]
fn test_code_lint_invalid_config() {
    let temp_dir = tempfile::tempdir().unwrap();
    fs::write(
        temp_dir.path().join(omni::core::CONFIG_FILE_NAME),
        "select = [invalid syntax]",
    )
    .unwrap();
    let output = run_and_sanitize_cli(
        "omni-code-lint",
        &[],
        Some(temp_dir.path()),
        &[temp_dir.path()],
    );
    insta::assert_snapshot!(output);
}

#[test]
fn test_command_lint_help_flag() {
    let output = run_and_sanitize_cli("omni-command-lint", &["--help"], None, &[]);
    insta::assert_snapshot!(output);
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

    let output = run_and_sanitize_cli(
        "omni-command-lint",
        &["--cmd", "jj edit @"],
        Some(temp_repo.path()),
        &[temp_repo.path()],
    );
    insta::assert_snapshot!(output);
}

#[test]
fn test_command_lint_empty_commit_allowed() {
    let temp_repo = setup_temp_jj_repo();
    let output = run_and_sanitize_cli(
        "omni-command-lint",
        &["--cmd", "jj edit @"],
        Some(temp_repo.path()),
        &[temp_repo.path()],
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
    let output_full = run_and_sanitize_cli(
        "omni-code-lint",
        &["test.py"],
        Some(repo_path),
        &[repo_path],
    );
    assert!(output_full.contains("Nested function definition `inner`"));
    assert!(output_full.contains("Nested function definition `world`"));

    // 5. Run with --diff-rev @- (compares @ relative to parent @-)
    // It should ONLY report `world` because `inner` was already in the parent commit!
    let output_diff = run_and_sanitize_cli(
        "omni-code-lint",
        &["--diff-rev", "@-", "test.py"],
        Some(repo_path),
        &[repo_path],
    );
    assert!(!output_diff.contains("Nested function definition `inner`"));
    assert!(output_diff.contains("Nested function definition `world`"));
}

#[test]
fn test_code_lint_non_existent_file_fails() {
    let output = run_and_sanitize_cli("omni-code-lint", &["non_existent_file.rs"], None, &[]);
    assert!(output.contains("Path does not exist: non_existent_file.rs"));
    assert!(output.contains("exit code ---\n2"));
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

    let output = run_and_sanitize_cli(
        "omni-code-lint",
        &["--diff-rev", "immutable()..", "test.py"],
        Some(repo_path),
        &[repo_path],
    );
    assert!(output.contains("Nested function definition `inner`"));
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

    let output = run_and_sanitize_cli(
        "omni-code-lint",
        &["--diff-rev", "@-..@", "test.py"],
        Some(repo_path),
        &[repo_path],
    );
    assert!(output.contains("Nested function definition `inner`"));
}

