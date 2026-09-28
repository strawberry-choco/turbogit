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
//! - hunk nav aims the hunk it names, from any scroll position, in both modes
//! - Ignore whitespace toggle affects the diff
//! - add/del rows paint token-exact backgrounds; gutters show muted numbers
//! - the diff's ghost icon buttons (nav pair, gutter pair) are two scale
//!   settings of one shared primitive and keep their labels and enabled flags

use std::path::{Path, PathBuf};
use std::process::Command;

use egui::{Color32, Pos2, Rect, Shape, Vec2};
use egui_kittest::{Harness, kittest::NodeT, kittest::Queryable};
use turbogit_app::events::AppEvent;
use turbogit_app::keyed_read::{DiffTarget, Keyed};
use turbogit_app::state::{AppState, DiffComparison};
use turbogit_domain::error::TgError;
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

/// Run `git <args>` in `dir` without asserting success — a merge that
/// conflicts exits non-zero, which is exactly the state under test.
fn git_unchecked(dir: &Path, args: &[&str]) {
    let _ = Command::new("git").args(args).current_dir(dir).output();
}

/// Repo left mid-merge on `conf.txt`, so it reports
/// `ChangeStatus::Conflicted` and its hunk-header gutter pair renders
/// visible-but-disabled.
fn repo_conflicted() -> (tempfile::TempDir, PathBuf) {
    let (tmp, repo) = init_repo();
    write_file(&repo, "conf.txt", "one\ntwo\n");
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "c1"]);
    run_git(&repo, &["checkout", "-q", "-b", "side"]);
    write_file(&repo, "conf.txt", "one\nside\n");
    run_git(&repo, &["add", "conf.txt"]);
    run_git(&repo, &["commit", "-m", "side"]);
    run_git(&repo, &["checkout", "-q", "main"]);
    write_file(&repo, "conf.txt", "one\nmain line\n");
    run_git(&repo, &["add", "conf.txt"]);
    run_git(&repo, &["commit", "-m", "main line"]);
    git_unchecked(&repo, &["merge", "--no-edit", "side"]);
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
    // Tall enough that the shell frame (issue #03: repo header + center tabs +
    // status bar — nothing claims the top edge) leaves the preview column
    // room for the tallest comparison diff plus the message editor below it.
    harness.set_size(egui::vec2(1024.0, 900.0));
    harness
}

#[test]
fn diff_renders_its_keyed_read_waiting_message_through_the_shared_presenter() {
    let (_tmp, repo) = repo_two_hunks();
    let mut harness = diff_harness(&repo);
    harness.state_mut().ui.preview_change = Some(repo.join("nav.txt"));
    harness.step();
    assert_painted(&harness, "Computing diff…");
}

