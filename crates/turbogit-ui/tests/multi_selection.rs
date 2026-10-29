use egui_kittest::{Harness, kittest::Queryable};
use std::path::{Path, PathBuf};
use test_support::git_seed::repo_with_origin;
use test_support::harness::{
    assert_matrix_geometry, assert_not_painted, assert_painted, painted_text, settle,
    shell_harness_over_unstyled,
};
use turbogit_app::state::AppState;
use turbogit_domain::model::RootId;

fn temp_repo(parent: &Path, name: &str) -> PathBuf {
    repo_with_origin(parent, name)
}

fn two_repo_project(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
    let base = test_support::scratch_root(&format!("msm-{tag}"));
    let _ = std::fs::remove_dir_all(&base);
    let project = base.join("wsb");
    let frontend = project.join("frontend");
    let oss = project.join("oss");
    std::fs::create_dir_all(&frontend).unwrap();
    std::fs::create_dir_all(&oss).unwrap();
    let alpha = temp_repo(&frontend, "alpha");
    let lib = temp_repo(&oss, "lib");
    (project, alpha, lib)
}

fn four_repo_project(tag: &str) -> (PathBuf, Vec<PathBuf>) {
    let base = test_support::scratch_root(&format!("msf-{tag}"));
    let _ = std::fs::remove_dir_all(&base);
    let project = base.join("wsb");
    let frontend = project.join("frontend");
    let oss = project.join("oss");
    std::fs::create_dir_all(&frontend).unwrap();
    std::fs::create_dir_all(&oss).unwrap();
    let alpha = temp_repo(&frontend, "alpha");
    let ui = temp_repo(&frontend, "ui");
    let cli = temp_repo(&oss, "cli");
    let lib = temp_repo(&oss, "lib");
    (project, vec![alpha, ui, cli, lib])
}

#[track_caller]
fn assert_galley(harness: &Harness<'_, AppState>, text: &str) {
    let texts = painted_text(harness);
    let _ = &harness.state().ui.activity.entries;
    assert!(
        texts.iter().any(|t| t == text),
        "`{text}` was not painted as an exact galley; painted text:\n{texts:#?}"
    );
}

fn promoted_project(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
    let base = test_support::scratch_root(&format!("msp-{tag}"));
    let _ = std::fs::remove_dir_all(&base);
    let project = base.join("ws");
    let foo_dir = project.join("foo");
    std::fs::create_dir_all(&foo_dir).unwrap();
    let side = temp_repo(&project, "side");
    let bar = temp_repo(&foo_dir, "bar");
    (project, side, bar)
}

/// Headless harness driving the full app UI (mirrors `workspace_sidebar`).
///
/// Unstyled: the assertions are painted *strings*, and a different font would
/// re-measure the truncation and ellipsis behaviour they are about. The
/// per-frame worker drain is kept, so a toast can land before `wait_painted`.
///
/// `max_steps` is 1024, not kittest's default of 4, which `Harness::run`
/// panics past.
fn harness(state: AppState) -> Harness<'static, AppState> {
    shell_harness_over_unstyled(state, egui::vec2(1280.0, 800.0), 1024)
}

fn wait_painted(harness: &mut Harness<'_, AppState>, needle: &str) {
    for _ in 0..1000 {
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(10));
        if painted_text(harness).iter().any(|t| t.contains(needle)) {
            return;
        }
    }
    panic!("`{needle}` was not painted within 1000 frames");
}

#[test]
fn checking_a_repo_checkbox_selects_it_and_opens_the_summary() {
    let (project, alpha, lib) = two_repo_project("repo-check");
    let state = AppState::for_roots(&project, &[alpha, lib]);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Select repo alpha").click();
    settle(&mut h);

    assert_painted(&h, "Multi-repo selection");
    assert_painted(&h, "REPOS");
    assert_painted(&h, "AHEAD");
    assert_painted(&h, "BEHIND");
    assert_painted(&h, "DIRTY FILES");
    assert_painted(&h, "REPO");
    assert_painted(&h, "BRANCH");
    assert_painted(&h, "SYNC");
    assert_painted(&h, "DIRTY");
    assert_painted(&h, "LAST COMMIT");
    assert_painted(&h, "alpha");
    assert_galley(&h, "1 selected / 2");
    assert_painted(&h, "Fetch");
    assert_painted(&h, "Pull");
    assert_painted(&h, "Branch…");
    assert_painted(&h, "Clear");
    assert_painted(&h, "Pin as view");
}

