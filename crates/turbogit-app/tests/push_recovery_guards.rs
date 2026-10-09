//! The rejected-push recovery's update is a rewrite like any other, so it pays
//! the same two guards: a protected branch refuses *before* git is touched, and
//! a rebasing update records the rewrite backup it is about to make possible.
//!
//! The path under test is the whole round trip — a push real git rejects, the
//! settlement that classifies it, the silent update the `Update without asking`
//! setting opts into, and the recovery that runs before the re-push. Nothing
//! here reaches for an internal function: `dispatch_push` and `drain_events` are
//! the same seam the Shell drives.
//!
//! The pairing is deliberate. Each guarded case sits next to the case that must
//! still run, so a refusal that widened to "never update a protected branch" or
//! a backup that leaked onto a plain merge fails here too:
//!
//! - protected branch, Rebase update method: refuses, history untouched, remote
//!   untouched, and no rewrite happened.
//! - unprotected branch, Rebase update method: rebases, and the pre-rebase tip
//!   is recorded at the backup ref the Undo verb reads.
//! - protected branch, Merge update method: merges and pushes, because a merge
//!   rewrites nothing and so owes neither guard.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;
use test_support::git_seed::{commit, git, repo_with_origin};
use turbogit_app::operation::PushRequest;
use turbogit_app::state::AppState;
use turbogit_domain::model::{UpdateMethod, VcsSettings};
use turbogit_services::sync_service::PushOutcome;

// ---------------------------------------------------------------- fixtures --

