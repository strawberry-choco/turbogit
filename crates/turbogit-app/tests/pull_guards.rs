//! A pull that rebases is a rewrite like any other, so every user-initiated one
//! pays the same two guards the rejected-push recovery pays: a protected branch
//! refuses *before* git is touched, and a rebase that runs records the rewrite
//! backup the Undo verb reads.
//!
//! The seam is [`AppState::dispatch_pull`], the one dispatch every pull verb in
//! the shell routes through — the palette's `Pull`, the branch menu's `Pull` and
//! `Checkout and pull`, the branch row's hover `Pull`. The verbs differ only in
//! label and in whether a checkout precedes the update, so the guarded behaviour
//! is asserted once per shape here, and the source sweep at the bottom of this
//! file is what says no surface bypasses the seam.
//!
//! The pairing is deliberate: each refusing case sits next to the case that must
//! still run, so a guard that widened to "never integrate into a protected
//! branch by pull" or a backup that leaked onto a plain merge fails here too.
//!
//! - protected branch, `Rebase`: refuses, history untouched, not even a fetch.
//! - unprotected branch, `Rebase`: rebases, and the pre-rebase tip is at the
//!   backup ref.
//! - protected branch, `Merge`: merges, because a merge rewrites nothing and so
//!   owes neither guard.
//! - checkout-then-pull: refuses before the worktree moves, and otherwise
//!   guards the branch the update lands on.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;
use test_support::git_seed::{commit, git, repo_with_origin};
use turbogit_app::state::AppState;
use turbogit_domain::model::{UpdateMethod, VcsSettings};

// ---------------------------------------------------------------- fixtures --

/// The repository a pull runs in: `main` published to a bare `origin`, then
/// genuinely diverged — one local commit, one commit landed on the remote by a
/// second clone. Real divergence, because the only proof that a rebasing pull
/// *ran* is a replayed local commit, and the only proof that a refused one did
/// not is a history the remote's commit never entered.
///
/// The working repository has not fetched, so its `refs/remotes/origin/main` is
/// still the commit it pushed: a change to that ref is evidence of a fetch.
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

/// The [`diverged_repo`] shape with `main` left at its own local commit and HEAD
/// moved to a `feature` branch carrying one commit of its own: the shape a
/// `Checkout and pull` of `main` is asked for.
fn feature_above_a_behind_main(project: &Path, name: &str) -> (PathBuf, PathBuf) {
    let (work, bare) = diverged_repo(project, name);
    git(&work, &["branch", "-f", "feature", "main"]);
    git(&work, &["checkout", "-q", "feature"]);
    commit(&work, "feature.txt", "feature\n", "feature work");
    (work, bare)
}

fn rev(dir: &Path, r: &str) -> String {
    git(dir, &["rev-parse", r]).trim().to_string()
}

/// The branch HEAD is on — how a test says "the checkout never ran".
fn branch_of(dir: &Path) -> String {
    git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .trim()
        .to_string()
}

/// Whether `ancestor` is in `rev`'s history — the way to say "the remote's
/// commit was integrated" without naming a merge or a rebase.
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

/// `origin`'s tip, read from the bare repository itself: the remote's real state,
/// which the working repository only learns about when something fetches.
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

/// Settings with the update method the case names and the protected list either
/// covering `main` (the shipped default) or empty.
fn settings(method: UpdateMethod, protect_main: bool) -> VcsSettings {
    VcsSettings {
        update_method: method,
        protected_branch_patterns: if protect_main {
            vec!["main".to_string(), "master".to_string()]
        } else {
            Vec::new()
        },
        ..VcsSettings::default()
    }
}

/// A state over the fixture's single root, with the failure record cleared so a
/// test can read the outcome of exactly one dispatch.
fn state_over(project: &Path, work: &Path, method: UpdateMethod, protect_main: bool) -> AppState {
    let roots = [work.to_path_buf()];
    let mut state =
        AppState::for_roots(project, &roots).with_settings(settings(method, protect_main));
    state.last_error = None;
    state
}

