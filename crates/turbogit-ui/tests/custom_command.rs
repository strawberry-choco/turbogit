//! Issue 13 — custom command across a selection: the UI seam.
//!
//! Tests drive the real [`turbogit_ui::ui::render`] through `egui_kittest`
//! over temporary git repositories and assert on public surfaces: painted
//! labels, accessible widget labels, and `AppState` transitions. The
//! "Custom command…" tile opens the preflight modal with a command input
//! and the workspace's recent commands; a destructive-looking command needs
//! a second, explicit confirmation before anything runs.

use egui::Key;
use egui_kittest::{Harness, kittest::Queryable};
use std::path::{Path, PathBuf};
use std::time::Duration;
use test_support::harness::{assert_not_painted, assert_painted, painted_text, settle};
use turbogit_app::state::AppState;
use turbogit_services::bulk_ops::BulkOp;
use turbogit_services::bulk_run::RowState;

/// Run `git <args>` in `repo`, asserting success, and return stdout.
fn git(repo: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git invocation");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 stdout")
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

/// A project with both repos under one group so a single group click
/// selects the fleet.
fn two_repo_project(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
    let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join(format!(".scratch/custom-command-{tag}"));
    let _ = std::fs::remove_dir_all(&base);
    let project = base.join("wsb");
    let frontend = project.join("frontend");
    std::fs::create_dir_all(&frontend).unwrap();
    let alpha = temp_repo(&frontend, "alpha");
    let ui = temp_repo(&frontend, "ui");
    (project, alpha, ui)
}

/// Headless harness driving the full app UI (mirrors `cascade_branch`).
fn harness(state: AppState) -> Harness<'static, AppState> {
    let mut h = Harness::builder().with_max_steps(1024).build_ui_state(
        |ui, state| {
            state.drain_events();
            turbogit_ui::ui::render(ui, state);
        },
        state,
    );
    h.set_size(egui::vec2(1280.0, 800.0));
    h
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

/// Step until `needle` is painted, or fail loudly.
///
/// Confirming a run closes the modal and opens the monitor; the monitor fills
/// its rows a frame or two later, so a single `settle` can land on the frame
/// between "modal gone" and "monitor drawn" and then report the command as
/// missing. Under the whole-workspace suite's parallel load that gap widens,
/// which made the command assertion flake.
fn wait_for_painted(harness: &mut Harness<'_, AppState>, needle: &str) {
    for _ in 0..1000 {
        harness.step();
        if painted_text(harness).iter().any(|t| t.contains(needle)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("`{needle}` was never painted within 1000 frames");
}

/// Select the fleet group and open the custom-command modal.
fn open_custom_command(h: &mut Harness<'_, AppState>) {
    h.get_by_label("Select group frontend").click();
    settle(h);
    h.get_by_label("Custom command…").click();
    settle(h);
}

#[test]
fn the_custom_command_tile_opens_a_modal_that_runs_the_typed_command() {
    let (project, alpha, ui) = two_repo_project("tile");
    let state = AppState::for_roots(&project, &[alpha.clone(), ui.clone()]);
    let mut h = harness(state);
    settle(&mut h);

    open_custom_command(&mut h);
    assert_eq!(h.state().ui.bulk_op, Some(BulkOp::Custom));

    // The modal has a labeled command input; with nothing typed, confirming
    // is inert — nothing dispatches.
    assert_painted(&h, "Command");
    h.get_by_label("Run on 2 of 2").click();
    settle(&mut h);
    assert!(h.state().ui.bulk_run.is_none(), "no run without a command");
    assert_eq!(h.state().ui.bulk_op, Some(BulkOp::Custom));

    // Typing the command fills the matrix — custom commands never skip.
    h.get_by_label("Command input").focus();
    h.get_by_label("Command input")
        .type_text("git branch bulk-marker");
    settle(&mut h);
    assert_painted(&h, "Will run on all 2 repositories.");

    // Confirming closes the modal and opens the run monitor, which shows
    // the typed command on the running rows.
    h.get_by_label("Run on 2 of 2").click();
    settle(&mut h);
    assert_eq!(h.state().ui.bulk_op, None);
    assert!(h.state().ui.bulk_run.is_some(), "the monitor opened");
    wait_for_painted(&mut h, "git branch bulk-marker");

    wait_for(&mut h, |s| {
        s.ui.bulk_run
            .as_ref()
            .is_some_and(|v| v.rows.len() == 2 && v.rows.iter().all(|r| r.state == RowState::Done))
    });

    // The command's effect is real on every selected repo.
    for repo in [&alpha, &ui] {
        assert!(
            git(repo, &["branch", "--list", "bulk-marker"]).contains("bulk-marker"),
            "{repo:?} got the branch"
        );
    }

    // Esc closes the monitor.
    h.key_press(Key::Escape);
    settle(&mut h);
}

#[test]
fn a_destructive_command_needs_a_second_explicit_confirmation() {
    let (project, alpha, ui) = two_repo_project("destructive");
    let state = AppState::for_roots(&project, &[alpha.clone(), ui.clone()]);
    let mut h = harness(state);
    settle(&mut h);

    open_custom_command(&mut h);
    h.get_by_label("Command input").focus();
    // Destructive-looking, but on these clean single-commit repos it is a
    // no-op — the point is the confirmation gate, not the damage.
    h.get_by_label("Command input")
        .type_text("git reset --hard");
    settle(&mut h);

    // The first Run click only arms: the warning strip appears with the
    // reason and the button becomes an explicit confirm naming the command.
    h.get_by_label("Run on 2 of 2").click();
    settle(&mut h);
    assert!(h.state().ui.bulk_run.is_none(), "nothing dispatched yet");
    assert!(h.state().ui.bulk_command_armed, "the confirm is armed");
    assert_painted(&h, "Destructive command");

    // The second click dispatches.
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
    let mut state = AppState::for_roots(&project, &[alpha.clone(), ui.clone()]);
    state.ui.recent_custom_commands = vec!["git gc".to_string()];
    let mut h = harness(state);
    settle(&mut h);

    open_custom_command(&mut h);

    // The recent command is offered; clicking it fills the input.
    assert_painted(&h, "Recent commands");
    h.get_by_label("Use git gc").click();
    settle(&mut h);
    assert_eq!(h.state().ui.bulk_command, "git gc");

    // And the refilled command runs on confirm.
    h.get_by_label("Run on 2 of 2").click();
    settle(&mut h);
    assert!(h.state().ui.bulk_run.is_some(), "the monitor opened");
    wait_for(&mut h, |s| {
        s.ui.bulk_run
            .as_ref()
            .is_some_and(|v| v.rows.iter().all(|r| r.state == RowState::Done))
    });

    // A confirmed run is remembered for the next modal.
    let recents = &h.state().ui.recent_custom_commands;
    assert!(
        recents.first().is_some_and(|c| c == "git gc"),
        "the confirmed command is recorded: {recents:?}"
    );
    assert_not_painted(&h, "git remote prune origin");
}
