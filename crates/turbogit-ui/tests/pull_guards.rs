//! The command palette's `Pull` is the surface the unguarded rebase lived on: it
//! read `settings.update_method` and handed the flag straight to the port, so a
//! developer pulling `main` with the `Rebase` method rewrote `main` with no
//! refusal and no backup to undo to.
//!
//! These tests drive the real action entry point — [`popups::run_action`] with
//! `Action::Pull`, the same call the palette's row makes — over a temporary
//! repository and the production event pump, and assert the repository's
//! references, not an internal function's return value.
//!
//! The last test in this file is the other half: the **sidebar's Pull
//! selection**, the fleet verb that is merge-only by construction, which the
//! source sweep used to pin and which now has a live test in its place.

use std::path::{Path, PathBuf};
use std::process::Command;

use egui_kittest::kittest::Queryable as _;
use tempfile::TempDir;
use test_support::git_seed::{commit, git, repo_with_origin};
use test_support::harness::{settle, shell_harness_over_animated};
use turbogit_app::state::{AppState, Toast, ToastKind};
use turbogit_domain::model::{UpdateMethod, VcsSettings};
use turbogit_ui::ui::popups::{Action, run_action};

const BACKUP: &str = "refs/turbogit/preflight-backup";

/// `main` with a commit on `origin` it does not have yet: a pull has something
/// real to integrate, so "the pull ran" is a fact readable from the repository.
fn behind_origin(project: &Path, name: &str) -> PathBuf {
    let work = repo_with_origin(project, name);
    commit(&work, "second.txt", "2\n", "second");
    git(&work, &["push", "-q", "origin", "main"]);
    git(&work, &["reset", "-q", "--hard", "HEAD~1"]);
    work
}

fn rev(dir: &Path, r: &str) -> String {
    git(dir, &["rev-parse", r]).trim().to_string()
}

fn ref_exists(dir: &Path, name: &str) -> bool {
    Command::new("git")
        .args(["show-ref", "--verify", "--quiet", name])
        .current_dir(dir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// How many parents `HEAD` has: **one** after a rebase (the local commits
/// replayed), **two** after an integration.
fn parent_count(dir: &Path) -> usize {
    git(dir, &["rev-list", "--parents", "-n", "1", "HEAD"])
        .split_whitespace()
        .count()
        - 1
}

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

/// One palette action, settled through the production pump.
fn run_and_settle(state: &mut AppState, action: Action) {
    run_action(state, action);
    for _ in 0..6 {
        if state.drain_events() == 0 {
            break;
        }
    }
}

/// A protected branch is never rebased by the palette's `Pull`: the guard's own
/// refusal is what the developer gets, and the repository is exactly as it was.
#[test]
fn the_palettes_pull_refuses_to_rebase_a_protected_branch() {
    let tmp: TempDir = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let work = behind_origin(&project, "alpha");
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&work))
        .with_settings(settings(UpdateMethod::Rebase, true));
    state.last_error = None;
    let head_before = rev(&work, "HEAD");

    run_and_settle(&mut state, Action::Pull);

    assert_eq!(
        rev(&work, "HEAD"),
        head_before,
        "the protected branch was rewritten anyway"
    );
    assert!(
        !ref_exists(&work, BACKUP),
        "a refused rebase must not claim the backup slot"
    );
    let refusal = state.last_error.clone().expect("the refusal is reported");
    assert!(
        refusal.contains("protected branch"),
        "the refusal names the guard, got {refusal}"
    );
    assert!(
        matches!(
            state.ui.toast,
            Some(Toast {
                kind: ToastKind::Error,
                ..
            })
        ),
        "and it is an error the shell paints, not a silent no-op"
    );
}

/// The very same verb on an unprotected branch is allowed — and leaves the net
/// the Undo verb reads, which is the half of the defect a refusal alone would
/// not have fixed.
#[test]
fn the_palettes_pull_records_the_backup_a_rebase_leaves_nothing_to_undo_without() {
    let tmp: TempDir = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let work = behind_origin(&project, "alpha");
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&work))
        .with_settings(settings(UpdateMethod::Rebase, false));
    let head_before = rev(&work, "HEAD");
    let origin_tip = rev(&work, "refs/remotes/origin/main");

    run_and_settle(&mut state, Action::Pull);

    assert_eq!(state.last_error, None, "{:?}", state.ui.toast);
    assert_eq!(
        rev(&work, "HEAD"),
        origin_tip,
        "the pull integrated the remote's commit"
    );
    assert_eq!(
        rev(&work, BACKUP),
        head_before,
        "and recorded the tip it rewrote away from"
    );
}

