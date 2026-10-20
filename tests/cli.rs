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

    // Initialize temporary repository with test user identity so the initial working-copy
    // commit is authored by the test user before repo config is written.
    let init_output = std::process::Command::new("jj")
        .args([
            "--config",
            "user.name=\"Test User\"",
            "--config",
            "user.email=\"test@example.com\"",
            "git",
            "init",
        ])
        .current_dir(temp.path())
        .output()
        .expect("failed to run jj git init");
    assert!(init_output.status.success(), "jj git init failed");

    for (key, value) in [
        ("user.name", "Test User"),
        ("user.email", "test@example.com"),
    ] {
        let output = std::process::Command::new("jj")
            .args(["config", "set", "--repo", key, value])
            .current_dir(temp.path())
            .output()
            .expect("failed to set jj repo config");
        assert!(output.status.success());
    }

    temp
}

const BIN_NAME: &str = "omnicoach";

fn run_cli(args: &[&str], current_dir: Option<&std::path::Path>) -> String {
    let mut cmd = Command::cargo_bin(BIN_NAME).unwrap();
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
        &format!("{BIN_NAME}{}", std::env::consts::EXE_SUFFIX),
        BIN_NAME,
    )
}

#[test]
fn test_code_lint_help_flag() {
    let output = run_cli(&["--help"], None);
    insta::assert_snapshot!(output);
}

