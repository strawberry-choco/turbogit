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
//!
//! Ticket 12 (the scope axes and the statistics that sit with what they
//! describe) is asserted **by geometry**, never by which helper painted what —
//! a test that fails because a helper was renamed says nothing about the design:
//!
//! - the comparison axis and the granularity axis each read as ONE grouped
//!   control: contiguous segment rects inside one outer boundary each, with the
//!   two boundaries of different kinds so the groups read as different groups
//! - a hidden granularity axis leaves no empty group behind
//! - the add/remove counts and the whitespace toggle paint on the file row, and
//!   the toolbar paints neither
//! - the file row's band height and its label origin are the row shell's
//! - the synthesised rename header is one muted band across the row
//!
//! **Ticket 12's "no diff token changed" assertion now lives in
//! `tests/design_tokens.rs`**, with the rest of the token contract: it pins six
//! diff values by hex, measures both text-on-band pairs against AA, and asserts
//! the pane paints them. It was here first because this file was another agent's
//! partition while that ticket was in flight; the token contract is where a
//! palette belongs, and this suite keeps the diff's *behaviour*.

use std::path::{Path, PathBuf};
use std::process::Command;

use egui::{Color32, Pos2, Rect, Shape, Vec2};
use egui_kittest::{Harness, kittest::NodeT, kittest::Queryable};
use test_support::harness::{painted_galleys, stroked_rects};
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

/// Two commits, so a commit-to-commit target (`HEAD~1` ↔ `HEAD`) has real
/// content to render. Working-tree-only controls (the granularity axis) have
/// nothing to say about such a comparison.
fn repo_two_commits() -> (tempfile::TempDir, PathBuf) {
    let (tmp, repo) = init_repo();
    let base: Vec<String> = (1..=10).map(|i| format!("l{i:02}")).collect();
    write_file(&repo, "hist.txt", &format!("{}\n", base.join("\n")));
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "c1"]);
    let mut edited = base;
    edited[2] = "SECOND".to_string();
    write_file(&repo, "hist.txt", &format!("{}\n", edited.join("\n")));
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "c2"]);
    (tmp, repo)
}

/// A commit that `git` reports as a **rename with content edits** — similarity
/// below 100%, so the pane keeps its scroller and the synthesised rename header
/// renders as a row of it rather than taking the pure-rename shortcut.
///
/// Committed, and therefore read through the commit-to-commit target: rename
/// detection needs BOTH sides of the change in the diff, so a path-limited
/// working-tree diff of the renamed file can only ever report it as a new file.
fn repo_renamed_with_edit() -> (tempfile::TempDir, PathBuf) {
    let (tmp, repo) = init_repo();
    let body: Vec<String> = (1..=12).map(|i| format!("line{i:02}")).collect();
    write_file(&repo, "old.txt", &format!("{}\n", body.join("\n")));
    write_file(&repo, "keep.txt", "untouched\n");
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "c1"]);
    run_git(&repo, &["mv", "old.txt", "new.txt"]);
    let mut moved = body;
    moved[3] = "line04 edited".to_string();
    write_file(&repo, "new.txt", &format!("{}\n", moved.join("\n")));
    run_git(&repo, &["add", "-A"]);
    run_git(&repo, &["commit", "-m", "c2"]);
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
    // Tall enough that the shell frame (issue #03: center tabs + status bar —
    // nothing claims the top edge) leaves the preview column room for the
    // tallest comparison diff plus the message editor below it.
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

// =============================================================================
// Ticket 12 — the two axes of scope, and the statistics that sit with what they
// describe.
//
// Everything below is asserted from PAINTED GEOMETRY, never from which helper
// painted what. A grouped control is a shape (contiguous segments, one outer
// boundary), and a shape survives a rename; a name does not.
// =============================================================================

/// Half a pixel of slack: egui rounds UI rects, so exact float equality is the
/// wrong question to ask about two rects that are meant to coincide.
const EPS: f32 = 0.5;

/// The rect an axis's segments occupy together — what "one group" means: the
/// segments touch, so their union is a single run with no gaps in it.
#[track_caller]
fn assert_contiguous(what: &str, segments: &[Rect]) -> Rect {
    for pair in segments.windows(2) {
        assert!(
            (pair[0].right() - pair[1].left()).abs() <= EPS,
            "{what}: its segments are not edge to edge — {:?} then {:?} leave a gap",
            pair[0],
            pair[1]
        );
    }
    let top = segments
        .iter()
        .map(|r| r.top())
        .fold(f32::INFINITY, f32::min);
    let bottom = segments
        .iter()
        .map(|r| r.bottom())
        .fold(f32::NEG_INFINITY, f32::max);
    Rect::from_min_max(
        Pos2::new(
            segments
                .iter()
                .map(|r| r.left())
                .fold(f32::INFINITY, f32::min),
            top,
        ),
        Pos2::new(
            segments
                .iter()
                .map(|r| r.right())
                .fold(f32::NEG_INFINITY, f32::max),
            bottom,
        ),
    )
}

