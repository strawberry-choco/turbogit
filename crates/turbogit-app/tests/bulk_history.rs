//! Issue 12 — recent bulk operations history: the app seam.
//!
//! Headless tests over real temporary repositories through the production
//! registration path ([`AppState::for_roots`]): a completed bulk/cascade run
//! is recorded — time, operation, repo count, outcome summary — through the
//! real event pump; Details data (the per-repo outcomes) rides in the
//! record; Rollback is a public [`AppState`] action offered only for
//! reversible operations; and the list persists with the workspace.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use turbogit_app::state::AppState;
use turbogit_services::bulk_ops::{BulkOp, BulkPlan};
use turbogit_services::bulk_run::RowState;

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

fn rid(path: &Path) -> turbogit_domain::model::RootId {
    turbogit_domain::model::RootId(path.to_path_buf().into())
}

/// Step the event pump until every monitor row is in a terminal state
/// (Done / Failed / Skipped) or the deadline passes.
fn wait_for_run_end(state: &mut AppState) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        state.drain_events();
        let Some(view) = state.ui.bulk_run.as_ref() else {
            panic!("monitor closed before the run ended");
        };
        let busy_row = view
            .rows
            .iter()
            .any(|r| matches!(r.state, RowState::Queued { .. } | RowState::Running));
        if !busy_row {
            return;
        }
        assert!(Instant::now() < deadline, "run did not end within 10s");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Step the event pump until `pred` holds: the completion side effects
/// (toast, activity, history) arrive as a `BulkCompleted` event that can
/// lag the last monitor row event.
fn wait_for(state: &mut AppState, pred: impl Fn(&AppState) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        state.drain_events();
        if pred(state) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "condition was not met within 10s"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Wait for the run to end and its record to land in the history with the
/// expected summary.
fn wait_for_record(state: &mut AppState, summary: &str) {
    wait_for_run_end(state);
    wait_for(state, |s| {
        s.ui.bulk_history.len() == 1 && s.ui.bulk_history[0].summary() == summary
    });
}

// -- Recording completed runs --------------------------------------------------

#[test]
fn a_completed_run_is_recorded_with_time_op_count_and_outcome_summary() {
    let (_tmp, project, alpha, ui) = project("hist-record");
    let mut state = app_with_selection(&project, &[alpha.clone(), ui.clone()], &[&alpha, &ui]);

    // Pull all: alpha runs (clean + synced), ui is preflight-skipped (dirty).
    let plan = BulkPlan {
        op: BulkOp::PullAll,
        roots: vec![rid(&alpha)],
        rebase: false,
        branch: String::new(),
        command: String::new(),
    };
    state.run_bulk_confirmed(plan);
    wait_for_record(&mut state, "1 ok · 1 skipped (dirty worktree)");

    assert_eq!(state.ui.bulk_history.len(), 1, "one record per run");
    let rec = &state.ui.bulk_history[0];
    assert_eq!(rec.op, BulkOp::PullAll);
    assert_eq!(
        rec.repos.len(),
        2,
        "the record covers every repo the run saw, skipped included"
    );
    assert_eq!(
        rec.summary(),
        "1 ok · 1 skipped (dirty worktree)",
        "the outcome summary matches the history row"
    );
    let alpha_row = rec.repos.iter().find(|r| r.name == "alpha").unwrap();
    assert_eq!(
        alpha_row.outcome,
        turbogit_app::bulk_history::RepoOutcome::Done
    );
    let ui_row = rec.repos.iter().find(|r| r.name == "ui").unwrap();
    assert_eq!(
        ui_row.outcome,
        turbogit_app::bulk_history::RepoOutcome::Skipped {
            reason: "dirty worktree".to_string()
        }
    );
    // The recorded time is the completion time (within a minute of now).
    let now = chrono::Utc::now().timestamp_millis();
    assert!(
        (rec.at..=now + 60_000).contains(&rec.at) && now - rec.at < 60_000,
        "recorded at completion time"
    );
}

/// A three-repo project for the branch cascade: `alpha` clean and synced,
/// `existing` already carrying `feature/x`, and `ui` with a dirty worktree.
fn branch_project(tag: &str) -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join(tag);
    let frontend = project.join("frontend");
    std::fs::create_dir_all(&frontend).unwrap();
    let alpha = temp_repo(&frontend, "alpha");
    let existing = temp_repo(&frontend, "existing");
    git(&existing, &["branch", "feature/x"]);
    std::fs::write(existing.join("base.txt"), "moved on\n").unwrap();
    git(&existing, &["add", "."]);
    git(&existing, &["commit", "-q", "-m", "main moved on"]);
    let ui = temp_repo(&frontend, "ui");
    std::fs::write(ui.join("dirty.txt"), "uncommitted\n").unwrap();
    (tmp, project, alpha, existing, ui)
}