#[test]
fn diff_renders_its_keyed_read_failure_message_through_the_shared_presenter() {
    let (_tmp, repo) = repo_two_hunks();
    let mut harness = diff_harness(&repo);
    let path = repo.join("nav.txt");
    harness.state_mut().ui.preview_change = Some(path.clone());
    let target = DiffTarget::new(
        repo.clone(),
        None,
        None,
        DiffComparison::Local,
        false,
        Some(path),
    );
    harness
        .state_mut()
        .tx
        .send(AppEvent::DiffReady {
            key: target.key(),
            result: Err(TgError::Other("diff read failed".into())),
        })
        .expect("send diff failure");
    harness.step();
    assert_painted(&harness, "diff read failed");
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

/// Step frames until painted output AND the diff read's activity stabilize
/// (the fingerprint includes the read's verdict so a late `DiffReady` can never
/// be mistaken for a settled frame). Budgeted by wall-clock time — not frame
/// count — so a contended `git` subprocess cannot starve it.
pub fn settle(harness: &mut Harness<'_, AppState>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut prev = String::new();
    while std::time::Instant::now() < deadline {
        harness.step();
        let fingerprint = format!(
            "{:?}|read={}",
            painted_text(harness),
            harness.state().read_pending()
        );
        if fingerprint == prev && !harness.state().read_pending() {
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
fn relative_row_preview_uses_index_not_worktree_for_staged_changes() {
    for original in ["", "HEAD ONLY\n"] {
        let (_tmp, repo) = init_repo();
        write_file(&repo, "file.txt", original);
        run_git(&repo, &["add", "."]);
        run_git(&repo, &["commit", "-m", "base"]);
        write_file(&repo, "file.txt", "INDEX ONLY\n");
        run_git(&repo, &["add", "file.txt"]);
        write_file(&repo, "file.txt", "WORKTREE ONLY\n");

        let mut h = diff_harness(&repo);
        assert_eq!(
            h.state().settings.backend,
            turbogit_domain::model::GitBackend::InProcessReads
        );
        assert!(h.state().settings.in_process_diffs);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while h.query_by_label("file.txt").is_none() {
            h.step();
            assert!(
                std::time::Instant::now() < deadline,
                "file row did not load"
            );
        }
        h.get_by_label("file.txt").click();
        settle(&mut h);
        assert_eq!(h.state().ui.preview_change, Some(PathBuf::from("file.txt")));
        h.get_by_label("Staged").click();
        settle(&mut h);

        assert_painted(&h, "INDEX ONLY");
        assert_not_painted(&h, "WORKTREE ONLY");
        assert_not_painted(&h, "(no differences)");
    }
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

// --- Cycle 6: one shared ghost icon button at two scales --------------------

/// Drive the shared ghost icon-button primitive directly at both scales a
/// host uses: the standard 24 px square (the diff nav control) and the dense
/// 18 px rect a gutter supplies itself. One enabled, one not.
fn ghost_primitive_harness() -> Harness<'static, ()> {
    use turbogit_ui::ui::icons::{self, Icon};
    use turbogit_ui::ui::widgets::{self, ButtonVariant};
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, _state| {
            configure_style(ui.ctx());
            if !fonts_installed {
                install_fonts(ui.ctx());
                fonts_installed = true;
            }
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(24.0), egui::Sense::hover());
                widgets::ghost_icon_button(
                    ui,
                    rect,
                    ui.id().with("Nav ghost"),
                    "Nav ghost",
                    true,
                    |ui, rect, state| {
                        let ink = ButtonVariant::Ghost.text(state);
                        icons::centered_icon(ui, Icon::CHECK, rect.center(), 14.0, ink);
                    },
                );
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(18.0), egui::Sense::hover());
                widgets::ghost_icon_button(
                    ui,
                    rect,
                    ui.id().with("Gutter ghost"),
                    "Gutter ghost",
                    false,
                    |ui, _rect, state| {
                        let ink = ButtonVariant::Ghost.text(state);
                        icons::icon(ui, Icon::CHECK, 12.0, ink);
                    },
                );
            });
        },
        (),
    );
    harness.set_size(egui::vec2(240.0, 80.0));
    harness
}

