//! Issue 13 — custom command across a selection: the app seam.
//!
//! Headless tests over real temporary repositories through the production
//! registration path ([`AppState::for_roots`]): a user-typed git command
//! runs on every confirmed repo through the same run monitor the built-in
//! bulk operations use, recent commands are remembered per workspace, and
//! destructive-looking commands expose the extra-confirmation state.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use test_support::git_seed::git;
use turbogit_app::state::AppState;
use turbogit_services::bulk_ops::{BulkOp, BulkPlan};
use turbogit_services::bulk_run::RowState;

/// Create an initialized temp repository with one base commit on `main`.
/// Kept local, not `test_support::git_seed::repo_with_one_commit`: the base
/// commit is `base.txt`, not `README.md`, and the change list, diff pane and
/// branch/HEAD rows in this suite name the file they changed.
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

/// Settings with nothing protected: the fan-out mechanics below predate the
/// per-root protected-branch refusal, and the shipped default would refuse
/// every repo they run on (`main`). The refusal itself is asserted with the
/// default settings in the protected suite at the bottom of this file.
fn unprotected() -> turbogit_domain::model::VcsSettings {
    turbogit_domain::model::VcsSettings {
        protected_branch_patterns: Vec::new(),
        ..Default::default()
    }
}

fn app_with_selection(project: &Path, roots: &[PathBuf], selected: &[&PathBuf]) -> AppState {
    let mut state = AppState::for_roots(project, roots).with_settings(unprotected());
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
/// (Done / Failed / Skipped), the run has settled, or the deadline passes.
///
/// The settle is part of the wait, not a courtesy: terminal rows arrive as
/// progress events, and the pass's completion is a separate event that lands
/// afterwards — so a wait that reads only the rows can return before the
/// aggregate toast and the per-root feed entries exist, and a later feed
/// assertion then races the worker.
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
        if !busy_row && !state.ui.busy {
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
    assert_eq!(pf.will_run(), 2, "an unprotected fleet all runs");
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
        RowState::Failed {
            error,
            protected_branch,
        } => {
            assert!(
                error.contains("definitely-not-a-git-command"),
                "git stderr surfaced: {error:?}"
            );
            assert!(
                !protected_branch,
                "a git failure is not a guard refusal, so it owes no per-root refusal entry"
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
    state.save_durable_ui();

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

// -- Protected-branch policy ----------------------------------------------------

/// The refusal is per root and about the root: a command typed against a
/// protected branch is refused whatever it was, its monitor row and feed
/// entry name the branch (never the command), and nothing runs.
#[test]
fn a_protected_root_is_refused_whatever_the_command_and_names_the_branch() {
    let (_tmp, project, alpha, ui) = project("cc-protected");
    // Shipped defaults: `main` is protected, and both repos sit on it.
    let mut state = AppState::for_roots(&project, &[alpha.clone(), ui.clone()]);
    state.ui.repo_selection = [rid(&alpha), rid(&ui)].into_iter().collect();

    state.open_bulk_preflight(BulkOp::Custom);
    state.ui.bulk_command = "git branch bulk-marker".to_string();
    let pf = state.bulk_preflight(BulkOp::Custom);
    assert_eq!(
        pf.will_run(),
        0,
        "an ordinary command buys a protected root nothing"
    );

    let plan = BulkPlan {
        op: BulkOp::Custom,
        roots: Vec::new(),
        rebase: false,
        branch: String::new(),
        command: "git branch bulk-marker".to_string(),
    };
    state.run_bulk_confirmed(plan);
    wait_for_run_end(&mut state);

    // The monitor's row labels name the branch, not the command.
    let view = state.ui.bulk_run.as_ref().expect("monitor stays open");
    assert_eq!(view.rows.len(), 2);
    for row in &view.rows {
        match &row.state {
            RowState::Skipped { reason } => {
                assert!(
                    reason.contains("protected branch — main"),
                    "the row names the refused branch: {reason:?}"
                );
                assert!(
                    !reason.contains("bulk-marker"),
                    "the row never quotes the command: {reason:?}"
                );
            }
            other => panic!("a protected root is skipped with a reason: {other:?}"),
        }
    }

    // The same reason is durable in the activity feed, one entry per root.
    let per_root: Vec<&str> = state
        .ui
        .activity
        .entries
        .iter()
        .filter(|e| e.repo.as_deref() == Some("alpha") || e.repo.as_deref() == Some("ui"))
        .map(|e| e.message.as_str())
        .collect();
    assert_eq!(per_root.len(), 2, "one durable refusal per root");
    for message in per_root {
        assert!(
            message.contains("'main' is a protected branch"),
            "the feed entry names the branch: {message}"
        );
        assert!(
            !message.contains("bulk-marker"),
            "not the command: {message}"
        );
    }

    for repo in [&alpha, &ui] {
        assert!(
            git(repo, &["branch", "--list", "bulk-marker"]).is_empty(),
            "the refused command ran nowhere"
        );
    }
}

/// Retry re-dispatches every Skipped row — forcing the attempt the policy
/// refused — so for `Custom` the refusal that BEARS the policy is the one on
/// the step: the matrix's seeding can be skipped past by a plan that names no
/// roots, and the row must then still be refused where the command would
/// otherwise reach git. The command here moves HEAD if it runs, so the
/// refusal is legible as a branch that did not.
#[test]
fn retry_cannot_force_a_custom_command_onto_a_protected_branch() {
    let (_tmp, project, alpha, _ui) = project("cc-retry-protected");
    // Shipped defaults: `main` is protected, and `alpha` sits on it.
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    state.ui.repo_selection = [rid(&alpha)].into_iter().collect();

    let plan = BulkPlan {
        op: BulkOp::Custom,
        roots: Vec::new(),
        rebase: false,
        branch: String::new(),
        command: "git commit --allow-empty -m bulk-ran".to_string(),
    };
    state.run_bulk_confirmed(plan);
    wait_for_run_end(&mut state);
    let head_before = git(&alpha, &["rev-parse", "HEAD"]).trim().to_string();

    state.bulk_retry_skipped();
    wait_for_run_end(&mut state);

    let view = state.ui.bulk_run.as_ref().expect("monitor stays open");
    let row = view.rows.iter().find(|r| r.name == "alpha").unwrap();
    match &row.state {
        RowState::Failed { error, .. } => {
            assert!(
                error.contains("'main' is a protected branch"),
                "the forced step refused, naming the branch: {error}"
            );
            assert!(
                !error.contains("bulk-ran"),
                "the step's refusal is about the root, never the command: {error}"
            );
        }
        other => panic!("the retry reached the branch it was refused: {other:?}"),
    }
    assert_eq!(
        git(&alpha, &["rev-parse", "HEAD"]).trim(),
        head_before,
        "the refused command ran nothing on the protected branch"
    );
    // `Error` and not the seeding's `Warning`: a preflight refusal records
    // itself per root too, with the same words, so only the kind tells this
    // entry apart from the one the matrix wrote before anything ran.
    assert!(
        state.ui.activity.entries.iter().any(|e| {
            e.repo.as_deref() == Some("alpha")
                && e.message.contains("'main' is a protected branch")
                && e.kind == turbogit_app::activity::ActivityKind::Error
        }),
        "the step's refusal is durable in the feed"
    );
}

/// The grammar is unrestricted: the policy refuses roots, not commands. A
/// destructive-looking command still runs — the confirmation gate is unchanged
/// and this seam dispatches past it exactly as before.
#[test]
fn a_destructive_command_still_runs_on_an_unprotected_root() {
    let (_tmp, project, alpha, _ui) = project("cc-grammar");
    let mut state = app_with_selection(&project, std::slice::from_ref(&alpha), &[&alpha]);

    let plan = BulkPlan {
        op: BulkOp::Custom,
        roots: vec![rid(&alpha)],
        rebase: false,
        branch: String::new(),
        command: "git reset --hard".to_string(),
    };
    state.run_bulk_confirmed(plan);
    wait_for_run_end(&mut state);

    let view = state.ui.bulk_run.as_ref().expect("monitor stays open");
    assert_eq!(view.tally(), (1, 0, 0, 0, 0), "the command ran");
}
