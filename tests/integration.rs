//! Integration tests for cc-statusline
//!
//! These tests use real temporary directories and git repositories to test
//! the git detection and caching functionality.

use std::env;
use std::fs;
use std::io::Write as IoWrite;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use tempfile::TempDir;

/// Get the path to the built binary
fn get_binary_path() -> PathBuf {
    // The binary is in target/debug/cc-statusline (or release)
    let mut path = env::current_exe().expect("failed to get current exe path");
    // Go up from deps directory to target/debug
    path.pop(); // Remove test binary name
    path.pop(); // Remove deps
    path.push("cc-statusline");

    // On Windows, add .exe extension
    #[cfg(windows)]
    path.set_extension("exe");

    path
}

/// Helper to create a git repository in a temp directory
fn create_git_repo() -> (TempDir, PathBuf) {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let repo_path = temp_dir.path().to_path_buf();

    // Initialize git repo
    Command::new("git")
        .args(["init"])
        .current_dir(&repo_path)
        .output()
        .expect("failed to init git repo");

    // Configure user for commits
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(&repo_path)
        .output()
        .expect("failed to config email");

    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(&repo_path)
        .output()
        .expect("failed to config name");

    (temp_dir, repo_path)
}

/// Helper to make a commit in the repo
fn make_commit(repo_path: &PathBuf, message: &str) {
    // Create a file to commit
    let file_path = repo_path.join(format!("file-{}.txt", message.replace(' ', "-")));
    fs::write(&file_path, message).expect("failed to write file");

    Command::new("git")
        .args(["add", "."])
        .current_dir(repo_path)
        .output()
        .expect("failed to git add");

    Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(repo_path)
        .output()
        .expect("failed to commit");
}

/// Run the binary with JSON input and return stdout
fn run_with_json(work_dir: &PathBuf, json_input: &str) -> String {
    run_with_json_env(work_dir, json_input, &[])
}

/// Run the binary with JSON input, extra env vars, and optional env removals; return stdout
fn run_with_json_env(work_dir: &PathBuf, json_input: &str, env_vars: &[(&str, &str)]) -> String {
    run_with_json_env_full(work_dir, json_input, env_vars, &[])
}

fn run_with_json_env_full(
    work_dir: &PathBuf,
    json_input: &str,
    env_vars: &[(&str, &str)],
    env_remove: &[&str],
) -> String {
    let binary = get_binary_path();

    let mut cmd = Command::new(&binary);
    cmd.current_dir(work_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    for &(key, val) in env_vars {
        cmd.env(key, val);
    }
    for &key in env_remove {
        cmd.env_remove(key);
    }

    let mut child = cmd.spawn().expect("failed to spawn binary");

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(json_input.as_bytes())
        .expect("failed to write stdin");

    let output = child.wait_with_output().expect("failed to wait");
    String::from_utf8_lossy(&output.stdout).to_string()
}

// =============================================================================
// Git Detection Tests
// =============================================================================

#[test]
fn detect_git_repo() {
    let (_temp_dir, repo_path) = create_git_repo();
    make_commit(&repo_path, "initial commit");

    let stdout = run_with_json(&repo_path, "{}");

    // Should contain branch name (main or master depending on git config)
    assert!(
        stdout.contains("main") || stdout.contains("master"),
        "Expected branch name in output: {}",
        stdout
    );
}

#[test]
fn detect_branch_name() {
    let (_temp_dir, repo_path) = create_git_repo();
    make_commit(&repo_path, "initial commit");

    // Create and switch to a feature branch
    Command::new("git")
        .args(["checkout", "-b", "feature-test"])
        .current_dir(&repo_path)
        .output()
        .expect("failed to create branch");

    let stdout = run_with_json(&repo_path, "{}");

    assert!(
        stdout.contains("feature-test"),
        "Expected 'feature-test' branch in output: {}",
        stdout
    );
}

#[test]
fn non_git_dir_shows_no_git() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let path = temp_dir.path().to_path_buf();

    let stdout = run_with_json(&path, "{}");

    assert!(
        stdout.contains("no git"),
        "Expected 'no git' in output: {}",
        stdout
    );
}

#[test]
fn detect_changed_files() {
    let (_temp_dir, repo_path) = create_git_repo();
    make_commit(&repo_path, "initial commit");

    // Modify a tracked file
    let file_path = repo_path.join("file-initial-commit.txt");
    fs::write(&file_path, "modified content").expect("failed to modify file");

    let stdout = run_with_json(&repo_path, "{}");

    // Should show file count
    assert!(
        stdout.contains("file") || stdout.contains("1"),
        "Expected changed file indicator in output: {}",
        stdout
    );
}

