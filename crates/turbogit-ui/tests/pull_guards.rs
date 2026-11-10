use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use tempfile::TempDir;
use test_support::git_seed::{commit, git, repo_with_origin};
use turbogit_app::state::{AppState, Toast, ToastKind};
use turbogit_domain::model::{RootId, UpdateMethod, VcsSettings};
use turbogit_services::bulk_ops::{BulkOp, BulkPlan};
use turbogit_services::bulk_run::RowState;
use turbogit_ui::ui::popups::{Action, run_action};

const BACKUP: &str = "refs/turbogit/preflight-backup";

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

fn run_and_settle(state: &mut AppState, action: Action) {
    run_action(state, action);
    for _ in 0..6 {
        if state.drain_events() == 0 {
            break;
        }
    }
}

fn settle_fleet(state: &mut AppState) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        state.drain_events();
        let busy = state.ui.bulk_run.as_ref().is_some_and(|view| {
            view.rows
                .iter()
                .any(|r| matches!(r.state, RowState::Queued { .. } | RowState::Running))
        });
        if !busy && !state.ui.busy {
            return;
        }
        assert!(Instant::now() < deadline, "the fleet pull did not finish");
        std::thread::sleep(Duration::from_millis(10));
    }
}

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

fn diverged_from_origin(project: &Path, name: &str) -> (PathBuf, String) {
    let work = repo_with_origin(project, name);
    commit(&work, "remote.txt", "remote work\n", "remote work");
    git(&work, &["push", "-q", "origin", "main"]);
    // Step back over the remote's commit so the local repository really is
    // behind its own `refs/remotes/origin/main`, pushing updates that ref, so
    // committing on top of it would leave nothing to pull and nothing to prove.
    git(&work, &["reset", "-q", "--hard", "HEAD~1"]);
    commit(&work, "local.txt", "local work\n", "local work");
    let local_tip = rev(&work, "HEAD");
    (work, local_tip)
}

#[test]
fn the_fleet_pull_integrates_two_repos_without_rewriting() {
    let tmp: TempDir = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = diverged_from_origin(&project, "alpha");
    let beta = diverged_from_origin(&project, "beta");
    let alpha_local = alpha.1.clone();
    let beta_local = beta.1.clone();
    let (alpha, beta) = (alpha.0, beta.0);

    let mut state = AppState::for_roots(&project, &[alpha.clone(), beta.clone()])
        .with_settings(settings(UpdateMethod::Merge, false));
    let roots: Vec<RootId> = state.multi.roots.iter().map(|r| r.id.clone()).collect();
    state.open_bulk_preflight(BulkOp::PullAll, &roots);
    state.run_bulk_confirmed(BulkPlan {
        op: BulkOp::PullAll,
        roots: roots.clone(),
        rebase: state.ui.bulk_rebase,
        branch: String::new(),
        command: String::new(),
    });
    settle_fleet(&mut state);

    assert_eq!(state.last_error, None, "{:?}", state.ui.toast);
    let view = state
        .ui
        .bulk_run
        .as_ref()
        .expect("the monitor reports the run");
    assert!(
        view.rows.iter().all(|r| r.state == RowState::Done),
        "both roots integrate: {:?}",
        view.rows
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