/// Contract: the diff's ghost icon controls — the hunk-nav pair and the
/// hunk-header gutter pair — are two scale settings of one general primitive
/// built over the shared button-state vocabulary. What must survive that
/// collapse is everything *not* in the ladder: the control is still findable
/// by its accessibility label, it still reports enabled/disabled honestly,
/// and a conflicted hunk's gutter control still reports disabled rather than
/// disappearing.
#[test]
fn ghost_icon_controls_keep_their_labels_and_enabled_flags() {
    // --- the general primitive itself, at both scales ---------------------
    let mut primitives = ghost_primitive_harness();
    primitives.step();

    assert!(
        !primitives
            .get_by_label("Nav ghost")
            .accesskit_node()
            .is_disabled(),
        "an enabled ghost icon button must report itself enabled"
    );
    assert!(
        primitives
            .get_by_label("Gutter ghost")
            .accesskit_node()
            .is_disabled(),
        "a disabled ghost icon button must report itself disabled"
    );

    // --- the migrated hosts, on a real diff ------------------------------
    let (_tmp, repo) = repo_two_hunks();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo.join("nav.txt"));

    for label in ["Previous hunk", "Next hunk"] {
        assert!(
            !h.get_by_label(label).accesskit_node().is_disabled(),
            "the `{label}` nav control must stay findable by label and enabled \
             while the preview carries hunks"
        );
    }
    // The gutter pair on hunk 1 of a clean file is live.
    for label in ["Stage hunk 1", "Unstage hunk 1"] {
        assert!(
            !h.get_by_label(label).accesskit_node().is_disabled(),
            "the `{label}` gutter control must stay findable by label and \
             enabled on a clean hunk"
        );
    }

    // A conflicted hunk keeps the pair visible but inert.
    let (_tmp, conflicted) = repo_conflicted();
    let mut c = diff_harness(&conflicted);
    settle(&mut c);
    c.get_by_label("C conf.txt").click();
    settle(&mut c);
    assert!(
        !c.query_by_label("Stage hunk 1").is_none(),
        "conflicted files must still render their granular gutter controls"
    );
    assert!(
        c.get_by_label("Stage hunk 1")
            .accesskit_node()
            .is_disabled(),
        "a gutter control on a conflicted hunk must report disabled"
    );
}

// --- Cycle 7: hunk navigation aims the hunk it names -------------------------

/// A repo whose unstaged edit is `hunks` hunks of `changed` lines each, with
/// `gap` unchanged lines between them — a diff far taller than the preview pane,
/// and the shape hunk navigation needs: consecutive hunks are tens of rows
/// apart, so the list can be scrolled deep enough that the built row window
/// starts hundreds of rows down while a jump aims a hunk a few rows away.
fn repo_deep_diff(hunks: usize, changed: usize, gap: usize) -> (tempfile::TempDir, PathBuf) {
    let (tmp, repo) = init_repo();
    let span = changed + gap;
    let mut base: Vec<String> = Vec::new();
    for i in 0..(hunks * span + gap) {
        base.push(format!("l{i:04}"));
    }
    write_file(&repo, "deep.txt", &format!("{}\n", base.join("\n")));
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "c1"]);
    let mut edited = base.clone();
    for k in 0..hunks {
        for j in 0..changed {
            edited[k * span + j] = format!("X{k:02}{j:02}");
        }
    }
    write_file(&repo, "deep.txt", &format!("{}\n", edited.join("\n")));
    (tmp, repo)
}

/// The hunk header bands `git` itself reports for `deep.txt`, newest first, so
/// a test can name ONE hunk's header exactly as the pane paints it.
fn diff_hunk_headers(repo: &Path) -> Vec<String> {
    run_git(repo, &["diff", "-U3", "--", "deep.txt"])
        .lines()
        .filter(|l| l.starts_with("@@"))
        .map(str::to_owned)
        .collect()
}

/// Every hunk header the last frame painted.
fn painted_hunk_headers(harness: &Harness<'_, AppState>) -> Vec<String> {
    painted_text(harness)
        .into_iter()
        .filter(|t| t.starts_with("@@"))
        .collect()
}

/// Settle a frame that ISSUED a scroll. The aim is applied when the scroll area
/// ends the frame it was issued in, so the new offset first appears in the NEXT
/// frame's paint — and `settle` returns as soon as two frames agree, which it
/// can do on the frame before that. So a scroll is settled by stepping past it
/// first, then letting the layout settle.
fn settle_after_aim(harness: &mut Harness<'_, AppState>) {
    for _ in 0..3 {
        harness.step();
    }
    settle(harness);
}

#[track_caller]
fn assert_hunk_aimed_at(painted: &[String], want: &str, must_not_see: &[&str], what: &str) {
    assert!(
        painted.contains(&want.to_owned()),
        "{what}: the aimed hunk's header ({want}) is not on screen; painted \
         headers:\n{painted:#?}"
    );
    for away in must_not_see {
        assert!(
            !painted.contains(&away.to_string()),
            "{what}: the list is nowhere near the aimed hunk — {away} is on screen \
             instead; painted headers:\n{painted:#?}"
        );
    }
}

