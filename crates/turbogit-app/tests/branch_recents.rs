use std::path::{Path, PathBuf};

use test_support::git_seed::{GitEnv, git_with_env};
use turbogit_app::state::AppState;
use turbogit_domain::model::BranchKind;

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

    assert_eq!(names(&state, &work), Vec::<String>::new());
}
