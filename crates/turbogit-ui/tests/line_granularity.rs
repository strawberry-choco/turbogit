//! Line-granularity staging (issue 19, screen 06) — headless harness tests.
//!
//! Drives the real `turbogit_ui::ui::render()` through `egui_kittest` over a
//! temporary repository with a [`RecordingExecutor`] injected at the
//! executor boundary (the `partial_dispatch.rs` pattern). Covers the issue's
//! harness layer: the File|Hunk|Line granularity toggle, Line-mode-only line
//! toggling, drag-selected character ranges, Enter/Esc, the selection
//! readout, and the status-bar granularity readout.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use test_support::harness::painted_text;
use test_support::{RecordedCall, RecordingExecutor};

use egui::FontFamily::Monospace;
use egui::FontId;
use egui::epaint::text::CharIndex;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use turbogit_app::state::AppState;
use turbogit_domain::model::VcsSettings;
use turbogit_engine::{ApplyDirection, GitExecutor, cli::CliExecutor};

// ---------------------------------------------------------------- helpers --

/// Run `git` in `repo`, asserting success, and return stdout.
fn git(repo: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git should be on PATH");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

struct Repo {
    path: PathBuf,
}

/// Create an initialized temp repository with one base commit on the default
/// branch and repo-local user config so commits work headlessly. The caller
/// keeps `parent` (a `TempDir`) alive for the duration of the test.
fn temp_repo(parent: &Path, name: &str) -> Repo {
    let path = parent.join(name);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("base.txt"), "base\n").unwrap();
    git(&path, &["add", "."]);
    git(&path, &["commit", "-q", "-m", "init"]);
    Repo { path }
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

/// The worktree text of the edited line — the addition row's accessibility
/// label (rows are labeled by their content text).
const ADD_ROW: &str = "    let calculated_value = compute(x);";
const DEL_ROW: &str = "    let x = 1;";

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
fn harness(state: AppState) -> Harness<'static, AppState> {
    let mut h = Harness::builder().with_max_steps(1024).build_ui_state(
        |ui, state| {
            state.drain_events();
            turbogit_ui::ui::render(ui, state);
        },
        state,
    );
    h.set_size(egui::vec2(1280.0, 800.0));
    test_support::harness::settle(&mut h);
    h
}

/// Poll `f` until it returns true or the deadline elapses (worker threads run
/// asynchronously, so completion is observed by polling).
fn wait_until<F: Fn() -> bool>(ms: u64, f: F) -> bool {
    let start = Instant::now();
    loop {
        if f() {
            return true;
        }
        if start.elapsed() >= Duration::from_millis(ms) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
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
    let rect = h.get_by_label(ADD_ROW).rect();
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
fn granularity_toggle_switches_selection_semantics() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "toggle");
    seed_single_line_edit(&repo);

    let (state, _recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    let mut h = harness(state);
    open_preview(&mut h);

    // Line is the default: clicking a changed row toggles it.
    assert_eq!(
        h.state().ui.diff_granularity,
        turbogit_app::state::Granularity::Line
    );
    h.get_by_label(DEL_ROW).click();
    h.run();
    assert!(
        h.state()
            .ui
            .line_selections
            .contains_key(Path::new("code.rs")),
        "in Line mode a row click toggles the line selection"
    );

    // Switching to Hunk granularity clears the line selection…
    h.get_by_label("Hunk").click();
    h.run();
    assert_eq!(
        h.state().ui.diff_granularity,
        turbogit_app::state::Granularity::Hunk
    );
    assert!(
        h.state().ui.line_selections.is_empty(),
        "the granularity switch must clear accumulated line selections"
    );
    // …and changed rows stop being interactive at all: no click target,
    // no toggle (the whole hunk is the unit now).
    assert!(
        h.query_by_label(ADD_ROW).is_none(),
        "in Hunk mode changed rows must not be click targets"
    );

    // Back to Line: rows return and toggling works again.
    h.get_by_label("Line").click();
    h.run();
    assert!(
        wait_until(15_000, || h.query_by_label(ADD_ROW).is_some()),
        "switching back to Line restores the row click targets"
    );
    h.get_by_label(ADD_ROW).click();
    h.run();
    assert!(
        h.state()
            .ui
            .line_selections
            .contains_key(Path::new("code.rs")),
        "switching back to Line restores the line-selection protocol"
    );
}

#[test]
fn selection_readout_paints_lines_chars_and_granularity() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "readout");
    seed_single_line_edit(&repo);

    let (state, _recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    let mut h = harness(state);
    open_preview(&mut h);

    // A line selection arms the readout.
    h.get_by_label(DEL_ROW).click();
    h.run();
    let text = painted_text(&h).join("\n");
    assert!(
        text.contains("1 line selection") && text.contains("granularity: line"),
        "readout must report the selected lines and granularity, got:\n{text}"
    );

    // An armed char range adds the char count and the Enter/Esc hints.
    h.state_mut().ui.char_selection = Some(turbogit_app::state::CharSelection {
        path: PathBuf::from("code.rs"),
        hunk: 0,
        ord: 1,
        start: 8,
        end: 24,
    });
    h.run();
    let text = painted_text(&h).join("\n");
    assert!(
        text.contains("2 line selections · 16 chars")
            && text.contains("↵ stage")
            && text.contains("Esc clear"),
        "readout must report chars selected and the stage/clear hints, got:\n{text}"
    );
}