/// Aim three hunks in one open diff and check each one arrives on screen, the
/// list getting there from wherever the previous aim left it: the last hunk
/// (from the top), the hunk one step above it, and the first hunk.
///
/// `side_by_side` picks the rendering mode, because the two modes page over
/// DIFFERENT row streams over the same model and a hunk's row index is a slot
/// in one of them. Both modes are asked the same question here on purpose: an
/// aim that only works in one of them is a bug, and the mode names it.
fn hunk_aim_sequence(repo: &Path, side_by_side: bool) {
    const HUNKS: usize = 20;
    let headers = diff_hunk_headers(repo);
    assert_eq!(
        headers.len(),
        HUNKS,
        "the fixture's premise: {HUNKS} hunk headers to aim between"
    );
    let mode = if side_by_side {
        "side-by-side"
    } else {
        "unified"
    };

    let mut h = diff_harness(repo);
    h.state_mut().ui.diff_side_by_side = side_by_side;
    // Note what opening a diff costs the cursor: the keyed read's dispatch puts
    // `diff_current_hunk` back to 0, so the FIRST paint of this diff aims hunk 0
    // and spends that hunk's one aim. The near-the-top leg below therefore aims
    // hunk 1 — the next hunk down, still at the top of a 740-row diff — which is
    // also the one leg an "index space" bug cannot hide behind.
    settle(&mut h);
    open_preview(&mut h, &repo.join("deep.txt"));
    assert_eq!(h.state().ui.diff_side_by_side, side_by_side);
    assert!(
        painted_hunk_headers(&h).len() < HUNKS,
        "the pane cannot show all {HUNKS} headers at once — the premise of this test"
    );

    // --- the last hunk, from wherever the list is resting ---
    h.state_mut().ui.diff_current_hunk = HUNKS - 1;
    settle_after_aim(&mut h);
    let deep = painted_hunk_headers(&h);
    assert_hunk_aimed_at(&deep, &headers[HUNKS - 1], &[&headers[0]], mode);
    assert!(
        deep.iter().all(|hd| {
            headers
                .iter()
                .position(|x| x == hd)
                .is_some_and(|k| k >= HUNKS / 2)
        }),
        "{mode}: every painted header should be in the bottom half of the diff after \
         aiming the last hunk, got {deep:?}"
    );

    // --- one hunk's step UP, from deep: the built row window now starts
    // hundreds of rows down, and the target is a few rows above the fold ---
    h.get_by_label("Previous hunk").click();
    settle_after_aim(&mut h);
    assert_eq!(h.state().ui.diff_current_hunk, HUNKS - 2);
    let stepped = painted_hunk_headers(&h);
    assert_hunk_aimed_at(
        &stepped,
        &headers[HUNKS - 2],
        &[&headers[0]],
        &format!("{mode}, one step up from deep"),
    );

    // --- and a hunk near the top, from down there ---
    h.state_mut().ui.diff_current_hunk = 1;
    settle_after_aim(&mut h);
    let top = painted_hunk_headers(&h);
    assert_hunk_aimed_at(
        &top,
        &headers[1],
        &[&headers[HUNKS - 1]],
        &format!("{mode}, a hunk near the top from deep"),
    );
}

/// Hunk navigation aims a row the list never built, so a row INDEX has to
/// become a position correctly whether the list is at the top or hundreds of
/// rows down — in the rendering mode the developer is actually in.
#[test]
fn hunk_navigation_aims_the_hunk_it_names_side_by_side() {
    let (_tmp, repo) = repo_deep_diff(20, 15, 20);
    hunk_aim_sequence(&repo, true);
}

/// The same three aims, in the unified mode the app opens in.
#[test]
fn hunk_navigation_aims_the_hunk_it_names_unified() {
    let (_tmp, repo) = repo_deep_diff(20, 15, 20);
    hunk_aim_sequence(&repo, false);
}