// =============================================================================
// JSON Input Tests
// =============================================================================

#[test]
fn json_input_model_name() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let path = temp_dir.path().to_path_buf();

    let stdout = run_with_json(&path, r#"{"model": {"display_name": "Claude Opus 4.5"}}"#);

    assert!(
        stdout.contains("Claude Opus 4.5"),
        "Expected model name in output: {}",
        stdout
    );
}

#[test]
fn json_input_git_branch() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let path = temp_dir.path().to_path_buf();

    let stdout = run_with_json(&path, r#"{"git": {"branch": "my-feature-branch"}}"#);

    assert!(
        stdout.contains("my-feature-branch"),
        "Expected branch name from JSON in output: {}",
        stdout
    );
}

#[test]
fn json_input_pr_info() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let path = temp_dir.path().to_path_buf();

    let json_input = r#"{
        "pr": {
            "number": 42,
            "state": "open",
            "url": "https://github.com/owner/repo/pull/42",
            "comments": 3,
            "check_status": "passed"
        }
    }"#;

    let stdout = run_with_json(&path, json_input);

    assert!(
        stdout.contains("#42"),
        "Expected PR number in output: {}",
        stdout
    );
    assert!(
        stdout.contains("open"),
        "Expected PR state in output: {}",
        stdout
    );
}

#[test]
fn json_input_context_percentage() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let path = temp_dir.path().to_path_buf();

    let stdout = run_with_json(
        &path,
        r#"{"context_window": {"remaining_percentage": 75.5}}"#,
    );

    assert!(
        stdout.contains("75%"),
        "Expected context percentage in output: {}",
        stdout
    );
}

#[test]
fn json_input_token_counts() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let path = temp_dir.path().to_path_buf();

    let json_input = r#"{
        "context_window": {
            "total_input_tokens": 50000,
            "total_output_tokens": 2500000
        }
    }"#;

    let stdout = run_with_json(&path, json_input);

    assert!(
        stdout.contains("50K"),
        "Expected input tokens in output: {}",
        stdout
    );
    assert!(
        stdout.contains("2.5M"),
        "Expected output tokens in output: {}",
        stdout
    );
}

// =============================================================================
// Worktree Tests
// =============================================================================

#[test]
fn detect_worktree() {
    let (_temp_dir, repo_path) = create_git_repo();
    make_commit(&repo_path, "initial commit");

    // Create a worktree
    let worktree_path = repo_path.parent().unwrap().join("worktree-test");
    let worktree_result = Command::new("git")
        .args([
            "worktree",
            "add",
            worktree_path.to_str().unwrap(),
            "-b",
            "wt-branch",
        ])
        .current_dir(&repo_path)
        .output();

    // Skip test if worktree creation fails (some CI environments may not support it)
    if worktree_result.is_err() || !worktree_result.as_ref().unwrap().status.success() {
        eprintln!("Skipping worktree test: worktree creation not supported");
        return;
    }

    let stdout = run_with_json(&worktree_path, "{}");

    // Should show worktree name
    assert!(
        stdout.contains("worktree-test"),
        "Expected worktree name in output: {}",
        stdout
    );
}

// =============================================================================
// Empty Input Tests
// =============================================================================

#[test]
fn empty_input_produces_output() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let path = temp_dir.path().to_path_buf();

    let stdout = run_with_json(&path, "");

    // Should produce some output (at least the path line)
    assert!(
        !stdout.is_empty(),
        "Expected some output even with empty input"
    );
}

#[test]
fn invalid_json_produces_output() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let path = temp_dir.path().to_path_buf();

    let stdout = run_with_json(&path, "{invalid json}");

    // Should handle gracefully and produce output
    assert!(
        !stdout.is_empty(),
        "Expected some output even with invalid JSON"
    );
}

// =============================================================================
// cwd Field Tests
// =============================================================================

#[test]
fn cwd_takes_priority_over_workspace_current_dir() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let path = temp_dir.path().to_path_buf();

    let json_input = r#"{
        "cwd": "/tmp/cwd-wins",
        "workspace": {
            "current_dir": "/tmp/workspace-loses",
            "project_dir": "/tmp/project-loses"
        }
    }"#;

    let stdout = run_with_json(&path, json_input);

    assert!(
        stdout.contains("cwd-wins"),
        "Expected cwd path in output (cwd should take priority): {}",
        stdout
    );
    assert!(
        !stdout.contains("workspace-loses"),
        "workspace.current_dir should NOT appear when cwd is set: {}",
        stdout
    );
}

