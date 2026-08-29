//! Issue #13 — diff viewer restyle: modes, comparison chips, tokens.
//!
//! Headless egui_kittest harness driving [`turbogit_ui::ui::render`] over real
//! temp repositories (system `git`) seeded with staged/unstaged edits.
//! Asserts only on public surfaces:
//!
//! - **Painted output** — text galleys carry their strings; filled rects
//!   carry geometry + token color (`Palette::DIFF_*`).
//! - **State transitions** — public `AppState` fields after the frames.
//!
//! Covered (spec §8.4):
//! - Repo/Staged/Local chips select HEAD↔worktree / HEAD↔index / index↔worktree
//! - segmented control toggles side-by-side/unified rendering
//! - hunk nav ‹ n/N › counts and steps correctly
//! - Ignore whitespace toggle affects the diff
//! - add/del rows paint token-exact backgrounds; gutters show muted numbers

use std::path::{Path, PathBuf};
use std::process::Command;

use egui::{Color32, Pos2, Rect, Shape};
use egui_kittest::{Harness, kittest::Queryable};
use turbogit_app::state::{AppState, DiffComparison};
use turbogit_ui::theme::{Palette, configure_style, install_fonts};

// --- git seeding -------------------------------------------------------------

/// Run `git <args>` in `dir`, asserting success; returns stdout.
fn run_git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawning git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn write_file(repo: &Path, name: &str, content: &str) {
    std::fs::write(repo.join(name), content).expect("writing file");
}

fn init_repo() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repo dir");
    run_git(&repo, &["init", "-b", "main"]);
    run_git(&repo, &["config", "user.email", "test@example.com"]);
    run_git(&repo, &["config", "user.name", "Test"]);
    (tmp, repo)
}

/// Repo where `file.txt` carries BOTH a staged edit (line 2 `beta`→`BETA`)
/// and an unstaged edit (line 8 `gamma`→`GAMMA`). The edits sit far enough
/// apart that each chip's diff shows its own change without the other's
/// content leaking into surrounding context lines:
///
/// - Repo  (HEAD↔worktree): both `BETA` and `GAMMA`
/// - Staged (HEAD↔index):   `BETA` only
/// - Local (index↔worktree): `GAMMA` only
fn repo_mixed() -> (tempfile::TempDir, PathBuf) {
    let (tmp, repo) = init_repo();
    write_file(
        &repo,
        "file.txt",
        "alpha\nbeta\ndelta\nepsilon\nzeta\neta\ntheta\ngamma\niota\n",
    );
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "c1"]);
    // Staged edit: beta → BETA.
    write_file(
        &repo,
        "file.txt",
        "alpha\nBETA\ndelta\nepsilon\nzeta\neta\ntheta\ngamma\niota\n",
    );
    run_git(&repo, &["add", "file.txt"]);
    // Unstaged edit on top: gamma → GAMMA.
    write_file(
        &repo,
        "file.txt",
        "alpha\nBETA\ndelta\nepsilon\nzeta\neta\ntheta\nGAMMA\niota\n",
    );
    (tmp, repo)
}

/// Repo where `one.txt` is fully staged (no unstaged part): the Local chip
/// must report "(no differences)" while Staged still shows the change.
fn repo_staged_only() -> (tempfile::TempDir, PathBuf) {
    let (tmp, repo) = init_repo();
    write_file(&repo, "one.txt", "one\ntwo\n");
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "c1"]);
    write_file(&repo, "one.txt", "one\nTWO\n");
    run_git(&repo, &["add", "one.txt"]);
    (tmp, repo)
}

/// Repo whose only change is a whitespace-only unstaged edit
/// (`gamma` → `g amma`): toggling Ignore whitespace empties the diff.
fn repo_whitespace() -> (tempfile::TempDir, PathBuf) {
    let (tmp, repo) = init_repo();
    write_file(&repo, "ws.txt", "alpha\nbeta\ngamma\n");
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "c1"]);
    write_file(&repo, "ws.txt", "alpha\nbeta\ng amma\n");
    (tmp, repo)
}

/// Repo with two separated unstaged edits in `nav.txt` (lines 2 and 12),
/// producing exactly two hunks for navigation tests.
fn repo_two_hunks() -> (tempfile::TempDir, PathBuf) {
    let (tmp, repo) = init_repo();
    let base: Vec<String> = (1..=16).map(|i| format!("l{i:02}")).collect();
    write_file(&repo, "nav.txt", &format!("{}\n", base.join("\n")));
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "c1"]);
    let mut edited = base;
    edited[1] = "X2".to_string();
    edited[11] = "X12".to_string();
    write_file(&repo, "nav.txt", &format!("{}\n", edited.join("\n")));
    (tmp, repo)
}

