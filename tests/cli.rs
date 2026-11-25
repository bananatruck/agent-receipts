use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use tempfile::TempDir;

struct Repository {
    directory: TempDir,
}

impl Repository {
    fn new() -> Self {
        let repository = Self {
            directory: tempfile::tempdir().unwrap(),
        };
        repository.git(&["init", "--initial-branch=main"]);
        repository.git(&["config", "user.name", "Fixture"]);
        repository.git(&["config", "user.email", "fixture@example.invalid"]);
        repository.git(&["config", "commit.gpgsign", "false"]);
        repository.git(&["config", "core.hooksPath", "/dev/null"]);
        repository
    }

    fn path(&self) -> &Path {
        self.directory.path()
    }

    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(self.path())
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_OBJECT_DIRECTORY")
            .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
            .env_remove("GIT_NAMESPACE")
            .env_remove("GIT_CONFIG_PARAMETERS")
            .env("GIT_CONFIG_COUNT", "0")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn write(&self, path: &str, contents: &str) {
        let path = self.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn commit(&self) -> String {
        self.git(&["add", "."]);
        self.git(&["commit", "-m", "Add fixture"]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn check(&self, report: &str) -> Output {
        self.check_with(report, &[], &[])
    }

    fn check_with(&self, report: &str, args: &[&str], environment: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_agent-receipts"));
        if !args.contains(&"--repo") {
            command.arg("--repo").arg(self.path());
        }
        command.args(args);
        for (name, value) in environment {
            command.env(name, value);
        }
        let mut process = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        process
            .stdin
            .take()
            .unwrap()
            .write_all(report.as_bytes())
            .unwrap();
        process.wait_with_output().unwrap()
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn assert_verdict(output: &Output, line: usize, check: &str, verdict: &str) {
    let text = stdout(output);
    let row = text
        .lines()
        .find(|row| row.starts_with(&format!("{line}\t")))
        .unwrap_or_else(|| panic!("missing line {line}: {text}"));
    let cells: Vec<_> = row.split('\t').collect();
    assert_eq!(cells.len(), 5, "{row}");
    assert_eq!(cells[2], check, "{row}");
    assert_eq!(cells[3], verdict, "{row}");
    assert!(!cells[4].is_empty(), "{row}");
}

#[test]
fn checks_commit_and_file_statements_against_head() {
    let repository = Repository::new();
    repository.write("src/lib.rs", "pub fn value() -> u8 { 7 }\n");
    let hash = repository.commit();
    let report = format!(
        "# Synthetic report\n\n- Committed as `{hash}`.\n- File `src/lib.rs` exists.\n\
         - Commit 0000000000000000000000000000000000000000 exists.\n\
         - File `src/missing.rs` exists.\n"
    );
    let output = repository.check(&report);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(stdout(&output).starts_with("LINE\tSTATEMENT\tCHECK\tVERDICT\tEVIDENCE\n"));
    assert_verdict(&output, 3, "commit exists locally", "confirmed");
    assert_verdict(&output, 4, "file at HEAD", "confirmed");
    assert_verdict(&output, 5, "commit exists locally", "contradicted");
    assert_verdict(&output, 6, "file at HEAD", "contradicted");
    assert!(stdout(&output).contains(&hash));
    assert!(stdout(&output).contains("2 confirmed, 2 contradicted, 1 not checkable"));
    assert!(output.stderr.is_empty());
}

#[test]
fn checks_clean_tree_including_untracked_unstaged_and_staged_changes() {
    let repository = Repository::new();
    repository.write("README.md", "fixture\n");
    repository.commit();
    let output = repository.check("The tree is clean.\n");
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_verdict(&output, 1, "working tree clean", "confirmed");

    repository.write("new.txt", "untracked\n");
    let output = repository.check("Tree is clean\n");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_verdict(&output, 1, "working tree clean", "contradicted");
    assert!(stdout(&output).contains("?? new.txt"));
    repository.commit();

    repository.write("README.md", "changed\n");
    let output = repository.check("Tree is clean\n");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(stdout(&output).contains(" M README.md"));
    repository.git(&["add", "README.md"]);
    let output = repository.check("Tree is clean\n");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(stdout(&output).contains("M  README.md"));
}

#[test]
fn quoted_examples_and_nonassertions_cannot_confirm_claims() {
    let repository = Repository::new();
    repository.write("README.md", "fixture\n");
    repository.commit();
    let output = repository.check(
        "# Synthetic report\n```text\nTree is clean\n```\n~~~text\n\
         File README.md exists\n~~~\n> Tree is clean\nIf the tree is clean\n\
         I did not commit abcdef0\n    Tree is clean\n",
    );
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    for line in [3, 6, 8, 9, 10, 11] {
        assert_verdict(&output, line, "unsupported", "not checkable");
    }
    assert!(stdout(&output).contains("0 confirmed, 0 contradicted, 7 not checkable"));
}

#[test]
fn file_checks_use_head_and_never_follow_workspace_paths() {
    let repository = Repository::new();
    repository.write("src/lib.rs", "fixture\n");
    repository.commit();
    fs::remove_file(repository.path().join("src/lib.rs")).unwrap();
    repository.write("src/workspace.rs", "uncommitted\n");
    let output = repository.check(
        "File src/lib.rs exists\nFile src/workspace.rs exists\nFile src exists\n\
         File ../outside.txt exists\nFile /etc/passwd exists\nFile HEAD:src/lib.rs exists\n",
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_verdict(&output, 1, "file at HEAD", "confirmed");
    for line in [2, 3] {
        assert_verdict(&output, line, "file at HEAD", "contradicted");
    }
    for line in [4, 5, 6] {
        assert_verdict(&output, line, "unsupported", "not checkable");
    }
}

#[test]
fn commit_checks_require_commit_objects_and_accept_unique_abbreviations() {
    let repository = Repository::new();
    repository.write("README.md", "fixture\n");
    let hash = repository.commit();
    let blob = repository.git(&["rev-parse", "HEAD:README.md"]);
    let tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    let output = repository.check(&format!(
        "Commit {} exists\nCommit {blob} exists\nCommit {tree} exists\n",
        hash[..7].to_ascii_uppercase()
    ));
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_verdict(&output, 1, "commit exists locally", "confirmed");
    for line in [2, 3] {
        assert_verdict(&output, line, "commit exists locally", "contradicted");
    }
}

#[test]
fn unborn_head_leaves_file_evidence_unknown() {
    let repository = Repository::new();
    repository.write("README.md", "not committed\n");
    let output = repository.check("File README.md exists\n");
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert_verdict(&output, 1, "file at HEAD", "not checkable");
    assert!(stdout(&output).contains("HEAD does not resolve"));
}

#[test]
fn unsupported_reports_never_succeed_without_evidence() {
    let repository = Repository::new();
    let output = repository.check("77 tests passed\nNot pushed\nRan `cargo test`\n");
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    for line in [1, 3] {
        assert_verdict(&output, line, "unsupported", "not checkable");
    }
    assert_verdict(&output, 2, "local tracking ref ancestry", "not checkable");
    let output = repository.check("# Synthetic report\n\n");
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(stdout(&output).contains("0 confirmed, 0 contradicted, 1 not checkable"));
}

#[test]
fn reads_markdown_report_files_and_paths_with_spaces() {
    let repository = Repository::new();
    repository.write("notes/a b.txt", "fixture\n");
    let hash = repository.commit();
    let report = tempfile::NamedTempFile::new().unwrap();
    fs::write(
        report.path(),
        format!(
            "## Synthetic report\n1. COMMIT `{hash}` EXISTS.\n2) File `notes/a b.txt` exists.\n"
        ),
    )
    .unwrap();
    let output = repository.check_with("", &["--report", report.path().to_str().unwrap()], &[]);
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert_verdict(&output, 2, "commit exists locally", "confirmed");
    assert_verdict(&output, 3, "file at HEAD", "confirmed");
}

#[test]
fn reports_input_and_repository_errors_on_stderr() {
    let repository = Repository::new();
    let missing = repository.path().join("missing-report.txt");
    let output = repository.check_with("", &["--report", missing.to_str().unwrap()], &[]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot read report"));

    let not_repository = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_agent-receipts"))
        .args(["--repo", not_repository.path().to_str().unwrap()])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("working Git repository"));

    let output = repository.check_with("", &["--unknown"], &[]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let output = repository.check_with("Tree is clean\n", &[], &[("PATH", "")]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("install Git"));
}

#[test]
fn help_and_version_work_without_repository_or_input() {
    let directory = tempfile::tempdir().unwrap();
    for option in ["--help", "--version"] {
        let output = Command::new(env!("CARGO_BIN_EXE_agent-receipts"))
            .current_dir(directory.path())
            .arg(option)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert!(stdout(&output).contains("agent-receipts"));
        if option == "--help" {
            assert!(stdout(&output).contains("Examples:"));
        }
    }
}

#[test]
fn output_cannot_inject_terminal_controls_or_table_cells() {
    let repository = Repository::new();
    let output = repository.check("Synthetic\ttext\u{1b}[31m\\x\n");
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert_verdict(&output, 1, "unsupported", "not checkable");
    let text = stdout(&output);
    assert!(!text.contains('\u{1b}'));
    assert!(text.contains("Synthetic\\ttext\\u{1b}[31m\\\\x"));
}

#[test]
fn repository_selection_ignores_inherited_git_location() {
    let repository = Repository::new();
    let other = Repository::new();
    other.write("only-there.txt", "fixture\n");
    other.commit();
    let git_dir = other.path().join(".git");
    let output = repository.check_with(
        "File only-there.txt exists\n",
        &[],
        &[("GIT_DIR", git_dir.to_str().unwrap())],
    );
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert_verdict(&output, 1, "file at HEAD", "not checkable");
}

#[test]
fn ignores_ignored_files_and_does_not_refresh_the_index() {
    let repository = Repository::new();
    repository.write(".gitignore", "ignored.txt\n");
    repository.commit();
    repository.write("ignored.txt", "excluded\n");
    let index = repository.path().join(".git/index");
    let before = fs::metadata(&index).unwrap().modified().unwrap();
    let output = repository.check("Tree is clean\n");
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(before, fs::metadata(index).unwrap().modified().unwrap());
}

#[test]
fn file_paths_are_literal_and_root_relative_from_subdirectories() {
    let repository = Repository::new();
    repository.write("src/lib.rs", "fixture\n");
    repository.commit();
    let output = repository.check("File `src/*.rs` exists\n");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_verdict(&output, 1, "file at HEAD", "contradicted");

    let output = Command::new(env!("CARGO_BIN_EXE_agent-receipts"))
        .args(["--repo", repository.path().join("src").to_str().unwrap()])
        .args(["--report", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut process| {
            process
                .stdin
                .take()
                .unwrap()
                .write_all(b"File src/lib.rs exists\nFile missing.rs exists\n")?;
            process.wait_with_output()
        })
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_verdict(&output, 1, "file at HEAD", "confirmed");
    assert_verdict(&output, 2, "file at HEAD", "contradicted");
}

#[test]
fn unreadable_git_evidence_is_unknown_instead_of_a_contradiction() {
    let repository = Repository::new();
    repository.write("README.md", "fixture\n");
    repository.commit();
    let blob = repository.git(&["rev-parse", "HEAD:README.md"]);
    let object = repository
        .path()
        .join(".git/objects")
        .join(&blob[..2])
        .join(&blob[2..]);
    fs::remove_file(object).unwrap();
    let output = repository.check("File README.md exists\n");
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert_verdict(&output, 1, "file at HEAD", "not checkable");
    assert!(stdout(&output).contains("cannot read object"));

    fs::write(repository.path().join(".git/index"), "invalid index").unwrap();
    let output = repository.check("Tree is clean\n");
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert_verdict(&output, 1, "working tree clean", "not checkable");
}

#[test]
fn rejects_unpaired_quotes_and_does_not_execute_report_text() {
    let repository = Repository::new();
    repository.write("README.md", "fixture\n");
    let hash = repository.commit();
    let output = repository.check(&format!(
        "File `README.md exists\nFile README.md` exists\nCommit `{hash} exists\n\
         Committed as {hash}`\nCommitted as {hash} if tests pass\n\
         File `$(touch OWNED)` exists\n"
    ));
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    for line in 1..=5 {
        assert_verdict(&output, line, "unsupported", "not checkable");
    }
    assert_verdict(&output, 6, "file at HEAD", "contradicted");
    assert!(!repository.path().join("OWNED").exists());
    assert!(!Path::new("OWNED").exists());
}

#[test]
fn invalid_utf8_reports_are_input_errors() {
    let repository = Repository::new();
    let report = tempfile::NamedTempFile::new().unwrap();
    fs::write(report.path(), [0xff, 0xfe]).unwrap();
    let output = repository.check_with("", &["--report", report.path().to_str().unwrap()], &[]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot read report"));
}

#[cfg(unix)]
#[test]
fn stored_symlinks_count_as_blobs_without_reading_the_target() {
    let repository = Repository::new();
    std::os::unix::fs::symlink("/nonexistent/target", repository.path().join("link")).unwrap();
    repository.commit();
    let output = repository.check("File link exists\n");
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_verdict(&output, 1, "file at HEAD", "confirmed");
    assert!(stdout(&output).contains("has type blob"));
}

#[test]
fn test_counts_require_a_matching_command_log_and_matching_summary() {
    let repository = Repository::new();
    let log = tempfile::NamedTempFile::new().unwrap();
    fs::write(log.path(), "$ cargo test\nrunning 77 tests\ntest result: ok. 77 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n").unwrap();
    let flag = format!("cargo test={}", log.path().display());
    let output = repository.check_with(
        "Ran `cargo test`, 77 passed\nRan `cargo test`, 76 passed\nRan `pytest`, 77 passed\n",
        &["--log", &flag],
        &[],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_verdict(&output, 1, "test counts in supplied log", "confirmed");
    assert_verdict(&output, 2, "test counts in supplied log", "contradicted");
    assert_verdict(&output, 3, "test counts in supplied log", "not checkable");
    assert!(stdout(&output).contains("only what the supplied log contains"));
    let output = repository.check("Ran `cargo test`, 77 passed\n");
    assert_eq!(output.status.code(), Some(3));
    assert_verdict(&output, 1, "test counts in supplied log", "not checkable");
}

#[test]
fn logs_reject_missing_ambiguous_or_failed_evidence() {
    let repository = Repository::new();
    let log = tempfile::NamedTempFile::new().unwrap();
    for (command, text, expected) in [
        ("pytest", "$ pytest\n7 passed in 0.10s\n", "confirmed"),
        (
            "pytest",
            "$ pytest\n1 failed, 7 passed in 0.10s\n",
            "contradicted",
        ),
        ("jest", "$ jest\nTests: 7 passed, 7 total\n", "confirmed"),
        (
            "vitest run",
            "$ vitest run\nTests 7 passed (7)\n",
            "confirmed",
        ),
        (
            "cargo test",
            "$ cargo test\ntest result: FAILED. 7 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n",
            "contradicted",
        ),
        (
            "cargo test",
            "test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n",
            "not checkable",
        ),
        ("pytest", "$ other\n7 passed in 0.10s\n", "not checkable"),
        (
            "pytest",
            "$ pytest\n7 passed in 0.10s\n$ pytest\n7 passed in 0.10s\n",
            "not checkable",
        ),
        ("pytest", "$ pytest\ncollecting ...\n", "not checkable"),
    ] {
        fs::write(log.path(), text).unwrap();
        let flag = format!("{command}={}", log.path().display());
        let output = repository.check_with(
            &format!("Ran `{command}`, 7 passed\n"),
            &["--log", &flag],
            &[],
        );
        assert_verdict(&output, 1, "test counts in supplied log", expected);
    }
    for flag in ["bad".to_owned(), "pytest=/nonexistent/log.txt".to_owned()] {
        let output = repository.check_with("", &["--log", &flag], &[]);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn pushed_statements_compare_named_commits_with_a_local_tracking_ref() {
    let repository = Repository::new();
    repository.write("README.md", "first\n");
    let first = repository.commit();
    repository.git(&["update-ref", "refs/remotes/origin/main", &first]);
    repository.write("README.md", "second\n");
    let second = repository.commit();
    let args = ["--remote-ref", "refs/remotes/origin/main"];
    let output = repository.check_with("Not pushed\nPushed\n", &args, &[]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_verdict(&output, 1, "local tracking ref ancestry", "confirmed");
    assert_verdict(&output, 2, "local tracking ref ancestry", "contradicted");
    let output = repository.check_with(
        &format!("Commit {first} exists\nPushed\nNot pushed {second}\nPushed {second}\n"),
        &args,
        &[],
    );
    assert_verdict(&output, 2, "local tracking ref ancestry", "contradicted");
    assert_verdict(&output, 3, "local tracking ref ancestry", "confirmed");
    assert_verdict(&output, 4, "local tracking ref ancestry", "contradicted");
    assert!(stdout(&output).contains("local snapshot"));
    for args in [vec![], vec!["--remote-ref", "refs/remotes/missing/main"]] {
        let output = repository.check_with("Not pushed\n", &args, &[]);
        assert_eq!(output.status.code(), Some(3));
        assert_verdict(&output, 1, "local tracking ref ancestry", "not checkable");
    }
}

#[test]
fn scope_statements_check_every_path_in_the_committed_diff() {
    let repository = Repository::new();
    repository.write("src/a.rs", "first\n");
    repository.write("keep.txt", "keep\n");
    let base = repository.commit();
    repository.write("src/a.rs", "second\n");
    repository.write("notes/a b.txt", "new\n");
    repository.commit();
    let output = repository.check_with("Only touched src/a.rs\nOnly touched src/a.rs, `notes/a b.txt`\nDid not modify keep.txt\nDid not modify src/a.rs\n", &["--base", &base], &[]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    for (line, verdict) in [
        (1, "contradicted"),
        (2, "confirmed"),
        (3, "confirmed"),
        (4, "contradicted"),
    ] {
        assert_verdict(&output, line, "committed diff scope", verdict);
    }
    let output = repository.check("Only touched src/a.rs\nDid not modify keep.txt\n");
    assert_eq!(output.status.code(), Some(3));
    for line in [1, 2] {
        assert_verdict(&output, line, "committed diff scope", "not checkable");
    }
    let output = repository.check_with(
        "Did not modify keep.txt\n",
        &["--base", "0000000000000000000000000000000000000000"],
        &[],
    );
    assert_verdict(&output, 1, "committed diff scope", "not checkable");
}

#[test]
fn path_actions_require_the_matching_diff_status() {
    let repository = Repository::new();
    repository.write("gone.txt", "delete\n");
    repository.write("old name.txt", "rename this file with unchanged content\n");
    repository.write("modified.txt", "before\n");
    let base = repository.commit();
    repository.git(&["rm", "gone.txt"]);
    repository.git(&["mv", "old name.txt", "new name.txt"]);
    repository.write("added.txt", "add\n");
    repository.write("modified.txt", "after\n");
    repository.commit();
    let output = repository.check_with("Added added.txt\nCreated added.txt\nDeleted gone.txt\nRenamed `new name.txt`\nRenamed `old name.txt` to `new name.txt`\nAdded modified.txt\nCreated gone.txt\nDeleted added.txt\nRenamed added.txt\nDid not modify `old name.txt`\nOnly touched added.txt, gone.txt, modified.txt, `new name.txt`\n", &["--base", &base], &[]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    for line in 1..=9 {
        assert_verdict(
            &output,
            line,
            "committed path action",
            if line <= 5 {
                "confirmed"
            } else {
                "contradicted"
            },
        );
    }
    for line in [10, 11] {
        assert_verdict(&output, line, "committed diff scope", "contradicted");
    }
    let output = repository
        .check("Added added.txt\nCreated added.txt\nDeleted gone.txt\nRenamed `new name.txt`\n");
    assert_eq!(output.status.code(), Some(3));
    for line in 1..=4 {
        assert_verdict(&output, line, "committed path action", "not checkable");
    }
}

#[test]
fn json_output_preserves_verdicts_summary_and_exit_codes() {
    let repository = Repository::new();
    repository.write("README.md", "fixture\n");
    repository.commit();
    for (report, code, confirmed, contradicted, unknown) in [
        ("File README.md exists\n", 0, 1, 0, 0),
        ("File missing.txt exists\nUnknown\n", 1, 0, 1, 1),
        ("Unknown\n", 3, 0, 0, 1),
        ("", 3, 0, 0, 0),
    ] {
        let output = repository.check_with(report, &["--json"], &[]);
        assert_eq!(output.status.code(), Some(code), "{output:?}");
        let text = stdout(&output);
        assert!(text.starts_with("{\"schema_version\":1,"), "{text}");
        assert!(text.contains(&format!("\"confirmed\":{confirmed},\"contradicted\":{contradicted},\"not_checkable\":{unknown}")), "{text}");
        assert!(text.contains(&format!("\"exit_code\":{code}")), "{text}");
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["exit_code"], code);
        assert_eq!(
            value["statements"].as_array().unwrap().len(),
            (confirmed + contradicted + unknown) as usize
        );
        if !report.is_empty() {
            assert_eq!(value["statements"][0]["line"], 1);
            assert_eq!(
                value["statements"][0]["statement"],
                report.lines().next().unwrap()
            );
            assert!(value["statements"][0]["evidence"].as_str().is_some());
        }
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn logs_cannot_hide_runner_failures_or_unfinished_suites() {
    let repository = Repository::new();
    let log = tempfile::NamedTempFile::new().unwrap();
    for (command, text, expected) in [
        (
            "cargo test",
            "$ cargo test\ntest result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\nrunning 2 tests\n",
            "not checkable",
        ),
        (
            "cargo test",
            "$ cargo test\ntest result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\nerror: test failed, to rerun pass --test other\n",
            "contradicted",
        ),
        (
            "jest",
            "$ jest\nTest Suites: 1 failed, 1 passed, 2 total\nTests: 7 passed, 7 total\n",
            "contradicted",
        ),
        (
            "vitest run",
            "$ vitest run\nTest Files 1 failed | 1 passed (2)\nTests 7 passed (7)\n",
            "contradicted",
        ),
    ] {
        fs::write(log.path(), text).unwrap();
        let output = repository.check_with(
            &format!("Ran `{command}`, 7 passed\n"),
            &["--log", &format!("{command}={}", log.path().display())],
            &[],
        );
        assert_verdict(&output, 1, "test counts in supplied log", expected);
    }
}

#[test]
fn diff_scope_is_root_relative_even_with_relative_diff_configuration() {
    let repository = Repository::new();
    repository.write("src/a.rs", "before\n");
    repository.write("outside.txt", "before\n");
    let base = repository.commit();
    repository.write("src/a.rs", "after\n");
    repository.write("outside.txt", "after\n");
    repository.commit();
    repository.git(&["config", "diff.relative", "true"]);
    let report = tempfile::NamedTempFile::new().unwrap();
    fs::write(
        report.path(),
        "Only touched src/a.rs\nDid not modify outside.txt\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_agent-receipts"))
        .arg("--repo")
        .arg(repository.path().join("src"))
        .args(["--base", &base, "--report", report.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    for line in [1, 2] {
        assert_verdict(&output, line, "committed diff scope", "contradicted");
    }
}

#[test]
fn negative_scope_requires_an_existing_literal_file() {
    let repository = Repository::new();
    repository.write("src/main.rs", "before\n");
    repository.write("README.md", "kept\n");
    let base = repository.commit();
    repository.write("src/main.rs", "after\n");
    repository.commit();
    for path in [
        "src",
        "src/*.rs",
        "src/main.rs..",
        "missing.txt",
        "readme.md",
    ] {
        let output =
            repository.check_with(&format!("Did not modify {path}\n"), &["--base", &base], &[]);
        assert_eq!(output.status.code(), Some(3), "{path}: {output:?}");
    }
    let output = repository.check_with(
        "Did not modify README.md\nOnly touched missing.txt\n",
        &["--base", "HEAD"],
        &[],
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
}

#[test]
fn file_actions_do_not_confirm_gitlinks_or_their_contents() {
    let repository = Repository::new();
    repository.write("keep.txt", "kept\n");
    let base = repository.commit();
    repository.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{base},sub"),
    ]);
    repository.git(&["commit", "-m", "Add link"]);
    let output = repository.check_with(
        "Added sub\nCreated sub\nDid not modify sub/file.rs\n",
        &["--base", &base],
        &[],
    );
    for line in 1..=3 {
        assert_verdict(
            &output,
            line,
            if line == 3 {
                "committed diff scope"
            } else {
                "committed path action"
            },
            "not checkable",
        );
    }
}

#[test]
fn bare_push_checks_head_and_explicit_hashes_require_commit_objects() {
    let repository = Repository::new();
    repository.write("a.txt", "first\n");
    let first = repository.commit();
    repository.git(&["update-ref", "refs/remotes/origin/main", &first]);
    repository.git(&["tag", "-a", "v1", "-m", "Fixture tag"]);
    let tag = repository.git(&["rev-parse", "v1"]);
    repository.write("a.txt", "second\n");
    repository.commit();
    let args = ["--remote-ref", "refs/remotes/origin/main"];
    let output = repository.check_with(&format!("Pushed {tag}\nNot pushed {tag}\n"), &args, &[]);
    for line in 1..=2 {
        assert_verdict(
            &output,
            line,
            "local tracking ref ancestry",
            "not checkable",
        );
    }
    let output = repository.check_with(
        &format!("Commit {first} exists\nPushed\nNot pushed\n"),
        &args,
        &[],
    );
    assert_verdict(&output, 2, "local tracking ref ancestry", "contradicted");
    assert_verdict(&output, 3, "local tracking ref ancestry", "confirmed");
}

#[test]
fn logs_require_anchored_records_complete_suites_and_no_runner_errors() {
    let repository = Repository::new();
    let log = tempfile::NamedTempFile::new().unwrap();
    let mut failures = Vec::new();
    for (command, body, expected) in [
        (
            "pytest",
            "note: $ pytest\n7 passed in 0.10s\n$ pytest\ncollecting ...\n",
            "not checkable",
        ),
        (
            "cargo test --no-fail-fast",
            "$ cargo test --no-fail-fast\nRunning unittests src/lib.rs\nrunning 10 tests\nerror: process didn't exit successfully: SIGSEGV\nRunning tests/cli.rs\nrunning 7 tests\ntest result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\nerror: 1 target failed:\n",
            "contradicted",
        ),
        (
            "cargo test",
            "$ cargo test\nrunning 10 tests\nRunning tests/cli.rs\nrunning 7 tests\ntest result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n",
            "not checkable",
        ),
        (
            "cargo test",
            "$ cargo test\ntest result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n",
            "not checkable",
        ),
        (
            "cargo test",
            "$ cargo test\nrunning 2 tests\ntest result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n",
            "not checkable",
        ),
        (
            "cargo test",
            "$ cargo test\nrunning 2 tests\ntest result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\ntest result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n",
            "not checkable",
        ),
        (
            "vitest",
            "$ vitest\nTests 7 passed (7)\nErrors 1 error\n",
            "contradicted",
        ),
        (
            "vitest",
            "$ vitest\nTests 7 passed (7)\nErrors 2 errors\n",
            "contradicted",
        ),
        (
            "vitest",
            "$ vitest\nTests 7 passed (7)\nType Errors 1 error\n",
            "contradicted",
        ),
        (
            "vitest",
            "$ vitest\nTests 7 passed (7)\nUnhandled Errors\n",
            "contradicted",
        ),
        (
            "pytest",
            "$ pytest\n7 passed in 0.10s\nINTERNALERROR> broken hook\n",
            "contradicted",
        ),
        ("vitest", "$ vitest\nTests 7 passed (7)\n", "confirmed"),
    ] {
        fs::write(log.path(), body).unwrap();
        let output = repository.check_with(
            &format!("Ran `{command}`, 7 passed\n"),
            &["--log", &format!("{command}={}", log.path().display())],
            &[],
        );
        let expected_cell = format!("\t{expected}\t");
        if !stdout(&output).contains(&expected_cell) {
            failures.push(format!("{body:?}: {}", stdout(&output)));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn markdown_code_context_cannot_become_an_assertion() {
    let repository = Repository::new();
    repository.write("a.txt", "fixture\n");
    repository.commit();
    let mut failures = Vec::new();
    for report in [
        "```\n    ```\nTree is clean\n```\n",
        " \tTree is clean\n",
        "-     Tree is clean\n",
        "- ```\n  Tree is clean\n  ```\n",
    ] {
        let output = repository.check(report);
        if output.status.code() != Some(3) || stdout(&output).contains("\tconfirmed\t") {
            failures.push(format!("{report:?}: {}", stdout(&output)));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn repository_overlays_cannot_hide_missing_evidence_or_dirty_files() {
    let mut failures = Vec::new();
    for flag in ["--assume-unchanged", "--skip-worktree"] {
        let repository = Repository::new();
        repository.write("a.txt", "before\n");
        repository.commit();
        repository.git(&["update-index", flag, "a.txt"]);
        repository.write("a.txt", "after\n");
        let output = repository.check("Tree is clean\n");
        if stdout(&output).contains("\tconfirmed\t") {
            failures.push(format!("{flag}: {}", stdout(&output)));
        }
    }
    let repository = Repository::new();
    repository.write("a.txt", "before\n");
    let first = repository.commit();
    repository.write("new.txt", "new\n");
    let second = repository.commit();
    repository.git(&["checkout", "--detach", &first]);
    repository.git(&["replace", &first, &second]);
    let output = repository.check("File new.txt exists\n");
    if !stdout(&output).contains("\tcontradicted\t") {
        failures.push(stdout(&output));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn cleanliness_never_executes_configured_filters() {
    let repository = Repository::new();
    repository.write(".gitattributes", "a.txt filter=probe\n");
    repository.write("a.txt", "before\n");
    repository.commit();
    let marker = repository.path().join("executed");
    repository.git(&[
        "config",
        "filter.probe.clean",
        &format!("touch '{}'; cat", marker.display()),
    ]);
    repository.write("a.txt", "change\n");
    let output = repository.check("Tree is clean\n");
    assert!(!marker.exists(), "configured filter executed: {output:?}");
    assert_eq!(output.status.code(), Some(3), "{output:?}");
}

#[test]
fn clean_filter_preflight_ignores_inherited_config_file() {
    let repository = Repository::new();
    repository.write(".gitattributes", "a.txt filter=probe\n");
    repository.write("a.txt", "before\n");
    repository.commit();
    let environment = [("GIT_CONFIG", "/dev/null")];
    let output = repository.check_with("Tree is clean\n", &[], &environment);
    assert_eq!(output.status.code(), Some(0), "{output:?}");

    repository.write("a.txt", "change\n");
    let output = repository.check_with("Tree is clean\n", &[], &environment);
    assert_eq!(output.status.code(), Some(1), "{output:?}");

    let marker = repository.path().join(".git/filter-executed");
    repository.git(&[
        "config",
        "filter.probe.clean",
        &format!("touch '{}'; cat", marker.display()),
    ]);
    let output = repository.check_with("Tree is clean\n", &[], &environment);
    assert!(!marker.exists(), "configured filter executed: {output:?}");
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert_verdict(&output, 1, "working tree clean", "not checkable");
    assert!(stdout(&output).contains("configured clean or process filters"));
}

#[test]
fn terminal_output_escapes_direction_and_format_controls() {
    let repository = Repository::new();
    let output = repository.check(
        "File \u{202e}detcidartnoc\tconfirmed exists\n\u{2066}hidden\u{2069}\n\u{200b}zero\n",
    );
    for c in ['\u{202e}', '\u{2066}', '\u{2069}', '\u{200b}'] {
        assert!(
            !stdout(&output).contains(c),
            "raw format control in {}",
            stdout(&output)
        );
    }
    assert!(stdout(&output).contains("\\u{202e}"));
}

#[test]
fn unverified_markdown_lines_prevent_success_in_both_output_formats() {
    let repository = Repository::new();
    repository.write("a.txt", "fixture\n");
    repository.commit();
    for line in [
        "```cargo test``` failed on 5 suites",
        "~~~cargo test~~~ failed",
        "    # Indented claim",
        "    ```",
        "# Ran `cargo test`, 999 passed",
    ] {
        for args in [vec![], vec!["--json"]] {
            let output = repository.check_with(&format!("Tree is clean\n{line}\n"), &args, &[]);
            assert_eq!(output.status.code(), Some(3), "{line:?}: {output:?}");
            if args.is_empty() {
                assert_verdict(&output, 2, "unsupported", "not checkable");
            } else {
                let doc: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(doc["summary"]["not_checkable"], 1);
                assert_eq!(doc["exit_code"], 3);
                assert_eq!(doc["statements"].as_array().unwrap().len(), 2);
            }
        }
    }
}

#[test]
fn matching_failure_counts_are_valid_evidence_and_stdout_is_not_runner_status() {
    let repository = Repository::new();
    let log = tempfile::NamedTempFile::new().unwrap();
    let mut failures = Vec::new();
    for (command, body, passed, failed) in [
        ("pytest", "1 failed, 7 passed in 0.10s\n", 7, 1),
        (
            "cargo test",
            "running 8 tests\ntest result: FAILED. 7 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\nerror: test failed, to rerun pass --lib\n",
            7,
            1,
        ),
        (
            "jest",
            "FAIL a.test.js\nTest Suites: 1 failed, 1 total\nTests: 1 failed, 7 passed, 8 total\n",
            7,
            1,
        ),
        (
            "vitest",
            "Test Files 1 failed (1)\nTests 7 passed | 1 failed (8)\n",
            7,
            1,
        ),
        (
            "cargo test",
            "running 7 tests\nFAIL case handled properly\ntest result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n",
            7,
            0,
        ),
    ] {
        fs::write(log.path(), format!("$ {command}\n{body}")).unwrap();
        let output = repository.check_with(
            &format!("Ran `{command}`, {passed} passed, {failed} failed\n"),
            &["--log", &format!("{command}={}", log.path().display())],
            &[],
        );
        if output.status.code() != Some(0) {
            failures.push(stdout(&output));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn cargo_abnormal_exit_causes_are_independent_of_assertion_failure_counts() {
    let repository = Repository::new();
    let log = tempfile::NamedTempFile::new().unwrap();
    let command = "cargo test -q --no-fail-fast";
    let assertion = "running 1 test\nassertion --- FAILED\n\nfailures:\n    assertion\n\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\nerror: test failed, to rerun pass `--test a_assertion`\n";
    for (tail, expected, verdict) in [
        (
            "error: test failed, to rerun pass `--test z_abnormal`\n\nCaused by:\n  process didn't exit successfully: `/tmp/fixture/target/debug/deps/z_abnormal-1234` (exit status: 4)\nerror: 2 targets failed:\n    `--test a_assertion`\n    `--test z_abnormal`\n",
            1,
            "contradicted",
        ),
        (
            "Caused by:\n  process didn't exit successfully: `/tmp/fixture/target/debug/deps/z_abnormal-1234` (signal: 11, SIGSEGV: invalid memory reference)\n",
            1,
            "contradicted",
        ),
        (
            "error: process didn't exit successfully: `/tmp/fixture/target/debug/deps/z_abnormal-1234` (exit status: 4)\n",
            1,
            "contradicted",
        ),
        (
            "error: 1 target failed:\n    `--test a_assertion`\n",
            0,
            "confirmed",
        ),
        ("running 1 test\n", 3, "not checkable"),
    ] {
        fs::write(log.path(), format!("$ {command}\n{assertion}{tail}")).unwrap();
        let mapping = format!("{command}={}", log.path().display());
        let report = format!("Ran `{command}`, 0 passed, 1 failed\n");
        let output = repository.check_with(&report, &["--log", &mapping], &[]);
        assert_eq!(output.status.code(), Some(expected), "{}", stdout(&output));
        assert_verdict(&output, 1, "test counts in supplied log", verdict);

        let output = repository.check_with(&report, &["--log", &mapping, "--json"], &[]);
        assert_eq!(output.status.code(), Some(expected), "{}", stdout(&output));
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(json["exit_code"], expected);
        assert_eq!(json["statements"][0]["verdict"], verdict);
    }
}

#[test]
fn reported_failures_do_not_resolve_ambiguous_test_counts() {
    let repository = Repository::new();
    let log = tempfile::NamedTempFile::new().unwrap();
    for (command, body) in [
        ("cargo test", "error: test failed, to rerun pass `--lib`\n"),
        (
            "cargo test",
            "running 1 test\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\nerror: test failed, to rerun pass `--lib`\n     Running tests/later.rs (target/debug/deps/later-1234)\n",
        ),
        ("jest", "FAIL assertion.test.js\nTests: 1 failed,\n"),
        ("npx jest", "Test Suites: 1 failed, 1 total\nTests:\n"),
        ("vitest", "Test Files 1 failed (1)\nTests 1 failed (\n"),
        ("npx vitest", "FAIL assertion.test.js\n"),
        ("pytest", "1 failed in\n"),
    ] {
        fs::write(log.path(), format!("$ {command}\n{body}")).unwrap();
        let output = repository.check_with(
            &format!("Ran `{command}`, 0 passed, 1 failed\n"),
            &["--log", &format!("{command}={}", log.path().display())],
            &[],
        );
        assert_eq!(output.status.code(), Some(3), "{}", stdout(&output));
        assert_verdict(&output, 1, "test counts in supplied log", "not checkable");
    }
}

#[test]
fn additions_and_deletions_check_endpoints_despite_rename_similarity() {
    let repository = Repository::new();
    repository.write("old.rs", "same content\n");
    let base = repository.commit();
    repository.git(&["mv", "old.rs", "new.rs"]);
    repository.commit();
    let output = repository.check_with(
        "Added new.rs\nCreated new.rs\nDeleted old.rs\nRenamed old.rs to new.rs\n",
        &["--base", &base],
        &[],
    );
    for line in 1..=4 {
        assert_verdict(&output, line, "committed path action", "confirmed");
    }
}

#[cfg(unix)]
#[test]
fn file_presence_does_not_resolve_intermediate_symlinks() {
    let repository = Repository::new();
    repository.write("sub/file.txt", "fixture\n");
    std::os::unix::fs::symlink("sub", repository.path().join("dir_link")).unwrap();
    repository.commit();
    let output = repository.check("File dir_link/file.txt exists\nFile sub/file.txt exists\n");
    assert_verdict(&output, 1, "file at HEAD", "contradicted");
    assert_verdict(&output, 2, "file at HEAD", "confirmed");
}

#[test]
fn rename_separator_is_case_insensitive_and_outside_path_quotes() {
    let repository = Repository::new();
    repository.write("old to name.txt", "fixture\n");
    let base = repository.commit();
    repository.git(&["mv", "old to name.txt", "new name.txt"]);
    repository.commit();
    for report in [
        "Renamed `old to name.txt` to `new name.txt`\n",
        "RENAMED `old to name.txt` TO `new name.txt`\n",
        "Renamed `new name.txt`\n",
    ] {
        let output = repository.check_with(report, &["--base", &base], &[]);
        assert_eq!(output.status.code(), Some(0), "{report}: {output:?}");
    }
}

#[test]
fn log_flags_accept_commands_with_equals_arguments() {
    let repository = Repository::new();
    let log = tempfile::NamedTempFile::new().unwrap();
    let command = "pytest --maxfail=1";
    fs::write(log.path(), format!("$ {command}\n7 passed in 0.10s\n")).unwrap();
    let output = repository.check_with(
        &format!("Ran `{command}`, 7 passed\n"),
        &["--log", &format!("{command}={}", log.path().display())],
        &[],
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
}

#[test]
fn normal_stdout_is_not_an_extra_test_summary() {
    let repository = Repository::new();
    let log = tempfile::NamedTempFile::new().unwrap();
    let mut failures = Vec::new();
    for (command, body) in [
        (
            "pytest",
            "100 items processed in 0.05s\n================ 7 passed in 0.10s ================\n",
        ),
        ("jest", "Tests for login module\nTests: 7 passed, 7 total\n"),
    ] {
        fs::write(log.path(), format!("$ {command}\n{body}")).unwrap();
        let output = repository.check_with(
            &format!("Ran `{command}`, 7 passed\n"),
            &["--log", &format!("{command}={}", log.path().display())],
            &[],
        );
        if output.status.code() != Some(0) {
            failures.push(stdout(&output));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn parenthesized_cargo_stdout_preserves_a_completed_suite() {
    let repository = Repository::new();
    let log = tempfile::NamedTempFile::new().unwrap();
    let command = "cargo test -- --nocapture";
    fs::write(
        log.path(),
        format!("$ {command}\n     Running tests/query.rs (target/debug/deps/query-1234)\nrunning 1 test\nRunning query (cached)\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n"),
    )
    .unwrap();
    let output = repository.check_with(
        &format!("Ran `{command}`, 1 passed\n"),
        &["--log", &format!("{command}={}", log.path().display())],
        &[],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
    assert_verdict(&output, 1, "test counts in supplied log", "confirmed");
}

#[test]
fn cargo_stdout_does_not_announce_a_suite_but_runner_status_does() {
    let repository = Repository::new();
    let log = tempfile::NamedTempFile::new().unwrap();
    let command = "cargo test -- --nocapture";
    let announcement = "     Running unittests src/lib.rs (target/debug/deps/fixture-1234)";
    let summary = "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s";
    for output_line in [
        "Running initialization",
        "Running query (cached)",
        "Running initialization (phase one)",
        "     Running query (cached)",
        "note:      Running tests/query.rs (target/debug/deps/query-1234)",
    ] {
        for body in [
            format!("{announcement}\nrunning 1 test\n{output_line}\n{summary}\n"),
            format!("{output_line}\n{announcement}\nrunning 1 test\n{summary}\n"),
            format!("{announcement}\nrunning 1 test\n{summary}\n{output_line}\n"),
        ] {
            fs::write(log.path(), format!("$ {command}\n{body}")).unwrap();
            let output = repository.check_with(
                &format!("Ran `{command}`, 1 passed\n"),
                &["--log", &format!("{command}={}", log.path().display())],
                &[],
            );
            assert_eq!(
                output.status.code(),
                Some(0),
                "{body:?}: {}",
                stdout(&output)
            );
            assert_verdict(&output, 1, "test counts in supplied log", "confirmed");
        }
    }

    for next_suite in [
        "     Running unittests src/other.rs (target/debug/deps/other-1234)",
        "     Running tests/other.rs (target/debug/deps/other-1234)",
        "     Running checks/custom (target/debug/deps/custom-1234)",
        "   Doc-tests fixture",
        "Running tests/query.rs (target/debug/deps/query-1234)",
        "Running unittests src/lib.rs",
        "Running tests/query.rs",
        "Doc-tests fixture",
        "     Running unittests src/other.rs",
        "     Running tests/other.rs",
        "     Running checks/custom (",
        "     Running tests/other.rs (target/debug/deps/other-1234) trailing",
        "   Doc-tests fixture trailing",
        "     Running ",
        "   Doc-tests ",
    ] {
        for body in [
            format!("{announcement}\n{next_suite}\nrunning 1 test\n{summary}\n"),
            format!("{announcement}\nrunning 1 test\n{next_suite}\n{summary}\n"),
            format!("{announcement}\nrunning 1 test\n{summary}\n{next_suite}\n"),
            format!("{announcement}\nrunning 1 test\n{summary}\n{next_suite}\nrunning 2 tests\n"),
        ] {
            fs::write(log.path(), format!("$ {command}\n{body}")).unwrap();
            let output = repository.check_with(
                &format!("Ran `{command}`, 1 passed\n"),
                &["--log", &format!("{command}={}", log.path().display())],
                &[],
            );
            assert_eq!(
                output.status.code(),
                Some(3),
                "{body:?}: {}",
                stdout(&output)
            );
            assert_verdict(&output, 1, "test counts in supplied log", "not checkable");
        }
    }

    let body = format!(
        "{announcement}\nrunning 1 test\n{summary}\n     Running tests/query.rs (target/debug/deps/query-1234)\nrunning 1 test\nRunning query (cached)\n{summary}\n   Doc-tests fixture\nrunning 1 test\n{summary}\n"
    );
    fs::write(log.path(), format!("$ {command}\n{body}")).unwrap();
    let output = repository.check_with(
        &format!("Ran `{command}`, 3 passed\n"),
        &["--log", &format!("{command}={}", log.path().display())],
        &[],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
    assert_verdict(&output, 1, "test counts in supplied log", "confirmed");
}

#[test]
fn incomplete_summaries_cannot_leave_a_completed_summary_confirmed() {
    let repository = Repository::new();
    let log = tempfile::NamedTempFile::new().unwrap();
    let mut failures = Vec::new();
    for (command, complete, incomplete, passed) in [
        ("vitest", "Tests 1 passed (1)", "Tests 1 failed (", 1),
        ("jest", "Tests: 1 passed, 1 total", "Tests:", 1),
        ("pytest", "7 passed in 0.10s", "=== 3 failed, 7 pa", 7),
        ("pytest", "7 passed in 0.10s", "7 passed in", 7),
    ] {
        for body in [
            format!("{complete}\n{incomplete}\n"),
            format!("{incomplete}\n{complete}\n"),
            format!("{incomplete}\n"),
        ] {
            fs::write(log.path(), format!("$ {command}\n{body}")).unwrap();
            let output = repository.check_with(
                &format!("Ran `{command}`, {passed} passed\n"),
                &["--log", &format!("{command}={}", log.path().display())],
                &[],
            );
            if output.status.code() != Some(3) {
                failures.push(format!("{body:?}: {}", stdout(&output)));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn suite_loading_errors_cannot_be_accounted_for_by_failed_test_counts() {
    let repository = Repository::new();
    let log = tempfile::NamedTempFile::new().unwrap();
    for command in ["jest", "npx jest"] {
        for (diagnostic, suites, expected) in [
            (
                "FAIL load.test.js\n  Test suite failed to run\n    Cannot find module './missing'\n",
                2,
                1,
            ),
            (
                "FAIL load.test.js\n  ● Test suite failed to run\n    Cannot find module './missing'\n",
                2,
                1,
            ),
            ("", 1, 0),
        ] {
            fs::write(
                log.path(),
                format!("$ {command}\n{diagnostic}FAIL assertion.test.js\nTest Suites: {suites} failed, {suites} total\nTests: 1 failed, 1 total\n"),
            )
            .unwrap();
            let output = repository.check_with(
                &format!("Ran `{command}`, 0 passed, 1 failed\n"),
                &["--log", &format!("{command}={}", log.path().display())],
                &[],
            );
            assert_eq!(output.status.code(), Some(expected), "{}", stdout(&output));
            assert_verdict(
                &output,
                1,
                "test counts in supplied log",
                if expected == 1 {
                    "contradicted"
                } else {
                    "confirmed"
                },
            );
        }
    }
}

#[test]
fn pytest_collection_errors_cannot_be_accounted_for_by_failed_test_counts() {
    let repository = Repository::new();
    let log = tempfile::NamedTempFile::new().unwrap();
    for heading in [
        "_____ ERROR collecting test_broken.py _____",
        "ERROR collecting test_broken.py",
    ] {
        fs::write(
            log.path(),
            format!("$ pytest\n{heading}\n1 error in 0.10s\n"),
        )
        .unwrap();
        let output = repository.check_with(
            "Ran `pytest`, 0 passed, 1 failed\n",
            &["--log", &format!("pytest={}", log.path().display())],
            &[],
        );
        assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
        assert_verdict(&output, 1, "test counts in supplied log", "contradicted");
    }

    fs::write(
        log.path(),
        "$ pytest\n_____ test_assertion _____\nE assert 1 == 2\n1 failed in 0.10s\n",
    )
    .unwrap();
    let output = repository.check_with(
        "Ran `pytest`, 0 passed, 1 failed\n",
        &["--log", &format!("pytest={}", log.path().display())],
        &[],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
    assert_verdict(&output, 1, "test counts in supplied log", "confirmed");
}

#[test]
fn runner_diagnostics_are_scoped_to_the_command_family() {
    let repository = Repository::new();
    let log = tempfile::NamedTempFile::new().unwrap();
    let mut failures = Vec::new();
    for (command, family, summary) in [
        (
            "cargo test",
            "cargo",
            "running 1 test\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n",
        ),
        ("pytest -s", "pytest", "1 passed in 0.10s\n"),
        ("vitest", "javascript", "Tests 1 passed (1)\n"),
        ("npx vitest", "javascript", "Tests 1 passed (1)\n"),
        ("jest", "javascript", "Tests: 1 passed, 1 total\n"),
        ("npx jest", "javascript", "Tests: 1 passed, 1 total\n"),
    ] {
        for (origin, diagnostic) in [
            ("cargo", "error: could not compile fixture"),
            (
                "cargo",
                "error: process didn't exit successfully: signal: 11",
            ),
            (
                "cargo",
                "  process didn't exit successfully: `target/debug/deps/fixture` (exit status: 4)",
            ),
            ("cargo", "error: test failed, to rerun pass --lib"),
            ("cargo", "error: 1 target failed:"),
            ("cargo", "test result: FAILED."),
            ("pytest", "INTERNALERROR> RuntimeError: fixture"),
            ("pytest", "ERROR collecting tests/test_fixture.py"),
            (
                "pytest",
                "_____ ERROR collecting tests/test_fixture.py _____",
            ),
            ("javascript", "Unhandled Errors"),
            ("javascript", "Errors 1 error"),
            ("javascript", "Type Errors 1 error"),
            ("javascript", "FAIL fixture.test.js"),
            ("javascript", "Test Suites: 1 failed, 1 total"),
            ("javascript", "Test Files 1 failed (1)"),
        ] {
            fs::write(log.path(), format!("$ {command}\n{diagnostic}\n{summary}")).unwrap();
            let output = repository.check_with(
                &format!("Ran `{command}`, 1 passed\n"),
                &["--log", &format!("{command}={}", log.path().display())],
                &[],
            );
            let expected = if family == origin { 1 } else { 0 };
            if output.status.code() != Some(expected) {
                failures.push(format!("{command}, {diagnostic:?}: {}", stdout(&output)));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn all_file_actions_refuse_submodule_entries() {
    let repository = Repository::new();
    repository.write("a.txt", "fixture\n");
    let hash = repository.commit();
    repository.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{hash},sub"),
    ]);
    repository.git(&["commit", "-m", "Add link"]);
    let base = repository.git(&["rev-parse", "HEAD"]);
    repository.git(&["update-index", "--force-remove", "sub"]);
    repository.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{hash},newsub"),
    ]);
    repository.git(&["commit", "-m", "Move link"]);
    let output = repository.check_with(
        "Deleted sub\nRenamed sub to newsub\n",
        &["--base", &base],
        &[],
    );
    for line in 1..=2 {
        assert_verdict(&output, line, "committed path action", "not checkable");
    }
}

#[test]
fn clean_checks_from_subdirectories_inspect_all_index_flags() {
    let repository = Repository::new();
    repository.write("src/a.txt", "fixture\n");
    repository.write("outside.txt", "before\n");
    repository.commit();
    repository.git(&["update-index", "--skip-worktree", "outside.txt"]);
    repository.write("outside.txt", "after\n");
    let output = repository.check_with(
        "Tree is clean\n",
        &["--repo", repository.path().join("src").to_str().unwrap()],
        &[],
    );
    assert_eq!(output.status.code(), Some(3), "{output:?}");
}

#[test]
fn list_code_indentation_counts_tab_columns() {
    let repository = Repository::new();
    repository.write("a.txt", "fixture\n");
    repository.commit();
    let output = repository.check("- \t\tTree is clean\n");
    assert_eq!(output.status.code(), Some(3), "{output:?}");
}

#[test]
fn list_padding_tabs_include_the_original_marker_column() {
    let repository = Repository::new();
    repository.write("a.txt", "fixture\n");
    repository.commit();
    for (report, expected) in [
        ("  - \tTree is clean\n", 3),
        ("  * \tTree is clean\n", 3),
        ("1. \t\tTree is clean\n", 3),
        ("  -\tTree is clean\n", 0),
        (" - \tTree is clean\n", 0),
        ("  -    Tree is clean\n", 0),
    ] {
        for args in [vec![], vec!["--json"]] {
            let output = repository.check_with(report, &args, &[]);
            assert_eq!(
                output.status.code(),
                Some(expected),
                "{report:?}: {}",
                stdout(&output)
            );
            if args.is_empty() {
                assert_verdict(
                    &output,
                    1,
                    if expected == 3 {
                        "unsupported"
                    } else {
                        "working tree clean"
                    },
                    if expected == 3 {
                        "not checkable"
                    } else {
                        "confirmed"
                    },
                );
            } else {
                let doc: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(doc["exit_code"], expected);
                assert_eq!(
                    doc["summary"]["confirmed"],
                    if expected == 0 { 1 } else { 0 }
                );
                assert_eq!(
                    doc["summary"]["not_checkable"],
                    if expected == 3 { 1 } else { 0 }
                );
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn pushed_hashes_reject_prefix_collisions_across_object_types() {
    let repository = Repository::new();
    repository.git(&["hash-object", "-w", "-t", "tree", "/dev/null"]);
    repository.write("commit-input", "tree 4b825dc642cb6eb9a060e54bf8d69288fbee4904\nauthor Fixture <fixture@example.invalid> 1 +0000\ncommitter Fixture <fixture@example.invalid> 1 +0000\n\nPrefix fixture 10484\n");
    let commit = repository.git(&["hash-object", "-w", "-t", "commit", "commit-input"]);
    repository.write("blob-input", "prefix fixture blob 34141\n");
    let blob = repository.git(&["hash-object", "-w", "blob-input"]);
    assert_eq!(&commit[..7], "b01b666");
    assert_eq!(&blob[..7], "b01b666");
    repository.git(&["update-ref", "refs/remotes/origin/main", &commit]);
    let output = repository.check_with(
        "Pushed b01b666\nNot pushed b01b666\n",
        &["--remote-ref", "refs/remotes/origin/main"],
        &[],
    );
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    for line in 1..=2 {
        assert_verdict(
            &output,
            line,
            "local tracking ref ancestry",
            "not checkable",
        );
    }
}
