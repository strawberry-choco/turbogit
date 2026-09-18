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
fn fetch_worktrees_fills_the_cache_from_the_cheap_list_only() {
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
    assert!(
        wts.iter().all(|w| w.dirty.is_none()),
        "the fill is the cheap list alone — no dirty probe ran"
    );
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

/// An unrelated operation on the focused root (the `OpCompleted` →
/// `refresh(Root)` shape) leaves the cached worktree list in place: nothing
/// refetches the list for an operation that cannot have changed worktrees.
#[test]
fn refresh_on_the_focused_root_keeps_the_worktree_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = temp_repo(tmp.path(), "alpha");
    add_worktree(&repo, tmp.path(), "wt-feature", "feature");
    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&repo));
    let root = RootId(repo.clone().into());

    state.fetch_worktrees(root.clone());
    wait_for(&mut state, |s| s.caches.worktrees(&root).is_some());
    let project =
        |w: &turbogit_domain::model::Worktree| (w.path.clone(), w.branch.clone(), w.dirty);
    let before: Vec<_> = state
        .caches
        .worktrees(&root)
        .unwrap()
        .iter()
        .map(project)
        .collect();

    state.refresh(Affected::Root(root.clone()));
    assert!(
        state.caches.worktrees(&root).is_some(),
        "the worktree cache survives an unrelated root refresh"
    );
    let after: Vec<_> = state
        .caches
        .worktrees(&root)
        .unwrap()
        .iter()
        .map(project)
        .collect();
    assert_eq!(
        before, after,
        "the refresh did not refetch (and thus rewrite) the cached list"
    );
}

/// An operation completed in another root leaves the focused root's worktree
/// cache untouched.
#[test]
fn refresh_on_another_root_leaves_the_focused_root_cache_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let a = temp_repo(tmp.path(), "alpha");
    let b = temp_repo(tmp.path(), "beta");
    let mut state = AppState::for_roots(tmp.path(), &[a.clone(), b.clone()]);
    let root_a = RootId(a.into());

    state.fetch_worktrees(root_a.clone());
    wait_for(&mut state, |s| s.caches.worktrees(&root_a).is_some());

    state.refresh(Affected::Root(RootId(b.as_path().into())));
    assert!(
        state.caches.worktrees(&root_a).is_some(),
        "an op on another root leaves the focused root's worktree cache alone"
    );
}

/// clear all lists on rescan, including roots no longer discovered (ticket 02).
#[test]
fn rescan_clears_every_worktree_list_and_fetch_refills() {
    let tmp = tempfile::tempdir().unwrap();
    let a = temp_repo(tmp.path(), "alpha");
    let b = temp_repo(tmp.path(), "beta");
    let mut state = AppState::for_roots(tmp.path(), &[a.clone(), b.clone()]);
    let root_a = RootId(a.clone().into());
    let root_b = RootId(b.into());
    for root in [&root_a, &root_b] {
        state.fetch_worktrees(root.clone());
    }
    wait_for(&mut state, |s| {
        s.caches.worktrees(&root_a).is_some() && s.caches.worktrees(&root_b).is_some()
    });

    // Rescan a narrower discovery scope: beta's old list must disappear too.
    state.project_dir = a;
    state.rescan();
    assert!(state.caches.worktrees(&root_a).is_none());
    assert!(state.caches.worktrees(&root_b).is_none());
    state.fetch_worktrees(root_a.clone());
    wait_for(&mut state, |s| s.caches.worktrees(&root_a).is_some());
    assert!(state.caches.worktrees(&root_a).unwrap().is_empty());
    assert!(state.caches.worktrees(&root_b).is_none());
}

