//! The branch pane's checkout writes are named entries, and each one still runs
//! the git the closure ran.
//!
//! These drive [`turbogit_app::shell_writes::branch`] through the Headless
//! harness and assert **what the repository becomes** — HEAD on the branch, the
//! new local branch tracking `origin/<name>`, HEAD detached at the tag's sha, the
//! restored branch back at its tip — plus the label each operation carries. The
//! labels are what the user reads in the toast, so they are a user-facing fact
//! rather than a shape.

use std::path::{Path, PathBuf};

use test_support::git_seed::{commit, git, repo_with_origin};
use turbogit_app::root_caches::Affected;
use turbogit_app::shell_writes::branch;
use turbogit_app::state::AppState;

fn git_out(dir: &Path, args: &[&str]) -> String {
    git(dir, args).trim().to_string()
}

/// One repo with an `origin`, a published `main`, a pushed-then-deleted `feat`
/// (the remote branch row), and a `v1` tag.
fn repo(project: &Path) -> PathBuf {
    let work = repo_with_origin(project, "work");
    git(&work, &["branch", "feat", "main"]);
    git(&work, &["push", "-q", "origin", "feat"]);
    git(&work, &["branch", "-D", "feat"]);
    git(&work, &["tag", "v1"]);
    work
}

/// The headless state over `work`, whose parent project directory holds the bare
/// `origin` the harness walks.
fn state_for(work: &Path) -> AppState {
    let roots = [work.to_path_buf()];
    AppState::for_roots(work.parent().unwrap(), &roots)
}

#[test]
fn checkout_entry_checks_the_branch_out_under_its_own_label() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo(tmp.path());
    let mut state = state_for(&work);
    let root = state.selected_root.clone().unwrap();

    let op = branch::checkout(&root, "feat", Affected::Root(root.clone()));
    assert_eq!(op.label(), "Checkout feat");
    state.dispatch(op);
    state.drain_events();

    assert_eq!(
        git_out(&work, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "feat",
        "the checkout is what moved HEAD"
    );
}

#[test]
fn checkout_new_local_entry_creates_the_branch_at_origin() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo(tmp.path());
    let mut state = state_for(&work);
    let root = state.selected_root.clone().unwrap();

    let op = branch::checkout_new_local(&root, "feat", Affected::Root(root.clone()));
    assert_eq!(op.label(), "Checkout feat (new local)");
    state.dispatch(op);
    state.drain_events();

    assert_eq!(
        git_out(&work, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "feat",
        "the new local branch is what HEAD names"
    );
    assert_eq!(
        git_out(&work, &["rev-parse", "origin/feat"]),
        git_out(&work, &["rev-parse", "feat"]),
        "the local branch starts at the remote branch's tip"
    );
}

#[test]
fn checkout_detached_entry_leaves_head_on_the_tag() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo(tmp.path());
    let mut state = state_for(&work);
    let root = state.selected_root.clone().unwrap();

    let op = branch::checkout_detached(&root, "v1", Affected::Root(root.clone()));
    assert_eq!(op.label(), "Checkout v1");
    state.dispatch(op);
    state.drain_events();

    assert_eq!(
        git_out(&work, &["rev-parse", "HEAD"]),
        git_out(&work, &["rev-parse", "refs/tags/v1"]),
        "HEAD is the tag's commit"
    );
    assert_eq!(
        git_out(&work, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "HEAD",
        "a tag checkout leaves HEAD detached"
    );
}

#[test]
fn restore_entry_brings_the_branch_back_at_its_tip() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo(tmp.path());
    git(&work, &["branch", "doomed", "main"]);
    commit(&work, "doomed.txt", "work\n", "doomed work");
    let tip = git_out(&work, &["rev-parse", "doomed"]);
    git(&work, &["checkout", "-q", "main"]);
    git(&work, &["branch", "-D", "doomed"]);

    let mut state = state_for(&work);
    let root = state.selected_root.clone().unwrap();

    let op = branch::restore(&root, "doomed", &tip);
    assert_eq!(op.label(), "Restore branch doomed");
    state.dispatch(op);
    state.drain_events();

    assert_eq!(
        git_out(&work, &["rev-parse", "doomed"]),
        tip,
        "the restored branch points at the tip the record captured"
    );
}
