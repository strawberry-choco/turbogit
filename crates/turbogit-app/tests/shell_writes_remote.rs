use std::path::{Path, PathBuf};

use test_support::git_seed::{git, repo_with_origin};
use turbogit_app::shell_writes::remote as writes;
use turbogit_app::state::AppState;

fn state_for(work: &Path) -> AppState {
    let roots = [work.to_path_buf()];
    AppState::for_roots(work.parent().unwrap(), &roots)
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    git(dir, args).trim().to_string()
}

#[test]
fn add_entry_writes_the_remote_with_its_fetch_url() {
    let tmp = tempfile::tempdir().unwrap();
    let work: PathBuf = repo_with_origin(tmp.path(), "work");
    let mut state = state_for(&work);
    let root = state.selected_root.clone().unwrap();
    let second = tmp.path().join("second.git");
    git(
        tmp.path(),
        &["init", "-q", "--bare", second.to_str().unwrap()],
    );

    let op = writes::add(
        &root,
        work.as_path(),
        "backup",
        second.to_str().unwrap(),
        "",
    );
    assert_eq!(op.label(), "Add remote backup");
    state.dispatch(op);
    state.drain_events();

    assert_eq!(
        git_out(&work, &["remote", "get-url", "backup"]),
        second.to_str().unwrap(),
        "a blank push URL falls back to the fetch URL"
    );
}

#[test]
fn add_entry_prefers_the_push_url_when_it_is_given() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo_with_origin(tmp.path(), "work");
    let mut state = state_for(&work);
    let root = state.selected_root.clone().unwrap();
    let fetch = tmp.path().join("fetch.git");
    let push = tmp.path().join("push.git");
    git(
        tmp.path(),
        &["init", "-q", "--bare", fetch.to_str().unwrap()],
    );
    git(
        tmp.path(),
        &["init", "-q", "--bare", push.to_str().unwrap()],
    );

    let op = writes::add(
        &root,
        work.as_path(),
        "split",
        fetch.to_str().unwrap(),
        push.to_str().unwrap(),
    );
    assert_eq!(op.label(), "Add remote split");
    state.dispatch(op);
    state.drain_events();

    assert_eq!(
        git_out(&work, &["remote", "get-url", "split"]),
        push.to_str().unwrap(),
        "the push URL wins when the form carried one"
    );
}

#[test]
fn rename_entry_carries_the_new_name() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo_with_origin(tmp.path(), "work");
    let mut state = state_for(&work);
    let root = state.selected_root.clone().unwrap();

    let op = writes::rename(&root, work.as_path(), "origin", "upstream");
    assert_eq!(op.label(), "Rename remote origin");
    state.dispatch(op);
    state.drain_events();

    assert!(
        git_out(&work, &["remote", "get-url", "upstream"])
            .starts_with(tmp.path().to_str().unwrap()),
        "the renamed remote keeps its URL under the new name"
    );
}

#[test]
fn update_entry_splits_fetch_and_push_and_keeps_a_blank_side_off() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo_with_origin(tmp.path(), "work");
    let mut state = state_for(&work);
    let root = state.selected_root.clone().unwrap();
    let fresh = tmp.path().join("fresh.git");
    git(
        tmp.path(),
        &["init", "-q", "--bare", fresh.to_str().unwrap()],
    );

    let op = writes::update(&root, work.as_path(), "origin", fresh.to_str().unwrap(), "");
    assert_eq!(op.label(), "Update remote origin");
    state.dispatch(op);
    state.drain_events();

    assert_eq!(
        git_out(&work, &["remote", "get-url", "origin"]),
        fresh.to_str().unwrap(),
        "the fetch side is the URL that was given"
    );
    let all = git_out(&work, &["remote", "-v"]);
    assert_eq!(
        all.lines().filter(|l| l.contains("(push)")).count(),
        1,
        "a blank push URL means 'leave the push side alone', not 'clear it': {all}"
    );
}

#[test]
fn set_upstream_entry_records_the_tracking_ref() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo_with_origin(tmp.path(), "work");
    git(&work, &["branch", "side", "main"]);
    git(&work, &["push", "-q", "origin", "side"]);
    let mut state = state_for(&work);
    let root = state.selected_root.clone().unwrap();

    let op = writes::set_upstream(&root, work.as_path(), "side", "origin/side");
    assert_eq!(op.label(), "Set upstream side → origin/side");
    state.dispatch(op);
    state.drain_events();

    assert_eq!(
        git_out(&work, &["rev-parse", "--abbrev-ref", "side@{upstream}"]),
        "origin/side",
        "the branch now tracks the remote it was pointed at"
    );
}

#[test]
fn remove_entry_drops_the_remote() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo_with_origin(tmp.path(), "work");
    let mut state = state_for(&work);
    let root = state.selected_root.clone().unwrap();

    let op = writes::remove(&root, work.as_path(), "origin");
    assert_eq!(op.label(), "Remove remote origin");
    state.dispatch(op);
    state.drain_events();

    assert!(git_out(&work, &["remote"]).is_empty(), "no remote is left");
}