/// clear old project lists before fetching the new project's list (ticket 02).
#[test]
fn project_switch_clears_every_worktree_list_and_fetch_refills() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let a = temp_repo(first.path(), "alpha");
    let b = temp_repo(first.path(), "beta");
    let c = temp_repo(second.path(), "gamma");
    let wt = add_worktree(&c, second.path(), "wt-feature", "feature");
    let mut state = AppState::for_roots(first.path(), &[a.clone(), b.clone()]);
    state.recents_config_dir = Some(cfg.path().to_path_buf());
    let root_a = RootId(a.into());
    let root_b = RootId(b.into());
    let root_c = RootId(c.clone().into());
    for root in [&root_a, &root_b] {
        state.fetch_worktrees(root.clone());
    }
    wait_for(&mut state, |s| {
        s.caches.worktrees(&root_a).is_some() && s.caches.worktrees(&root_b).is_some()
    });

    state.open_project(&c);
    assert!(state.caches.worktrees(&root_a).is_none());
    assert!(state.caches.worktrees(&root_b).is_none());
    assert!(state.caches.worktrees(&root_c).is_none());
    state.fetch_worktrees(root_c.clone());
    wait_for(&mut state, |s| s.caches.worktrees(&root_c).is_some());
    let list = state.caches.worktrees(&root_c).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].path, wt);
    assert!(state.caches.worktrees(&root_a).is_none());
    assert!(state.caches.worktrees(&root_b).is_none());
}

/// keep sibling lists intact when a real worktree mutation settles (ticket 02).
#[test]
fn worktree_mutation_refills_only_the_affected_root() {
    let tmp = tempfile::tempdir().unwrap();
    let a = temp_repo(tmp.path(), "alpha");
    let b = temp_repo(tmp.path(), "beta");
    let sibling_wt = add_worktree(&b, tmp.path(), "wt-sibling", "sibling");
    let mut state = AppState::for_roots(tmp.path(), &[a.clone(), b.clone()]);
    let root_a = RootId(a.into());
    let root_b = RootId(b.into());
    for root in [&root_a, &root_b] {
        state.fetch_worktrees(root.clone());
    }
    wait_for(&mut state, |s| {
        s.caches.worktrees(&root_a).is_some() && s.caches.worktrees(&root_b).is_some()
    });
    // A recognizable cached value would be reset by a cheap-list refetch.
    state
        .caches
        .update_worktree_dirty(&root_b, &sibling_wt, true);
    let assert_sibling = |s: &AppState| {
        let list = s.caches.worktrees(&root_b).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].path, sibling_wt);
        assert_eq!(list[0].dirty, Some(true));
    };
    let wt = tmp.path().join("wt-feature");
    state.add_worktree(wt.clone(), "feature".into());
    wait_for(&mut state, |s| {
        s.ui.toast
            .as_ref()
            .is_some_and(|t| t.kind == turbogit_app::state::ToastKind::Success)
    });
    assert!(state.caches.worktrees(&root_a).is_none());
    assert_sibling(&state);
    state.fetch_worktrees(root_a.clone());
    wait_for(&mut state, |s| s.caches.worktrees(&root_a).is_some());
    let list = state.caches.worktrees(&root_a).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].path, wt.canonicalize().unwrap());
    assert_sibling(&state);
}

/// surface current fetch errors but silently discard stale errors (ticket 02).
#[test]
fn worktree_fetch_errors_surface_only_for_the_current_epoch() {
    use turbogit_app::events::AppEvent;
    use turbogit_domain::error::TgError;

    let tmp = tempfile::tempdir().unwrap();
    let repo = temp_repo(tmp.path(), "alpha");
    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&repo));
    let root = RootId(repo.into());
    state
        .tx
        .send(AppEvent::WorktreesMutated { root: root.clone() })
        .unwrap();
    state
        .tx
        .send(AppEvent::WorktreesLoaded {
            root: root.clone(),
            worktrees: Err(TgError::Other("list unavailable".into())),
            epoch: 1,
        })
        .unwrap();
    state.drain_events();
    assert!(
        state
            .last_error
            .as_ref()
            .is_some_and(|e| e.contains("list unavailable"))
    );
    assert!(state.caches.worktrees(&root).is_none());

    state.last_error = None;
    state
        .tx
        .send(AppEvent::WorktreesLoaded {
            root: root.clone(),
            worktrees: Err(TgError::Other("list unavailable".into())),
            epoch: 0,
        })
        .unwrap();
    state.drain_events();
    assert!(state.last_error.is_none());
    assert!(state.caches.worktrees(&root).is_none());
    state.fetch_worktrees(root.clone());
    wait_for(&mut state, |s| s.caches.worktrees(&root).is_some());
    assert!(state.last_error.is_none());
}

