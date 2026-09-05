//! Issue 15 — Log commit actions, service seam: cherry-pick onto a chosen
//! branch and revert, with protected-branch and dirty-worktree guardrails.
//!
//! Real git in temporary repositories (the checkout → cherry-pick →
//! checkout-back orchestration is invisible to the in-memory fake), driven
//! through the public `integrate_service` functions only.

use std::path::{Path, PathBuf};

use turbogit_domain::model::VcsSettings;
use turbogit_engine::cli::CliExecutor;
use turbogit_services::integrate_service;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("git must be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn commit_file(dir: &Path, name: &str, body: &str, msg: &str) -> String {
    std::fs::write(dir.join(name), body).unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", msg]);
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

/// A repo on `main` with two commits, plus a `feature` branch forked at the
/// first commit carrying one extra commit. Returns (main tip, feature commit).
fn repo_with_feature_branch() -> (PathBuf, String, String) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "user.email", "t@t"]);
    git(&root, &["config", "user.name", "t"]);
    git(&root, &["config", "core.autocrlf", "false"]);
    let c1 = commit_file(&root, "a.txt", "one\n", "c1");
    let c2 = commit_file(&root, "b.txt", "two\n", "c2 on main");
    git(&root, &["checkout", "-q", "-b", "feature", &c1]);
    let fc = commit_file(&root, "f.txt", "feature\n", "feature work");
    git(&root, &["checkout", "-q", "main"]);
    std::mem::forget(tmp); // keep alive for the test body via leak of path usage
    (root, c2, fc)
}

fn clean_settings() -> VcsSettings {
    VcsSettings {
        protected_branch_patterns: vec![],
        ..VcsSettings::default()
    }
}

// --- cherry_pick_to ------------------------------------------------------------

#[test]
fn cherry_pick_to_applies_commit_onto_target_and_returns_to_original_branch() {
    let (root, _c2, fc) = repo_with_feature_branch();
    let engine = CliExecutor {
        settings: clean_settings(),
    };

    integrate_service::cherry_pick_to(&engine, &root, &fc, "main", &clean_settings())
        .expect("cherry-pick onto main should succeed");

    // The commit landed on main (its file exists at main's tip)…
    git(&root, &["checkout", "-q", "main"]);
    assert_eq!(
        std::fs::read_to_string(root.join("f.txt")).unwrap(),
        "feature\n",
        "cherry-picked commit must bring its change onto the target branch"
    );
    // …exactly once (one extra commit beyond the original two)…
    let count = git(&root, &["rev-list", "--count", "main"])
        .trim()
        .parse::<usize>()
        .unwrap();
    assert_eq!(count, 3, "main must gain exactly one cherry-picked commit");
    // …and HEAD returned to the branch that was checked out before.
    assert_eq!(
        git(&root, &["symbolic-ref", "--short", "HEAD"]).trim(),
        "main",
        "the original branch stays checked out"
    );
}

#[test]
fn cherry_pick_to_refuses_protected_target_branch_without_touching_git() {
    let (root, _c2, fc) = repo_with_feature_branch();
    let engine = CliExecutor {
        settings: clean_settings(),
    };
    let before = git(&root, &["rev-parse", "main"]);

    let settings = VcsSettings {
        protected_branch_patterns: vec!["main".to_string()],
        ..VcsSettings::default()
    };
    let err = integrate_service::cherry_pick_to(&engine, &root, &fc, "main", &settings)
        .expect_err("protected target must be refused");
    assert!(
        err.to_string().contains("protected"),
        "refusal must explain the protected-branch guardrail: {err}"
    );
    assert_eq!(
        git(&root, &["rev-parse", "main"]),
        before,
        "refused cherry-pick must not move the target branch"
    );
    assert!(
        !root.join("f.txt").exists() || git(&root, &["rev-list", "--count", "main"]).trim() == "2",
        "no commit may land on the protected branch"
    );
}

#[test]
fn cherry_pick_to_refuses_dirty_worktree() {
    let (root, _c2, fc) = repo_with_feature_branch();
    let engine = CliExecutor {
        settings: clean_settings(),
    };
    std::fs::write(root.join("a.txt"), "uncommitted\n").unwrap();

    let err = integrate_service::cherry_pick_to(&engine, &root, &fc, "main", &clean_settings())
        .expect_err("dirty worktree must be refused");
    assert!(
        err.to_string().to_lowercase().contains("dirty")
            || err.to_string().to_lowercase().contains("uncommitted"),
        "refusal must explain the dirty-worktree guardrail: {err}"
    );
    // Nothing was applied: main still has two commits.
    let count = git(&root, &["rev-list", "--count", "main"])
        .trim()
        .parse::<usize>()
        .unwrap();
    assert_eq!(count, 2);
}

// --- revert_commit -------------------------------------------------------------

#[test]
fn revert_commit_creates_the_inverse_commit_on_the_current_branch() {
    let (root, c2, _fc) = repo_with_feature_branch();
    let engine = CliExecutor {
        settings: clean_settings(),
    };

    integrate_service::revert_commit(&engine, &root, &c2, &clean_settings())
        .expect("revert should succeed");

    let subject = git(&root, &["log", "-1", "--format=%s"]).trim().to_string();
    assert!(
        subject.to_lowercase().starts_with("revert"),
        "HEAD must be a revert commit; got {subject:?}"
    );
    // The reverted change is undone in the tree.
    assert!(
        !root.join("b.txt").exists(),
        "reverting c2 must remove the file it added"
    );
}

#[test]
fn revert_commit_refuses_protected_current_branch() {
    let (root, c2, _fc) = repo_with_feature_branch();
    let engine = CliExecutor {
        settings: clean_settings(),
    };
    let before = git(&root, &["rev-parse", "HEAD"]);

    let settings = VcsSettings {
        protected_branch_patterns: vec!["main".to_string()],
        ..VcsSettings::default()
    };
    let err = integrate_service::revert_commit(&engine, &root, &c2, &settings)
        .expect_err("revert on a protected branch must be refused");
    assert!(
        err.to_string().contains("protected"),
        "refusal must explain the protected-branch guardrail: {err}"
    );
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), before);
}

#[test]
fn revert_commit_refuses_dirty_worktree() {
    let (root, c2, _fc) = repo_with_feature_branch();
    let engine = CliExecutor {
        settings: clean_settings(),
    };
    std::fs::write(root.join("a.txt"), "uncommitted\n").unwrap();
    let before = git(&root, &["rev-parse", "HEAD"]);

    let err = integrate_service::revert_commit(&engine, &root, &c2, &clean_settings())
        .expect_err("dirty worktree must be refused");
    assert!(
        err.to_string().to_lowercase().contains("dirty")
            || err.to_string().to_lowercase().contains("uncommitted"),
        "refusal must explain the dirty-worktree guardrail: {err}"
    );
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), before);
}
