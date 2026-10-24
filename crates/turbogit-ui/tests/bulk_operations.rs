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
use test_support::git_seed::git;
use test_support::harness::{
    assert_matrix_geometry, assert_not_painted, assert_painted, painted_galleys, painted_text,
    settle, shell_harness_over_unstyled,
};
use turbogit_app::state::AppState;
use turbogit_services::bulk_ops::BulkOp;

/// Create an initialized temp repository with one base commit on `main`
/// plus an `origin` remote so upstream reads can be exercised.
/// Local repo builder, not `git_seed::repo_with_origin`: this suite's change
/// lists and diff panes name the file, and the recipe commits `README.md` where
/// this commits `base.txt`.
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
    let base = test_support::scratch_root(&format!("bulk-{tag}"));
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
///
/// Unstyled: the grid's painted labels and status rows are read off the frame,
/// and the embedded JetBrains Mono would lay them out in a different face.
///
/// `max_steps` is 1024, not kittest's default of 4, which `Harness::run`
/// panics past.
fn harness(state: AppState) -> Harness<'static, AppState> {
    shell_harness_over_unstyled(state, egui::vec2(1280.0, 800.0), 1024)
}

/// A matrix OUTCOME cell for a root on a protected branch: the skip reason's
/// own label and the modal's detail for it, composed exactly as the row paints
/// them.
const PROTECTED_OUTCOME: &str = "Skipped (protected branch) — the run may not touch it";

/// The repos whose matrix row refuses a protected branch.
///
/// Scoped by position, and it has to be: the same sentence is painted by the
/// run monitor and restated by the history list after a run has finished, so
/// "the frame contains these words" would pass on a stale surface. This asks
/// instead that the refusal sits in the modal's own OUTCOME column — the x the
/// `OUTCOME` header paints at — and returns the repo sharing its line, so the
/// claim is one refusal per row of *this* matrix.
fn protected_refusals(h: &Harness<'_, AppState>) -> Vec<String> {
    let galleys = painted_galleys(h);
    let same_line = |a: f32, b: f32| (a - b).abs() < 1.0;
    // The matrix's own header row, and the two column edges it declares. Read
    // off the line that carries `OUTCOME`, because the shell paints another
    // table's `REPO` header elsewhere in the frame — taking the first galley
    // that says `REPO` hands back that other table's column edge and pairs
    // every refusal with nothing.
    let mut headers: Vec<(f32, f32)> = Vec::new();
    for outcome in galleys.iter().filter(|g| g.text == "OUTCOME") {
        if let Some(repo) = galleys
            .iter()
            .find(|g| g.text == "REPO" && same_line(g.pos.y, outcome.pos.y))
        {
            headers.push((repo.pos.x, outcome.pos.x));
        }
    }
    assert_eq!(
        headers.len(),
        1,
        "exactly one matrix header row is painted; found {headers:?}"
    );
    let (repo_x, outcome_x) = headers.into_iter().next().expect("one matrix header row");

    let mut refusals = Vec::new();
    for row in galleys.iter().filter(|g| g.text == PROTECTED_OUTCOME) {
        assert!(
            same_line(row.pos.x, outcome_x),
            "`{PROTECTED_OUTCOME}` painted outside the matrix's OUTCOME column at {:?} \
             (column starts at {outcome_x})",
            row.pos
        );
        let repo = galleys
            .iter()
            .find(|g| same_line(g.pos.x, repo_x) && same_line(g.pos.y, row.pos.y))
            .unwrap_or_else(|| panic!("no repo cell shares the refusal's line at {:?}", row.pos));
        refusals.push(repo.text.clone());
    }
    refusals.sort();
    refusals
}

/// The texts painted on the same line as `needle`, in reading order, including
/// `needle` itself. The pairing, not the lookup, is the point: a count read off
/// the frame alone could be any surface's count — and a line of this frame
/// carries the operations grid's tile labels as well as the modal's own header,
/// so "everything on this y" is never the claim. Ask for the two texts that
/// belong together and, where the order is a fact of the design, for that order.
fn line_with(h: &Harness<'_, AppState>, needle: &str) -> Vec<String> {
    let galleys = painted_galleys(h);
    let line = galleys
        .iter()
        .find(|g| g.text == needle)
        .unwrap_or_else(|| panic!("`{needle}` was not painted; {:?}", painted_text(h)))
        .pos
        .y;
    let mut texts: Vec<(f32, String)> = galleys
        .iter()
        .filter(|g| (g.pos.y - line).abs() < 1.0)
        .map(|g| (g.pos.x, g.text.clone()))
        .collect();
    texts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    texts.into_iter().map(|(_, t)| t).collect()
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

    // The matrix's column geometry: every header label over its cell, and both
    // band heights the painter was handed.
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
}

// -- Protected-branch policy (screen 02) --------------------------------------

/// The protected case, rendered: with the rebase policy on, every root on a
/// protected branch is a row the matrix refuses to touch, and the refusal is
/// what the header counts. The shipped defaults protect `main` and both repos
/// sit on it, so the policy toggle is what brings the refusal up — and it
/// weighs before the tree does: `ui` is dirty, and once the toggle lands its
/// row says nothing about the dirt.
#[test]
fn a_rebasing_pull_refuses_every_protected_root_in_the_matrix() {
    let (project, alpha, ui) = two_repo_project("protected");
    let state = AppState::for_roots(&project, &[alpha.clone(), ui.clone()]);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Select group frontend").click();
    settle(&mut h);
    h.get_by_label("Pull all").click();
    settle(&mut h);

    // A merging update rewrites nothing, so `main` is runnable and no row
    // refuses it: the protection is about the rewrite, not the branch's name.
    assert_painted(&h, "Merge upstream");
    assert!(
        protected_refusals(&h).is_empty(),
        "a merging pull names no protected branch"
    );

    h.get_by_label("Rebase onto upstream instead of merging")
        .click();
    settle(&mut h);
    assert!(
        h.state().ui.bulk_rebase,
        "the policy control took the click"
    );

    assert_eq!(
        protected_refusals(&h),
        vec!["alpha".to_string(), "ui".to_string()],
        "one refusal row per root on the protected branch"
    );
    assert_not_painted(&h, "Skipped (dirty worktree)");
    // The header counts the refusals the matrix just predicted — the sentence
    // and its count read left to right off one line, the count right-anchored
    // the way the modal puts it.
    let header: Vec<String> = line_with(&h, "0 of 2 will run")
        .into_iter()
        .filter(|t| t.ends_with("will run") || t.ends_with("will be skipped."))
        .collect();
    assert_eq!(
        header,
        vec![
            "Will run on 0 repositories. 2 will be skipped.".to_string(),
            "0 of 2 will run".to_string(),
        ],
        "the refusal count and the sentence explaining it are one line of the modal"
    );

    // And there is nothing to confirm: the run button is inert, so the click
    // cannot start the pass that would rewrite the refused branch. Asserted
    // through the widget and the state, never through its ink — a button's
    // galley carries the layout WHITE rather than the theme's label colour.
    h.get_by_label("Run on 0 of 2").click();
    settle(&mut h);
    assert_eq!(
        h.state().ui.bulk_op,
        Some(BulkOp::PullAll),
        "the modal is still preflighting"
    );
    assert!(h.state().ui.bulk_run.is_none(), "nothing ran");
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
