//! Issue 13 — custom command across a selection: the app seam.
//!
//! Headless tests over real temporary repositories through the production
//! registration path ([`AppState::for_roots`]): a user-typed git command
//! runs on every confirmed repo through the same run monitor the built-in
//! bulk operations use, recent commands are remembered per workspace, and
//! destructive-looking commands expose the extra-confirmation state.

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

/// Create an initialized temp repository with one base commit on `main`.
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
    path
}

/// A two-repo project: `alpha` and `ui`.
fn project(tag: &str) -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join(tag);
    let frontend = project.join("frontend");
    std::fs::create_dir_all(&frontend).unwrap();
    let alpha = temp_repo(&frontend, "alpha");
    let ui = temp_repo(&frontend, "ui");
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

// -- Custom command across the selection ---------------------------------------

#[test]
fn a_typed_command_runs_on_every_selected_repo_through_the_monitor() {
    let (_tmp, project, alpha, ui) = project("cc-run");
    let mut state = app_with_selection(&project, &[alpha.clone(), ui.clone()], &[&alpha, &ui]);

    // The modal opens from the operations grid's "Custom command…" tile and
    // the command is typed in; the preflight matrix predicts a full run.
    state.open_bulk_preflight(BulkOp::Custom);
    state.ui.bulk_command = "git branch bulk-marker".to_string();
    let pf = state.bulk_preflight(BulkOp::Custom);
    assert_eq!(pf.will_run(), 2, "custom commands never preflight-skip");
    assert_eq!(pf.total(), 2);

    // Confirming runs the will-run rows through the run monitor.
    let plan = BulkPlan {
        op: BulkOp::Custom,
        roots: pf.rows.iter().map(|r| r.root.clone()).collect(),
        rebase: false,
        branch: String::new(),
        command: state.ui.bulk_command.clone(),
    };
    state.run_bulk_confirmed(plan);

    assert_eq!(state.ui.bulk_op, None, "the preflight modal closed");
    let view = state.ui.bulk_run.as_ref().expect("monitor opened");
    assert_eq!(view.op, BulkOp::Custom);
    assert_eq!(view.command, "git branch bulk-marker");
    assert_eq!(view.rows.len(), 2, "one row per selected repo");

    wait_for_run_end(&mut state);
    let view = state.ui.bulk_run.as_ref().expect("monitor stays open");
    assert_eq!(view.tally(), (2, 0, 0, 0, 0), "both repos ran the command");
    assert_eq!(
        view.running_command(),
        "git branch bulk-marker",
        "the monitor shows the typed command, not an op name"
    );

    // The command's effect is real, on every selected repo.
    for repo in [&alpha, &ui] {
        let branches = git(repo, &["branch", "--list", "bulk-marker"]);
        assert!(branches.contains("bulk-marker"), "{repo:?} got the branch");
    }
}

#[test]
fn a_failed_command_surfaces_the_git_stderr_on_the_row() {
    let (_tmp, project, alpha, _ui) = project("cc-fail");
    let mut state = app_with_selection(&project, std::slice::from_ref(&alpha), &[&alpha]);

    let plan = BulkPlan {
        op: BulkOp::Custom,
        roots: vec![rid(&alpha)],
        rebase: false,
        branch: String::new(),
        command: "definitely-not-a-git-command".to_string(),
    };
    state.run_bulk_confirmed(plan);
    wait_for_run_end(&mut state);

    let view = state.ui.bulk_run.as_ref().expect("monitor stays open");
    let row = view.rows.first().unwrap();
    match &row.state {
        RowState::Failed { error } => {
            assert!(
                error.contains("definitely-not-a-git-command"),
                "git stderr surfaced: {error:?}"
            );
        }
        other => panic!("an unknown command must fail: {other:?}"),
    }
}

// -- Destructive-looking commands ----------------------------------------------

#[test]
fn destructive_commands_expose_the_extra_confirmation_state() {
    let (_tmp, project, alpha, ui) = project("cc-destructive");
    let mut state = app_with_selection(&project, &[alpha.clone(), ui.clone()], &[&alpha, &ui]);
    state.open_bulk_preflight(BulkOp::Custom);

    assert!(
        !state.bulk_command_is_destructive(),
        "a fresh modal has no command yet"
    );
    state.ui.bulk_command = "git gc".to_string();
    assert!(!state.bulk_command_is_destructive(), "gc is ordinary");

    state.ui.bulk_command = "git reset --hard HEAD~1".to_string();
    assert!(
        state.bulk_command_is_destructive(),
        "reset is destructive-looking and must arm the extra confirmation"
    );
}

// -- Recent commands ------------------------------------------------------------

#[test]
fn confirmed_commands_are_remembered_newest_first_without_duplicates() {
    let (_tmp, project, alpha, _ui) = project("cc-recents");
    let mut state = app_with_selection(&project, std::slice::from_ref(&alpha), &[&alpha]);

    let run = |state: &mut AppState, command: &str| {
        state.open_bulk_preflight(BulkOp::Custom);
        state.ui.bulk_command = command.to_string();
        let plan = BulkPlan {
            op: BulkOp::Custom,
            roots: vec![rid(&alpha)],
            rebase: false,
            branch: String::new(),
            command: command.to_string(),
        };
        state.run_bulk_confirmed(plan);
        wait_for_run_end(state);
    };
    run(&mut state, "git gc");
    run(&mut state, "git remote prune origin");
    run(&mut state, "git gc");

    assert_eq!(
        state.ui.recent_custom_commands,
        vec!["git gc".to_string(), "git remote prune origin".to_string(),],
        "newest first, deduplicated, recorded at confirmation"
    );
}

#[test]
fn recent_commands_persist_with_the_workspace_and_carry_over_a_restart() {
    let (_tmp, project, alpha, _ui) = project("cc-persist");
    let mut state = app_with_selection(&project, std::slice::from_ref(&alpha), &[&alpha]);

    let plan = BulkPlan {
        op: BulkOp::Custom,
        roots: vec![rid(&alpha)],
        rebase: false,
        branch: String::new(),
        command: "git gc".to_string(),
    };
    state.run_bulk_confirmed(plan);
    wait_for_run_end(&mut state);
    state.persist_ui();

    let reloaded = turbogit_app::persistence::load_ui_state(&project);
    assert_eq!(
        reloaded.recent_custom_commands,
        vec!["git gc".to_string()],
        "recent commands survive a restart"
    );
}

#[test]
fn the_recent_list_is_capped_and_stays_newest_first() {
    let (_tmp, project, alpha, _ui) = project("cc-cap");
    let mut state = app_with_selection(&project, std::slice::from_ref(&alpha), &[&alpha]);

    for i in 0..10 {
        state.record_custom_command(&format!("git cmd-{i}"));
    }
    state.record_custom_command("git cmd-9");

    let recents = &state.ui.recent_custom_commands;
    assert_eq!(recents.len(), 8, "the list is capped at 8");
    assert_eq!(recents[0], "git cmd-9", "a re-run moves to the front");
    assert_eq!(recents[1], "git cmd-8");
    assert!(!recents.iter().any(|c| c == "git cmd-0"), "oldest dropped");
}
