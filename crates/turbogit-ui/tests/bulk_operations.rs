//! Issue 09 — bulk operations grid with preflight matrix.
//!
//! With repos selected, the central selection surface grows the operations
//! grid (screen 04): Fetch all / Pull all / Push all / Stash all. Choosing
//! any of them opens the preflight matrix modal (screen 02) — per-repo
//! state, predicted outcome, skip reasons, "N of M will run", and a policy
//! control — and confirming runs the operation across the fleet, reporting
//! per-repo outcomes.
//!
//! Tests drive the real [`turbogit_ui::ui::render`] through `egui_kittest`
//! over temporary git repositories and assert on public surfaces: painted
//! labels, accessible widget labels, and `AppState` transitions.
use egui_kittest::{Harness, kittest::Queryable};
use std::path::{Path, PathBuf};
use std::time::Duration;
use test_support::harness::{assert_not_painted, assert_painted, settle};
use turbogit_app::state::AppState;
use turbogit_services::bulk_ops::BulkOp;

/// Run `git` in `repo`, asserting success, and return stdout.
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
        .nth(2)
        .unwrap()
        .join(format!(".scratch/bulk-{tag}"));
    let _ = std::fs::remove_dir_all(&base);
    let project = base.join("wsb");
    let frontend = project.join("frontend");
    std::fs::create_dir_all(&frontend).unwrap();
    let alpha = temp_repo(&frontend, "alpha");
    let ui = temp_repo(&frontend, "ui");
    std::fs::write(ui.join("base.txt"), "uncommitted\n").unwrap();
    (project, alpha, ui)
}

/// Headless harness driving the full app UI (mirrors `multi_selection`).
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

// -- Operations grid ----------------------------------------------------------

#[test]
fn selecting_repos_offers_the_bulk_operations_grid() {
    let (project, alpha, ui) = two_repo_project("grid");
    let state = AppState::for_roots(&project, &[alpha.clone(), ui.clone()]);
    let mut h = harness(state);
    settle(&mut h);

    // Nothing selected → no operations grid on the Commit surface.
    assert!(
        !test_support::harness::painted_text(&h)
            .iter()
            .any(|t| t == "OPERATIONS")
    );

    h.get_by_label("Select group frontend").click();
    settle(&mut h);

    // The selection surface hosts the operations grid (screen 04).
    assert_painted(&h, "OPERATIONS");
    h.get_by_label("Fetch all").click();
    settle(&mut h);

    // Choosing an operation opens the preflight matrix modal first.
    assert_eq!(h.state().ui.bulk_op, Some(BulkOp::FetchAll));
    assert_painted(&h, "Run on 2 of 2");

    // Cancel closes the modal without dispatching anything.
    h.get_by_label("Cancel").click();
    settle(&mut h);
    assert_eq!(h.state().ui.bulk_op, None);
    assert_not_painted(&h, "Run on 2 of 2");
}

// -- Preflight matrix (screen 02) ---------------------------------------------

#[test]
fn the_preflight_matrix_shows_per_repo_status_and_skip_reasons() {
    let (project, alpha, ui) = two_repo_project("matrix");
    let state = AppState::for_roots(&project, &[alpha.clone(), ui.clone()]);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Select group frontend").click();
    settle(&mut h);
    h.get_by_label("Pull all").click();
    settle(&mut h);

    assert_eq!(h.state().ui.bulk_op, Some(BulkOp::PullAll));
    // The header states what will run and totals the skips.
    assert_painted(&h, "1 of 2 will run");
    assert_painted(&h, "1 will be skipped");
    // The matrix columns (screen 02).
    assert_painted(&h, "REPO");
    assert_painted(&h, "CURRENT BRANCH");
    assert_painted(&h, "STATUS");
    assert_painted(&h, "OUTCOME");
    // ui is dirty → its row names the skip reason; alpha predicts a merge.
    assert_painted(&h, "Skipped (dirty worktree) — commit or stash first");
    assert_painted(&h, "Merge upstream");
    assert_painted(&h, "Dirty");
    assert_painted(&h, "Clean");
    // The run button names the exact confirmed scope.
    assert_painted(&h, "Run on 1 of 2");
    // The pull policy control, seeded from settings (merge default).
    assert!(!h.state().ui.bulk_rebase);
    assert_painted(&h, "Rebase onto upstream instead of merging");
}

#[test]
fn confirming_the_preflight_runs_the_fleet_and_reports_outcomes() {
    let (project, alpha, ui) = two_repo_project("run");
    let state = AppState::for_roots(&project, &[alpha.clone(), ui.clone()]);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Select group frontend").click();
    settle(&mut h);
    // Stash all: ui (dirty) runs, alpha (clean) is skipped.
    h.get_by_label("Stash all").click();
    settle(&mut h);
    assert_painted(&h, "Run on 1 of 2");
    h.get_by_label("Run on 1 of 2").click();

    // The modal closes on confirm and the aggregate outcome toast lands.
    wait_for(&mut h, |s| s.ui.toast.is_some() && s.ui.bulk_op.is_none());
    assert_eq!(h.state().ui.bulk_op, None);
    let toast = h.state().ui.toast.as_ref().unwrap();
    assert_eq!(toast.message, "Stash all · 1 of 1 ok");

    // The planned root was stashed; the skipped root keeps its changes.
    let ui_status = git(&ui, &["status", "--porcelain"]);
    assert!(ui_status.trim().is_empty(), "ui was stashed: {ui_status:?}");
    let ui_stashes = git(&ui, &["stash", "list"]);
    assert!(
        !ui_stashes.trim().is_empty(),
        "ui's changes are in the stash"
    );
    let alpha_status = git(&alpha, &["status", "--porcelain"]);
    assert!(
        alpha_status.trim().is_empty(),
        "alpha was clean all along: {alpha_status:?}"
    );
    let alpha_stashes = git(&alpha, &["stash", "list"]);
    assert!(
        alpha_stashes.trim().is_empty(),
        "skipped root must not be stashed: {alpha_stashes:?}"
    );
}

/// Step frames, yielding to the worker thread, until `pred(state)` holds.
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