// =============================================================================
// Output Style Tests
// =============================================================================

#[test]
fn json_input_output_style() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let path = temp_dir.path().to_path_buf();

    let stdout = run_with_json(&path, r#"{"output_style": {"name": "verbose"}}"#);

    assert!(
        stdout.contains("verbose"),
        "Expected output style in output: {}",
        stdout
    );
}

#[test]
fn json_input_duration() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let path = temp_dir.path().to_path_buf();

    let stdout = run_with_json(&path, r#"{"cost": {"total_duration_ms": 125000}}"#);

    // 125000ms = 2m 5s, should show as "2m"
    assert!(
        stdout.contains("2m"),
        "Expected duration in output: {}",
        stdout
    );
}

// =============================================================================
// SSH Hostname Detection Tests
// =============================================================================

#[test]
fn ssh_session_shows_hostname() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let path = temp_dir.path().to_path_buf();

    let stdout = run_with_json_env(
        &path,
        r#"{"workspace": {"project_dir": "/tmp/myproject"}}"#,
        &[("SSH_CONNECTION", "192.168.1.1 22 192.168.1.2 22")],
    );

    // The hostname should appear in the output (exact value depends on the machine)
    // Row 1 should have at least 3 separator-delimited segments: hostname • project • path
    let first_line = stdout.lines().next().unwrap_or("");
    // Count visible " • " separators (the separator includes ANSI codes, but the text " • " is present)
    let sep_count = first_line.matches(" • ").count();
    assert!(
        sep_count >= 2,
        "Expected at least 2 separators in SSH row1 (hostname • project • path), got {}: {}",
        sep_count,
        first_line
    );
}

#[test]
fn ssh_client_env_shows_hostname() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let path = temp_dir.path().to_path_buf();

    let stdout = run_with_json_env(
        &path,
        r#"{"workspace": {"project_dir": "/tmp/myproject"}}"#,
        &[("SSH_CLIENT", "192.168.1.1 12345 22")],
    );

    let first_line = stdout.lines().next().unwrap_or("");
    let sep_count = first_line.matches(" • ").count();
    assert!(
        sep_count >= 2,
        "Expected at least 2 separators with SSH_CLIENT set, got {}: {}",
        sep_count,
        first_line
    );
}

#[test]
fn no_ssh_env_no_hostname() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let path = temp_dir.path().to_path_buf();

    // Explicitly remove SSH env vars (test runner might itself be in an SSH session)
    let stdout = run_with_json_env_full(
        &path,
        r#"{"workspace": {"project_dir": "/tmp/myproject"}}"#,
        &[],
        &["SSH_CONNECTION", "SSH_CLIENT"],
    );

    let first_line = stdout.lines().next().unwrap_or("");
    // Without SSH, row1 should have exactly 1 separator: project • path
    let sep_count = first_line.matches(" • ").count();
    assert!(
        sep_count == 1,
        "Expected exactly 1 separator without SSH (project • path), got {}: {}",
        sep_count,
        first_line
    );
}

// =============================================================================
// Official JSON Fixture Test (Issue #20)
// =============================================================================

#[test]
fn official_docs_json_fixture() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let path = temp_dir.path().to_path_buf();

    // Load the full official JSON from https://code.claude.com/docs/en/statusline
    // Contains fields not used by ClaudeInput (hook_event_name, session_id,
    // version, model.id, context_window.current_usage). Serde ignores unknown
    // fields by default, so this confirms the binary doesn't crash.
    let json_input =
        fs::read_to_string("tests/fixtures/official_input.json").expect("failed to read fixture");

    let stdout = run_with_json(&path, &json_input);

    // Binary should not crash — output must be non-empty
    assert!(
        !stdout.is_empty(),
        "Expected non-empty output from official JSON fixture"
    );

    // Model display name
    assert!(
        stdout.contains("Opus"),
        "Expected model display_name 'Opus' in output: {}",
        stdout
    );

    // Context remaining percentage (57.5 → 57%)
    assert!(
        stdout.contains("57%"),
        "Expected context remaining '57%' in output: {}",
        stdout
    );

    // Total input tokens (15234 → 15K)
    assert!(
        stdout.contains("15K"),
        "Expected total_input_tokens '15K' in output: {}",
        stdout
    );

    // Total output tokens (4521 → 4K)
    assert!(
        stdout.contains("4K"),
        "Expected total_output_tokens '4K' in output: {}",
        stdout
    );
}