/// One dispatch, settled: drain until the state stops moving, the headless form
/// of the frames a live Shell spends between the verb and its notification.
fn settle(state: &mut AppState) {
    for _ in 0..6 {
        if state.drain_events() == 0 {
            break;
        }
    }
}

/// Whether the completion the developer sees is a success.
fn succeeded(state: &AppState) -> bool {
    match &state.ui.toast {
        Some(toast) => matches!(toast.kind, turbogit_app::state::ToastKind::Success),
        None => false,
    }
}

const BACKUP: &str = "refs/turbogit/preflight-backup";

// -------------------------------------------------------------------- tests --

/// `Pull` with `Rebase` as the update method on a protected branch would rewrite
/// that branch, so it refuses — and refuses before any git command: no fetch
/// moved the remote-tracking ref, no backup claimed the repository's single
/// backup slot, and no commit was replayed.
#[test]
fn a_manual_rebasing_pull_refuses_a_protected_branch_and_touches_nothing() {
    let tmp: TempDir = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let (work, bare) = diverged_repo(&project, "alpha");
    let mut state = state_over(&project, &work, UpdateMethod::Rebase, true);
    let root = state.multi.roots[0].id.clone();

    let head_before = rev(&work, "HEAD");
    let fetched_before = rev(&work, "refs/remotes/origin/main");
    let remote_commit = bare_tip(&bare, "refs/heads/main").expect("origin/main exists");

    state.dispatch_pull("Pull", root, None);
    settle(&mut state);

    assert_eq!(
        rev(&work, "HEAD"),
        head_before,
        "a protected branch must not be rewritten by a pull"
    );
    assert!(
        !is_ancestor(&work, &remote_commit, "HEAD"),
        "the remote's commit was integrated anyway — the pull ran"
    );
    assert_eq!(
        rev(&work, "refs/remotes/origin/main"),
        fetched_before,
        "the refusal precedes the fetch, so not even a remote-tracking ref moved"
    );
    assert!(
        !ref_exists(&work, BACKUP),
        "no rewrite ran, so no rewrite may have claimed the single backup slot"
    );
    // A refusal is an answer, never silence: the same text every other rewrite's
    // guard produces, on the surface the developer is looking at.
    let refusal = state.last_error.clone().expect("a refused pull says so");
    assert!(
        refusal.contains("protected branch"),
        "the refusal names the guard, got {refusal}"
    );
    assert!(!state.ui.busy, "the refusal settles rather than hanging");
}

/// The same pull on an unprotected branch is allowed — and must record the state
/// it is about to destroy at the backup reference, the same net every other
/// rewrite pays. Without it a rebasing pull has nothing to undo.
#[test]
fn a_manual_rebasing_pull_records_its_backup_before_it_rewrites() {
    let tmp: TempDir = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let (work, bare) = diverged_repo(&project, "alpha");
    let mut state = state_over(&project, &work, UpdateMethod::Rebase, false);
    let root = state.multi.roots[0].id.clone();

    let head_before = rev(&work, "HEAD");
    let remote_commit = bare_tip(&bare, "refs/heads/main").expect("origin/main exists");

    state.dispatch_pull("Pull", root, None);
    settle(&mut state);

    assert!(succeeded(&state), "the pull ran, not refused");
    assert!(
        is_ancestor(&work, &remote_commit, "HEAD"),
        "the rebasing pull must have integrated the remote's commit"
    );
    assert_ne!(
        rev(&work, "HEAD"),
        head_before,
        "a rebase replays the local commit, so the tip moves"
    );
    assert_eq!(
        rev(&work, BACKUP),
        head_before,
        "the rewrite backup must hold the pre-rebase tip"
    );
}

/// The guard is on the rewrite, not on the branch: a protected branch pulled by
/// a merge is not rewritten, so the verb still runs and owes no backup.
#[test]
fn a_manual_merging_pull_still_runs_on_a_protected_branch() {
    let tmp: TempDir = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let (work, bare) = diverged_repo(&project, "alpha");
    let mut state = state_over(&project, &work, UpdateMethod::Merge, true);
    let root = state.multi.roots[0].id.clone();

    let remote_commit = bare_tip(&bare, "refs/heads/main").expect("origin/main exists");

    state.dispatch_pull("Pull", root, None);
    settle(&mut state);

    assert!(succeeded(&state), "a merge owes neither guard");
    assert!(
        is_ancestor(&work, &remote_commit, "HEAD"),
        "the merging pull must integrate the remote's commit"
    );
    assert!(
        !ref_exists(&work, BACKUP),
        "a merge rewrites nothing, so it must not consume the backup slot"
    );
}

