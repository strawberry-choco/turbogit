//! Issue 14 — Worktrees & Submodules tool tabs: the app-side data seam.
//!
//! Drives the real CLI engine over temporary repositories through the
//! `AppState::for_roots` headless harness (CONTEXT.md "Headless harness") and
//! asserts only on public surfaces: `AppState::fetch_worktrees` /
//! `fetch_submodules` filling the root caches, `AppState::refresh`
//! invalidating them, and the dispatched tab actions.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use turbogit_app::root_caches::Affected;
use turbogit_app::state::AppState;
use turbogit_domain::model::{RootId, SubmoduleState};

fn run_git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git should be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Canonicalized temp repo with one base commit (canonical because the
/// engine's main-worktree filtering compares paths).
fn temp_repo(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    std::fs::create_dir_all(&path).unwrap();
    run_git(&path, &["init", "-q", "-b", "main"]);
    run_git(&path, &["config", "user.email", "test@example.com"]);
    run_git(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("base.txt"), "base\n").unwrap();
    run_git(&path, &["add", "."]);
    run_git(&path, &["commit", "-q", "-m", "init"]);
    path.canonicalize().unwrap()
}

/// Pump worker events until `pred` holds or the deadline passes.
fn wait_for(state: &mut AppState, pred: impl Fn(&AppState) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        state.drain_events();
        if pred(state) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("condition not met within 10s");
}

/// A worktree of `repo` on an existing branch, canonicalized like the root.
fn add_worktree(repo: &Path, parent: &Path, name: &str, branch: &str) -> PathBuf {
    run_git(repo, &["branch", branch]);
    let wt = parent.join(name);
    run_git(repo, &["worktree", "add", wt.to_str().unwrap(), branch]);
    wt.canonicalize().unwrap()
}

/// A superproject root with one submodule at `child`; returns the
/// canonicalized root and the submodule's relative path.
fn super_with_submodule(parent: &Path, name: &str) -> (PathBuf, PathBuf) {
    let child_src = parent.join(format!("{name}-child-src"));
    std::fs::create_dir_all(&child_src).unwrap();
    run_git(&child_src, &["init", "-q", "-b", "main"]);
    run_git(&child_src, &["config", "user.email", "test@example.com"]);
    run_git(&child_src, &["config", "user.name", "Test"]);
    std::fs::write(child_src.join("c.txt"), "one\n").unwrap();
    run_git(&child_src, &["add", "."]);
    run_git(&child_src, &["commit", "-q", "-m", "c1"]);

    let repo = parent.join(name);
    std::fs::create_dir_all(&repo).unwrap();
    run_git(&repo, &["init", "-q", "-b", "main"]);
    run_git(&repo, &["config", "user.email", "test@example.com"]);
    run_git(&repo, &["config", "user.name", "Test"]);
    run_git(
        &repo,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            &format!("../{name}-child-src"),
            "child",
        ],
    );
    run_git(&repo, &["commit", "-q", "-m", "add child"]);
    (repo.canonicalize().unwrap(), PathBuf::from("child"))
}

#[test]
fn fetch_worktrees_fills_the_cache_with_dirty_state() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = temp_repo(tmp.path(), "alpha");
    let wt = add_worktree(&repo, tmp.path(), "wt-feature", "feature");
    std::fs::write(wt.join("base.txt"), "changed\n").unwrap();

    let state = AppState::for_roots(tmp.path(), std::slice::from_ref(&repo));
    let root = RootId(repo.clone().into());
    let mut state = state;
    state.fetch_worktrees(root.clone());
    wait_for(&mut state, |s| s.caches.worktrees(&root).is_some());

    let wts = state.caches.worktrees(&root).unwrap();
    assert_eq!(wts.len(), 1, "the linked worktree is listed");
    assert_eq!(wts[0].path, wt);
    assert_eq!(wts[0].branch, "feature");
    assert!(wts[0].dirty, "the modified worktree reads dirty");
}

#[test]
fn fetch_submodules_fills_the_cache_with_state() {
    let tmp = tempfile::tempdir().unwrap();
    // Superproject with one submodule (see the engine suite's fixture).
    let child_src = tmp.path().join("child-src");
    std::fs::create_dir_all(&child_src).unwrap();
    run_git(&child_src, &["init", "-q", "-b", "main"]);
    run_git(&child_src, &["config", "user.email", "test@example.com"]);
    run_git(&child_src, &["config", "user.name", "Test"]);
    std::fs::write(child_src.join("c.txt"), "one\n").unwrap();
    run_git(&child_src, &["add", "."]);
    run_git(&child_src, &["commit", "-q", "-m", "c1"]);

    let repo = tmp.path().join("super");
    std::fs::create_dir_all(&repo).unwrap();
    run_git(&repo, &["init", "-q", "-b", "main"]);
    run_git(&repo, &["config", "user.email", "test@example.com"]);
    run_git(&repo, &["config", "user.name", "Test"]);
    run_git(
        &repo,
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
    run_git(&repo, &["commit", "-q", "-m", "add child"]);
    let repo = repo.canonicalize().unwrap();

    let state = AppState::for_roots(tmp.path(), std::slice::from_ref(&repo));
    let root = RootId(repo.into());
    let mut state = state;
    state.fetch_submodules(root.clone());
    wait_for(&mut state, |s| s.caches.submodules(&root).is_some());

    let subs = state.caches.submodules(&root).unwrap();
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].path, PathBuf::from("child"));
    assert_eq!(subs[0].state, SubmoduleState::UpToDate);
    assert_eq!(subs[0].head, subs[0].recorded);
}

