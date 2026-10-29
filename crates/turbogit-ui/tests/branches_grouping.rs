use std::path::PathBuf;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use tempfile::TempDir;
use test_support::git_seed::{GitEnv, git_with_env};
use test_support::harness::{assert_not_painted, galley_origin, painted_galleys, settle_quiet};
use turbogit_app::state::{AppState, Tab};

// No repository `user.*` here, so every call is pinned per call through the
// fixture crate's runner: a headless machine has no global identity to borrow.

fn grouped_project() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = project.join("alpha");
    git_with_env(
        &project,
        &["init", "-q", "-b", "main", alpha.to_str().unwrap()],
        &GitEnv::fixture(),
    );
    std::fs::write(alpha.join("README.md"), "x\n").unwrap();
    git_with_env(&alpha, &["add", "."], &GitEnv::fixture());
    git_with_env(&alpha, &["commit", "-q", "-m", "init"], &GitEnv::fixture());
    for b in ["feature/a", "feature/b", "plain"] {
        git_with_env(&alpha, &["branch", b], &GitEnv::fixture());
    }
    git_with_env(&alpha, &["tag", "v1.0"], &GitEnv::fixture());
    git_with_env(&alpha, &["tag", "v2.0"], &GitEnv::fixture());
    (tmp, project)
}

