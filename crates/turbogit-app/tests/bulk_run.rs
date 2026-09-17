//! Issue 10 — cascade run monitor: the app seam.
//!
//! Headless tests over real temporary repositories through the production
//! registration path ([`AppState::for_roots`]): a confirmed bulk plan now
//! opens a live monitor (`ui.bulk_run`) whose rows, durations, and tallies
//! advance through the real event pump, with Stop-remaining, Retry-skipped,
//! and Resolve as public [`AppState`] actions.

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
/// (Done / Failed / Skipped) and BulkCompleted has cleared the busy flag,
/// publishing the aggregate toast and history, or the deadline passes.
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
        // Terminal row progress can arrive before the separate BulkCompleted
        // event. Wait for that event too, including on retries with an old toast.
        if !busy_row && !state.ui.busy {
            return;
        }
        assert!(Instant::now() < deadline, "run did not end within 10s");
        std::thread::sleep(Duration::from_millis(10));
    }
}

// -- Live monitor -------------------------------------------------------------

#[test]
fn run_bulk_confirmed_opens_a_live_monitor_that_tracks_every_selected_repo() {
    let (_tmp, project, alpha, ui) = project("mon-live");
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

    // The monitor opened in place of the preflight modal and every selected
    // repo has a row from the start: the plan root queued, the preflight
    // skip seeded with its reason.
    assert_eq!(state.ui.bulk_op, None);
    let view = state.ui.bulk_run.as_ref().expect("monitor opened");
    assert_eq!(view.op, BulkOp::PullAll);
    assert_eq!(view.rows.len(), 2, "one row per selected repo");
    let alpha_row = view.rows.iter().find(|r| r.name == "alpha").unwrap();
    let ui_row = view.rows.iter().find(|r| r.name == "ui").unwrap();
    assert!(matches!(
        alpha_row.state,
        RowState::Queued { .. } | RowState::Running
    ));
    assert_eq!(
        ui_row.state,
        RowState::Skipped {
            reason: "dirty worktree".to_string()
        }
    );

    wait_for_run_end(&mut state);
    let view = state.ui.bulk_run.as_ref().expect("monitor stays open");
    let alpha_row = view.rows.iter().find(|r| r.name == "alpha").unwrap();
    let ui_row = view.rows.iter().find(|r| r.name == "ui").unwrap();
    assert_eq!(alpha_row.state, RowState::Done, "alpha pulled");
    assert!(alpha_row.duration.is_some(), "done rows carry durations");
    assert_eq!(
        ui_row.state,
        RowState::Skipped {
            reason: "dirty worktree".to_string()
        }
    );

    // Footer tallies: 1 done, 1 skipped, nothing else.
    assert_eq!(view.tally(), (1, 0, 0, 1, 0));
    assert_eq!(view.done_count(), 1);
    assert_eq!(view.total(), 2);
}

// -- Stop remaining + Retry skipped --------------------------------------------