/// One axis, read straight off the painted output: where its segments are, and
/// which kind of boundary is drawn around them.
#[derive(Debug)]
struct Axis {
    segments: Rect,
    /// The frame: `true` when the group is bounded by a laid-down fill, `false`
    /// when it is bounded by a drawn hairline. Two groups with the same answer
    /// here are two groups that do not read as different groups.
    bounded_by_fill: bool,
    frame: Rect,
}

/// Measure one axis of the toolbar: its three segments' rects (through their
/// accessibility labels, which are the labels the controls already carried) and
/// the ONE boundary painted around them.
///
/// The frame is the painted shape at the segments' horizontal extent that is
/// TALLER than the segments — a group's boundary is never coincident with a
/// segment's own fill, so exactly one shape can answer.
fn read_axis(harness: &Harness<'_, AppState>, what: &str, labels: [&str; 3]) -> Axis {
    let segments: Vec<Rect> = labels
        .iter()
        .map(|l| harness.get_by_label(l).rect())
        .collect();
    let union = assert_contiguous(what, &segments);
    let at_extent = |r: Rect| {
        (r.left() - union.left()).abs() <= EPS && (r.right() - union.right()).abs() <= EPS
    };

    let fills: Vec<Rect> = filled_rects(harness)
        .into_iter()
        .filter(|(_, c)| *c == Palette::SURFACE_2)
        .map(|(r, _)| r)
        .filter(|r| at_extent(*r) && r.height() > union.height() + EPS)
        .collect();
    let strokes: Vec<Rect> = stroked_rects(harness)
        .into_iter()
        .filter(|(_, c, _)| *c == Palette::LINE)
        .map(|(r, _, _)| r)
        .filter(|r| at_extent(*r) && r.height() > union.height() + EPS)
        .collect();

    // One outer boundary: exactly one shape, in exactly one of the two kinds.
    match (fills.len(), strokes.len()) {
        (1, 0) => Axis {
            segments: union,
            bounded_by_fill: true,
            frame: fills[0],
        },
        (0, 1) => Axis {
            segments: union,
            bounded_by_fill: false,
            frame: strokes[0],
        },
        (n_f, n_s) => panic!(
            "{what}: an axis must be bounded by exactly ONE outer boundary; \
             painted {n_f} raised fills and {n_s} hairlines around {union:?}"
        ),
    }
}

/// The toolbar's open span — between the view-mode control and the hunk
/// navigator — which is precisely where the axes of scope live. Boundaries
/// found here belong to an axis; the view-mode control's own track is found and
/// excluded, so it cannot be counted as one.
fn axis_span(harness: &Harness<'_, AppState>) -> Rect {
    let mode = harness.get_by_label("Unified").rect();
    // The view-mode control's own track, by containment: the span starts where
    // that control ends.
    let track = filled_rects(harness)
        .into_iter()
        .find(|(r, c)| *c == Palette::SURFACE_2 && r.contains(mode.center()))
        .map(|(r, _)| r)
        .expect("the view-mode control paints its own track");
    let nav = harness.get_by_label("Previous hunk").rect();
    Rect::from_min_max(
        Pos2::new(track.right(), track.top().min(nav.top())),
        Pos2::new(nav.left(), track.bottom().max(nav.bottom())),
    )
}

/// The axis **group frames** in the toolbar's open span, as `(rect, is_fill)`.
///
/// Two filters, and both matter. The horizontal one is strict: the view-mode
/// control's own track ends exactly where the span begins, and an inclusive test
/// would count that track as a third group. The height one is what makes this
/// "how many GROUPS are here" rather than "how many shapes are here": a group's
/// frame stands taller than the toolbar row, while the segments it groups are
/// row-height. An empty frame is a frame with nothing in it, so this count
/// answers the question the missing-axis case asks.
fn axis_groups(harness: &Harness<'_, AppState>) -> Vec<(Rect, bool)> {
    let span = axis_span(harness);
    let row_h = filled_rects(harness)
        .into_iter()
        .find(|(r, c)| {
            *c == Palette::SURFACE_2 && r.contains(Pos2::new(span.left(), span.center().y))
        })
        .map(|(r, _)| r.height())
        .expect("the view-mode control's track");
    let inside = |r: Rect| {
        r.left() > span.left() + EPS && r.right() < span.right() - EPS && r.height() > row_h + EPS
    };
    let mut out: Vec<(Rect, bool)> = filled_rects(harness)
        .into_iter()
        .filter(|(_, c)| *c == Palette::SURFACE_2)
        .map(|(r, _)| r)
        .filter(|r| inside(*r))
        .map(|r| (r, true))
        .collect();
    out.extend(
        stroked_rects(harness)
            .into_iter()
            .filter(|(_, c, _)| *c == Palette::LINE)
            .map(|(r, _, _)| r)
            .filter(|r| inside(*r))
            .map(|r| (r, false)),
    );
    out
}