fn two_repo_project() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    for name in ["alpha", "beta"] {
        let dir = project.join(name);
        git_with_env(
            &project,
            &["init", "-q", "-b", "main", dir.to_str().unwrap()],
            &GitEnv::fixture(),
        );
        std::fs::write(dir.join("README.md"), "x\n").unwrap();
        git_with_env(&dir, &["add", "."], &GitEnv::fixture());
        git_with_env(&dir, &["commit", "-q", "-m", "init"], &GitEnv::fixture());
        git_with_env(
            &dir,
            &["branch", &format!("only-{name}")] as &[_],
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

fn open_gear(harness: &mut Harness<'_, AppState>) {
    harness.get_by_label("settings").click();
    settle_quiet(harness);
}

fn toggle(harness: &mut Harness<'_, AppState>, label: &str) {
    open_gear(harness);
    harness.get_by_label(label).click();
    settle_quiet(harness);
}

fn painted_count(harness: &Harness<'_, AppState>, text: &str) -> usize {
    painted_galleys(harness)
        .into_iter()
        .filter(|g| g.text == text)
        .count()
}

fn has_painted(harness: &Harness<'_, AppState>, text: &str) -> bool {
    painted_count(harness, text) > 0
}

#[test]
fn grouped_directory_paints_the_header_and_strips_the_leaf_prefix() {
    let (_tmp, project) = grouped_project();
    let mut harness = branches_harness(project);
    open_branches_tab(&mut harness);

    assert!(
        has_painted(&harness, "feature/"),
        "the directory header paints its segment with its separator"
    );
    assert!(has_painted(&harness, "a"), "the leaf is stripped");
    assert!(has_painted(&harness, "b"), "the leaf is stripped");
    for full in ["feature/a", "feature/b"] {
        assert_not_painted(&harness, full);
    }
}

#[test]
fn ungrouped_directory_paints_full_names_and_no_directory_header() {
    let (_tmp, project) = grouped_project();
    let mut harness = branches_harness(project);
    open_branches_tab(&mut harness);
    toggle(&mut harness, "Group by Directory");

    assert!(has_painted(&harness, "feature/a"));
    assert!(has_painted(&harness, "feature/b"));
    assert_eq!(painted_count(&harness, "feature/"), 0);
    for name in ["plain", "main"] {
        assert!(has_painted(&harness, name), "{name} is still listed");
    }
}

#[test]
fn directory_toggle_changes_the_rows_between_the_header_and_the_last_row() {
    let (_tmp, project) = grouped_project();
    let mut harness = branches_harness(project);
    open_branches_tab(&mut harness);

    let header_before = galley_origin(&harness, "LOCAL").expect("LOCAL header");
    let first_before = galley_origin(&harness, "main").expect("first row");
    let last_before = galley_origin(&harness, "b").expect("last row when grouped");

    toggle(&mut harness, "Group by Directory");

    let header_after = galley_origin(&harness, "LOCAL").expect("LOCAL header");
    let first_after = galley_origin(&harness, "main").expect("first row");
    let last_after = galley_origin(&harness, "feature/b").expect("last row when flat");

    assert_eq!(
        header_before, header_after,
        "the group header is not part of what grouping moves"
    );
    assert_eq!(
        first_before, first_after,
        "the first row keeps its origin, whatever the level below it does"
    );
    assert_ne!(
        last_before.y, last_after.y,
        "a directory header fewer lifts the last row — the interior did change"
    );
    assert!(
        last_after.y < last_before.y,
        "flattening shortens the list: {last_after:?} vs {last_before:?}"
    );
}

#[test]
fn show_tags_off_paints_no_tag_row_and_no_tags_header() {
    let (_tmp, project) = grouped_project();
    let mut harness = branches_harness(project);
    open_branches_tab(&mut harness);

    harness.get_by_label("Tags").click();
    settle_quiet(&mut harness);
    assert!(has_painted(&harness, "TAGS"), "the group is showing");
    assert!(has_painted(&harness, "v1.0"));
    let local_before = galley_origin(&harness, "LOCAL").expect("LOCAL header");

    toggle(&mut harness, "Show Tags");

    let galleys = painted_galleys(&harness);
    let in_list = |t: &str| {
        galleys
            .iter()
            .any(|g| g.text == t && g.rect.min.x > 220.0 && g.rect.min.x < 900.0)
    };
    assert!(!in_list("TAGS"), "no Tags header in the list");
    assert!(!in_list("v1.0"), "no tag row in the list");
    assert!(!in_list("v2.0"), "no tag row in the list");
    assert_eq!(
        galley_origin(&harness, "LOCAL"),
        Some(local_before),
        "the Local group is untouched by hiding Tags"
    );
}

#[test]
fn the_gear_exposes_the_four_documented_toggles() {
    let (_tmp, project) = grouped_project();
    let mut harness = branches_harness(project);
    open_branches_tab(&mut harness);
    open_gear(&mut harness);

    for label in [
        "Group by Directory",
        "Group by Repository",
        "Show Recent Branches",
        "Show Tags",
    ] {
        assert!(
            has_painted(&harness, label),
            "{label} is one of the four: painted {:?}",
            painted_galleys(&harness)
                .into_iter()
                .map(|g| g.text)
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn show_recent_off_hides_the_group_the_other_ticket_built() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = project.join("alpha");
    git_with_env(
        &project,
        &["init", "-q", "-b", "main", alpha.to_str().unwrap()],
        &GitEnv::fixture(),
    );
    std::fs::write(alpha.join("README.md"), "x\n").unwrap();
    git_with_env(&alpha, &["add", "."], &GitEnv::fixture());
    git_with_env(&alpha, &["commit", "-q", "-m", "init"], &GitEnv::fixture());
    git_with_env(&alpha, &["branch", "work"], &GitEnv::fixture());
    git_with_env(&alpha, &["checkout", "-q", "work"], &GitEnv::fixture());

    let mut harness = branches_harness(project);
    open_branches_tab(&mut harness);
    assert!(has_painted(&harness, "RECENT"), "the group is on");

    toggle(&mut harness, "Show Recent Branches");
    assert_eq!(painted_count(&harness, "RECENT"), 0);
    assert!(has_painted(&harness, "work"));
}

#[test]
fn repository_grouping_off_removes_the_level_without_losing_a_branch() {
    let (_tmp, project) = two_repo_project();
    let mut harness = branches_harness(project);
    open_branches_tab(&mut harness);

    assert_eq!(
        painted_count(&harness, "LOCAL"),
        2,
        "each repository is its own grouping level"
    );
    for name in ["only-alpha", "only-beta"] {
        assert!(has_painted(&harness, name), "{name} is listed");
    }

    toggle(&mut harness, "Group by Repository");

    assert_eq!(
        painted_count(&harness, "LOCAL"),
        1,
        "one Local group over both repositories"
    );
    for name in ["only-alpha", "only-beta"] {
        assert!(has_painted(&harness, name), "{name} is still listed");
    }
    assert!(
        has_painted(&harness, "main · alpha") || has_painted(&harness, "main · beta"),
        "an un-grouped list still names the repository a row came from"
    );
}

#[test]
fn folding_every_repository_leaves_the_ungrouped_list_whole() {
    let (_tmp, project) = two_repo_project();
    let mut harness = branches_harness(project);
    open_branches_tab(&mut harness);

    let roots: Vec<turbogit_domain::model::RootId> = harness
        .state()
        .multi
        .roots
        .iter()
        .map(|r| r.id.clone())
        .collect();
    assert_eq!(roots.len(), 2, "the fixture is two repositories");
    for root in &roots {
        harness
            .state_mut()
            .ui
            .branches_tree
            .collapsed_repos
            .insert(root.clone());
    }
    settle_quiet(&mut harness);
    for name in ["only-alpha", "only-beta"] {
        assert!(
            !has_painted(&harness, name),
            "grouped, a folded repository paints no `{name}` row"
        );
    }

    toggle(&mut harness, "Group by Repository");

    for name in ["only-alpha", "only-beta"] {
        assert!(
            has_painted(&harness, name),
            "{name} paints with grouping off, fold or no fold"
        );
    }
    assert_eq!(
        harness.state().ui.branches_tree.collapsed_repos.len(),
        2,
        "the level that ignores the fold leaves it in place"
    );
}

#[test]
fn the_gear_reopens_after_a_toggle_closed_it() {
    let (_tmp, project) = grouped_project();
    let mut harness = branches_harness(project);
    open_branches_tab(&mut harness);

    toggle(&mut harness, "Group by Directory");
    open_gear(&mut harness);

    assert!(
        has_painted(&harness, "Group by Directory"),
        "the gear reopens after its toggle list closed on a change"
    );
}

#[test]
fn the_gear_reopens_after_escape_closed_it() {
    let (_tmp, project) = grouped_project();
    let mut harness = branches_harness(project);
    open_branches_tab(&mut harness);

    open_gear(&mut harness);
    assert!(has_painted(&harness, "Group by Directory"));
    harness.key_press(egui::Key::Escape);
    settle_quiet(&mut harness);
    assert_eq!(painted_count(&harness, "Group by Directory"), 0);

    open_gear(&mut harness);
    assert!(
        has_painted(&harness, "Group by Directory"),
        "the gear reopens after Escape closed its toggle list"
    );
}

#[test]
fn a_toggled_grouping_comes_back_from_the_project_on_the_next_launch() {
    let (tmp, project) = grouped_project();
    {
        let mut harness = branches_harness(project.clone());
        open_branches_tab(&mut harness);
        toggle(&mut harness, "Group by Directory");
        assert!(has_painted(&harness, "feature/a"));
        harness.state().save_durable_ui();
    }
    let persisted = std::fs::read_to_string(project.join(".turbogit").join("ui.ron"))
        .expect("ui.ron holds the toggles");
    assert!(
        persisted.contains("group_by_directory: false"),
        "the flip is written down, not just painted: {persisted}"
    );

    let mut again = branches_harness(project);
    open_branches_tab(&mut again);
    assert!(
        has_painted(&again, "feature/a"),
        "a fresh launch reads the preference back"
    );
    assert_eq!(painted_count(&again, "feature/"), 0);
    drop(tmp);
}