#[test]
fn stop_remaining_halts_queued_repos_and_retry_skipped_redispatches_them() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let frontend = tmp.path().join("fleet");
    std::fs::create_dir_all(&frontend).unwrap();
    // Twelve dirty repos: four worker slots keep running while the queued
    // rest must halt when Stop remaining lands.
    let paths: Vec<PathBuf> = (0..12)
        .map(|i| {
            let p = temp_repo(&frontend, &format!("r{i}"));
            std::fs::write(p.join("base.txt"), "changed\n").unwrap();
            p
        })
        .collect();
    let mut state = app_with_selection(tmp.path(), &paths, &paths.iter().collect::<Vec<_>>());

    let plan = BulkPlan {
        op: BulkOp::StashAll,
        roots: paths.iter().map(|p| rid(p)).collect(),
        rebase: false,
        branch: String::new(),
        command: String::new(),
    };
    state.run_bulk_confirmed(plan);
    // Halt the queued work immediately; running/completed roots finish.
    state.bulk_stop_remaining();
    wait_for_run_end(&mut state);

    let view = state.ui.bulk_run.as_ref().expect("monitor stays open");
    assert!(
        view.tally().3 > 0,
        "stop halted at least the queued rows: {:?}",
        view.tally()
    );
    // Done rows are exactly the repos that got stashed; skipped rows are
    // exactly the ones still untouched — a stop never undoes completed work.
    for (row, path) in view.rows.iter().zip(&paths) {
        let stashed = !git(path, &["stash", "list"]).trim().is_empty();
        match &row.state {
            RowState::Done => assert!(stashed, "{} is Done but was not stashed", row.name),
            RowState::Skipped { .. } => {
                assert!(!stashed, "{} was halted but its work was stashed", row.name)
            }
            other => panic!("{} still mid-run after stop: {other:?}", row.name),
        }
    }

    // Retry skipped re-dispatches exactly the halted rows through a fresh
    // pass; every repo ends Done with its changes stashed.
    state.bulk_retry_skipped();
    wait_for_run_end(&mut state);
    let view = state.ui.bulk_run.as_ref().expect("monitor stays open");
    assert_eq!(view.tally(), (12, 0, 0, 0, 0), "retry cleared every skip");
    for path in &paths {
        let stashed = !git(path, &["stash", "list"]).trim().is_empty();
        assert!(stashed, "retried repo was stashed");
    }
}

// -- Failed rows + Resolve -----------------------------------------------------

#[test]
fn a_failed_row_carries_the_git_error_and_resolve_jumps_to_the_repo() {
    let (_tmp, project, alpha, _ui) = project("mon-fail");
    // Break alpha's remote so the fetch fails deterministically.
    git(
        &alpha,
        &["remote", "set-url", "origin", "/nonexistent/broken"],
    );
    let mut state = app_with_selection(&project, std::slice::from_ref(&alpha), &[&alpha]);

    let plan = BulkPlan {
        op: BulkOp::FetchAll,
        roots: vec![rid(&alpha)],
        rebase: false,
        branch: String::new(),
        command: String::new(),
    };
    state.run_bulk_confirmed(plan);
    wait_for_run_end(&mut state);

    // The failed row carries the git error text.
    let view = state.ui.bulk_run.as_ref().expect("monitor stays open");
    let alpha_row = view.rows.iter().find(|r| r.name == "alpha").unwrap();
    match &alpha_row.state {
        RowState::Failed { error } => {
            assert!(error.contains("fatal"), "git stderr surfaced: {error:?}");
        }
        other => panic!("alpha should have failed: {other:?}"),
    }
    assert!(alpha_row.duration.is_some(), "failed rows carry durations");

    // The aggregate toast still reports the run (issue 09 semantics).
    let toast = state.ui.toast.as_ref().expect("completion toast");
    assert!(toast.message.contains("Fetch all"), "{:?}", toast.message);
    assert!(toast.message.contains("0 of 1 ok"), "{:?}", toast.message);

    // Resolve jumps to the repo in question and closes the monitor.
    state.bulk_resolve(rid(&alpha));
    assert_eq!(state.selected_root, Some(rid(&alpha)));
    assert!(state.ui.bulk_run.is_none(), "monitor closed on resolve");
}

// -- Cascade create & checkout branch (issue 11) -------------------------------

use turbogit_services::bulk_ops::BranchAction;