#[test]
fn refresh_drops_worktree_and_submodule_cache_entries() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = temp_repo(tmp.path(), "alpha");
    let state = AppState::for_roots(tmp.path(), std::slice::from_ref(&repo));
    let root = RootId(repo.into());
    let mut state = state;

    state.fetch_worktrees(root.clone());
    state.fetch_submodules(root.clone());
    wait_for(&mut state, |s| {
        s.caches.worktrees(&root).is_some() && s.caches.submodules(&root).is_some()
    });

    state.refresh(Affected::All);
    assert!(
        state.caches.worktrees(&root).is_none() && state.caches.submodules(&root).is_none(),
        "refresh invalidates the worktree and submodule caches"
    );
}

// --------------------------------------------------- dispatched actions --

/// `add_worktree` dispatches through the worker and, once the completion
/// toast lands, the refetched cache lists the new worktree.
#[test]
fn add_worktree_works_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = temp_repo(tmp.path(), "alpha");
    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&repo));
    let root = RootId(repo.into());

    let wt = tmp.path().join("wt-feature");
    state.add_worktree(wt.clone(), "feature".to_string());
    wait_for(&mut state, |s| {
        s.ui.toast
            .as_ref()
            .is_some_and(|t| t.kind == turbogit_app::state::ToastKind::Success)
    });

    state.fetch_worktrees(root.clone());
    wait_for(&mut state, |s| s.caches.worktrees(&root).is_some());
    let wts = state.caches.worktrees(&root).unwrap();
    assert_eq!(wts.len(), 1);
    assert_eq!(wts[0].branch, "feature");
    assert_eq!(wts[0].path, wt.canonicalize().unwrap());
}

/// `run_confirmed(RemoveWorktree)` deletes the worktree: the refetched
/// cache no longer lists it and the directory is gone.
#[test]
fn remove_worktree_works_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = temp_repo(tmp.path(), "alpha");
    let wt = add_worktree(&repo, tmp.path(), "wt-feature", "feature");
    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&repo));
    let root = RootId(repo.into());

    state.run_confirmed(turbogit_app::state::PendingConfirm::RemoveWorktree { path: wt.clone() });
    wait_for(&mut state, |s| {
        s.ui.toast
            .as_ref()
            .is_some_and(|t| t.kind == turbogit_app::state::ToastKind::Success)
    });

    assert!(!wt.exists(), "removed worktree directory is gone");
    state.fetch_worktrees(root.clone());
    wait_for(&mut state, |s| s.caches.worktrees(&root).is_some());
    assert!(
        state.caches.worktrees(&root).unwrap().is_empty(),
        "the removed worktree leaves the listing"
    );
}

/// `update_submodule` checks the recorded commit back out on a submodule
/// whose HEAD moved off the record.
#[test]
fn update_submodule_works_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, sub_path) = super_with_submodule(tmp.path(), "alpha");
    // Move the submodule's HEAD off the recorded commit.
    let sub_wc = repo.join(&sub_path);
    std::fs::write(sub_wc.join("c.txt"), "two\n").unwrap();
    run_git(&sub_wc, &["add", "."]);
    run_git(&sub_wc, &["commit", "-q", "-m", "c2"]);

    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&repo));
    let root = RootId(repo.into());
    state.update_submodule(sub_path.clone(), false);
    wait_for(&mut state, |s| {
        s.ui.toast
            .as_ref()
            .is_some_and(|t| t.kind == turbogit_app::state::ToastKind::Success)
    });

    state.fetch_submodules(root.clone());
    wait_for(&mut state, |s| s.caches.submodules(&root).is_some());
    let subs = state.caches.submodules(&root).unwrap();
    assert_eq!(subs[0].state, SubmoduleState::UpToDate);
    assert_eq!(subs[0].head, subs[0].recorded);
}

/// `run_confirmed(DeinitSubmodule)` un-initializes the submodule: the
/// refetched cache reports it uninitialized.
#[test]
fn deinit_submodule_works_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, sub_path) = super_with_submodule(tmp.path(), "alpha");
    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&repo));
    let root = RootId(repo.into());

    state.run_confirmed(turbogit_app::state::PendingConfirm::DeinitSubmodule {
        path: sub_path.clone(),
    });
    wait_for(&mut state, |s| {
        s.ui.toast
            .as_ref()
            .is_some_and(|t| t.kind == turbogit_app::state::ToastKind::Success)
    });

    state.fetch_submodules(root.clone());
    wait_for(&mut state, |s| s.caches.submodules(&root).is_some());
    let subs = state.caches.submodules(&root).unwrap();
    assert_eq!(subs[0].state, SubmoduleState::Uninitialized);
    assert_eq!(subs[0].head, None);
}