// --------------------------------------------------- dispatched actions --

/// `add_worktree` dispatches through the worker: once the operation completes
/// it invalidates the cached worktree list, and the refetched cache lists the
/// new worktree.
#[test]
fn add_worktree_works_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = temp_repo(tmp.path(), "alpha");
    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&repo));
    let root = RootId(repo.into());

    let wt = tmp.path().join("wt-feature");
    state.fetch_worktrees(root.clone());
    wait_for(&mut state, |s| s.caches.worktrees(&root).is_some());
    state.add_worktree(wt.clone(), "feature".to_string());
    wait_for(&mut state, |s| {
        s.ui.toast
            .as_ref()
            .is_some_and(|t| t.kind == turbogit_app::state::ToastKind::Success)
    });
    assert!(
        state.caches.worktrees(&root).is_none(),
        "completing an add invalidates the cached list"
    );

    state.fetch_worktrees(root.clone());
    wait_for(&mut state, |s| s.caches.worktrees(&root).is_some());
    let wts = state.caches.worktrees(&root).unwrap();
    assert_eq!(wts.len(), 1);
    assert_eq!(wts[0].branch, "feature");
    assert_eq!(wts[0].path, wt.canonicalize().unwrap());
}

/// `run_confirmed(RemoveWorktree)` deletes the worktree: its completion
/// invalidates the cached list and the refetched cache no longer lists it.
#[test]
fn remove_worktree_works_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = temp_repo(tmp.path(), "alpha");
    let wt = add_worktree(&repo, tmp.path(), "wt-feature", "feature");
    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&repo));
    let root = RootId(repo.into());

    state.fetch_worktrees(root.clone());
    wait_for(&mut state, |s| s.caches.worktrees(&root).is_some());
    state.run_confirmed(turbogit_app::state::PendingConfirm::RemoveWorktree { path: wt.clone() });
    wait_for(&mut state, |s| {
        s.ui.toast
            .as_ref()
            .is_some_and(|t| t.kind == turbogit_app::state::ToastKind::Success)
    });
    assert!(
        state.caches.worktrees(&root).is_none(),
        "completing a remove invalidates the cached list"
    );

    assert!(!wt.exists(), "removed worktree directory is gone");
    state.fetch_worktrees(root.clone());
    wait_for(&mut state, |s| s.caches.worktrees(&root).is_some());
    assert!(
        state.caches.worktrees(&root).unwrap().is_empty(),
        "the removed worktree leaves the listing"
    );
}

// --------------------------------- stale worktree-list settlement --

/// hold a real worker settlement across a mutation, then replay through the pump (ticket 01).
///
/// The mutation is injected while the fetch's result is still *unsettled* — it
/// was dispatched before the mutation and settles after it — so the pump's
/// ordering is fully driven by controlled event injection, never by timing.
#[test]
fn stale_pre_mutation_list_settlement_is_dropped_and_fresh_refetch_is_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = temp_repo(tmp.path(), "alpha");
    let wt = add_worktree(&repo, tmp.path(), "wt-feature", "feature");
    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&repo));
    let root = RootId(repo.clone().into());

    use turbogit_app::events::AppEvent;

    state.fetch_worktrees(root.clone());
    let held = receive_worktree_list(&mut state);
    let AppEvent::WorktreesLoaded {
        worktrees: Ok(rows),
        root: loaded_root,
        ..
    } = &held
    else {
        panic!("expected the real worktree-list worker result");
    };
    assert_eq!(loaded_root, &root);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].path, wt);
    assert!(state.caches.worktrees(&root).is_none());

    // The worker has answered, but the request stays outstanding until its
    // settlement is routed through the pump — that is the window under test.
    run_git(&repo, &["worktree", "remove", wt.to_str().unwrap()]);
    state
        .tx
        .send(AppEvent::WorktreesMutated { root: root.clone() })
        .unwrap();
    state.drain_events();
    assert!(state.caches.worktrees(&root).is_none());

    state.tx.send(held).unwrap();
    state.drain_events();
    assert!(
        state.caches.worktrees(&root).is_none(),
        "the stale pre-mutation settlement is discarded, not stored"
    );

    state.fetch_worktrees(root.clone());
    let fresh = receive_worktree_list(&mut state);
    assert!(
        matches!(&fresh, AppEvent::WorktreesLoaded { root: loaded_root, worktrees: Ok(_), .. } if loaded_root == &root)
    );
    state.tx.send(fresh).unwrap();
    state.drain_events();
    let wts = state.caches.worktrees(&root).unwrap();
    assert!(
        !wts.iter().any(|w| w.path == wt),
        "the removed worktree never reappears through the reader"
    );
    assert_eq!(
        wts.len(),
        0,
        "the fresh post-mutation listing is what the reader returns"
    );
}