/// A three-repo project for the branch cascade: `alpha` clean and synced,
/// `existing` already carrying `feature/x` at its first commit, and `ui`
/// with a dirty worktree.
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
fn a_confirmed_branch_cascade_preflights_the_fleet_and_lands_repos_on_the_branch() {
    let (_tmp, project, alpha, existing, ui) = branch_project("br-live");
    let mut state = app_with_selection(
        &project,
        &[alpha.clone(), existing.clone(), ui.clone()],
        &[&alpha, &existing, &ui],
    );

    // The cascade modal opens with the branch name typed in; the preflight
    // matrix predicts per-repo what will happen.
    state.open_bulk_preflight(BulkOp::CreateBranch);
    state.ui.bulk_branch_name = "feature/x".to_string();
    let pf = state.bulk_branch_preflight();
    let action = |name: &str| {
        pf.rows
            .iter()
            .find(|r| r.name == name)
            .unwrap()
            .action
            .clone()
    };
    assert_eq!(action("alpha"), BranchAction::CreateFromHead);
    assert_eq!(action("existing"), BranchAction::CheckoutExisting);
    assert_eq!(
        action("ui"),
        BranchAction::Skip(turbogit_services::bulk_ops::SkipReason::DirtyWorktree)
    );
    assert_eq!(pf.will_run(), 2);
    assert_eq!(pf.total(), 3);

    // Confirming runs the will-run rows through the run monitor; the dirty
    // repo is seeded as skipped with its preflight reason.
    let plan = turbogit_services::bulk_ops::BulkPlan {
        op: BulkOp::CreateBranch,
        roots: pf.plan_roots(),
        rebase: state.ui.bulk_from_upstream,
        branch: pf.name.clone(),
        command: String::new(),
    };
    state.run_bulk_confirmed(plan);

    assert_eq!(state.ui.bulk_op, None, "the preflight modal closed");
    let view = state.ui.bulk_run.as_ref().expect("monitor opened");
    assert_eq!(view.op, BulkOp::CreateBranch);
    assert_eq!(view.branch, "feature/x");
    assert_eq!(view.rows.len(), 3, "one row per selected repo");

    wait_for_run_end(&mut state);
    let view = state.ui.bulk_run.as_ref().expect("monitor stays open");
    let by_name = |name: &str| view.rows.iter().find(|r| r.name == name).unwrap();
    assert_eq!(by_name("alpha").state, RowState::Done);
    assert_eq!(by_name("existing").state, RowState::Done);
    assert_eq!(
        by_name("ui").state,
        RowState::Skipped {
            reason: "dirty worktree".to_string()
        }
    );
    assert_eq!(view.tally(), (2, 0, 0, 1, 0));

    // The git state matches the predictions: alpha was created from HEAD
    // and checked out; existing was checked out, not recreated.
    assert_eq!(
        git(&alpha, &["branch", "--show-current"]).trim(),
        "feature/x"
    );
    assert_eq!(
        git(&existing, &["branch", "--show-current"]).trim(),
        "feature/x"
    );
    let first_commit = git(&existing, &["rev-list", "--max-parents=0", "HEAD"])
        .trim()
        .to_string();
    assert_eq!(
        git(&existing, &["rev-parse", "feature/x"]).trim(),
        first_commit,
        "the existing branch kept its original tip"
    );
}

#[test]
fn retry_skipped_lands_the_branch_on_repos_that_became_clean() {
    let (_tmp, project, alpha, existing, ui) = branch_project("br-retry");
    let mut state = app_with_selection(
        &project,
        &[alpha.clone(), existing.clone(), ui.clone()],
        &[&alpha, &existing, &ui],
    );

    let plan = turbogit_services::bulk_ops::BulkPlan {
        op: BulkOp::CreateBranch,
        roots: vec![rid(&alpha), rid(&existing)],
        rebase: true,
        branch: "feature/x".to_string(),
        command: String::new(),
    };
    state.run_bulk_confirmed(plan);
    wait_for_run_end(&mut state);
    assert_eq!(
        state.ui.bulk_run.as_ref().unwrap().tally(),
        (2, 0, 0, 1, 0),
        "ui is skipped as dirty"
    );

    // ui gets cleaned (its changes committed), then Retry skipped forces
    // the attempt the policy refused.
    std::fs::write(ui.join("dirty.txt"), "committed now\n").unwrap();
    git(&ui, &["add", "."]);
    git(&ui, &["commit", "-q", "-m", "clean up"]);

    state.bulk_retry_skipped();
    wait_for_run_end(&mut state);
    let view = state.ui.bulk_run.as_ref().expect("monitor stays open");
    assert_eq!(view.tally(), (3, 0, 0, 0, 0), "retry cleared the skip");
    assert_eq!(git(&ui, &["branch", "--show-current"]).trim(), "feature/x");
}