/// Harness rendering ONLY the diff pane, over a commit-to-commit target — the
/// comparison the working-tree-only controls have nothing to say about.
fn commit_diff_harness(repo: &Path) -> Harness<'static, AppState> {
    let state = AppState::new(repo.to_path_buf());
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, state| {
            configure_style(ui.ctx());
            if !fonts_installed {
                install_fonts(ui.ctx());
                fonts_installed = true;
            }
            state.drain_events();
            let (left, right) = ("HEAD~1".to_owned(), "HEAD".to_owned());
            turbogit_ui::ui::diff::render_diff(ui, state, &Some(left), &Some(right), &None);
        },
        state,
    );
    harness.set_size(egui::vec2(1024.0, 900.0));
    harness
}

/// Contract: the two axes of scope each read as ONE grouped control, and the
/// two groups are told apart by their boundary rather than by their labels.
///
/// Proven by geometry, in the four parts the claim actually has:
/// 1. each axis's three segments are edge to edge (no gaps — the "loose
///    buttons" reading is gone),
/// 2. each axis has exactly one outer boundary,
/// 3. the two boundaries are of different kinds, so the two groups do not
///    read as two copies of the same control,
/// 4. the order is mode → comparison → granularity → navigator, so the
///    navigator does not sit between the two axes.
#[test]
fn the_two_scope_axes_each_read_as_one_grouped_control() {
    let (_tmp, repo) = repo_mixed();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo.join("file.txt"));

    let comparison = read_axis(&h, "the comparison axis", ["Repo", "Staged", "Local"]);
    let granularity = read_axis(&h, "the granularity axis", ["File", "Hunk", "Line"]);

    // 3: two neighbours are different kinds of group. Same silhouette, different
    // boundary — a filled pad beside a hairline outline.
    assert_ne!(
        comparison.bounded_by_fill, granularity.bounded_by_fill,
        "the two axes of scope must not be bounded the same way, or they read as \
         two copies of one control: {comparison:?} vs {granularity:?}"
    );
    assert!(
        (comparison.frame.height() - granularity.frame.height()).abs() <= EPS,
        "the two groups are the same size so the difference between them is the \
         boundary, not the size: {comparison:?} vs {granularity:?}"
    );
    assert!(
        comparison.segments.right() <= granularity.segments.left() + EPS,
        "the two axes must not overlap: {comparison:?} vs {granularity:?}"
    );

    // 4: the navigator follows the pair rather than splitting it.
    let nav = h.get_by_label("Previous hunk").rect();
    assert!(
        granularity.segments.right() <= nav.left() + EPS,
        "the hunk navigator must come after both axes, not between them: \
         {granularity:?} then {nav:?}"
    );
}

/// Contract: the granularity axis is a working-tree-only axis, and when it is
/// hidden it leaves nothing behind. A grouping that assumed both axes always
/// exist would paint an empty frame here, and this is the assertion that says
/// it does not.
#[test]
fn a_hidden_granularity_axis_leaves_no_empty_group_behind() {
    // --- the working tree: BOTH axes are present, as two groups -----------
    let (_tmp, repo) = repo_mixed();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo.join("file.txt"));
    assert!(
        h.query_by_label("Line").is_some() && h.query_by_label("Repo").is_some(),
        "a working-tree comparison shows both axes"
    );
    let working_tree_groups = axis_groups(&h);
    assert_eq!(
        working_tree_groups.len(),
        2,
        "the working-tree toolbar frames exactly two groups: the comparison pad \
         (a fill) and the granularity outline (a hairline) — got {working_tree_groups:?}"
    );

    // --- a commit pair: NEITHER axis applies, so nothing is framed ---------
    let (_tmp, commits) = repo_two_commits();
    let mut c = commit_diff_harness(&commits);
    settle(&mut c);
    assert_painted(&c, "SECOND");
    for label in ["Repo", "Staged", "Local", "File", "Hunk", "Line"] {
        assert!(
            c.query_by_label(label).is_none(),
            "a commit-to-commit comparison has no revision axis and no index to \
             stage into, so `{label}` must not be offered"
        );
    }
    assert_eq!(
        axis_groups(&c),
        Vec::new(),
        "the hidden axes must leave no empty group behind: no frame, no outline, \
         nothing but the view-mode control and the hunk navigator"
    );
}

