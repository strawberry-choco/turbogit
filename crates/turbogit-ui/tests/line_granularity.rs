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

struct Repo {
    path: PathBuf,
}

fn temp_repo(parent: &Path, name: &str) -> Repo {
    Repo {
        path: repo_with_one_commit(parent, name),
    }
}

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

const ADD_ROW: &str = "    let calculated_value = compute(x);";
const DEL_ROW: &str = "    let x = 1;";

fn row(text: &str, staged: bool) -> String {
    format!("{text} ({})", if staged { "staged" } else { "unstaged" })
}

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

fn harness(state: AppState) -> Harness<'static, AppState> {
    let mut h = shell_harness_over_unstyled(state, egui::vec2(1280.0, 800.0), 1024);
    test_support::harness::settle(&mut h);
    h
}

fn open_preview(h: &mut Harness<'_, AppState>) {
    h.get_by_label("code.rs").click();
    h.run();
    assert!(
        wait_until(15_000, || h.query_by_label("Stage hunk 1").is_some()),
        "diff preview should load"
    );
    h.run();
}

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

const TEXT_X: f32 = 16.0 + 40.0 + 12.0;

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

#[test]
fn row_click_stages_the_line_in_git() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "toggle");
    seed_single_line_edit(&repo);

    let (state, _recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    let mut h = harness(state);
    open_preview(&mut h);

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