/// `main` **diverged** from `origin`: one commit only the remote has, and one
/// commit only the local repository has, so a pull has to combine them and the
/// two update methods do not agree on the result.
///
/// `behind_origin` above cannot tell the two apart — a strictly-behind
/// repository fast-forwards under either, and the backup ref is written by the
/// guarded seam, which this verb deliberately does not reach — so the only
/// property that distinguishes "integrated" from "rebased" here is the shape of
/// the commit that lands.
fn diverged_from_origin(project: &Path, name: &str) -> (PathBuf, String) {
    let work = repo_with_origin(project, name);
    commit(&work, "remote.txt", "remote work\n", "remote work");
    git(&work, &["push", "-q", "origin", "main"]);
    // Step back over the remote's commit so the local repository really is
    // behind its own `refs/remotes/origin/main` — pushing updates that ref, so
    // committing on top of it would leave nothing to pull and nothing to prove.
    git(&work, &["reset", "-q", "--hard", "HEAD~1"]);
    commit(&work, "local.txt", "local work\n", "local work");
    let local_tip = rev(&work, "HEAD");
    (work, local_tip)
}

/// The sidebar's `Pull` selection over **two** roots that have diverged from
/// their remotes, under the settings that make the guarded palette verb refuse
/// outright: the selection integrates both, and rewrites neither.
///
/// `UpdateMethod::Rebase` with `main` protected is the sharpest contrast — the
/// palette's pull of either root is refused by the guard (the first test in this
/// file). This verb is merge-only by construction, so both roots move, and each
/// moves by integrating rather than by replaying its local commit.
#[test]
fn the_sidebars_pull_selection_integrates_two_repos_without_rewriting() {
    let tmp: TempDir = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = diverged_from_origin(&project, "alpha");
    let beta = diverged_from_origin(&project, "beta");
    let alpha_local = alpha.1.clone();
    let beta_local = beta.1.clone();
    let (alpha, beta) = (alpha.0, beta.0);

    let state = AppState::for_roots(&project, &[alpha.clone(), beta.clone()])
        .with_settings(settings(UpdateMethod::Rebase, true));
    let mut harness = shell_harness_over_animated(state, egui::vec2(1280.0, 800.0));
    // Both repos selected: the fleet verb exists for the selection, and a
    // single-root selection would make the two-root half of this vacuous.
    harness.state_mut().ui.repo_selection = harness
        .state()
        .multi
        .roots
        .iter()
        .map(|r| r.id.clone())
        .collect();
    settle(&mut harness);

    // The selection bar's own `Pull`, found by its accessibility label so a
    // same-named control elsewhere in the shell cannot be what was aimed at.
    harness.get_by_label("Pull selection").click();
    settle(&mut harness);

    let toast = harness.state().ui.toast.clone();
    assert_eq!(
        harness.state().last_error,
        None,
        "the selection integrates; it owes no refusal: {toast:?}"
    );
    assert!(
        matches!(
            &toast,
            Some(Toast {
                message,
                kind: ToastKind::Success,
                ..
            }) if message == "Pull · 2 repos"
        ),
        "the fleet verb reports itself by name: {toast:?}"
    );

    for (work, local_tip) in [(&alpha, &alpha_local), (&beta, &beta_local)] {
        let name = work.display();
        let remote_tip = rev(work, "refs/remotes/origin/main");
        assert_ne!(
            remote_tip, *local_tip,
            "the fixture really did diverge, or nothing below is evidence: {name}"
        );
        assert_ne!(
            rev(work, "HEAD"),
            *local_tip,
            "the pull moved the branch in {name}"
        );
        assert_eq!(
            parent_count(work),
            2,
            "the pull INTEGRATED the remote's commit, which is what a two-parent \
             HEAD says; a rebasing pull replays the local commit onto the remote \
             tip and leaves a single-parent HEAD — {name}"
        );
        assert_eq!(
            rev(work, "HEAD^2"),
            remote_tip,
            "the second parent IS the remote's tip, so the remote's work landed: {name}"
        );
        assert!(
            !ref_exists(work, BACKUP),
            "a merge-only pull rewrites nothing, so {name} claims no backup slot"
        );
    }
}