/// receive the next real worktree-list result without polling, deferring any
/// unrelated event back onto the channel so dispatch order is never assumed.
fn receive_worktree_list(state: &mut AppState) -> turbogit_app::events::AppEvent {
    use turbogit_app::events::AppEvent;
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut deferred = Vec::new();
    let event = loop {
        let event = state
            .rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
        if matches!(event, AppEvent::WorktreesLoaded { .. }) {
            break event;
        }
        deferred.push(event);
    };
    for event in deferred {
        state.tx.send(event).unwrap();
    }
    event
}

/// reject held successes and errors across reset, then refill after reopening (ticket 02).
fn assert_reset_rejects_late_worktree_list(reset: impl FnOnce(&mut AppState, &Path)) {
    use turbogit_app::events::AppEvent;
    use turbogit_domain::error::TgError;
    let tmp = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let repo = temp_repo(tmp.path(), "alpha");
    let other = temp_repo(tmp.path(), "beta");
    let wt = add_worktree(&repo, tmp.path(), "wt-feature", "feature");
    let mut state = AppState::for_roots(&repo, std::slice::from_ref(&repo));
    state.recents_config_dir = Some(cfg.path().to_path_buf());
    let root = RootId(repo.clone().into());
    state.fetch_worktrees(root.clone());
    let first = receive_worktree_list(&mut state);
    state.tx.send(first).unwrap();
    state.drain_events();
    assert_eq!(state.caches.worktrees(&root).unwrap()[0].path, wt);

    state.fetch_worktrees(root.clone());
    let held = receive_worktree_list(&mut state);
    let AppEvent::WorktreesLoaded { epoch, .. } = &held else {
        unreachable!()
    };
    let old_epoch = *epoch;
    reset(&mut state, &other);
    assert!(state.caches.worktrees(&root).is_none());
    run_git(&repo, &["worktree", "remove", wt.to_str().unwrap()]);
    state.tx.send(held).unwrap();
    state.drain_events();
    assert!(
        state.caches.worktrees(&root).is_none(),
        "reset must reject a late pre-reset list"
    );
    state.last_error = None;
    state
        .tx
        .send(AppEvent::WorktreesLoaded {
            root: root.clone(),
            epoch: old_epoch,
            worktrees: Err(TgError::Other("old project error".into())),
        })
        .unwrap();
    state.drain_events();
    assert!(state.last_error.is_none());

    // Reopening the same root must not reuse its pre-reset epoch.
    state.open_project(&repo);
    state.fetch_worktrees(root.clone());
    let fresh = receive_worktree_list(&mut state);
    state
        .tx
        .send(AppEvent::WorktreesLoaded {
            root: root.clone(),
            epoch: old_epoch,
            worktrees: Err(TgError::Other("late duplicate".into())),
        })
        .unwrap();
    state.drain_events();
    assert!(state.last_error.is_none());
    assert!(state.caches.worktrees(&root).is_none());
    state.tx.send(fresh).unwrap();
    state.drain_events();
    assert!(state.caches.worktrees(&root).unwrap().is_empty());
}

