//! Issue 16 — `check_patch`: would this patch apply? through the engine
//! seam, over real git (tempdir + system `git`). The prediction for the
//! cherry-pick-across targets table is built on this; the repo is never
//! touched by a check.

use std::path::{Path, PathBuf};
use std::process::Command;

use turbogit_domain::error::TgError;
use turbogit_domain::model::VcsSettings;
use turbogit_engine::GitExecutor;
use turbogit_engine::cli::CliExecutor;

fn run_git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawning git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// Fresh repo on `main` with `file.txt` at "one\n". Returns (guard, path).
fn repo_with_file() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repo dir");
    run_git(&repo, &["init", "-b", "main"]);
    run_git(&repo, &["config", "user.email", "t@t"]);
    run_git(&repo, &["config", "user.name", "t"]);
    std::fs::write(repo.join("file.txt"), "one\n").expect("write");
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "c1"]);
    (tmp, repo)
}

fn engine() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

fn patch(context: &str, added: &str) -> String {
    // One physical line each: a `\n\` continuation would swallow the
    // context line's significant leading space.
    format!(
        "diff --git a/file.txt b/file.txt\n--- a/file.txt\n+++ b/file.txt\n@@ -1 +1,2 @@\n {context}\n+{added}\n"
    )
}

#[test]
fn a_clean_patch_checks_ok_and_touches_nothing() {
    let (_tmp, repo) = repo_with_file();
    engine()
        .check_patch(&repo, &patch("one", "two"))
        .expect("a patch matching the tree should check clean");
    assert_eq!(
        std::fs::read_to_string(repo.join("file.txt")).unwrap(),
        "one\n",
        "a check must never mutate the repository"
    );
}

#[test]
fn a_conflicting_patch_is_rejected_with_the_verbatim_stderr() {
    let (_tmp, repo) = repo_with_file();
    let err = engine()
        .check_patch(&repo, &patch("wrong context", "two"))
        .expect_err("a patch that does not apply must be rejected");
    match err {
        TgError::Cli { stderr, .. } => {
            assert!(
                stderr.contains("does not apply") || stderr.contains("patch failed"),
                "the git stderr should travel with the error: {stderr}"
            );
        }
        other => panic!("expected a Cli error, got {other:?}"),
    }
}
