use std::path::PathBuf;

use test_support::git_seed::{commit, git, repo_with_one_commit};
use turbogit_app::confirm::{ConfirmAction, apply};
use turbogit_app::state::{AppState, Dialog, PendingConfirm, UnmergedCommit, UnmergedCommits};
use turbogit_domain::model::{BranchKind, Change, ChangeStatus};

fn tracking_project() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = project.join("alpha");
    git(
        &project,
        &["-c", "init.defaultBranch=main", "init", "alpha"],
    );
    git(&alpha, &["config", "user.email", "t@t"]);
    git(&alpha, &["config", "user.name", "t"]);
    commit(&alpha, "a.txt", "one\n", "init");
    git(&project, &["clone", "--bare", "alpha", "origin.git"]);
    git(&alpha, &["remote", "add", "origin", "../origin.git"]);
    git(&alpha, &["push", "-q", "-u", "origin", "main"]);
    git(&alpha, &["checkout", "-q", "-b", "work"]);
    commit(&alpha, "w.txt", "work\n", "work commit");
    git(&alpha, &["push", "-q", "-u", "origin", "work"]);
    git(&alpha, &["checkout", "-q", "main"]);
    (tmp, project)
}

#[test]
fn a_branch_delete_captures_the_tip_and_the_upstream_before_it_removes_them() {
    let (_tmp, project) = tracking_project();
    let alpha = project.join("alpha");
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    let root = state.selected_root.clone().unwrap();
    let tip = git(&alpha, &["rev-parse", "work"]).trim().to_string();
    state.ui.confirm = Some(PendingConfirm::DeleteLocalBranch {
        name: "work".to_string(),
    });
    state.ui.branches_delete_consequence = Some("This branch is unmerged.".to_string());
    state.ui.branches_delete_unmerged = Some(UnmergedCommits {
        root: root.clone(),
        branch: "work".to_string(),
        merged_target: "main".to_string(),
        commits: vec![UnmergedCommit {
            id: tip.clone(),
            subject: Some("work commit".to_string()),
        }],
        tip_sha: Some(tip.clone()),
    });

    apply(
        &mut state,
        &PendingConfirm::DeleteLocalBranch {
            name: "work".to_string(),
        },
        ConfirmAction::Confirm,
    );

    let pending = state
        .ui
        .branches_delete_pending
        .as_ref()
        .expect("a confirmed delete leaves a record for the undo window");
    assert_eq!(pending.root, root);
    assert_eq!(pending.name, "work");
    assert_eq!(
        pending.tip_sha, tip,
        "the record holds the tip as it was before the delete"
    );
    assert_eq!(
        pending.upstream.as_ref().map(|u| u.git_ref()),
        Some("origin/work".to_string()),
        "the record holds the upstream the branch tracked before the delete"
    );
    assert_eq!(
        pending.unmerged.as_ref().map(|u| u.commits.len()),
        Some(1),
        "the disclosure the question carried travels with the record"
    );

    assert!(state.ui.confirm.is_none(), "the dialog is closed");
    assert!(
        state.ui.branches_delete_consequence.is_none(),
        "the consequence line belongs to the answered question"
    );
    assert!(
        state.ui.branches_delete_unmerged.is_none(),
        "the unmerged disclosure belongs to the answered question"
    );

    state.drain_events();
    assert!(
        state.branch_tip(&root, "work").is_none(),
        "the branch is deleted; the record is what survived it"
    );
}

fn dirty_project() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = repo_with_one_commit(&project, "alpha");
    for name in ["a.txt", "b.txt", "c.txt"] {
        std::fs::write(alpha.join(name), "baseline\n").unwrap();
    }
    commit(&alpha, "keep.txt", "committed\n", "tracked");
    for name in ["a.txt", "b.txt", "c.txt"] {
        std::fs::write(alpha.join(name), "modified\n").unwrap();
    }
    (tmp, project)
}

fn modified(names: &[&str]) -> Vec<Change> {
    names
        .iter()
        .map(|name| Change {
            path: PathBuf::from(name),
            status: ChangeStatus::Modified,
            chunks: vec![],
            staged: false,
            unstaged: true,
            orig_path: None,
        })
        .collect()
}

fn dirty_checkout_project() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = repo_with_one_commit(&project, "alpha");
    commit(&alpha, "notes.txt", "committed\n", "notes");
    git(&alpha, &["branch", "work"]);
    std::fs::write(alpha.join("notes.txt"), "modified\n").unwrap();
    (tmp, project)
}

fn checkout_dirty(root: turbogit_domain::model::RootId) -> PendingConfirm {
    PendingConfirm::CheckoutDirty {
        root,
        target: "work".to_string(),
        kind: BranchKind::Local,
        detach: false,
    }
}

