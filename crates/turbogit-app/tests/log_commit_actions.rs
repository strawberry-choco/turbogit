//! Issue 15 — Log commit actions, app seam: revert through the confirmation
//! gate (`PendingConfirm::RevertCommit` → `run_confirmed`) and cherry-pick
//! onto a chosen branch (`AppState::cherry_pick_to`), both dispatched through
//! the production dispatch worker path with events pumped back in.

use std::path::{Path, PathBuf};

use test_support::git_seed::{commit, git};
use turbogit_app::state::{AppState, PendingConfirm};

// `seeded_project` stays local: `main` with two commits, a `work` branch off the
// tip and a `feature` branch off the FIRST commit, with both SHAs handed back —
// a topology no `git_seed` recipe is, and one that needs no upstream.

/// `commit_file` is `git_seed::commit` plus the new HEAD's SHA.
fn commit_file(dir: &Path, name: &str, body: &str, msg: &str) -> String {
    commit(dir, name, body, msg);
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

/// A project root with one repo checked out on `work` (non-protected) with
/// two commits, plus a `feature` branch off the first commit carrying one
/// extra commit. Returns (project, repo, work c2, feature commit).
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
    // The success toast names the op (demo: feedback for the revert).
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
    // The default patterns protect main/master; this test exercises the
    // happy path (protection itself is pinned at the service seam).
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

    // The commit landed on main exactly once…
    let count = git(&repo, &["rev-list", "--count", "main"])
        .trim()
        .parse::<usize>()
        .unwrap();
    assert_eq!(count, 3, "main must gain exactly one cherry-picked commit");
    // …and the original checkout was restored (the root snapshot refreshed).
    let snapshot = state.multi.by_id(&root_id).unwrap();
    assert_eq!(
        snapshot.current_branch.as_deref(),
        Some("work"),
        "cherry-picking must return the repo to the branch it was on"
    );
}

/// A gate is the first line, not the only one: the history can move between
/// painting the menu and taking the item, and a stale surface can still reach
/// the verb. So the refusal names the service's OWN reason — the one piece of
/// information `Option` used to throw away — rather than a paraphrase that is
/// true of every refusal and helpful for none of them.
#[test]
fn a_refused_drop_preflight_repeats_the_reason_the_service_gave() {
    let (_tmp, project, _c2, _fc) = seeded_project();
    let repo = project.join("alpha");
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&repo));
    let root = state.multi.roots[0].id.clone();
    let first = git(&repo, &["rev-list", "--max-parents=0", "HEAD"]);
    let first = first.trim().to_string();

    // Reached the way a stale surface reaches it, or a history that moved under
    // the menu: straight at the app seam, with the commit the service refuses.
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

/// An editor seeded with nothing would hand the developer an empty field and
/// invite them to type a message for a commit they may no longer have — so a
/// commit the cache does not hold is refused, by name, and no dialog opens.
///
/// The same stale surface that can outrun the plan can outrun the log: this is
/// the app seam's version of the refusal the menu's own gates normally prevent.
#[test]
fn a_reword_editor_that_cannot_be_seeded_is_refused_and_states_why() {
    let (_tmp, project, _c2, _fc) = seeded_project();
    let repo = project.join("alpha");
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&repo));
    let root = state.multi.roots[0].id.clone();

    // Never logged: the cache holds nothing for this root, so there is no
    // message to seed an editor from.
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