#[test]
fn refresh_all_rejects_late_worktree_list_and_refills() {
    assert_reset_rejects_late_worktree_list(|state, _| state.refresh(Affected::All));
}

#[test]
fn rescan_rejects_late_worktree_list_and_refills() {
    assert_reset_rejects_late_worktree_list(|state, other| {
        state.project_dir = other.to_path_buf();
        state.rescan();
    });
}

#[test]
fn open_project_rejects_late_worktree_list_and_refills() {
    assert_reset_rejects_late_worktree_list(|state, other| state.open_project(other));
}

#[test]
fn attach_workspace_rejects_late_worktree_list_and_refills() {
    assert_reset_rejects_late_worktree_list(|state, other| state.attach_workspace(other));
}

#[test]
fn close_all_projects_rejects_late_worktree_list_and_refills() {
    assert_reset_rejects_late_worktree_list(|state, _| state.close_all_projects());
}

// --------------------------------------------- ticket 03 — dirty probes --

/// Dirty probes only run when requested (the Worktrees window is open): the
/// badge / eager-fill path alone never probes, so the cached rows keep their
/// unknown dirty state until `ensure_worktree_probes` is called.
#[test]
fn probes_do_not_run_until_requested() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = temp_repo(tmp.path(), "alpha");
    let wt = add_worktree(&repo, tmp.path(), "wt-feature", "feature");
    std::fs::write(wt.join("base.txt"), "changed\n").unwrap();
    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&repo));
    let root = RootId(repo.into());

    state.fetch_worktrees(root.clone());
    wait_for(&mut state, |s| s.caches.worktrees(&root).is_some());
    assert!(
        state
            .caches
            .worktrees(&root)
            .unwrap()
            .iter()
            .all(|w| w.dirty.is_none()),
        "no probe ran while the window was never opened"
    );

    state.ensure_worktree_probes();
    wait_for(&mut state, |s| {
        s.caches
            .worktrees(&root)
            .unwrap()
            .iter()
            .any(|w| w.dirty == Some(true))
    });
    assert!(
        state
            .caches
            .worktrees(&root)
            .unwrap()
            .iter()
            .any(|w| w.dirty == Some(true)),
        "opening the window probes the listed worktrees"
    );
}

/// Each worktree's dirty flag fills in as its own probe settles — rows
/// resolve independently, clean and dirty alike.
#[test]
fn probes_fill_each_row_independently() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = temp_repo(tmp.path(), "alpha");
    let clean = add_worktree(&repo, tmp.path(), "wt-clean", "feature");
    let dirty = add_worktree(&repo, tmp.path(), "wt-dirty", "other");
    std::fs::write(dirty.join("base.txt"), "changed\n").unwrap();
    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&repo));
    let root = RootId(repo.into());

    state.fetch_worktrees(root.clone());
    wait_for(&mut state, |s| {
        s.caches.worktrees(&root).is_some_and(|w| w.len() == 2)
    });
    state.ensure_worktree_probes();
    wait_for(&mut state, |s| {
        s.caches
            .worktrees(&root)
            .is_some_and(|w| w.len() == 2 && w.iter().all(|w| w.dirty.is_some()))
    });

    let wts = state.caches.worktrees(&root).unwrap();
    let by_path = |p: &std::path::Path| wts.iter().find(|w| w.path == p).unwrap();
    assert_eq!(
        by_path(&clean).dirty,
        Some(false),
        "a fresh worktree probes clean"
    );
    assert_eq!(
        by_path(&dirty).dirty,
        Some(true),
        "a modified worktree probes dirty"
    );
}