/// The file row is a row of the row shell's height and the row shell's text
/// origin, and gaining the statistics and the toggle did not change either.
/// Its fill is the one row-fill decision, so a file row is not a second
/// selection treatment.
fn hovered_file_row(harness: &mut Harness<'_, AppState>) -> (Rect, Pos2) {
    let galley = painted_galleys(harness)
        .into_iter()
        .find(|g| g.text.starts_with("diff --git "))
        .expect("the file row's own line is painted");
    // Inside the band, clear of the trailing zone and of the rounded corners.
    let point = Pos2::new(galley.pos.x + 1.0, galley.rect.center().y);
    harness.hover_at(point);
    harness.run();
    // Found by the row shell's OWN hover fill — not by a colour picked here —
    // so "the file row's fill is the one row-fill decision" is what makes this
    // find succeed at all.
    let hover_fill = turbogit_ui::ui::components::row_fill(
        turbogit_ui::ui::components::RowState::from_flags(false, true),
    );
    let band = filled_rects(harness)
        .into_iter()
        .find(|(r, c)| *c == hover_fill && r.contains(point))
        .map(|(r, _)| r)
        .expect("the hovered file row paints the row shell's hover fill");
    (band, point)
}

/// Contract: the statistics sit with the thing they describe. The counts and the
/// comparison toggle paint **on the file row**, and the diff pane's toolbar
/// paints neither of them any more.
///
/// Scoped by position, never by string: a `+1` is a perfectly ordinary thing for
/// another surface on screen to be painting (the commit window's preview card
/// header is still a legitimate place for it), so the question is not "how many
/// `+1`s are on screen" but "where are they".
#[test]
fn the_file_row_carries_the_counts_and_the_whitespace_toggle() {
    let (_tmp, repo) = repo_mixed();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo.join("file.txt"));

    let (band, _) = hovered_file_row(&mut h);
    let toolbar = axis_span(&h);

    // The counts: this file's one addition and one deletion, in the stats
    // accents, inside the file row's band — and not in the toolbar.
    for (text, ink) in [
        ("+1", Palette::DIFF_ADD_ACCENT),
        ("\u{2212}1", Palette::DIFF_DEL_ACCENT),
    ] {
        let galleys: Vec<_> = painted_galleys(&h)
            .into_iter()
            .filter(|g| g.text == text)
            .collect();
        let on_row: Vec<_> = galleys
            .iter()
            .filter(|g| band.contains(g.rect.center()))
            .collect();
        assert_eq!(
            on_row.len(),
            1,
            "`{text}` is the file row's own statistic and must paint on the file \
             row exactly once: {on_row:?} of {galleys:?}"
        );
        assert_eq!(on_row[0].color, ink, "`{text}` keeps its stats accent");
        assert!(
            galleys.iter().all(|g| !toolbar.contains(g.rect.center())),
            "the diff toolbar no longer paints the statistics: `{text}` at {:?}",
            galleys
                .iter()
                .find(|g| toolbar.contains(g.rect.center()))
                .map(|g| g.rect)
        );
    }

    // The toggle: one control, on the file row, nowhere in the toolbar.
    let toggles: Vec<Rect> = h
        .get_all_by_label("Ignore whitespace")
        .map(|n| n.rect())
        .collect();
    assert_eq!(
        toggles.len(),
        1,
        "the toolbar no longer paints the toggle; exactly one control remains"
    );
    assert!(
        band.contains(toggles[0].center()),
        "the comparison toggle belongs to the file row: {:?} is outside {band:?}",
        toggles[0]
    );
    assert!(
        !toolbar.intersects(toggles[0]),
        "the toggle moved off the toolbar onto the file row: {toggles:?}"
    );
}