// --- harness -----------------------------------------------------------------

/// Harness rendering the full shell over a real repository. Mirrors
/// production setup (`app.rs`): dark tokens every frame, fonts installed
/// once, and the worker-event channel drained every frame before render.
fn diff_harness(repo: &Path) -> Harness<'static, AppState> {
    let state = AppState::new(repo.to_path_buf());
    assert!(
        !state.multi.roots.is_empty(),
        "repo root must be discovered"
    );
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, state| {
            configure_style(ui.ctx());
            if !fonts_installed {
                install_fonts(ui.ctx());
                fonts_installed = true;
            }
            state.drain_events(); // production parity with app.rs
            turbogit_ui::ui::render(ui, state);
        },
        state,
    );
    harness.set_size(egui::vec2(1024.0, 768.0));
    harness
}

/// All text painted by the last completed frame.
pub fn painted_text(harness: &Harness<'_, AppState>) -> Vec<String> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect()
}

/// Assert `needle` appears in some painted text galley.
#[track_caller]
pub fn assert_painted(harness: &Harness<'_, AppState>, needle: &str) {
    let texts = painted_text(harness);
    assert!(
        texts.iter().any(|t| t.contains(needle)),
        "`{needle}` was not painted; painted text:\n{texts:#?}"
    );
}

/// Assert `needle` appears in no painted text galley.
#[track_caller]
pub fn assert_not_painted(harness: &Harness<'_, AppState>, needle: &str) {
    let texts = painted_text(harness);
    assert!(
        !texts.iter().any(|t| t.contains(needle)),
        "`{needle}` was unexpectedly painted; painted text:\n{texts:#?}"
    );
}

/// Paint-time origin of the first text galley painting exactly `text`.
pub fn galley_origin(harness: &Harness<'_, AppState>, text: &str) -> Option<Pos2> {
    harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(shape) if shape.galley.text() == text => Some(shape.pos),
            _ => None,
        })
}

/// Every filled rectangle painted by the last frame as `(rect, fill)`.
pub fn filled_rects(harness: &Harness<'_, AppState>) -> Vec<(Rect, Color32)> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Rect(rect_shape) if rect_shape.fill != Color32::TRANSPARENT => {
                Some((rect_shape.rect, rect_shape.fill))
            }
            _ => None,
        })
        .collect()
}

/// Step frames until the painted output stabilizes AND no async diff load is
/// pending (the fingerprint includes the loading flag so a late `DiffReady`
/// can never be mistaken for a settled frame). Budgeted by wall-clock time —
/// not frame count — so a contended `git` subprocess cannot starve it.
pub fn settle(harness: &mut Harness<'_, AppState>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut prev = String::new();
    while std::time::Instant::now() < deadline {
        harness.step();
        let fingerprint = format!(
            "{:?}|loading={}",
            painted_text(harness),
            harness.state().ui.diff_loading
        );
        if fingerprint == prev && !harness.state().ui.diff_loading {
            return;
        }
        prev = fingerprint;
    }
    panic!("diff viewer did not settle within 15s");
}

/// Open the Commit tab's inline preview for one file (public state
/// transition — the same thing clicking a row's Diff button does).
fn open_preview(harness: &mut Harness<'_, AppState>, file: &Path) {
    harness.state_mut().ui.preview_change = Some(file.to_path_buf());
    settle(harness);
}

// --- Cycle 1: comparison chips ----------------------------------------------

#[test]
fn chips_select_their_documented_comparison_pairs() {
    let (_tmp, repo) = repo_mixed();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo.join("file.txt"));

    // Default chip is Local (index↔worktree) — preserves pre-existing behavior.
    assert_eq!(h.state().ui.diff_comparison, DiffComparison::Local);
    assert_painted(&h, "GAMMA");
    assert_not_painted(&h, "BETA");

    // Staged (HEAD↔index): only the staged edit.
    h.get_by_label("Staged").click();
    settle(&mut h);
    assert_eq!(h.state().ui.diff_comparison, DiffComparison::Staged);
    assert_painted(&h, "BETA");
    assert_not_painted(&h, "GAMMA");

    // Repo (HEAD↔worktree): both edits.
    h.get_by_label("Repo").click();
    settle(&mut h);
    assert_eq!(h.state().ui.diff_comparison, DiffComparison::Repo);
    assert_painted(&h, "BETA");
    assert_painted(&h, "GAMMA");
}

