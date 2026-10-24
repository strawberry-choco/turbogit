//! The dialogs', the stash pane's, the rewrite banner's and the commit
//! window's writes are named entries, and each one still runs the git the
//! closure ran.
//!
//! Driven through the Headless harness against real temporary repositories, and
//! asserted as **what the repository becomes** — branch created, tag created
//! and pushed, stash pushed and popped, rewrite undone, rebase aborted, stash
//! applied, commit made — plus the label each operation carries, because the
//! label is what the user reads in the toast.

use std::path::{Path, PathBuf};

use test_support::git_seed::{commit, git, repo_with_history, repo_with_origin};
use turbogit_app::operation::Operation;
use turbogit_app::shell_writes::{
    branch as branch_writes, commit as commit_writes, rewrite as rewrite_writes,
    stash as stash_writes, tag as tag_writes,
};
use turbogit_app::state::{AppState, RewriteUndoVerb};
use turbogit_domain::model::TagSpec;

fn state_for(work: &Path) -> AppState {
    let roots = [work.to_path_buf()];
    AppState::for_roots(work.parent().unwrap(), &roots)
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    git(dir, args).trim().to_string()
}

#[test]
fn create_branch_entry_creates_the_branch_at_its_start_point() {
    let tmp = tempfile::tempdir().unwrap();
    let work: PathBuf = repo_with_history(tmp.path(), "work");
    let mut state = state_for(&work);
    let root = state.selected_root.clone().unwrap();
    let base = git_out(&work, &["rev-parse", "main"]);

    let op = branch_writes::create(&root, "topic", Some(&base), true);
    assert_eq!(op.label(), "Create branch topic");
    state.dispatch(op);
    state.drain_events();

    assert_eq!(
        git_out(&work, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "topic",
        "the entry was asked to check the branch out, and did"
    );
    assert_eq!(
        git_out(&work, &["rev-parse", "topic"]),
        base,
        "the branch starts where the dialog's start point named"
    );
}

#[test]
fn create_tag_entry_creates_the_tag_and_leaves_the_push_off_by_default() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo_with_origin(tmp.path(), "work");
    let mut state = state_for(&work);
    let root = state.selected_root.clone().unwrap();
    let spec = TagSpec {
        name: "v2".into(),
        target: None,
        message: None,
        tagger: None,
        sign: false,
    };

    let op = tag_writes::create(&root, &spec, false);
    assert_eq!(op.label(), "Create tag v2");
    state.dispatch(op);
    state.drain_events();

    assert_eq!(
        git_out(&work, &["tag", "--list", "v2"]),
        "v2",
        "the tag exists locally"
    );
    assert!(
        git_out(&work, &["ls-remote", "--tags", "origin", "v2"]).is_empty(),
        "no push was asked for, so the tag stayed local"
    );
}

#[test]
fn create_tag_entry_pushes_when_asked() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo_with_origin(tmp.path(), "work");
    let mut state = state_for(&work);
    let root = state.selected_root.clone().unwrap();
    let spec = TagSpec {
        name: "v3".into(),
        target: None,
        message: None,
        tagger: None,
        sign: false,
    };

    let op = tag_writes::create(&root, &spec, true);
    assert_eq!(op.label(), "Create tag v3");
    state.dispatch(op);
    state.drain_events();

    assert!(
        git_out(&work, &["ls-remote", "--tags", "origin", "v3"]).contains("v3"),
        "the tag reached the remote"
    );
}

#[test]
fn stash_entry_pushes_and_pop_latest_entry_takes_it_back() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo_with_origin(tmp.path(), "work");
    commit(&work, "wip.txt", "wip\n", "base");
    std::fs::write(work.join("wip.txt"), "changed\n").unwrap();
    assert!(
        !git_out(&work, &["status", "--porcelain"]).is_empty(),
        "the fixture leaves a change for the stash to take"
    );
    let mut state = state_for(&work);
    let root = state.selected_root.clone().unwrap();

    let op = stash_writes::push(&root, "work in progress", false);
    assert_eq!(op.label(), "Stash");
    state.dispatch(op);
    state.drain_events();
    assert!(
        git_out(&work, &["status", "--porcelain"]).is_empty(),
        "the stash took the change out of the working tree"
    );

    let op = stash_writes::pop_latest(&root);
    assert_eq!(op.label(), "Stash pop");
    state.dispatch(op);
    state.drain_events();
    assert_eq!(
        std::fs::read_to_string(work.join("wip.txt")).unwrap(),
        "changed\n",
        "the pop brought the stashed edit back"
    );
}

#[test]
fn commit_window_stash_entries_apply_and_pop_by_index() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo_with_origin(tmp.path(), "work");
    commit(&work, "first.txt", "one\n", "first");
    std::fs::write(work.join("first.txt"), "stashed one\n").unwrap();
    state_for(&work);
    git(&work, &["stash", "push", "-q", "-m", "one"]);
    commit(&work, "second.txt", "two\n", "second");
    std::fs::write(work.join("second.txt"), "stashed two\n").unwrap();
    git(&work, &["stash", "push", "-q", "-m", "two"]);

    let mut state = state_for(&work);
    let root = state.selected_root.clone().unwrap();

    let op = stash_writes::apply_at(&root, "stash@{0}", 0);
    assert_eq!(op.label(), "Apply stash stash@{0}");
    state.dispatch(op);
    state.drain_events();
    assert_eq!(
        std::fs::read_to_string(work.join("second.txt")).unwrap(),
        "stashed two\n",
        "apply puts the entry's content back and keeps the entry"
    );
    assert_eq!(
        git_out(&work, &["stash", "list"]).lines().count(),
        2,
        "an apply leaves both entries in the list"
    );

    // The apply left the edit in the tree, and `stash pop` refuses to overwrite
    // local changes — clear it so the pop is the only thing under test.
    git(&work, &["checkout", "--", "second.txt"]);

    let op = stash_writes::pop_at(&root, "stash@{0}", 0);
    assert_eq!(op.label(), "Pop stash stash@{0}");
    state.dispatch(op);
    state.drain_events();
    assert_eq!(
        git_out(&work, &["stash", "list"]).lines().count(),
        1,
        "a pop removes the entry it took"
    );
}

