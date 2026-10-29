use std::path::PathBuf;

use egui_kittest::{Harness, kittest::NodeT as _, kittest::Queryable as _};
use tempfile::TempDir;
use test_support::git_seed::{GitEnv, git_with_env};
use test_support::harness::{assert_not_painted, galley_origin, painted_galleys, settle_quiet};
use turbogit_app::state::{AppState, Tab};

// No repository `user.*` here, so every call is pinned per call through the
// fixture crate's runner: a headless machine has no global identity to borrow.

fn checked_out_project(count: usize) -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = project.join("alpha");
    git_with_env(
        &project,
        &["-c", "init.defaultBranch=main", "init", "alpha"],
        &GitEnv::fixture(),
    );
    std::fs::write(alpha.join("README.md"), "x\n").unwrap();
    git_with_env(&alpha, &["add", "."], &GitEnv::fixture());
    git_with_env(&alpha, &["commit", "-m", "init"], &GitEnv::fixture());
    for i in 1..=count {
        git_with_env(&alpha, &["branch", &format!("r{i}")], &GitEnv::fixture());
    }
    for i in 1..=count {
        git_with_env(
            &alpha,
            &["checkout", "-q", &format!("r{i}")],
            &GitEnv::fixture(),
        );
    }
    (tmp, project)
}

fn branches_harness(project_dir: PathBuf) -> Harness<'static, AppState> {
    let state = AppState::new(project_dir);
    let mut fonts_installed = false;
    let mut harness = Harness::builder().with_step_dt(1.0 / 60.0).build_ui_state(
        move |ui, state| {
            state.drain_events();
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            turbogit_ui::ui::render(ui, state);
        },
        state,
    );
    harness.set_size(egui::vec2(1024.0, 768.0));
    harness
}

fn open_branches_tab(harness: &mut Harness<'_, AppState>) {
    harness.state_mut().ui.tab = Tab::Branches;
    settle_quiet(harness);
}

/// The fixture plus an opened Branches tab; the `TempDir` rides along because
/// dropping it deletes the repository every later read answers from.
fn open_checked_out(count: usize) -> (TempDir, Harness<'static, AppState>) {
    let (tmp, dir) = checked_out_project(count);
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);
    (tmp, harness)
}

fn row_rects(harness: &Harness<'_, AppState>, name: &str) -> Vec<egui::Rect> {
    harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .filter(|n| n.accesskit_node().label() == Some(name.to_string()))
        .map(|n| n.rect())
        .collect()
}

fn header_rect(harness: &Harness<'_, AppState>, label: &str) -> egui::Rect {
    harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| n.accesskit_node().label() == Some(label.to_string()))
        .unwrap_or_else(|| panic!("no {label} header node"))
        .rect()
}

#[test]
fn recent_group_caps_at_five_rows_above_local() {
    let (_tmp, harness) = open_checked_out(7);

    let recent_headers = painted_galleys(&harness)
        .into_iter()
        .filter(|g| g.text == "RECENT")
        .count();
    assert_eq!(recent_headers, 1, "exactly one Recent group header");

    let local = header_rect(&harness, "Local");
    let mut prev_top: Option<f32> = None;
    let mut prev_name = "";
    for (i, name) in ["r7", "r6", "r5", "r4", "r3"].iter().enumerate() {
        let rects = row_rects(&harness, name);
        assert_eq!(
            rects.len(),
            2,
            "{name} paints in Recent and in Local: {rects:?}"
        );
        let above = rects.iter().filter(|r| r.max.y <= local.min.y).count();
        assert_eq!(above, 1, "{name} (recent slot {i}) has one row above Local");
        let row = rects.iter().find(|r| r.max.y <= local.min.y).unwrap();
        if let Some(prev_top) = prev_top {
            assert!(
                prev_top < row.top(),
                "{prev_name} (checked out more recently) must sit above {name}"
            );
        }
        prev_top = Some(row.top());
        prev_name = name;
    }
    for name in ["r2", "r1"] {
        let rects = row_rects(&harness, name);
        assert!(
            rects.iter().all(|r| r.min.y >= local.max.y),
            "{name} was never among the five most recent; every copy must be \
             below the Local header: {rects:?}"
        );
    }
}

#[test]
fn recent_header_sits_above_local_by_at_least_one_row() {
    let (_tmp, harness) = open_checked_out(7);
    let recent = galley_origin(&harness, "RECENT").expect("Recent header painted");
    let local = galley_origin(&harness, "LOCAL").expect("Local header painted");
    assert!(recent.y < local.y, "Recent must sit above Local");
    assert!(
        local.y - recent.y >= 30.0,
        "at least one row height between the two headers: {recent:?} vs {local:?}"
    );
}

#[test]
fn under_cap_recent_paints_its_rows_and_no_overflow() {
    let (_tmp, harness) = open_checked_out(3);
    let local = header_rect(&harness, "Local");
    for name in ["r3", "r2", "r1"] {
        let rects = row_rects(&harness, name);
        assert_eq!(rects.len(), 2, "{name} paints in Recent and in Local");
        assert!(
            rects.iter().any(|r| r.max.y <= local.min.y),
            "{name} has its Recent row above the Local header: {rects:?}"
        );
    }
    for overflow in ["Show all", "show all", "older", "more…"] {
        assert_not_painted(&harness, overflow);
    }
}