/// The repository the rejected push runs in: `main` published to a bare
/// `origin`, then genuinely diverged — one local commit, one commit landed on
/// the remote by a second clone.
///
/// Real divergence, not an injected error, because the defect this suite exists
/// for is reachable only through git's own `(non-fast-forward)` rejection: the
/// classification is what opts the update in silently.
fn diverged_repo(project: &Path, name: &str) -> (PathBuf, PathBuf) {
    let work = repo_with_origin(project, name);
    let bare = project.join(format!("{name}.git"));
    commit(&work, "local.txt", "local\n", "local work");

    let other = project.join(format!("{name}-other"));
    git(
        project,
        &[
            "clone",
            "-q",
            bare.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    git(&other, &["config", "user.email", "test@example.com"]);
    git(&other, &["config", "user.name", "Test"]);
    commit(&other, "remote-side.txt", "remote\n", "remote work");
    git(&other, &["push", "-q", "origin", "main"]);

    (work, bare)
}

fn rev(dir: &Path, r: &str) -> String {
    git(dir, &["rev-parse", r]).trim().to_string()
}

/// Whether `ancestor` is in `rev`'s history — how a test says "the remote's
/// commit was not integrated" without naming a merge or a rebase.
fn is_ancestor(dir: &Path, ancestor: &str, rev: &str) -> bool {
    Command::new("git")
        .args(["merge-base", "--is-ancestor", ancestor, rev])
        .current_dir(dir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Whether a reference exists at all — the rewrite backup's slot.
fn ref_exists(dir: &Path, name: &str) -> bool {
    Command::new("git")
        .args(["show-ref", "--verify", "--quiet", name])
        .current_dir(dir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// `origin`'s tip, read from the bare repository itself.
fn bare_tip(bare: &Path, refname: &str) -> Option<String> {
    let out = Command::new("git")
        .arg("--git-dir")
        .arg(bare)
        .args(["rev-parse", "--verify", refname])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Settings for the silent-update round trip: the opt-in on, the update method
/// the case names, and the protected list either the shipped default (which
/// covers `main`) or empty.
fn settings(method: UpdateMethod, protect_main: bool) -> VcsSettings {
    VcsSettings {
        update_method: method,
        protected_branch_patterns: if protect_main {
            vec!["main".to_string(), "master".to_string()]
        } else {
            Vec::new()
        },
        auto_update_on_push_rejected: true,
        ..VcsSettings::default()
    }
}

/// One rejected push, settled: dispatch and drain until the state stops moving.
///
/// The recovery dispatches from inside settlement, so one drain is not enough
/// for the round trip; the loop is the headless form of the frames a live Shell
/// spends between the rejection and its notification.
fn push_and_settle(state: &mut AppState, request: PushRequest) {
    state.dispatch_push(request, None);
    for _ in 0..6 {
        if state.drain_events() == 0 {
            break;
        }
    }
}

fn push_request(state: &AppState, settings: &VcsSettings) -> PushRequest {
    PushRequest::single(
        state.multi.roots[0].id.clone(),
        "origin".to_string(),
        "main".to_string(),
        settings,
    )
}

/// The rejection the settlement left on screen, if any.
fn rejection(state: &AppState) -> Option<PushOutcome> {
    state.ui.push_rejection.as_ref().map(|r| r.outcome.clone())
}

/// The class names the recovery's own failure, whichever of the two update
/// classes it landed in.
fn is_update_failure(outcome: &PushOutcome) -> bool {
    matches!(
        outcome,
        PushOutcome::UpdateErrored { .. } | PushOutcome::UpdateConflicted
    )
}

// -------------------------------------------------------------------- tests --

/// `Update without asking` on a protected branch with `Rebase` as the update
/// method must not rewrite the branch behind the user's back. The refusal is
/// the guard's own: nothing moves — not the branch, not the remote, not the
/// backup reference.
#[test]
fn a_silent_rebasing_update_refuses_a_protected_branch_and_rewrites_nothing() {
    let tmp: TempDir = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let (work, bare) = diverged_repo(&project, "alpha");
    let settings = settings(UpdateMethod::Rebase, true);

    let mut state =
        AppState::for_roots(&project, std::slice::from_ref(&work)).with_settings(settings.clone());
    let request = push_request(&state, &settings);

    let head_before = rev(&work, "HEAD");
    let remote_before = bare_tip(&bare, "refs/heads/main").expect("origin/main exists");

    push_and_settle(&mut state, request);

    assert_eq!(
        rev(&work, "HEAD"),
        head_before,
        "a protected branch must not be rewritten by the recovery's update"
    );
    assert!(
        !is_ancestor(&work, &remote_before, "HEAD"),
        "the remote's commit was integrated anyway — the update ran"
    );
    assert_eq!(
        bare_tip(&bare, "refs/heads/main").as_deref(),
        Some(remote_before.as_str()),
        "a refused recovery must not have pushed either"
    );
    assert!(
        !ref_exists(&work, "refs/turbogit/preflight-backup"),
        "no rewrite ran, so no rewrite may have claimed the single backup slot"
    );
    assert!(
        state.ui.push_in_flight.is_none(),
        "the recovery settled rather than hanging"
    );
    let outcome = rejection(&state).expect("a failed recovery always shows its reply");
    assert!(
        is_update_failure(&outcome),
        "the refusal must surface as the update's own failure, got {outcome:?}"
    );
}

/// The same update on an unprotected branch is allowed — and must record the
/// state it is about to destroy at the backup reference, the same net every
/// other rewrite pays. Without it the auto-update's rebase has nothing to undo.
#[test]
fn a_silent_rebasing_update_records_its_backup_before_it_rewrites() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let (work, bare) = diverged_repo(&project, "alpha");
    let settings = settings(UpdateMethod::Rebase, false);

    let mut state =
        AppState::for_roots(&project, std::slice::from_ref(&work)).with_settings(settings.clone());
    let request = push_request(&state, &settings);

    let head_before = rev(&work, "HEAD");
    let remote_commit = bare_tip(&bare, "refs/heads/main").expect("origin/main exists");

    push_and_settle(&mut state, request);

    assert_eq!(
        rejection(&state),
        None,
        "the recovery should have completed; local head {}, remote tip {:?}",
        rev(&work, "HEAD"),
        bare_tip(&bare, "refs/heads/main")
    );
    assert!(
        is_ancestor(&work, &remote_commit, "HEAD"),
        "the rebasing update must have integrated the remote's commit"
    );
    assert_ne!(
        rev(&work, "HEAD"),
        head_before,
        "a rebase replays the local commit, so the tip moves"
    );
    assert_eq!(
        rev(&work, "refs/turbogit/preflight-backup"),
        head_before,
        "the rewrite backup must hold the pre-rebase tip"
    );
    assert_eq!(
        bare_tip(&bare, "refs/heads/main").as_deref(),
        Some(rev(&work, "HEAD").as_str()),
        "the re-push must land the rebased line"
    );
}

/// The guard is on the rewrite, not on the branch: a protected branch updated by
/// a merge is not rewritten, so the silent update still runs and still pushes.
#[test]
fn a_silent_merging_update_still_runs_on_a_protected_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let (work, bare) = diverged_repo(&project, "alpha");
    let settings = settings(UpdateMethod::Merge, true);

    let mut state =
        AppState::for_roots(&project, std::slice::from_ref(&work)).with_settings(settings.clone());
    let request = push_request(&state, &settings);

    let remote_commit = bare_tip(&bare, "refs/heads/main").expect("origin/main exists");

    push_and_settle(&mut state, request);

    assert_eq!(
        rejection(&state),
        None,
        "a merging update owes neither guard; it must simply run; got {:?}",
        rejection(&state)
    );
    assert!(
        is_ancestor(&work, &remote_commit, "HEAD"),
        "the merging update must integrate the remote's commit"
    );
    assert_eq!(
        bare_tip(&bare, "refs/heads/main").as_deref(),
        Some(rev(&work, "HEAD").as_str()),
        "and the re-push must land it"
    );
    assert!(
        !ref_exists(&work, "refs/turbogit/preflight-backup"),
        "a merge rewrites nothing, so it must not consume the backup slot"
    );
}