/// Adding a worktree while the window is open re-probes the affected rows so
/// the tab stays fresh: the new row lists probe-free and fills on the next
/// probe pass.
#[test]
fn probes_refill_after_add_while_window_open() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = temp_repo(tmp.path(), "alpha");
    let existing = add_worktree(&repo, tmp.path(), "wt-existing", "feature");
    std::fs::write(existing.join("base.txt"), "changed\n").unwrap();
    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&repo));
    let root = RootId(repo.into());

    state.fetch_worktrees(root.clone());
    wait_for(&mut state, |s| s.caches.worktrees(&root).is_some());
    state.ensure_worktree_probes();
    wait_for(&mut state, |s| {
        s.caches
            .worktrees(&root)
            .is_some_and(|w| w.iter().all(|w| w.dirty.is_some()))
    });

    let wt_new = tmp.path().join("wt-new");
    state.add_worktree(wt_new.clone(), "feature-new".to_string());
    wait_for(&mut state, |s| {
        s.ui.toast
            .as_ref()
            .is_some_and(|t| t.kind == turbogit_app::state::ToastKind::Success)
    });
    state.fetch_worktrees(root.clone());
    wait_for(&mut state, |s| {
        s.caches.worktrees(&root).is_some_and(|w| w.len() == 2)
    });
    let new_row = state
        .caches
        .worktrees(&root)
        .unwrap()
        .iter()
        .find(|w| w.path == wt_new.canonicalize().unwrap())
        .expect("the added worktree is listed");
    assert_eq!(
        new_row.dirty, None,
        "the newly added row is probe-free until the next probe pass"
    );

    state.ensure_worktree_probes();
    wait_for(&mut state, |s| {
        s.caches
            .worktrees(&root)
            .is_some_and(|w| w.iter().all(|w| w.dirty.is_some()))
    });
    let new_row = state
        .caches
        .worktrees(&root)
        .unwrap()
        .iter()
        .find(|w| w.path == wt_new.canonicalize().unwrap())
        .unwrap();
    assert_eq!(new_row.dirty, Some(false), "a fresh worktree probes clean");
}

/// A probe that settles after its worktree has been removed (the row is no
/// longer listed) is a harmless no-op — the settlement must never resurrect a
/// stale row (ticket 03). The module routes the result to
/// `RootCaches::update_worktree_dirty`, which only touches an existing row.
#[test]
fn a_dirty_probe_for_a_missing_row_is_a_noop() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = temp_repo(tmp.path(), "alpha");
    let wt = add_worktree(&repo, tmp.path(), "wt-feature", "feature");
    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&repo));
    let root = RootId(repo.into());

    state.fetch_worktrees(root.clone());
    wait_for(&mut state, |s| s.caches.worktrees(&root).is_some());

    // Drop the cached list entirely, as a project switch / rescan would — the
    // row the in-flight probe targets no longer exists.
    state.caches.invalidate_worktrees(&root);
    assert!(
        state.caches.worktrees(&root).is_none(),
        "the row is gone before the probe settles"
    );

    // The probe settles late for the old path.
    use turbogit_app::events::AppEvent;
    state
        .tx
        .send(AppEvent::WorktreeDirty {
            root: root.clone(),
            path: wt,
            dirty: Ok(true),
        })
        .unwrap();
    state.drain_events();

    assert!(
        state.caches.worktrees(&root).is_none(),
        "a probe for a removed worktree must not bring the row back"
    );
}

/// Probe errors reach the app's normal feedback surface (last_error), like
/// every other git error — the relocated settlement path does not swallow them
/// (ticket 03).
#[test]
fn a_dirty_probe_error_surfaces_to_last_error() {
    use turbogit_app::events::AppEvent;
    use turbogit_domain::error::TgError;

    let tmp = tempfile::tempdir().unwrap();
    let repo = temp_repo(tmp.path(), "alpha");
    let wt = add_worktree(&repo, tmp.path(), "wt-feature", "feature");
    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&repo));
    let root = RootId(repo.into());

    state.fetch_worktrees(root.clone());
    wait_for(&mut state, |s| s.caches.worktrees(&root).is_some());
    state.last_error = None;

    // A probe fails for the listed row.
    state
        .tx
        .send(AppEvent::WorktreeDirty {
            root: root.clone(),
            path: wt,
            dirty: Err(TgError::Other("probe unavailable".into())),
        })
        .unwrap();
    state.drain_events();

    assert_eq!(
        state.last_error.as_deref(),
        Some("probe unavailable"),
        "probe errors are routed to last_error, not swallowed"
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
