use std::path::{Path, PathBuf};

use test_support::git_seed::{commit, git};
use turbogit_app::state::{AppState, PendingConfirm};

fn commit_file(dir: &Path, name: &str, body: &str, msg: &str) -> String {
    commit(dir, name, body, msg);
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

fn seeded_project() -> (tempfile::TempDir, PathBuf, String, String) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let repo = project.join("alpha");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    let c1 = commit_file(&repo, "a.txt", "one\n", "c1");
    let c2 = commit_file(&repo, "b.txt", "two\n", "c2");
    git(&repo, &["checkout", "-q", "-b", "work"]);
    commit_file(&repo, "w.txt", "work\n", "work commit");
    git(&repo, &["checkout", "-q", "-b", "feature", &c1]);
    let fc = commit_file(&repo, "f.txt", "feature\n", "feature work");
    git(&repo, &["checkout", "-q", "work"]);
    (tmp, project, c2, fc)
}

#[test]
fn revert_through_the_confirm_gate_creates_and_commits_the_revert() {
    let (_tmp, project, c2, _fc) = seeded_project();
    let mut state = AppState::for_roots(&project, &[project.join("alpha")]);
    let root_id = state.multi.roots[0].id.clone();

    state.run_confirmed(PendingConfirm::RevertCommit { commit: c2.clone() });
    state.drain_events();
    assert!(
        state.caches.log(&root_id).is_some_and(|cs| cs
            .iter()
            .any(|c| c.message.to_lowercase().starts_with("revert"))),
        "the completion refresh brought the revert into the cached log; last_error={:?}",
        state.last_error
    );

    let subject = git(&project.join("alpha"), &["log", "-1", "--format=%s"]);
    assert!(
        subject.to_lowercase().starts_with("revert"),
        "HEAD must be the revert commit; got {subject:?}"
    );
    assert!(
        !project.join("alpha").join("b.txt").exists(),
        "the reverted change must be undone in the tree"
    );
    assert!(
        state.ui.toast.is_some(),
        "a completed revert surfaces feedback through the toast"
    );
}

#[test]
fn cherry_pick_to_applies_the_commit_onto_the_target_branch() {
    let (_tmp, project, _c2, fc) = seeded_project();
    let repo = project.join("alpha");
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&repo));
    state.settings.protected_branch_patterns.clear();
    let root_id = state.multi.roots[0].id.clone();

    state.cherry_pick_to(fc, "main".to_string());
    state.drain_events();
    assert!(
        state
            .ui
            .toast
            .as_ref()
            .is_some_and(|t| t.kind == turbogit_app::state::ToastKind::Success),
        "the cherry-pick reported success; last_error={:?}",
        state.last_error
    );

    let count = git(&repo, &["rev-list", "--count", "main"])
        .trim()
        .parse::<usize>()
        .unwrap();
    assert_eq!(count, 3, "main must gain exactly one cherry-picked commit");
    let snapshot = state.multi.by_id(&root_id).unwrap();
    assert_eq!(
        snapshot.current_branch.as_deref(),
        Some("work"),
        "cherry-picking must return the repo to the branch it was on"
    );
}

#[test]
fn a_refused_drop_preflight_repeats_the_reason_the_service_gave() {
    let (_tmp, project, _c2, _fc) = seeded_project();
    let repo = project.join("alpha");
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&repo));
    let root = state.multi.roots[0].id.clone();
    let first = git(&repo, &["rev-list", "--max-parents=0", "HEAD"]);
    let first = first.trim().to_string();

    state.open_rewrite_preflight(&root, &first, turbogit_app::state::HistoryVerb::Drop);

    assert_eq!(
        state.ui.dialog, None,
        "no briefing is opened for a plan that does not exist"
    );
    assert_eq!(
        state.ui.dlg.rewrite_preflight, None,
        "and nothing is staged behind it"
    );
    let toast = state.ui.toast.as_ref().expect("the refusal is stated");
    assert_eq!(toast.kind, turbogit_app::state::ToastKind::Error);
    assert!(
        toast
            .message
            .contains("has no first parent to rewrite from"),
        "the toast carries the reason the service gave, not a paraphrase: {:?}",
        toast.message
    );
    assert!(
        toast.message.contains(&first),
        "and it names the commit the service named, in full: {:?}",
        toast.message
    );
    assert!(
        !toast.message.contains("can no longer be dropped"),
        "the old wording said nothing about why: {:?}",
        toast.message
    );
}

#[test]
fn a_reword_editor_that_cannot_be_seeded_is_refused_and_states_why() {
    let (_tmp, project, _c2, _fc) = seeded_project();
    let repo = project.join("alpha");
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&repo));
    let root = state.multi.roots[0].id.clone();

    state.open_reword_editor(&root, "0123456789abcdef0123456789abcdef01234567");

    assert_eq!(state.ui.dialog, None, "no editor opens with nothing in it");
    assert_eq!(state.ui.dlg.reword, None, "and nothing is staged behind it");
    let toast = state.ui.toast.as_ref().expect("the refusal is stated");
    assert_eq!(toast.kind, turbogit_app::state::ToastKind::Error);
    assert_eq!(
        toast.message, "0123456 is no longer in this log",
        "and it names the commit it cannot find: {:?}",
        toast.message
    );
}