#[test]
fn the_dirty_checkouts_set_aside_parks_the_changes_and_switches() {
    let (_tmp, project) = dirty_checkout_project();
    let alpha = project.join("alpha");
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    let verb = checkout_dirty(state.selected_root.clone().unwrap());

    apply(&mut state, &verb, ConfirmAction::SetAside);
    state.drain_events();

    assert!(state.ui.confirm.is_none(), "the question is answered");
    assert_eq!(
        git(&alpha, &["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "work",
        "the switch the question was about to happen"
    );
    let status = git(&alpha, &["status", "--porcelain"]);
    assert!(
        status.trim().is_empty(),
        "the worktree is clean because the changes were set aside; got {status:?}"
    );
    let parked = git(&alpha, &["stash", "list"]);
    assert!(
        !parked.trim().is_empty(),
        "the changes are recoverable afterwards, which is the whole reason for this answer; \
         got {parked:?}"
    );
}

#[test]
fn the_dirty_checkouts_bring_along_carries_the_changes_and_stashes_nothing() {
    let (_tmp, project) = dirty_checkout_project();
    let alpha = project.join("alpha");
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    let verb = checkout_dirty(state.selected_root.clone().unwrap());

    apply(&mut state, &verb, ConfirmAction::BringAlong);
    state.drain_events();

    assert!(state.ui.confirm.is_none(), "the question is answered");
    assert_eq!(
        git(&alpha, &["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "work",
        "the switch the question was about to happen"
    );
    let carried = std::fs::read_to_string(alpha.join("notes.txt")).unwrap();
    assert_eq!(
        carried, "modified\n",
        "the changes came along: this is what distinguishes the two answers"
    );
    let parked = git(&alpha, &["stash", "list"]);
    assert!(
        parked.trim().is_empty(),
        "nothing was set aside; got {parked:?}"
    );
}

#[test]
fn the_unmerged_link_opens_the_viewer_over_the_disclosure_it_carried() {
    let (_tmp, project) = tracking_project();
    let alpha = project.join("alpha");
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    state.ui.confirm = Some(PendingConfirm::DeleteLocalBranch {
        name: "work".to_string(),
    });
    state.ui.branches_delete_unmerged = Some(UnmergedCommits {
        root: state.selected_root.clone().unwrap(),
        branch: "work".to_string(),
        merged_target: "main".to_string(),
        commits: vec![UnmergedCommit {
            id: git(&alpha, &["rev-parse", "work"]).trim().to_string(),
            subject: Some("work commit".to_string()),
        }],
        tip_sha: None,
    });

    apply(
        &mut state,
        &PendingConfirm::DeleteLocalBranch {
            name: "work".to_string(),
        },
        ConfirmAction::OpenUnmerged,
    );

    assert_eq!(
        state.ui.dialog,
        Some(Dialog::BranchNotFullyMerged),
        "the link opens the viewer"
    );
    assert!(
        state.ui.confirm.is_some(),
        "the question is still open — the link is a look, not an answer"
    );
    assert!(
        state.ui.branches_delete_pending.is_none(),
        "a link that is only looked at must not arm the undo record"
    );
}

#[test]
fn the_discard_questions_shelve_first_parks_the_changes_rather_than_destroying_them() {
    let (_tmp, project) = dirty_project();
    let alpha = project.join("alpha");
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    let changes = modified(&["a.txt", "b.txt", "c.txt"]);

    apply(
        &mut state,
        &PendingConfirm::Discard {
            changes: changes.clone(),
        },
        ConfirmAction::ShelveFirst,
    );
    state.drain_events();

    assert!(state.ui.confirm.is_none(), "the question is answered");
    let status = git(&alpha, &["status", "--porcelain"]);
    assert!(
        status.trim().is_empty(),
        "the worktree is clean once the changes are parked; got {status:?}"
    );
    let parked = git(&alpha, &["stash", "show", "--name-only"]);
    for name in ["a.txt", "b.txt", "c.txt"] {
        assert!(
            parked.contains(name),
            "the parked entry holds {name}; got {parked:?}"
        );
    }
    let parked_entry = git(&alpha, &["stash", "list"]);
    assert!(
        parked_entry.contains("Shelved 3 file(s) before discard"),
        "the entry says how much was parked and what it was parked before, because that is \
         the note the reader meets when they come back for it; got {parked_entry:?}"
    );
    let toast = state
        .ui
        .toast
        .as_ref()
        .map(|t| t.message.clone())
        .unwrap_or_default();
    assert!(
        toast.contains("Shelve"),
        "the verb reports itself, not the alternative it was chosen over; got {toast:?}"
    );
}