/// Checkout-then-pull names the branch it means to rebase before anything runs,
/// so the refusal is answered *before* the worktree moves: a developer whose
/// `Checkout and pull` was refused is still on the branch they started on.
#[test]
fn a_checkout_and_pull_refuses_a_protected_branch_before_the_checkout() {
    let tmp: TempDir = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let (work, _bare) = feature_above_a_behind_main(&project, "alpha");
    let mut state = state_over(&project, &work, UpdateMethod::Rebase, true);
    let root = state.multi.roots[0].id.clone();
    let head_before = rev(&work, "HEAD");

    state.dispatch_pull("Pull main", root, Some("main".to_string()));
    settle(&mut state);

    assert_eq!(
        branch_of(&work),
        "feature",
        "a refused checkout-and-pull must not leave the user on `main`"
    );
    assert_eq!(rev(&work, "HEAD"), head_before, "nor move it");
    assert!(
        !ref_exists(&work, BACKUP),
        "no rewrite ran, so no backup was claimed"
    );
    let refusal = state.last_error.clone().expect("a refused pull says so");
    assert!(
        refusal.contains("protected branch"),
        "the refusal names the guard, got {refusal}"
    );
}

/// The same composite on an unprotected branch runs, and the rewrite it performs
/// is the one after the checkout — which is why the guard and the backup are read
/// against the branch the update lands on, not the one the verb started on.
#[test]
fn a_checkout_and_pull_of_an_unprotected_branch_rebases_and_records_its_backup() {
    let tmp: TempDir = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let (work, bare) = feature_above_a_behind_main(&project, "alpha");
    let mut state = state_over(&project, &work, UpdateMethod::Rebase, false);
    let root = state.multi.roots[0].id.clone();

    // What `main` is about to be rewritten away from, and what the pull has to
    // integrate — neither is HEAD, which is on `feature` when the verb is clicked.
    let main_before = rev(&work, "refs/heads/main");
    let remote_commit = bare_tip(&bare, "refs/heads/main").expect("origin/main exists");

    state.dispatch_pull("Pull main", root, Some("main".to_string()));
    settle(&mut state);

    assert_eq!(
        branch_of(&work),
        "main",
        "the checkout half of the composite ran"
    );
    assert!(
        succeeded(&state),
        "the pull ran, not refused: {:?}",
        state.ui.toast
    );
    assert!(
        is_ancestor(&work, &remote_commit, "HEAD"),
        "the rebasing pull must have integrated the remote's commit"
    );
    assert_eq!(
        rev(&work, BACKUP),
        main_before,
        "the backup holds the tip the rebase rewrote away from"
    );
}

// ---------------------------------------------------------------- the sweep --

/// The claim "every user-initiated pull is guarded" is checked against the
/// sources rather than against a list a later verb can forget to join: no file in
/// the UI crate may call the port's `pull`, because the guarded seam is what
/// owes the two guards. The one exception is the sidebar's `Pull selection`,
/// which passes the rebase flag as the literal `false` and so rewrites nothing —
/// a merge pull owes neither refusal nor backup.
#[test]
fn no_surface_pulls_the_executor_beyond_the_guarded_seam() {
    let ui_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../turbogit-ui/src");
    let offenders: Vec<String> = test_support::srcscan::call_sites(&ui_src, "pull")
        .into_iter()
        .filter(|(file, _)| file != "ui/sidebar.rs")
        .map(|(file, line)| format!("{file}:{line}"))
        .collect();
    assert!(
        offenders.is_empty(),
        "every pull verb must route through `AppState::dispatch_pull`; \
         unguarded `pull` call sites: {offenders:?}"
    );
}