#[test]
fn committing_the_index_lands_and_an_amend_rewrites_the_last_commit() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo_with_origin(tmp.path(), "work");
    std::fs::write(work.join("new.txt"), "new\n").unwrap();
    let mut state = state_for(&work);
    let root = state.selected_root.clone().unwrap();

    // The commit takes no file list: what lands is the index. The new
    // file has to reach the index first, through the staging entry — which is
    // the whole contract the two share.
    let op = commit_writes::set_staged(&root, Path::new("new.txt"), true);
    assert_eq!(op.label(), "Stage new.txt");
    state.dispatch(op);
    state.drain_events();
    assert_eq!(
        git_out(&work, &["diff", "--cached", "--name-only"]).trim(),
        "new.txt",
        "the stage entry put the file in the index"
    );

    let op = Operation::Commit {
        root: root.clone(),
        message: "add a file".into(),
        amend: false,
        no_verify: false,
    };
    assert_eq!(op.label(), "Commit");
    state.dispatch(op);
    state.drain_events();
    assert_eq!(
        git_out(&work, &["log", "-1", "--pretty=%s"]),
        "add a file",
        "the commit landed"
    );

    let op = Operation::Commit {
        root: root.clone(),
        message: "add a file, amended".into(),
        amend: true,
        no_verify: false,
    };
    assert_eq!(op.label(), "Commit");
    state.dispatch(op);
    state.drain_events();
    assert_eq!(
        git_out(&work, &["log", "-1", "--pretty=%s"]),
        "add a file, amended",
        "the amend replaced the tip rather than adding to it"
    );
    assert_eq!(
        git_out(&work, &["rev-list", "--count", "HEAD"]),
        "2",
        "and it did not add a commit"
    );
}

/// **A commit stages nothing.** The observable fact is *which files
/// land*: a tracked file the developer deliberately left out of the index must
/// stay out of the commit, and stay dirty afterwards. The absence of a stage
/// call is pinned at the services seam, against the recording executor —
/// from here it would only be visible through this same git answer.
#[test]
fn committing_takes_the_index_and_leaves_an_unstaged_file_out() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo_with_origin(tmp.path(), "work");
    std::fs::write(work.join("staged.txt"), "staged\n").unwrap();
    std::fs::write(work.join("kept.txt"), "kept out\n").unwrap();
    git_out(&work, &["add", "staged.txt", "kept.txt"]);
    git_out(&work, &["commit", "-q", "-m", "track both"]);
    std::fs::write(work.join("staged.txt"), "staged and edited\n").unwrap();
    std::fs::write(work.join("kept.txt"), "edited but unstaged\n").unwrap();
    git_out(&work, &["add", "staged.txt"]);
    let mut state = state_for(&work);
    let root = state.selected_root.clone().unwrap();

    let op = Operation::Commit {
        root: root.clone(),
        message: "index only".into(),
        amend: false,
        no_verify: false,
    };
    state.dispatch(op);
    state.drain_events();

    assert_eq!(
        git_out(&work, &["show", "--name-only", "--pretty=", "HEAD"]),
        "staged.txt",
        "the commit is exactly what the index held — the unstaged edit must \
         not be swept in"
    );
    assert!(
        git_out(&work, &["status", "--porcelain"]).contains("kept.txt"),
        "and the file left out of the index is still uncommitted and dirty"
    );
}

#[test]
fn undo_rewrite_entry_restores_the_branch_from_the_backup_ref() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo_with_history(tmp.path(), "work");
    let before = git_out(&work, &["rev-parse", "HEAD"]);
    git(&work, &["reset", "-q", "--hard", "HEAD~2"]);
    let dropped = git_out(&work, &["rev-parse", "HEAD"]);
    git(
        &work,
        &["update-ref", "refs/turbogit/preflight-backup", &before],
    );

    let mut state = state_for(&work);
    let root = state.selected_root.clone().unwrap();

    let op = rewrite_writes::undo(&root, RewriteUndoVerb::Drop, &dropped);
    assert_eq!(op.label(), "Undo drop");
    state.dispatch(op);
    state.drain_events();

    assert_eq!(
        git_out(&work, &["rev-parse", "HEAD"]),
        before,
        "the undo put the dropped commits back"
    );
}

#[test]
fn abort_interactive_rebase_entry_restores_from_the_backup() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo_with_history(tmp.path(), "work");
    let before = git_out(&work, &["rev-parse", "HEAD"]);
    git(
        &work,
        &["update-ref", "refs/turbogit/preflight-backup", &before],
    );
    git(&work, &["reset", "-q", "--hard", "HEAD~1"]);

    let mut state = state_for(&work);
    let root = state.selected_root.clone().unwrap();

    let op = rewrite_writes::abort_interactive_rebase(&root);
    assert_eq!(op.label(), "Abort interactive rebase");
    state.dispatch(op);
    state.drain_events();

    assert_eq!(
        git_out(&work, &["rev-parse", "HEAD"]),
        before,
        "the abort restored the pre-rewrite state"
    );
}