#[test]
fn a_retry_pass_merges_into_the_same_history_record() {
    let (_tmp, project, alpha, existing, ui) = branch_project("hist-retry");
    let mut state = app_with_selection(
        &project,
        &[alpha.clone(), existing.clone(), ui.clone()],
        &[&alpha, &existing, &ui],
    );

    let plan = BulkPlan {
        op: BulkOp::CreateBranch,
        roots: vec![rid(&alpha), rid(&existing)],
        rebase: true,
        branch: "feature/x".to_string(),
        command: String::new(),
    };
    state.run_bulk_confirmed(plan);
    wait_for_record(&mut state, "2 ok · 1 skipped (dirty worktree)");

    // ui gets cleaned, then Retry skipped forces the attempt the policy
    // refused: the record updates in place instead of gaining a second row.
    std::fs::write(ui.join("dirty.txt"), "committed now\n").unwrap();
    git(&ui, &["add", "."]);
    git(&ui, &["commit", "-q", "-m", "clean up"]);

    state.bulk_retry_skipped();
    wait_for_record(&mut state, "3 ok");
}

// -- Persistence across restarts ------------------------------------------------

#[test]
fn history_survives_a_restart_through_the_launch_path() {
    let (_tmp, project, alpha, ui) = project("hist-persist");
    let mut state = app_with_selection(&project, &[alpha.clone(), ui.clone()], &[&alpha, &ui]);
    let plan = BulkPlan {
        op: BulkOp::PullAll,
        roots: vec![rid(&alpha)],
        rebase: false,
        branch: String::new(),
        command: String::new(),
    };
    state.run_bulk_confirmed(plan);
    wait_for_record(&mut state, "1 ok · 1 skipped (dirty worktree)");

    // A fresh app process over the same workspace restores the history.
    let reloaded = AppState::launch_in(Some(project.clone()), None);
    assert_eq!(reloaded.ui.bulk_history.len(), 1);
    assert_eq!(
        reloaded.ui.bulk_history[0].summary(),
        state.ui.bulk_history[0].summary()
    );
}

#[test]
fn a_ui_ron_written_before_history_existed_loads_with_none() {
    let project = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(project.path().join(".turbogit")).unwrap();
    std::fs::write(
        project.path().join(".turbogit/ui.ron"),
        "(tab: \"Commit\", recent_repos: [], draft_message: \"hi\")",
    )
    .unwrap();

    let state = AppState::launch_in(Some(project.path().to_path_buf()), None);
    assert!(state.ui.bulk_history.is_empty());
}

// -- Rollback -------------------------------------------------------------------

#[test]
fn rollback_undoes_a_branch_cascade_and_marks_the_record() {
    let (_tmp, project, alpha, existing, _ui) = branch_project("hist-rollback");
    let mut state = app_with_selection(
        &project,
        &[alpha.clone(), existing.clone()],
        &[&alpha, &existing],
    );

    let plan = BulkPlan {
        op: BulkOp::CreateBranch,
        roots: vec![rid(&alpha), rid(&existing)],
        rebase: true,
        branch: "feature/x".to_string(),
        command: String::new(),
    };
    state.run_bulk_confirmed(plan);
    wait_for_record(&mut state, "2 ok");
    assert_eq!(
        git(&alpha, &["branch", "--show-current"]).trim(),
        "feature/x"
    );

    let rec_id = state.ui.bulk_history[0].id;
    state.bulk_rollback(rec_id);

    // The created branch is deleted and the prior checkout restored in
    // alpha; existing keeps its branch (the run only checked it out) but is
    // back on the branch it started on.
    assert_eq!(git(&alpha, &["branch", "--show-current"]).trim(), "main");
    assert!(
        !git(&alpha, &["branch", "--list", "feature/x"]).contains("feature/x"),
        "the created branch was deleted"
    );
    assert_eq!(git(&existing, &["branch", "--show-current"]).trim(), "main");
    assert!(
        git(&existing, &["branch", "--list", "feature/x"]).contains("feature/x"),
        "a pre-existing branch is kept, only the checkout is restored"
    );
    assert!(
        state.ui.bulk_history[0].rolled_back,
        "the record remembers the rollback"
    );
    // The rollback reached disk with the record.
    let loaded = turbogit_app::persistence::load_ui_state(&project);
    assert!(loaded.bulk_history[0].rolled_back);
}

#[test]
fn rollback_is_refused_for_irreversible_operations() {
    let (_tmp, project, alpha, _ui) = project("hist-irrev");
    let mut state = app_with_selection(&project, std::slice::from_ref(&alpha), &[&alpha]);
    let plan = BulkPlan {
        op: BulkOp::FetchAll,
        roots: vec![rid(&alpha)],
        rebase: false,
        branch: String::new(),
        command: String::new(),
    };
    state.run_bulk_confirmed(plan);
    wait_for_record(&mut state, "1 ok");

    let rec_id = state.ui.bulk_history[0].id;
    state.bulk_rollback(rec_id);

    assert!(
        !state.ui.bulk_history[0].rolled_back,
        "fetch has no undo; the record is untouched"
    );
}

#[test]
fn rollback_of_an_unknown_or_rolled_back_record_is_a_noop() {
    let (_tmp, project, alpha, _ui) = project("hist-noop");
    let mut state = app_with_selection(&project, std::slice::from_ref(&alpha), &[&alpha]);
    state.bulk_rollback(12345);
    assert!(state.ui.bulk_history.is_empty());
}
