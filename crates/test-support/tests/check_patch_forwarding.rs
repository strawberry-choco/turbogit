//! Issue 00 (deepen-git-engine) — the forwarding wrapper answers
//! `check_patch` the way the engine it wraps does, instead of inheriting the
//! port's rejecting default. A CLI engine inside the wrapper can genuinely
//! check a patch; suites that wrap one must not see "not supported".

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use test_support::RecordingExecutor;
use turbogit_domain::error::TgError;
use turbogit_domain::model::VcsSettings;
use turbogit_engine::GitExecutor;
use turbogit_engine::cli::CliExecutor;

/// Fresh repo on `main` with `file.txt` at "one\n". Returns (guard, path).
fn repo_with_file() -> (tempfile::TempDir, PathBuf) {
    let run = |dir: &Path, args: &[&str]| {
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
    };

    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repo dir");
    run(&repo, &["init", "-b", "main"]);
    run(&repo, &["config", "user.email", "t@t"]);
    run(&repo, &["config", "user.name", "t"]);
    std::fs::write(repo.join("file.txt"), "one\n").expect("write");
    run(&repo, &["add", "."]);
    run(&repo, &["commit", "-m", "c1"]);
    (tmp, repo)
}

/// A patch against `file.txt` at "one\n": applies when `context` matches the
/// file, does not when it does not.
fn patch(context: &str, added: &str) -> turbogit_domain::model::Patch {
    turbogit_engine::patch::parse_patch(&format!(
        "diff --git a/file.txt b/file.txt\n--- a/file.txt\n+++ b/file.txt\n@@ -1 +1,2 @@\n {context}\n+{added}\n"
    ))
}

fn wrapper() -> RecordingExecutor {
    RecordingExecutor::new(Arc::new(CliExecutor {
        settings: VcsSettings::default(),
    }))
}

#[test]
fn an_applicable_patch_checks_clean_through_the_wrapper() {
    let (_tmp, repo) = repo_with_file();
    wrapper()
        .check_patch(&repo, &patch("one", "two"))
        .expect("the wrapper must report its inner CLI engine's clean check");
}

#[test]
fn an_inapplicable_patch_is_rejected_by_git_through_the_wrapper() {
    let (_tmp, repo) = repo_with_file();
    let err = wrapper()
        .check_patch(&repo, &patch("wrong context", "two"))
        .expect_err("the wrapper must report git's own rejection");
    match err {
        TgError::Cli { stderr, .. } => assert!(
            stderr.contains("does not apply") || stderr.contains("patch failed"),
            "git's stderr should travel with the error: {stderr}"
        ),
        other => panic!("expected git's rejection, got {other:?}"),
    }
}
