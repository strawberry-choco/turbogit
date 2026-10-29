use egui::Key;
use egui_kittest::{Harness, kittest::Queryable};
use std::path::{Path, PathBuf};
use std::time::Duration;
use test_support::git_seed::{git, repo_with_one_commit};
use test_support::harness::{
    assert_matrix_geometry, assert_not_painted, assert_painted, settle, shell_harness_over_unstyled,
};
use turbogit_app::state::AppState;
use turbogit_domain::model::VcsSettings;
use turbogit_services::bulk_ops::BulkOp;
use turbogit_services::bulk_run::RowState;

fn temp_repo(parent: &Path, name: &str) -> PathBuf {
    repo_with_one_commit(parent, name)
}

fn unprotected() -> VcsSettings {
    VcsSettings {
        protected_branch_patterns: Vec::new(),
        ..VcsSettings::default()
    }
}

fn two_repo_project(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
    let base = test_support::scratch_root(&format!("custom-command-{tag}"));
    let _ = std::fs::remove_dir_all(&base);
    let project = base.join("wsb");
    let frontend = project.join("frontend");
    std::fs::create_dir_all(&frontend).unwrap();
    let alpha = temp_repo(&frontend, "alpha");
    let ui = temp_repo(&frontend, "ui");
    (project, alpha, ui)
}

fn harness(state: AppState) -> Harness<'static, AppState> {
    shell_harness_over_unstyled(state, egui::vec2(1280.0, 800.0), 1024)
}

fn wait_for(harness: &mut Harness<'_, AppState>, pred: impl Fn(&AppState) -> bool) {
    for _ in 0..1000 {
        harness.step();
        std::thread::sleep(Duration::from_millis(10));
        if pred(harness.state()) {
            return;
        }
    }
    panic!("condition was not met within 1000 frames");
}

fn open_custom_command(h: &mut Harness<'_, AppState>) {
    h.get_by_label("Select group frontend").click();
    settle(h);
    h.get_by_label("Custom command…").click();
    settle(h);
}

#[test]
fn the_custom_command_tile_opens_a_modal_that_runs_the_typed_command() {
    let (project, alpha, ui) = two_repo_project("tile");
    let state =
        AppState::for_roots(&project, &[alpha.clone(), ui.clone()]).with_settings(unprotected());
    let mut h = harness(state);
    settle(&mut h);

    open_custom_command(&mut h);
    assert_eq!(h.state().ui.bulk_op, Some(BulkOp::Custom));

    assert_painted(&h, "Command");
    h.get_by_label("Run on 2 of 2").click();
    settle(&mut h);
    assert!(h.state().ui.bulk_run.is_none(), "no run without a command");
    assert_eq!(h.state().ui.bulk_op, Some(BulkOp::Custom));

    assert_matrix_geometry(
        &h,
        "OUTCOME",
        &[
            ("REPO", 0.0),
            ("CURRENT BRANCH", 170.0),
            ("STATUS", 300.0),
            ("OUTCOME", 390.0),
        ],
        &["alpha", "ui"],
        22.0,
        30.0,
    );

    h.get_by_label("Command input").focus();
    h.get_by_label("Command input")
        .type_text("git branch bulk-marker");
    settle(&mut h);
    assert_painted(&h, "Will run on all 2 repositories.");

    h.get_by_label("Run on 2 of 2").click();
    settle(&mut h);
    assert_eq!(h.state().ui.bulk_op, None);
    assert!(h.state().ui.bulk_run.is_some(), "the monitor opened");

    wait_for(&mut h, |s| {
        s.ui.bulk_run
            .as_ref()
            .is_some_and(|v| v.rows.len() == 2 && v.rows.iter().all(|r| r.state == RowState::Done))
    });

    settle(&mut h);
    assert_painted(&h, "2 of 2 done");
    assert_painted(&h, "git branch bulk-marker");

    for repo in [&alpha, &ui] {
        assert!(
            git(repo, &["branch", "--list", "bulk-marker"]).contains("bulk-marker"),
            "{repo:?} got the branch"
        );
    }

    h.key_press(Key::Escape);
    settle(&mut h);
}

#[test]
fn a_destructive_command_needs_a_second_explicit_confirmation() {
    let (project, alpha, ui) = two_repo_project("destructive");
    let state =
        AppState::for_roots(&project, &[alpha.clone(), ui.clone()]).with_settings(unprotected());
    let mut h = harness(state);
    settle(&mut h);

    open_custom_command(&mut h);
    h.get_by_label("Command input").focus();
    h.get_by_label("Command input")
        .type_text("git reset --hard");
    settle(&mut h);

    h.get_by_label("Run on 2 of 2").click();
    settle(&mut h);
    assert!(h.state().ui.bulk_run.is_none(), "nothing dispatched yet");
    assert!(h.state().ui.bulk_command_armed, "the confirm is armed");
    assert_painted(&h, "Destructive command");

    h.get_by_label("Yes, run git reset --hard").click();
    settle(&mut h);
    assert!(h.state().ui.bulk_run.is_some(), "the confirmed run started");

    wait_for(&mut h, |s| {
        s.ui.bulk_run
            .as_ref()
            .is_some_and(|v| v.rows.len() == 2 && v.rows.iter().all(|r| r.state == RowState::Done))
    });
}

#[test]
fn recent_commands_are_offered_in_the_modal_and_refill_the_input() {
    let (project, alpha, ui) = two_repo_project("recents");
    let mut state =
        AppState::for_roots(&project, &[alpha.clone(), ui.clone()]).with_settings(unprotected());
    state.ui.recent_custom_commands = vec!["git gc".to_string()];
    let mut h = harness(state);
    settle(&mut h);

    open_custom_command(&mut h);

    assert_painted(&h, "Recent commands");
    h.get_by_label("Use git gc").click();
    settle(&mut h);
    assert_eq!(h.state().ui.bulk_command, "git gc");

    h.get_by_label("Run on 2 of 2").click();
    settle(&mut h);
    assert!(h.state().ui.bulk_run.is_some(), "the monitor opened");
    wait_for(&mut h, |s| {
        s.ui.bulk_run
            .as_ref()
            .is_some_and(|v| v.rows.iter().all(|r| r.state == RowState::Done))
    });

    let recents = &h.state().ui.recent_custom_commands;
    assert!(
        recents.first().is_some_and(|c| c == "git gc"),
        "the confirmed command is recorded: {recents:?}"
    );
    assert_not_painted(&h, "git remote prune origin");
}
