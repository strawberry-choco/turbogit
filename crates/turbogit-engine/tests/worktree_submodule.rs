use std::path::{Path, PathBuf};

use test_support::git_seed::git;
use turbogit_domain::model::{SubmoduleState, VcsSettings};
use turbogit_engine::GitExecutor;
use turbogit_engine::cli::CliExecutor;

fn engine() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

fn temp_repo(tag: &str) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join(tag);
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    std::fs::write(repo.join("base.txt"), "base\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    (tmp, repo.canonicalize().unwrap())
}

#[test]
fn worktree_list_reports_branch_and_probe_free_dirty() {
    let (_tmp, repo) = temp_repo("wt-dirty");
    git(&repo, &["branch", "feature"]);
    let wt_path = _tmp.path().join("wt-feature");
    git(
        &repo,
        &["worktree", "add", wt_path.to_str().unwrap(), "feature"],
    );
    let wt_path = wt_path.canonicalize().unwrap();

    let wts = engine().worktree_list(&repo).expect("worktree_list");
    assert_eq!(wts.len(), 1, "only the linked worktree is listed");
    assert_eq!(wts[0].path, wt_path);
    assert_eq!(wts[0].branch, "feature");
    assert_eq!(wts[0].dirty, None, "the list carries no probe result");

    std::fs::write(wt_path.join("base.txt"), "changed\n").unwrap();
    let wts = engine().worktree_list(&repo).expect("worktree_list");
    assert_eq!(
        wts[0].dirty, None,
        "a modified worktree still lists probe-free"
    );
}

#[test]
fn worktree_remove_refuses_dirty_without_force_and_removes_with_force() {
    let (_tmp, repo) = temp_repo("wt-remove");
    git(&repo, &["branch", "feature"]);
    let wt_path = _tmp.path().join("wt-feature");
    git(
        &repo,
        &["worktree", "add", wt_path.to_str().unwrap(), "feature"],
    );
    let wt_path = wt_path.canonicalize().unwrap();
    std::fs::write(wt_path.join("base.txt"), "changed\n").unwrap();

    let refused = engine().worktree_remove(&repo, &wt_path, false);
    assert!(refused.is_err(), "a dirty worktree needs force");
    assert!(wt_path.exists(), "refused removal leaves the tree alone");

    engine()
        .worktree_remove(&repo, &wt_path, true)
        .expect("forced removal of a dirty worktree");
    assert!(!wt_path.exists(), "removed worktree directory is gone");
    assert!(
        engine().worktree_list(&repo).unwrap().is_empty(),
        "removed worktree leaves the listing"
    );
}

fn temp_super_with_submodule(tag: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let child = tmp.path().join("child-src");
    std::fs::create_dir_all(&child).unwrap();
    git(&child, &["init", "-q", "-b", "main"]);
    git(&child, &["config", "user.email", "test@example.com"]);
    git(&child, &["config", "user.name", "Test"]);
    std::fs::write(child.join("c.txt"), "one\n").unwrap();
    git(&child, &["add", "."]);
    git(&child, &["commit", "-q", "-m", "c1"]);

    let superproject = tmp.path().join(tag);
    std::fs::create_dir_all(&superproject).unwrap();
    git(&superproject, &["init", "-q", "-b", "main"]);
    git(&superproject, &["config", "user.email", "test@example.com"]);
    git(&superproject, &["config", "user.name", "Test"]);
    git(
        &superproject,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            "../child-src",
            "child",
        ],
    );
    git(&superproject, &["commit", "-q", "-m", "add child"]);
    (tmp, superproject, child)
}

#[test]
fn submodule_status_reports_pinned_and_recorded_for_fresh_submodule() {
    let (_tmp, repo, _child) = temp_super_with_submodule("sub-fresh");
    let subs = engine().submodule_status(&repo).expect("submodule_status");
    assert_eq!(subs.len(), 1);
    let sub = &subs[0];
    assert_eq!(sub.path, PathBuf::from("child"));
    assert_eq!(sub.state, SubmoduleState::UpToDate);
    assert_eq!(sub.head, sub.recorded, "fresh checkout matches the record");
    let sha = sub.recorded.as_deref().expect("recorded sha");
    assert_eq!(sha.len(), 40, "a full gitlink sha is reported");
}

#[test]
fn submodule_status_flags_needs_update_when_head_moves_off_record() {
    let (_tmp, repo, _child_src) = temp_super_with_submodule("sub-ahead");
    let sub_wc = repo.join("child");
    std::fs::write(sub_wc.join("c.txt"), "two\n").unwrap();
    git(&sub_wc, &["add", "."]);
    git(&sub_wc, &["commit", "-q", "-m", "c2"]);

    let subs = engine().submodule_status(&repo).expect("submodule_status");
    let sub = &subs[0];
    assert_eq!(sub.state, SubmoduleState::NeedsUpdate);
    assert_ne!(sub.head, sub.recorded);
    let recorded = sub.recorded.as_deref().unwrap();
    assert_eq!(recorded.len(), 40);
}

#[test]
fn submodule_deinit_then_update_round_trips() {
    let (_tmp, repo, _child) = temp_super_with_submodule("sub-cycle");
    engine()
        .submodule_deinit(&repo, Path::new("child"), false)
        .expect("submodule_deinit");

    let subs = engine()
        .submodule_status(&repo)
        .expect("status after deinit");
    assert_eq!(subs[0].state, SubmoduleState::Uninitialized);
    assert_eq!(subs[0].head, None, "uninitialized has no HEAD");
    assert!(subs[0].recorded.is_some(), "the record survives deinit");

    engine()
        .submodule_update(&repo, Path::new("child"), true)
        .expect("submodule update --init");
    let subs = engine()
        .submodule_status(&repo)
        .expect("status after update");
    assert_eq!(subs[0].state, SubmoduleState::UpToDate);
    assert_eq!(subs[0].head, subs[0].recorded);
}

#[test]
fn submodule_update_checks_recorded_commit_back_out() {
    let (_tmp, repo, _child_src) = temp_super_with_submodule("sub-update");
    let sub_wc = repo.join("child");
    std::fs::write(sub_wc.join("c.txt"), "two\n").unwrap();
    git(&sub_wc, &["add", "."]);
    git(&sub_wc, &["commit", "-q", "-m", "c2"]);
    assert_eq!(
        engine().submodule_status(&repo).unwrap()[0].state,
        SubmoduleState::NeedsUpdate
    );

    engine()
        .submodule_update(&repo, Path::new("child"), false)
        .expect("submodule update");

    let subs = engine()
        .submodule_status(&repo)
        .expect("status after update");
    assert_eq!(subs[0].state, SubmoduleState::UpToDate);
    assert_eq!(subs[0].head, subs[0].recorded);
}
