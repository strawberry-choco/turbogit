//! Line-granularity staging (issue 19, screen 06) — headless harness tests.
//!
//! Drives the real `turbogit_ui::ui::render()` through `egui_kittest` over a
//! temporary repository with a [`RecordingExecutor`] injected at the
//! executor boundary (the `partial_dispatch.rs` pattern). Covers the harness
//! layer: row clicks staging their line in git, drag-released character
//! ranges, and the status bar's scope chip.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use test_support::git_seed::{git, repo_with_one_commit};
use test_support::harness::{painted_text, shell_harness_over_unstyled};
use test_support::{RecordedCall, RecordingExecutor, wait_until};

use egui::FontFamily::Monospace;
use egui::FontId;
use egui::epaint::text::CharIndex;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use turbogit_app::state::AppState;
use turbogit_domain::model::VcsSettings;
use turbogit_engine::{ApplyDirection, GitExecutor, cli::CliExecutor};

// ---------------------------------------------------------------- helpers --

struct Repo {
    path: PathBuf,
}

/// Create an initialized temp repository with one base commit on the default
/// branch and repo-local user config so commits work headlessly. The caller
/// keeps `parent` (a `TempDir`) alive for the duration of the test.
///
/// A thin wrapper over `git_seed::repo_with_one_commit`; nothing here reads the
/// seeded commit — every assertion is about `code.rs` and the diff rows from it.
fn temp_repo(parent: &Path, name: &str) -> Repo {
    Repo {
        path: repo_with_one_commit(parent, name),
    }
}

/// Commit `code.rs`, then diverge the worktree by editing one long line —
/// a single-hunk diff with one deletion and one addition to select in.
fn seed_single_line_edit(repo: &Repo) {
    std::fs::write(
        repo.path.join("code.rs"),
        "fn main() {\n    let x = 1;\n}\n",
    )
    .unwrap();
    git(&repo.path, &["add", "code.rs"]);
    git(&repo.path, &["commit", "-q", "-m", "code"]);
    std::fs::write(
        repo.path.join("code.rs"),
        "fn main() {\n    let calculated_value = compute(x);\n}\n",
    )
    .unwrap();
}

/// The worktree text of the edited line. A diff row's accessibility label is this
/// text followed by the state the per-line oracle answers for it, so the two are
/// one string only through [`row`].
const ADD_ROW: &str = "    let calculated_value = compute(x);";
const DEL_ROW: &str = "    let x = 1;";

/// A changed row's accessibility label, spelled the way the pane spells it.
///
/// Every fixture in this suite starts with the whole edit in the worktree, so
/// `false` is a row's resting state and the `true` calls are the flip this suite
/// is asserting, read off the accessibility tree rather than off an internal.
fn row(text: &str, staged: bool) -> String {
    format!("{text} ({})", if staged { "staged" } else { "unstaged" })
}

/// [`AppState`] with an injected recording engine — the swap happens AFTER
/// synchronous registration, so setup reads never reach the recorder.
fn app_state_with_recorder(
    project_dir: &Path,
    roots: &[PathBuf],
) -> (AppState, Arc<RecordingExecutor>) {
    let recorder = Arc::new(RecordingExecutor::new(Arc::new(CliExecutor {
        settings: VcsSettings::default(),
    })));
    let state = AppState::for_roots(project_dir, roots)
        .with_executor(recorder.clone() as Arc<dyn GitExecutor>);
    (state, recorder)
}

/// Headless harness driving the full app UI with event draining per frame.
///
/// Unstyled, with `max_steps` as the third argument: this suite drives its diff
/// interactions through `h.run()` (15 call sites) and `Harness::run` PANICS past
/// kittest's default budget of 4. The drag/character-range geometry below is
/// measured against egui's default metrics, not the production token set.
fn harness(state: AppState) -> Harness<'static, AppState> {
    let mut h = shell_harness_over_unstyled(state, egui::vec2(1280.0, 800.0), 1024);
    test_support::harness::settle(&mut h);
    h
}

/// Open the Commit tool window's preview of `code.rs` and wait for the diff.
fn open_preview(h: &mut Harness<'_, AppState>) {
    h.get_by_label("code.rs").click();
    h.run();
    assert!(
        wait_until(15_000, || h.query_by_label("Stage hunk 1").is_some()),
        "diff preview should load"
    );
    h.run();
}

/// X offset of `col` inside the row text, measured with the exact galley the
/// diff rows paint with (12px monospace). Single-line layout → one row.
fn char_x(h: &Harness<'_, AppState>, col: usize) -> f32 {
    h.ctx.fonts_mut(|f| {
        f.layout_no_wrap(
            ADD_ROW.to_owned(),
            FontId::new(12.0, Monospace),
            egui::Color32::WHITE,
        )
        .rows
        .first()
        .expect("layout of one line has one row")
        .x_offset(CharIndex(col))
    })
}

