//! The Branches tab's Recent group is derived from git, not remembered by the
//! app.
//!
//! The three claims that make the group worth building rather than bookkeeping:
//! a checkout made in a terminal is already in it, a checkout made here is at
//! the top of it on the frame it settles, and a repository with nothing to show
//! answers empty instead of failing. Each is asserted through the same seam the
//! surface uses — [`AppState::warm_branch_recents`] — because the ordering, the
//! join against the branch snapshot and the cap are the view model's business
//! and are pinned in `crates/turbogit-ui/src/ui/branches_tree.rs`.

use std::path::{Path, PathBuf};

use test_support::git_seed::{GitEnv, git_with_env};
use turbogit_app::state::AppState;
use turbogit_domain::model::BranchKind;

// No repository `user.*` here, so every commit this fixture makes is pinned per
// call through the fixture crate's runner: a headless machine has no global
// identity to borrow.

/// A repo on `main` with `a`, `b` and `c`, moved onto in that order **by git
/// itself** — the fixture never touches the app, which is the point.
fn three_checkouts() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let work = project.join("work");
    git_with_env(
        &project,
        &["init", "-q", "-b", "main", work.to_str().unwrap()],
        &GitEnv::fixture(),
    );
    std::fs::write(work.join("README.md"), "x\n").unwrap();
    git_with_env(&work, &["add", "."], &GitEnv::fixture());
    git_with_env(&work, &["commit", "-q", "-m", "init"], &GitEnv::fixture());
    for b in ["a", "b", "c"] {
        git_with_env(&work, &["branch", b], &GitEnv::fixture());
    }
    for b in ["a", "b", "c"] {
        git_with_env(&work, &["checkout", "-q", b], &GitEnv::fixture());
    }
    (tmp, work)
}

fn names(state: &AppState, work: &Path) -> Vec<String> {
    let root = state
        .multi
        .roots
        .iter()
        .find(|r| r.path == work)
        .expect("the fixture repo is a root")
        .id
        .clone();
    state
        .caches
        .recents(&root)
        .unwrap_or_default()
        .iter()
        .map(|r| r.name.clone())
        .collect()
}

#[test]
fn checkouts_made_outside_the_app_are_already_in_the_history() {
    let (_tmp, work) = three_checkouts();
    let mut state = AppState::for_roots(work.parent().unwrap(), std::slice::from_ref(&work));
    state.warm_branch_recents();

    // Newest first, and nothing the app itself did: the group is HEAD's reflog.
    assert_eq!(
        names(&state, &work),
        vec!["c", "b", "a"],
        "the reflog is the source, so a restart shows the same history"
    );
}

#[test]
fn a_checkout_made_here_reaches_the_top_of_the_history_it_settled_on() {
    let (_tmp, work) = three_checkouts();
    let mut state = AppState::for_roots(work.parent().unwrap(), std::slice::from_ref(&work));
    state.warm_branch_recents();
    let root = state.multi.roots[0].id.clone();

    // The app moves onto `a`, which the warmed history already lists last.
    state.checkout_branch_op(&root, BranchKind::Local, "a");
    state.drain_events();
    state.warm_branch_recents();

    assert_eq!(
        names(&state, &work).first().map(String::as_str),
        Some("a"),
        "the refresh the checkout settled with dropped the stale entry, so the\n         re-read puts the move the app just made first"
    );
}

#[test]
fn a_repository_with_no_commits_yet_warms_to_an_empty_history() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let work = project.join("fresh");
    git_with_env(
        &project,
        &["init", "-q", "-b", "main", work.to_str().unwrap()],
        &GitEnv::fixture(),
    );

    let mut state = AppState::for_roots(&project, std::slice::from_ref(&work));
    state.warm_branch_recents();

    // The tab paints this repository's branches before its first commit, so an
    // unborn HEAD must read as "nothing checked out yet" and not as a failure
    // the surface would have to guess the meaning of.
    assert_eq!(names(&state, &work), Vec::<String>::new());
}
