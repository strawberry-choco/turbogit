//! The Log view's COMMITS pane keeps a fixed width. Selecting a branch adds a
//! ref filter chip and a long branch name to its header, and neither may resize
//! the pane or paint into the neighbouring panes.

use egui::{Rect, Vec2};
use egui_kittest::{Harness, kittest::Queryable};
use std::path::PathBuf;
use tempfile::TempDir;
use test_support::git_seed::{commit, git};
use test_support::harness::{
    assert_not_painted, assert_painted, filled_rects, painted_galleys, settle,
};
use turbogit_app::events::{AppEvent, LogBatchMode};
use turbogit_app::log_filters::RootFilter;
use turbogit_app::state::{AppState, Tab};
use turbogit_domain::model::{LogOpts, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_engine_api::GitExecutor;
use turbogit_ui::theme::{Palette, configure_style, install_fonts};

const BOX: Vec2 = Vec2::new(1280.0, 800.0);
const LONG_BRANCH: &str = "feature/a-very-long-branch-name-that-keeps-going-and-going";
const FILTER_BUTTON_MAX_WIDTH: f32 = 168.0;

struct Seed {
    _tmp: TempDir,
    project: PathBuf,
}

fn seeded() -> Seed {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    std::fs::create_dir_all(&project).expect("project dir");
    let repo = project.join("alpha");
    git(
        &project,
        &["init", "-q", "-b", "main", repo.to_str().unwrap()],
    );
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    commit(&repo, "a.txt", "a\n", "initial commit on main");
    git(&repo, &["branch", LONG_BRANCH]);
    Seed { _tmp: tmp, project }
}

fn warm_log_and_refs(state: &mut AppState) {
    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };
    for root in state.multi.roots.clone() {
        let commits = engine.log(&root.path, &LogOpts::default()).expect("log");
        state
            .tx
            .send(AppEvent::LogLoaded {
                root: root.id.clone(),
                commits: Ok(commits),
                mode: LogBatchMode::Replace,
            })
            .expect("send LogLoaded");
        let deco = engine.ref_decorations(&root.path).expect("decorations");
        state
            .tx
            .send(AppEvent::RefsLoaded {
                root: root.id.clone(),
                deco: Ok(deco),
            })
            .expect("send RefsLoaded");
    }
    state.drain_events();
}

fn log_harness(seed: &Seed) -> Harness<'static, AppState> {
    log_harness_at(seed, BOX)
}

fn log_harness_at(seed: &Seed, size: Vec2) -> Harness<'static, AppState> {
    let mut state = AppState::new(seed.project.clone());
    warm_log_and_refs(&mut state);
    state.ui.tab = Tab::Log;
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, state| {
            configure_style(ui.ctx());
            if !fonts_installed {
                install_fonts(ui.ctx());
                fonts_installed = true;
            }
            turbogit_ui::ui::render(ui, state);
        },
        state,
    );
    harness.set_size(size);
    settle(&mut harness);
    harness
}

fn cards(harness: &Harness<'_, AppState>) -> Vec<Rect> {
    filled_rects(harness)
        .into_iter()
        .filter(|(r, c)| *c == Palette::CONTENT_BG && r.height() > 100.0)
        .map(|(r, _)| r)
        .collect()
}

fn commits_card(harness: &Harness<'_, AppState>) -> Rect {
    let all = cards(harness);
    let branches = *all
        .iter()
        .min_by(|a, b| a.left().total_cmp(&b.left()))
        .expect("branches pane card not painted");
    let right = *all
        .iter()
        .filter(|r| r.left() > branches.left())
        .max_by(|a, b| a.left().total_cmp(&b.left()))
        .expect("right column card not painted");
    *all.iter()
        .filter(|r| r.left() >= branches.right() && r.right() <= right.left())
        .max_by(|a, b| a.width().total_cmp(&b.width()))
        .expect("commits pane card not painted between the other panes")
}

/// A branch tree click writes both the ref scope and the root filter.
fn select_branch(harness: &mut Harness<'_, AppState>, name: &str) {
    let root = harness.state().multi.roots[0].id.clone();
    harness.state_mut().ui.log_ref_scope = Some(name.to_owned());
    harness.state_mut().ui.log_roots = RootFilter::Only(vec![root]);
    settle(harness);
}

#[test]
fn the_commits_pane_keeps_its_width_when_a_branch_is_selected() {
    let seed = seeded();
    let mut harness = log_harness(&seed);
    let before = commits_card(&harness);

    select_branch(&mut harness, "main");
    let short = commits_card(&harness);
    assert_eq!(
        before, short,
        "selecting a branch must not resize the commits pane"
    );

    select_branch(&mut harness, LONG_BRANCH);
    let long = commits_card(&harness);
    assert_eq!(
        before, long,
        "a long branch name must not resize the commits pane"
    );
}

#[test]
fn both_scope_chips_keep_the_pane_width_at_a_narrow_window() {
    let seed = seeded();
    let mut harness = log_harness_at(&seed, egui::vec2(560.0, 380.0));
    let before = commits_card(&harness).width();

    let root = harness.state().multi.roots[0].id.clone();
    harness.state_mut().ui.log_ref_scope = Some(LONG_BRANCH.to_owned());
    harness.state_mut().ui.log_roots = RootFilter::Only(vec![root.clone()]);
    harness.state_mut().ui.log_path_scope = Some((
        root,
        std::path::PathBuf::from("src/a/very/long/path/to/a/file.rs"),
    ));
    settle(&mut harness);

    assert_eq!(
        before,
        commits_card(&harness).width(),
        "both filter chips and a long path must not widen the commits pane"
    );
}

#[test]
fn a_branch_with_many_commits_does_not_widen_the_commits_pane() {
    let seed = seeded();
    let repo = seed.project.join("alpha");
    git(&repo, &["checkout", "-q", LONG_BRANCH]);
    for n in 0..60 {
        commit(
            &repo,
            &format!("f{n}.txt"),
            &format!("{n}\n"),
            &format!("commit number {n}"),
        );
    }
    let mut harness = log_harness(&seed);
    let before = commits_card(&harness);
    select_branch(&mut harness, LONG_BRANCH);
    assert_eq!(before.width(), commits_card(&harness).width());
}

#[test]
fn a_long_branch_name_is_elided_instead_of_widening_the_controls() {
    let seed = seeded();
    let mut harness = log_harness(&seed);
    select_branch(&mut harness, LONG_BRANCH);

    assert_painted(&harness, "Ref filter:");
    assert_not_painted(&harness, &format!("Ref filter: {LONG_BRANCH}"));
    assert_not_painted(&harness, &format!("Branch: {LONG_BRANCH}"));

    let button = harness.get_by_label("Branch").rect().width();
    assert!(
        button <= FILTER_BUTTON_MAX_WIDTH,
        "the branch filter button must elide a long name instead of growing with it, got {button:.1}px"
    );

    let elided = painted_galleys(&harness)
        .into_iter()
        .any(|g| g.text.starts_with("Branch: ") && g.text.contains('…'));
    assert!(elided, "the long branch name must be painted elided");
}
