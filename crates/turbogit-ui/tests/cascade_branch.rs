//! Issue 11 — cascade create & checkout branch: the UI seam.
//!
//! Tests drive the real [`turbogit_ui::ui::render`] through `egui_kittest`
//! over temporary git repositories and assert on public surfaces: painted
//! labels, accessible widget labels, and `AppState` transitions. The
//! cascade modal (screen 02) takes a branch name with the type/owner/value
//! pattern hint, predicts per-repo outcomes in the matrix, and only runs on
//! explicit confirmation — Esc cancels before anything executes.

use egui::Key;
use egui_kittest::{Harness, kittest::Queryable};
use std::path::{Path, PathBuf};
use std::time::Duration;
use test_support::harness::{assert_painted, settle};
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

/// A project with both repos under one group so a single group click
/// selects the fleet. `ui` starts with a dirty tracked file so skip
/// classification has something to bite on.
fn two_repo_project(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
    let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .join(format!(".scratch/cascade-branch-{tag}"));
    let _ = std::fs::remove_dir_all(&base);
    let project = base.join("wsb");
    let frontend = project.join("frontend");
    std::fs::create_dir_all(&frontend).unwrap();
    let alpha = temp_repo(&frontend, "alpha");
    let ui = temp_repo(&frontend, "ui");
    std::fs::write(ui.join("base.txt"), "uncommitted\n").unwrap();
    (project, alpha, ui)
}

/// Headless harness driving the full app UI (mirrors `bulk_operations`).
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

#[test]
fn the_cascade_modal_predicts_outcomes_and_runs_only_on_confirmation() {
    let (project, alpha, ui) = two_repo_project("modal");
    let state = AppState::for_roots(&project, &[alpha.clone(), ui.clone()]);
    let mut h = harness(state);
    settle(&mut h);

    // Selecting the group surfaces the operations grid; the cascade op is
    // wired and opens its modal.
    h.get_by_label("Select group frontend").click();
    settle(&mut h);
    h.get_by_label("Create & checkout branch").click();
    settle(&mut h);
    assert_eq!(h.state().ui.bulk_op, Some(BulkOp::CreateBranch));

    // The pattern hint and the apply-broadly policy control are painted.
    assert_painted(&h, "Pattern: type / owner / value");
    assert_painted(&h, "Existing matches are checked out");
    assert_painted(&h, "Apply broadly");

    // The caption is painted; the input carries the accessible label.
    assert_painted(&h, "Branch name");

    // With no branch name typed, confirming is inert — nothing dispatches.
    h.get_by_label("Run on 1 of 2").click();
    settle(&mut h);
    assert!(h.state().ui.bulk_run.is_none(), "no run without a name");
    assert_eq!(h.state().ui.bulk_op, Some(BulkOp::CreateBranch));

    // Typing the name fills the matrix: per-repo predicted outcomes under
    // the TARGET column.
    h.get_by_label("Branch name input").focus();
    h.get_by_label("Branch name input").type_text("feature/x");
    settle(&mut h);
    assert_painted(&h, "TARGET");
    assert_painted(&h, "Create from HEAD, checkout");
    assert_painted(&h, "Skipped (dirty worktree)");

    // Confirming closes the modal and opens the run monitor.
    h.get_by_label("Run on 1 of 2").click();
    settle(&mut h);
    assert_eq!(h.state().ui.bulk_op, None);
    assert!(h.state().ui.bulk_run.is_some(), "the monitor opened");

    // Every selected repo reaches a terminal state: alpha Done, ui skipped.
    wait_for(&mut h, |s| {
        s.ui.bulk_run.as_ref().is_some_and(|v| {
            v.rows.len() == 2
                && v.rows
                    .iter()
                    .all(|r| matches!(r.state, RowState::Done | RowState::Skipped { .. }))
        })
    });
    let view = h.state().ui.bulk_run.as_ref().unwrap();
    let by_name = |name: &str| view.rows.iter().find(|r| r.name == name).unwrap();
    assert_eq!(by_name("alpha").state, RowState::Done);
    assert!(matches!(by_name("ui").state, RowState::Skipped { .. }));

    // alpha actually landed on the new branch; ui is untouched.
    let on_branch = git(&alpha, &["branch", "--show-current"]);
    assert_eq!(on_branch.trim(), "feature/x");
    let ui_branch = git(&ui, &["branch", "--show-current"]);
    assert_eq!(ui_branch.trim(), "main");
}

#[test]
fn esc_cancels_the_cascade_before_anything_runs() {
    let (project, alpha, ui) = two_repo_project("esc");
    let state = AppState::for_roots(&project, &[alpha.clone(), ui.clone()]);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Select group frontend").click();
    settle(&mut h);
    h.get_by_label("Create & checkout branch").click();
    settle(&mut h);
    h.get_by_label("Branch name input").focus();
    h.get_by_label("Branch name input").type_text("feature/x");
    settle(&mut h);

    h.key_press(Key::Escape);
    settle(&mut h);

    // The modal closed without dispatching a run, and no repo moved.
    assert_eq!(h.state().ui.bulk_op, None);
    assert!(h.state().ui.bulk_run.is_none(), "no run was started");
    assert_eq!(git(&alpha, &["branch", "--show-current"]).trim(), "main");
    assert_eq!(git(&ui, &["branch", "--show-current"]).trim(), "main");
}