#[test]
fn drag_inside_a_line_arms_a_character_range_and_enter_stages_it() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "drag-stage");
    seed_single_line_edit(&repo);

    let (state, recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    let mut h = harness(state);
    open_preview(&mut h);

    drag_calculated_value(&mut h);

    let sel = h.state().ui.char_selection.clone().unwrap_or_else(|| {
        panic!(
            "the drag must arm a char selection, painted={:?}",
            painted_text(&h)
        )
    });
    assert_eq!(sel.hunk, 0);
    assert_eq!(sel.ord, 1, "the addition is changed-line ordinal 1");
    assert_eq!(
        (sel.start, sel.end),
        (8, 24),
        "chars 8..24 = calculated_value"
    );

    // The readout reflects the live drag: 1 line selection · 16 chars.
    let text = painted_text(&h).join("\n");
    assert!(
        text.contains("1 line selection · 16 chars"),
        "readout must show the char range, got:\n{text}"
    );

    // Enter stages exactly the selected bytes through the engine.
    h.key_press(egui::Key::Enter);
    h.run();
    assert!(
        wait_until(15_000, || recorder.recorded().contains(
            &RecordedCall::ApplyPatch {
                direction: ApplyDirection::Forward
            }
        )),
        "Enter must dispatch the char-range stage, recorded={:?}",
        recorder.recorded()
    );
    assert!(
        wait_until(15_000, || {
            let staged = git(&repo.path, &["diff", "--cached"]);
            staged.contains("+calculated_value\n") && !staged.contains("compute")
        }),
        "the index must gain exactly the selected bytes:\n{}",
        git(&repo.path, &["diff", "--cached"])
    );
}

#[test]
fn escape_clears_the_armed_character_range() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "esc");
    seed_single_line_edit(&repo);

    let (state, recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    let mut h = harness(state);
    open_preview(&mut h);

    drag_calculated_value(&mut h);
    assert!(h.state().ui.char_selection.is_some());

    h.key_press(egui::Key::Escape);
    h.run();
    assert!(
        h.state().ui.char_selection.is_none(),
        "Esc must clear the armed char selection"
    );
    assert!(
        !painted_text(&h).join("\n").contains("↵ stage"),
        "the readout must disappear with the selection"
    );

    // And Enter afterwards must be inert: nothing reaches the engine.
    h.key_press(egui::Key::Enter);
    h.run();
    std::thread::sleep(Duration::from_millis(300));
    h.run();
    assert!(
        recorder.recorded().is_empty(),
        "Enter without a selection must not dispatch, recorded={:?}",
        recorder.recorded()
    );
}

#[test]
fn status_bar_shows_granularity_and_repos_in_scope() {
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
        text.contains("granularity: line"),
        "the status bar must show the active granularity, got:\n{text}"
    );
    assert!(
        text.contains("1 repo in scope"),
        "the status bar must show the repos in scope, got:\n{text}"
    );
}