#[test]
fn local_chip_reports_no_differences_for_fully_staged_files() {
    let (_tmp, repo) = repo_staged_only();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo.join("one.txt"));

    assert_painted(&h, "(no differences)");

    h.get_by_label("Staged").click();
    settle(&mut h);
    assert_painted(&h, "TWO");
}

// --- Cycle 2: segmented mode control -----------------------------------------

#[test]
fn segmented_control_toggles_side_by_side_and_unified() {
    let (_tmp, repo) = repo_mixed();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo.join("file.txt"));

    // Unified is the default: no side-by-side pane headers.
    assert!(!h.state().ui.diff_side_by_side);
    assert_not_painted(&h, "After Working tree");

    h.get_by_label("Side-by-Side").click();
    settle(&mut h);
    assert!(h.state().ui.diff_side_by_side);
    // Pane headers document the active pair (Local chip: Index ↔ Working tree).
    assert_painted(&h, "Before Index");
    assert_painted(&h, "After Working tree");

    h.get_by_label("Unified").click();
    settle(&mut h);
    assert!(!h.state().ui.diff_side_by_side);
    assert_not_painted(&h, "After Working tree");
}

// --- Cycle 3: hunk navigation -------------------------------------------------

#[test]
fn hunk_navigation_counts_and_steps_across_two_hunks() {
    let (_tmp, repo) = repo_two_hunks();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo.join("nav.txt"));

    // Counter shows position 1 of 2.
    assert_painted(&h, "1/2");
    assert_eq!(h.state().ui.diff_current_hunk, 0);

    h.get_by_label("Next hunk").click();
    settle(&mut h);
    assert_eq!(h.state().ui.diff_current_hunk, 1);
    assert_painted(&h, "2/2");

    // Next at the last hunk stays put.
    h.get_by_label("Next hunk").click();
    settle(&mut h);
    assert_eq!(h.state().ui.diff_current_hunk, 1);

    h.get_by_label("Previous hunk").click();
    settle(&mut h);
    assert_eq!(h.state().ui.diff_current_hunk, 0);
    assert_painted(&h, "1/2");

    // Previous at the first hunk stays put.
    h.get_by_label("Previous hunk").click();
    settle(&mut h);
    assert_eq!(h.state().ui.diff_current_hunk, 0);
}

// --- Cycle 4: ignore whitespace -----------------------------------------------

#[test]
fn ignore_whitespace_toggle_affects_the_diff() {
    let (_tmp, repo) = repo_whitespace();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo.join("ws.txt"));

    assert!(!h.state().ui.diff_ignore_whitespace);
    assert_painted(&h, "g amma");

    h.get_by_label("Ignore whitespace").click();
    settle(&mut h);
    assert!(h.state().ui.diff_ignore_whitespace);
    assert_not_painted(&h, "g amma");
    assert_painted(&h, "(no differences)");

    // Toggling back restores the full diff.
    h.get_by_label("Ignore whitespace").click();
    settle(&mut h);
    assert!(!h.state().ui.diff_ignore_whitespace);
    assert_painted(&h, "g amma");
}

// --- Cycle 5: token-exact line styling ----------------------------------------

#[test]
fn add_del_rows_paint_token_backgrounds_with_muted_gutters() {
    let (_tmp, repo) = repo_mixed();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo.join("file.txt"));

    // The added line sits on the token add background…
    let add_pos = galley_origin(&h, "GAMMA").expect("added line painted");
    assert!(
        filled_rects(&h)
            .iter()
            .any(|(r, c)| *c == Palette::DIFF_ADD_BG && r.contains(add_pos)),
        "added line must be backed by DIFF_ADD_BG"
    );
    // …and the deleted line on the token del background.
    let del_pos = galley_origin(&h, "gamma").expect("deleted line painted");
    assert!(
        filled_rects(&h)
            .iter()
            .any(|(r, c)| *c == Palette::DIFF_DEL_BG && r.contains(del_pos)),
        "deleted line must be backed by DIFF_DEL_BG"
    );

    // Muted gutter numbers are painted alongside the changed lines
    // (new-file line number of the GAMMA/GAMMA hunk region).
    assert!(
        galley_origin(&h, "8").is_some(),
        "gutter line numbers must be painted"
    );

    // Hunk header band uses the SURFACE token.
    let hunk_header = painted_text(&h)
        .iter()
        .find(|t| t.starts_with("@@"))
        .cloned()
        .expect("hunk header painted");
    let hunk_pos = galley_origin(&h, &hunk_header).expect("hunk header origin");
    assert!(
        filled_rects(&h)
            .iter()
            .any(|(r, c)| *c == Palette::SURFACE && r.contains(hunk_pos)),
        "hunk header must be backed by SURFACE"
    );
}