/// Left edge of the row text inside a unified row band: sign column + line
/// number gutter + 12px inset (`TEXT_X` in the diff painter).
const TEXT_X: f32 = 16.0 + 40.0 + 12.0;

/// Drag from char 8 to char 24 of the addition row — the
/// `calculated_value` slice (four leading spaces precede it).
fn drag_calculated_value(h: &mut Harness<'_, AppState>) {
    let rect = h.get_by_label(&row(ADD_ROW, false)).rect();
    let origin = rect.left() + TEXT_X;
    let y = rect.center().y;
    h.drag_at(egui::pos2(origin + char_x(h, 8), y));
    h.run();
    h.hover_at(egui::pos2(origin + char_x(h, 24), y));
    h.run();
    h.drop_at(egui::pos2(origin + char_x(h, 24), y));
    h.run();
}

// ------------------------------------------------------------------ tests --

#[test]
fn row_click_stages_the_line_in_git() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "toggle");
    seed_single_line_edit(&repo);

    let (state, _recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    let mut h = harness(state);
    open_preview(&mut h);

    // A changed-row click IS the stage: git answers for itself, first the
    // deletion row, then the addition row of the same edit.
    h.get_by_label(&row(DEL_ROW, false)).click();
    h.run();
    assert!(
        wait_until(15_000, || git(
            &repo.path,
            &["diff", "--cached", "--", "code.rs"]
        )
        .contains("-    let x = 1;")),
        "the clicked deletion must land in the index, got:\n{}",
        git(&repo.path, &["diff", "--cached", "--", "code.rs"])
    );
    // The next click comes off the same preview: the combined read is
    // byte-stable under a stage, so the row that just went into the index is
    // still on screen beside its pair, and the addition row keeps its own
    // address. Nothing stale is dispatched, because the row's gutter numbers are
    // translated against the freshly re-derived retained sections.
    //
    // Both halves of the pair stay addressable, and each one's accessibility label
    // now names its own state: the clicked row flipped to staged, its partner did
    // not. A pane that moved the label with the paint would report a toggle here
    // that git had not made, and a pane that left it alone would hide one it had.
    assert!(
        wait_until(15_000, || {
            h.run();
            h.query_by_label(&row(DEL_ROW, true)).is_some()
                && h.query_by_label(&row(ADD_ROW, false)).is_some()
        }),
        "both rows of the edit stay in the preview after one of them is staged, and \
         only the clicked one says `staged`"
    );
    h.get_by_label(&row(ADD_ROW, false)).click();
    h.run();
    assert!(
        wait_until(15_000, || git(
            &repo.path,
            &["diff", "--cached", "--", "code.rs"]
        )
        .contains("+    let calculated_value = compute(x);")),
        "the clicked addition must land in the index, got:\n{}",
        git(&repo.path, &["diff", "--cached", "--", "code.rs"])
    );
}

#[test]
fn drag_release_stages_the_character_range_and_nothing_else() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "drag-stage");
    seed_single_line_edit(&repo);

    let (state, recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    let mut h = harness(state);
    open_preview(&mut h);

    drag_calculated_value(&mut h);

    // One `Sense::click_and_drag` response reports a finished drag and a
    // click, and both routes now stage, so exactly one forward apply may
    // reach the engine: the drag released, and the row click was suppressed.
    std::thread::sleep(Duration::from_millis(300));
    h.run();
    let forwards = recorder
        .recorded()
        .iter()
        .filter(|call| {
            matches!(
                call,
                RecordedCall::ApplyPatch {
                    direction: ApplyDirection::Forward
                }
            )
        })
        .count();
    assert_eq!(
        forwards,
        1,
        "the release must stage exactly once, recorded={:?}",
        recorder.recorded()
    );
    assert!(
        h.state().ui.char_drag.is_none(),
        "the draft dies with the release"
    );
    assert!(
        wait_until(15_000, || {
            let staged = git(&repo.path, &["diff", "--cached"]);
            staged.contains("+calculated_value\n") && !staged.contains("compute")
        }),
        "the index must gain exactly the dragged bytes, not the whole line:\n{}",
        git(&repo.path, &["diff", "--cached"])
    );
}

#[test]
fn status_bar_shows_repos_in_scope() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "status-bar");
    seed_single_line_edit(&repo);

    let (mut state, _recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    state.ui.show_status_bar = true;
    let mut h = harness(state);
    open_preview(&mut h);

    let text = painted_text(&h).join("\n");
    assert!(
        text.contains("1 repo in scope"),
        "the status bar must show the repos in scope, got:\n{text}"
    );
}
