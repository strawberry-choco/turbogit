//! Issue 09 — bulk operations grid with preflight matrix: the app seam.
//!
//! Headless tests over real temporary repositories through the production
//! registration path ([`AppState::for_roots`]): the preflight matrix over the
//! live multi-repo selection, the modal-open/policy state, and the confirmed
//! dispatch with its per-repo outcome reporting. No UI rendering — callers
//! pass pure intent exactly as the operations grid does.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use turbogit_app::state::AppState;
use turbogit_domain::model::UpdateMethod;
use turbogit_services::bulk_ops::{BulkOp, BulkPlan, SkipReason};

/// Run `git <args>` in `repo`, asserting success, and return stdout.
fn git(repo: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git should be on PATH");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Create an initialized temp repository with one base commit on `main`
/// plus an `origin` remote so upstream reads can be exercised.
fn temp_repo(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q", "-b", "main"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("base.txt"), "base\n").unwrap();
    git(&path, &["add", "."]);
    git(&path, &["commit", "-q", "-m", "init"]);
    let bare = parent.join(format!("{name}.origin"));
    let _ = std::fs::remove_dir_all(&bare);
    git(
        parent,
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    git(&path, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(&path, &["push", "-q", "origin", "main"]);
    git(&path, &["branch", "--set-upstream-to=origin/main", "main"]);
    path
}

/// A two-repo project: `alpha` clean and synced, `ui` with a dirty worktree.
fn project(tag: &str) -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join(tag);
    let frontend = project.join("frontend");
    std::fs::create_dir_all(&frontend).unwrap();
    let alpha = temp_repo(&frontend, "alpha");
    let ui = temp_repo(&frontend, "ui");
    std::fs::write(ui.join("dirty.txt"), "uncommitted\n").unwrap();
    (tmp, project, alpha, ui)
}

fn app_with_selection(project: &Path, roots: &[PathBuf], selected: &[&PathBuf]) -> AppState {
    let mut state = AppState::for_roots(project, roots);
    state.ui.repo_selection = selected
        .iter()
        .map(|p| turbogit_domain::model::RootId((*p).clone().into()))
        .collect();
    state
}

// -- Preflight over the selection ---------------------------------------------

#[test]
fn bulk_preflight_classifies_the_selected_repos_for_the_operation() {
    let (_tmp, project, alpha, ui) = project("pf-classify");
    let state = app_with_selection(&project, &[alpha.clone(), ui.clone()], &[&alpha, &ui]);

    // Pull: alpha (clean + upstream) runs; ui (dirty) is skipped.
    let pf = state.bulk_preflight(BulkOp::PullAll);
    assert_eq!(pf.rows.len(), 2, "one row per selected repo");
    let alpha_row = pf.rows.iter().find(|r| r.name == "alpha").unwrap();
    let ui_row = pf.rows.iter().find(|r| r.name == "ui").unwrap();
    assert!(alpha_row.outcome.is_ok(), "alpha should run: {alpha_row:?}");
    assert_eq!(ui_row.outcome, Err(SkipReason::DirtyWorktree));

    // Stash is the inverse: ui runs, alpha (clean) is skipped.
    let pf = state.bulk_preflight(BulkOp::StashAll);
    let alpha_row = pf.rows.iter().find(|r| r.name == "alpha").unwrap();
    let ui_row = pf.rows.iter().find(|r| r.name == "ui").unwrap();
    assert_eq!(alpha_row.outcome, Err(SkipReason::CleanTree));
    assert!(ui_row.outcome.is_ok(), "dirty ui should stash: {ui_row:?}");

    // Push ignores a dirty tree but needs an upstream.
    let pf = state.bulk_preflight(BulkOp::PushAll);
    assert_eq!(pf.will_run(), 2, "both repos are dirty-ok with upstreams");
}

#[test]
fn bulk_preflight_uses_the_cached_ahead_behind_counts_for_divergence() {
    let (_tmp, project, alpha, ui) = project("pf-diverged");
    let mut state = app_with_selection(&project, &[alpha.clone(), ui.clone()], &[&alpha, &ui]);
    // Seed the caches the way an AheadBehind event would: alpha diverged.
    state
        .caches
        .store_ahead_behind(turbogit_domain::model::RootId(alpha.clone().into()), (2, 1));

    let pf = state.bulk_preflight(BulkOp::PullAll);
    let alpha_row = pf.rows.iter().find(|r| r.name == "alpha").unwrap();
    assert_eq!(alpha_row.outcome, Err(SkipReason::Diverged));
    assert_eq!((alpha_row.ahead, alpha_row.behind), (2, 1));

    let pf = state.bulk_preflight(BulkOp::FetchAll);
    assert_eq!(pf.will_run(), 2, "fetch never skips");
}

#[test]
fn bulk_preflight_ignores_roots_outside_the_selection() {
    let (_tmp, project, alpha, ui) = project("pf-scope");
    let state = app_with_selection(&project, &[alpha.clone(), ui.clone()], &[&alpha]);

    let pf = state.bulk_preflight(BulkOp::PullAll);
    assert_eq!(pf.rows.len(), 1, "only the selected root is in the matrix");
    assert_eq!(pf.rows[0].name, "alpha");
}

// -- Modal open + policy ------------------------------------------------------

#[test]
fn open_bulk_preflight_seeds_the_pull_policy_from_settings() {
    let (_tmp, project, alpha, _ui) = project("pf-open");
    let mut state = app_with_selection(&project, std::slice::from_ref(&alpha), &[&alpha]);

    assert_eq!(state.ui.bulk_op, None, "no preflight modal initially");

    state.settings.update_method = UpdateMethod::Rebase;
    state.open_bulk_preflight(BulkOp::PullAll);
    assert_eq!(state.ui.bulk_op, Some(BulkOp::PullAll));
    assert!(state.ui.bulk_rebase, "policy seeds from settings");

    // A non-pull op does not carry the pull policy.
    state.open_bulk_preflight(BulkOp::FetchAll);
    assert_eq!(state.ui.bulk_op, Some(BulkOp::FetchAll));
}

// -- Confirmed dispatch -------------------------------------------------------

#[test]
fn run_bulk_confirmed_executes_the_plan_and_reports_per_repo_outcomes() {
    let (_tmp, project, alpha, ui) = project("pf-run");
    let mut state = app_with_selection(&project, &[alpha.clone(), ui.clone()], &[&alpha, &ui]);

    // alpha (the planned root) gets local changes to a tracked file; ui
    // stays dirty as the out-of-scope control.
    std::fs::write(alpha.join("base.txt"), "uncommitted\n").unwrap();

    let plan = BulkPlan {
        op: BulkOp::StashAll,
        roots: vec![turbogit_domain::model::RootId(alpha.clone().into())],
        rebase: false,
        branch: String::new(),
        command: String::new(),
    };
    state.run_bulk_confirmed(plan);

    // The preflight modal closes on confirm and the op runs async.
    assert_eq!(state.ui.bulk_op, None);
    wait_for_toast(&mut state);

    // The planned root was stashed; the out-of-scope root keeps its changes.
    let stashes = git(&alpha, &["stash", "list"]);
    assert!(!stashes.trim().is_empty(), "alpha's changes were stashed");
    let ui_status = git(&ui, &["status", "--porcelain"]);
    assert!(!ui_status.trim().is_empty(), "out-of-scope ui stayed dirty");

    // Aggregate reporting: the toast summarizes the per-repo outcomes.
    let toast = state.ui.toast.as_ref().expect("completion toast");
    assert!(
        toast.message.contains("Stash all"),
        "toast names the operation: {:?}",
        toast.message
    );
    assert!(
        toast.message.contains("1 of 1"),
        "toast counts ran vs planned: {:?}",
        toast.message
    );

    // The durable activity feed carries the outcome too.
    let last = state.ui.activity.entries.last().expect("activity entry");
    assert!(
        last.message.contains("Stash all"),
        "activity: {:?}",
        last.message
    );
}

#[test]
fn run_bulk_confirmed_reports_partial_failure_in_the_summary() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let alpha = tmp.path().join("alpha");
    std::fs::create_dir_all(&alpha).unwrap();

    // A pure reporting check of the aggregate message with a fabricated
    // per-repo outcome (a root whose push has no remote).
    let msg = turbogit_app::state::bulk_report_message(
        "Push all",
        &[
            (turbogit_domain::model::RootId(alpha.clone().into()), Ok(())),
            (
                turbogit_domain::model::RootId(PathBuf::from("/w/ui").into()),
                Err(turbogit_domain::error::TgError::Other("rejected".into())),
            ),
        ],
    );
    assert_eq!(msg, "Push all · 1 of 2 ok · failed: ui");
}

/// Step the event pump until the completion toast lands (the dispatch runs
/// on a worker thread even in the headless harness).
fn wait_for_toast(state: &mut AppState) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        state.drain_events();
        if state.ui.toast.is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("no completion toast within 10s");
}
