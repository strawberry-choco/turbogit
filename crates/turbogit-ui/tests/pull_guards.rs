//! The command palette's `Pull` is the surface the unguarded rebase lived on: it
//! read `settings.update_method` and handed the flag straight to the port, so a
//! developer pulling `main` with the `Rebase` method rewrote `main` with no
//! refusal and no backup to undo to.
//!
//! These tests drive the real action entry point — [`popups::run_action`] with
//! `Action::Pull`, the same call the palette's row makes — over a temporary
//! repository and the production event pump, and assert the repository's
//! references, not an internal function's return value.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;
use test_support::git_seed::{commit, git, repo_with_origin};
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
