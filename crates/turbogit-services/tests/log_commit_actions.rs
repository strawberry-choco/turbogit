use std::path::{Path, PathBuf};

use test_support::git_seed::{git, pin_eol_lf};
use turbogit_domain::model::VcsSettings;
use turbogit_engine::cli::CliExecutor;
use turbogit_services::integrate_service;

fn commit_file(dir: &Path, name: &str, body: &str, msg: &str) -> String {
    test_support::git_seed::commit(dir, name, body, msg);
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

fn repo_with_feature_branch() -> (PathBuf, String, String) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "user.email", "t@t"]);
    git(&root, &["config", "user.name", "t"]);
    pin_eol_lf(&root);
    let c1 = commit_file(&root, "a.txt", "one\n", "c1");
    let c2 = commit_file(&root, "b.txt", "two\n", "c2 on main");
    git(&root, &["checkout", "-q", "-b", "feature", &c1]);
    let fc = commit_file(&root, "f.txt", "feature\n", "feature work");
    git(&root, &["checkout", "-q", "main"]);
    std::mem::forget(tmp);
    (root, c2, fc)
}

fn clean_settings() -> VcsSettings {
    VcsSettings {
        protected_branch_patterns: vec![],
        ..VcsSettings::default()
    }
}

#[test]
fn cherry_pick_to_refuses_the_current_branch_without_touching_git() {
    let (root, _c2, fc) = repo_with_feature_branch();
    let engine = CliExecutor {
        settings: clean_settings(),
    };
    let before = git(&root, &["rev-parse", "main"]);

    let err = integrate_service::cherry_pick_to(&engine, &root, &fc, "main", &clean_settings())
        .expect_err("the checked-out branch is not a cherry-pick destination");
    assert!(
        err.to_string().contains("the current branch") && err.to_string().contains("'main'"),
        "the refusal must name the guardrail and the branch it is about: {err}"
    );
    assert_eq!(
        git(&root, &["rev-parse", "main"]),
        before,
        "a refused cherry-pick must not move the branch"
    );
    assert_eq!(
        git(&root, &["rev-list", "--count", "main"]).trim(),
        "2",
        "and no commit may land on it"
    );
    assert_eq!(
        git(&root, &["symbolic-ref", "--short", "HEAD"]).trim(),
        "main",
        "the checkout is left exactly as it was"
    );
}

fn stand_on_feature(root: &Path) {
    git(root, &["checkout", "-q", "feature"]);
}

#[test]
fn cherry_pick_to_applies_commit_onto_target_and_returns_to_original_branch() {
    let (root, _c2, fc) = repo_with_feature_branch();
    let engine = CliExecutor {
        settings: clean_settings(),
    };
    stand_on_feature(&root);

    integrate_service::cherry_pick_to(&engine, &root, &fc, "main", &clean_settings())
        .expect("cherry-pick onto another branch should succeed");

    git(&root, &["checkout", "-q", "main"]);
    assert_eq!(
        std::fs::read_to_string(root.join("f.txt")).unwrap(),
        "feature\n",
        "cherry-picked commit must bring its change onto the target branch"
    );
    let count = git(&root, &["rev-list", "--count", "main"])
        .trim()
        .parse::<usize>()
        .unwrap();
    assert_eq!(count, 3, "main must gain exactly one cherry-picked commit");
    stand_on_feature(&root);
    assert_eq!(
        git(&root, &["symbolic-ref", "--short", "HEAD"]).trim(),
        "feature",
        "the branch that was checked out before is checked out again"
    );
}

#[test]
fn cherry_pick_to_refuses_protected_target_branch_without_touching_git() {
    let (root, _c2, fc) = repo_with_feature_branch();
    let engine = CliExecutor {
        settings: clean_settings(),
    };
    stand_on_feature(&root);
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
    stand_on_feature(&root);
    std::fs::write(root.join("a.txt"), "uncommitted\n").unwrap();

    let err = integrate_service::cherry_pick_to(&engine, &root, &fc, "main", &clean_settings())
        .expect_err("dirty worktree must be refused");
    assert!(
        err.to_string().to_lowercase().contains("dirty")
            || err.to_string().to_lowercase().contains("uncommitted"),
        "refusal must explain the dirty-worktree guardrail: {err}"
    );
    let count = git(&root, &["rev-list", "--count", "main"])
        .trim()
        .parse::<usize>()
        .unwrap();
    assert_eq!(count, 2);
}

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