/// Contract: the file row's height and leading padding are the ROW SHELL's,
/// after the new occupants are added. The band it paints under the pointer is
/// the same band every other diff row paints — the same height, the same full
/// width — and its label starts at the same text origin as the changed rows'
/// text. The occupant that would be hand-placed instead (a taller row, or a
/// second text origin) breaks one of these two.
#[test]
fn the_file_row_takes_its_geometry_from_the_row_shell() {
    let (_tmp, repo) = repo_mixed();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo.join("file.txt"));

    let (band, _) = hovered_file_row(&mut h);
    let add_pos = galley_origin(&h, "GAMMA").expect("added line painted");
    let add_band = filled_rects(&h)
        .into_iter()
        .find(|(r, c)| *c == Palette::DIFF_ADD_BG && r.contains(add_pos))
        .map(|(r, _)| r)
        .expect("the added line's band");

    assert!(
        (band.height() - add_band.height()).abs() <= EPS,
        "the file row's band is {} tall, every other row's is {}: the height \
         belongs to the row shell, not to the file row",
        band.height(),
        add_band.height()
    );
    assert!(
        band.left() <= add_band.left() + EPS && band.right() >= add_band.right() - EPS,
        "the file row spans the row: {band:?} does not cover {add_band:?}"
    );
    let file_label = painted_galleys(&h)
        .into_iter()
        .find(|g| g.text.starts_with("diff --git "))
        .expect("the file row's own line is painted");
    assert!(
        (file_label.pos.x - add_pos.x).abs() <= EPS,
        "the file row's label starts at x={} but every other row's text starts at \
         x={}: the leading padding belongs to the row shell",
        file_label.pos.x,
        add_pos.x
    );
}

/// Contract: the synthesised rename header reads as one muted band spanning the
/// row, and its text is painted exactly once inside it.
#[test]
fn the_rename_header_reads_as_one_muted_band() {
    let (_tmp, repo) = repo_renamed_with_edit();
    let mut h = commit_diff_harness(&repo);
    settle(&mut h);

    let headers: Vec<_> = painted_galleys(&h)
        .into_iter()
        .filter(|g| g.text.starts_with("Renamed from "))
        .collect();
    assert_eq!(
        headers.len(),
        1,
        "the rename header is one band and paints its text once: {headers:?}"
    );
    let header = &headers[0];

    let bands: Vec<Rect> = filled_rects(&h)
        .into_iter()
        .filter(|(_, c)| *c == Palette::SECTION_BG)
        .map(|(r, _)| r)
        .filter(|r| r.contains(header.pos))
        .collect();
    assert_eq!(
        bands.len(),
        1,
        "one muted band, and the rename text is inside it: {bands:?}"
    );

    // ...and the band is the ROW's band, not a band shrunk around the text: the
    // same left edge, the same width, the same height as any other diff row.
    let edit_pos = galley_origin(&h, "line04 edited").expect("the edited line painted");
    let edit_band = filled_rects(&h)
        .into_iter()
        .find(|(r, c)| *c == Palette::DIFF_ADD_BG && r.contains(edit_pos))
        .map(|(r, _)| r)
        .expect("the added line's band");
    let band = bands[0];
    assert!(
        (band.left() - edit_band.left()).abs() <= EPS
            && (band.width() - edit_band.width()).abs() <= EPS
            && (band.height() - edit_band.height()).abs() <= EPS,
        "the rename band must span the whole row the way every other diff row \
         does: {band:?} against a diff row's {edit_band:?}"
    );
}

/// Contract: the file row gained two occupants, and neither of them landed on
/// top of a row's own click targets. This is the failure mode the placement
/// risks — a trailing zone hung at the wrong coordinate silently eats a
/// changed row's click — so it is asserted against every band the rows paint.
#[test]
fn the_file_rows_occupants_do_not_overlap_the_rows_own_click_targets() {
    let (_tmp, repo) = repo_mixed();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo.join("file.txt"));

    let (band, _) = hovered_file_row(&mut h);
    let toggle = h.get_by_label("Ignore whitespace").rect();
    let changed: Vec<Rect> = filled_rects(&h)
        .into_iter()
        .filter(|(_, c)| *c == Palette::DIFF_ADD_BG || *c == Palette::DIFF_DEL_BG)
        .map(|(r, _)| r)
        .collect();
    assert_eq!(
        changed.len(),
        2,
        "one addition and one deletion are on screen"
    );
    for row in &changed {
        assert!(
            !row.intersects(toggle),
            "the file row's toggle sits on a changed row's band {row:?} and would \
             swallow its click: {toggle:?}"
        );
    }
    for row in &changed {
        assert!(
            !row.intersects(band),
            "the file row's own band overlaps a changed row's band {row:?}: {band:?}"
        );
    }

    // The row's own protocol still answers: a changed line is still a click
    // target that arms its selection.
    h.get_by_label("GAMMA").click();
    h.run();
    let previewed = h
        .state()
        .ui
        .preview_change
        .clone()
        .expect("a file is previewed");
    assert!(
        h.state().ui.line_selections.contains_key(&previewed),
        "a changed line must still toggle its selection: {:?}",
        h.state().ui.line_selections
    );
}