#[test]
fn checking_a_group_selects_every_descendant_without_collapsing() {
    let (project, repos) = four_repo_project("group-check");
    let state = AppState::for_roots(&project, &repos);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Select group frontend").click();
    settle(&mut h);

    assert_galley(&h, "2 selected / 4");
    assert_painted(&h, "alpha");
    assert_matrix_geometry(
        &h,
        "LAST COMMIT",
        &[
            ("REPO", 0.0),
            ("BRANCH", 220.0),
            ("SYNC", 380.0),
            ("DIRTY", 470.0),
            ("LAST COMMIT", 545.0),
        ],
        &["alpha", "ui"],
        24.0,
        34.0,
    );

    h.get_by_label("Select group frontend").click();
    settle(&mut h);
    assert_not_painted(&h, "Multi-repo selection");
    assert_painted(&h, "alpha");
}

#[test]
fn the_selection_bar_clear_button_empties_the_selection() {
    let (project, alpha, lib) = two_repo_project("clear");
    let state = AppState::for_roots(&project, &[alpha, lib]);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Select repo alpha").click();
    settle(&mut h);
    assert_painted(&h, "Multi-repo selection");

    h.get_by_label("Clear selection").click();
    settle(&mut h);
    assert_not_painted(&h, "Multi-repo selection");
    assert_not_painted(&h, "1 selected / 2");
}

#[test]
fn clicking_a_summary_row_focuses_the_root_that_row_is_about() {
    let (project, repos) = four_repo_project("row-click");
    let state = AppState::for_roots(&project, &repos);
    let lib = repos[3].clone();
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Select repo lib").click();
    settle(&mut h);
    assert!(
        h.query_by_label("Focus lib").is_some(),
        "the summary row is reachable by its accessible label"
    );

    h.get_by_label("Focus lib").click();
    settle(&mut h);
    assert_eq!(
        h.state().selected_root,
        Some(RootId(lib.into())),
        "the click focuses the row's own root"
    );
}

#[test]
fn clicking_a_promoted_summary_row_focuses_the_repo_behind_its_path_label() {
    let (project, side, bar) = promoted_project("promoted-row");
    let state = AppState::for_roots(&project, &[side, bar.clone()]);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Select repo bar").click();
    settle(&mut h);
    assert!(
        h.query_by_label("Focus f/bar").is_some(),
        "the promoted summary row is reachable by its path label"
    );

    h.get_by_label("Focus f/bar").click();
    settle(&mut h);
    assert_eq!(
        h.state().selected_root,
        Some(RootId(bar.into())),
        "a path label resolves to the same root as the bare name"
    );
}

#[test]
fn fetch_and_pull_quick_actions_dispatch_over_the_selection() {
    let (project, repos) = four_repo_project("quick-ops");
    let state = AppState::for_roots(&project, &repos);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Select group frontend").click();
    settle(&mut h);
    h.get_by_label("Select group oss").click();
    settle(&mut h);

    h.get_by_label("Fetch selection").click();
    wait_painted(&mut h, "Fetch · nothing changed");

    h.get_by_label("Pull selection").click();
    wait_painted(&mut h, "Pull · 4 repos");
}

#[test]
fn the_branch_quick_action_opens_the_new_branch_dialog() {
    let (project, alpha, lib) = two_repo_project("branch-op");
    let state = AppState::for_roots(&project, &[alpha, lib]);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Select repo alpha").click();
    settle(&mut h);
    h.get_by_label("New branch for selection").click();
    settle(&mut h);

    assert_painted(&h, "New Branch");
}

#[test]
fn pin_as_view_saves_the_selection_and_a_pinned_view_restores_it() {
    let (project, repos) = four_repo_project("pin");
    let state = AppState::for_roots(&project, &repos);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Select group frontend").click();
    settle(&mut h);
    h.get_by_label("Select group oss").click();
    settle(&mut h);
    h.get_by_label("Pin as view").click();
    settle(&mut h);

    assert_painted(&h, "View 1");

    h.get_by_label("Clear selection").click();
    settle(&mut h);
    assert_not_painted(&h, "Multi-repo selection");

    h.get_by_label("Restore view View 1").click();
    settle(&mut h);
    assert_painted(&h, "Multi-repo selection");
    assert_galley(&h, "4 selected / 4");
}

#[test]
fn the_summary_table_shows_the_last_commit_subject_and_age() {
    let (project, alpha, lib) = two_repo_project("last-commit");
    let state = AppState::for_roots(&project, &[alpha, lib]);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Select repo alpha").click();
    settle(&mut h);

    assert_painted(&h, "init");
}