#[test]
fn test_code_lint_no_violations() {
    let clean_code = indoc! {r"
        def calculate_sum(num1, num2):
            return num1 + num2
    "};
    let temp_dir = create_temp_file("test.py", clean_code);
    let output = run_cli(&["code", "test.py"], Some(temp_dir.path()));
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
    let output = run_cli(&["code", "test.py"], Some(temp_dir.path()));
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
    let output = run_cli(&[], Some(temp_dir.path()));
    insta::assert_snapshot!(output);
}

#[rstest::rstest]
#[case::list_rules(&["--list-rules"], "list_rules")]
#[case::list_tags(&["--list-tags"], "list_tags")]
#[case::tag_typo(&["--list-rules", "--tag", "tesing"], "tag_typo")]
#[case::tag_without_list(&["--tag", "testing"], "tag_without_list")]
#[case::explain(&["--explain", "sleep-in-tests"], "explain_sleep_in_tests")]
#[case::explain_typo(&["command", "--explain", "sleep-in-test"], "explain_typo")]
#[case::format_typo(&["code", "--format", "jsno", "."], "format_typo")]
#[case::command_lint_needs_cmd(&["command"], "command_lint_needs_cmd")]
fn test_discovery_and_cli_errors(#[case] args: &[&str], #[case] snapshot: &str) {
    let output = run_cli(args, None);
    insta::assert_snapshot!(snapshot, output.replace(BIN_NAME, "<binary>"));
}

/// Top-level and subcommand discovery flags serve the same rule catalog.
#[rstest::rstest]
#[case::list_rules(&["--list-rules"])]
#[case::list_tags(&["--list-tags"])]
#[case::explain(&["--explain", "edit-of-described-commit"])]
fn test_discovery_is_identical_across_subcommands(#[case] args: &[&str]) {
    let top_level = run_cli(args, None);
    let code_args: Vec<&str> = std::iter::once("code")
        .chain(args.iter().copied())
        .collect();
    let command_args: Vec<&str> = std::iter::once("command")
        .chain(args.iter().copied())
        .collect();
    let code_lint = run_cli(&code_args, None);
    let command_lint = run_cli(&command_args, None);
    assert!(top_level.contains("--- exit code ---\n0\n"), "{top_level}");
    assert_eq!(top_level, code_lint);
    assert_eq!(top_level, command_lint);
}

#[test]
fn test_command_lint_described_commit_blocked() {
    let temp_repo = setup_temp_jj_repo();

    let describe_output = std::process::Command::new("jj")
        .args(["describe", "-m", "wip commit"])
        .current_dir(temp_repo.path())
        .output()
        .expect("failed to describe commit");
    let output = run_cli(&["command", "--cmd", "jj edit @"], Some(temp_repo.path()));

    assert!(describe_output.status.success());
    insta::assert_snapshot!(output);
}

#[test]
fn test_command_lint_empty_commit_allowed() {
    let temp_repo = setup_temp_jj_repo();
    let output = run_cli(&["command", "--cmd", "jj edit @"], Some(temp_repo.path()));
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
    let new_output = std::process::Command::new("jj")
        .arg("new")
        .current_dir(repo_path)
        .output()
        .expect("failed to run jj new");

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
    let output_full = run_cli(&["code", "test.py"], Some(repo_path));

    // 5. Run with --diff-rev @- (compares @ relative to parent @-)
    // It should ONLY report `world` because `inner` was already in the parent commit!
    let output_diff = run_cli(&["code", "--diff-rev", "@-", "test.py"], Some(repo_path));

    assert!(new_output.status.success());
    insta::assert_snapshot!("full_baseline", output_full);
    insta::assert_snapshot!("diff_only", output_diff);
}

#[test]
fn test_code_lint_non_existent_file_fails() {
    let output = run_cli(&["code", "non_existent_file.rs"], None);
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

    let output = run_cli(&["code", "latin1.py", "nested.py"], Some(temp_dir.path()));

    assert!(output.contains("exit code ---\n2"), "{output}");
    assert!(output.contains("nested.py:2:"), "{output}");
    assert!(
        output.contains("Error: Failed to read file 'latin1.py'"),
        "{output}"
    );
}

#[rstest::rstest]
#[case::complex_revset("immutable()..")]
#[case::range_revset("@-..@")]
fn test_code_lint_diff_revset_jj(#[case] revset: &str) {
    let temp_repo = setup_temp_jj_repo();
    let repo_path = temp_repo.path();

    let violating_code = indoc! {r"
        def outer():
            def inner():
                pass
    "};
    let file_path = repo_path.join("test.py");
    fs::write(&file_path, violating_code).unwrap();

    let output = run_cli(&["code", "--diff-rev", revset, "test.py"], Some(repo_path));
    assert!(output.contains("Function `inner` is defined inside another function."));
}

#[test]
fn test_self_dogfooding_code_lint() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::cargo_bin(BIN_NAME)
        .unwrap()
        .args(["code", "."])
        .current_dir(&manifest_dir)
        .output()
        .expect("failed to execute omnicoach on workspace");

    assert!(
        output.status.success(),
        "omnicoach reported violations on its own repository:\n{}{}",
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
        &["code", "--format", "json", "test.py"],
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
        &["code", "--format", format, "test.py"],
        Some(temp_dir.path()),
    );
    insta::assert_snapshot!(format!("explanation_hint_{format}"), output);
}

#[test]
fn test_cli_config_override_and_missing_file_error() {
    let temp_dir = create_temp_file("test.py", "def outer():\n    def inner():\n        pass\n");
    fs::write(
        temp_dir.path().join("custom.toml"),
        "ignore = [\"nested-function\"]\n",
    )
    .unwrap();
    let custom_output = run_cli(
        &["--config", "custom.toml", "code", "test.py"],
        Some(temp_dir.path()),
    );
    let missing_output = run_cli(
        &["--config", "missing.toml", "code", "test.py"],
        Some(temp_dir.path()),
    );

    assert!(
        custom_output.contains("--- exit code ---\n0\n"),
        "{custom_output}"
    );
    assert!(
        custom_output.contains("All checks passed!"),
        "{custom_output}"
    );
    assert!(
        missing_output.contains("--- exit code ---\n2\n"),
        "{missing_output}"
    );
    assert!(
        missing_output.contains("failed to read `missing.toml`"),
        "{missing_output}"
    );
}

#[test]
fn test_cli_select_and_ignore_override_toml_and_explain() {
    let temp_dir = create_temp_file("test.py", "def outer():\n    def inner():\n        pass\n");
    fs::write(
        temp_dir.path().join(omni::config::CONFIG_FILE_NAME),
        "select = [\"nested-function\"]\n",
    )
    .unwrap();
    let ignored_output = run_cli(
        &["code", "--ignore", "nested-function", "test.py"],
        Some(temp_dir.path()),
    );
    let selected_other_output = run_cli(
        &["code", "--select", "testing", "test.py"],
        Some(temp_dir.path()),
    );
    let explain_output = run_cli(
        &[
            "--select",
            "testing",
            "--ignore",
            "sleep-in-tests",
            "--explain",
            "sleep-in-tests",
        ],
        Some(temp_dir.path()),
    );

    assert!(
        ignored_output.contains("--- exit code ---\n0\n"),
        "{ignored_output}"
    );
    assert!(
        selected_other_output.contains("--- exit code ---\n0\n"),
        "{selected_other_output}"
    );
    assert!(
        explain_output.contains("Status: disabled by `ignore = [\"sleep-in-tests\"]`"),
        "{explain_output}"
    );
}
