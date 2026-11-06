use std::path::PathBuf;

use egui_kittest::kittest::Queryable as _;
use tempfile::TempDir;
use test_support::git_seed::{git, repo_with_one_commit};
use test_support::harness::{
    assert_painted, filled_circles, repo_shell_harness, settle_quiet, settle_reads,
    shell_harness_over,
};
use turbogit_app::log_filters::DateFilter;
use turbogit_app::state::{AppState, Tab};

fn log_harness() -> egui_kittest::Harness<'static, turbogit_app::state::AppState> {
    let (mut harness, _tmp) = repo_shell_harness();
    harness.state_mut().ui.tab = Tab::Log;
    settle_quiet(&mut harness);
    harness
}

#[test]
fn the_commits_pane_paints_the_four_intellij_filter_dropdowns() {
    let harness = log_harness();

    for label in ["Branch", "User", "Date", "Paths", "Clear"] {
        harness.get_by_label(label);
    }
    assert_painted(&harness, "Branch");
    assert_painted(&harness, "User");
    assert_painted(&harness, "Date");
    assert_painted(&harness, "Paths");
}

#[test]
fn a_date_preset_changes_the_button_and_asks_for_a_composite_read() {
    let mut harness = log_harness();
    harness.get_by_label("Date").click();
    settle_quiet(&mut harness);
    harness.get_by_label("Last 7 days").click();
    settle_quiet(&mut harness);

    assert!(matches!(
        harness.state().ui.log_date_filter,
        DateFilter::Last7Days { .. }
    ));
    let key = harness
        .state()
        .log_filter_key()
        .expect("a date preset must ask git for a filtered window");
    assert!(key.since.is_some());
    assert_eq!(key.until, None);
    assert_eq!(key.author, None);
    assert_painted(&harness, "Date: Last 7 days");
}

#[test]
fn a_user_filter_reads_only_matching_commits_from_the_real_repository() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().to_path_buf();
    let work: PathBuf = repo_with_one_commit(&project, "alpha");
    git(
        &work,
        &[
            "-c",
            "user.name=Alice",
            "-c",
            "user.email=alice@example.com",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "alice work",
        ],
    );

    let mut state = AppState::new(project);
    state.ui.tab = Tab::Log;
    let mut harness = shell_harness_over(state, egui::vec2(1024.0, 768.0));
    settle_reads(&mut harness);

    harness.state_mut().ui.log_author_filter = Some("Alice".to_owned());
    settle_reads(&mut harness);

    let key = harness
        .state()
        .log_filter_key()
        .expect("the author filter must build a composite key");
    assert_eq!(key.author.as_deref(), Some("Alice"));
    let root = harness.state().multi.roots[0].id.clone();
    let rows = harness
        .state()
        .caches
        .combined_log(&root, &key)
        .expect("the composite window must have been read");
    assert_eq!(rows.len(), 1, "only Alice's commit may survive");
    assert!(rows[0].message.contains("alice work"));
    assert_eq!(rows[0].author.name, "Alice");
    assert_painted(&harness, "User: Alice");
}

#[test]
fn two_legacy_filters_are_read_together_instead_of_one_silently_winning() {
    let mut harness = log_harness();
    let root = harness.state().multi.roots[0].id.clone();
    {
        let state = harness.state_mut();
        state.ui.log_ref_scope = Some((root, "main".to_owned()));
        state.ui.log_path_scope = Some("README.md".into());
    }

    let key = harness
        .state()
        .log_filter_key()
        .expect("branch and path must be read as one composite window");
    assert_eq!(key.branch.as_deref(), Some("main"));
    assert_eq!(key.path, Some(PathBuf::from("README.md")));
    assert_eq!(harness.state().log_filter_key().unwrap().author, None);
}

#[test]
fn the_paths_dropdown_carries_one_root_swatch_per_repository() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().to_path_buf();
    repo_with_one_commit(&project, "alpha");
    repo_with_one_commit(&project, "beta");

    let mut state = AppState::new(project);
    state.ui.tab = Tab::Log;
    let mut harness = shell_harness_over(state, egui::vec2(1280.0, 800.0));
    settle_reads(&mut harness);

    let names: Vec<String> = harness
        .state()
        .multi
        .roots
        .iter()
        .map(|r| r.id.name())
        .collect();
    assert_eq!(names.len(), 2, "both seeded repositories are discovered");
    let swatches = |h: &egui_kittest::Harness<'_, AppState>| {
        filled_circles(h)
            .into_iter()
            .filter(|(_, radius, _)| (*radius - 3.5).abs() < 0.01)
            .count()
    };
    let before = swatches(&harness);

    harness.get_by_label("Paths").click();
    settle_quiet(&mut harness);

    assert_eq!(
        swatches(&harness),
        before + names.len(),
        "the Roots group paints one colour swatch per repository"
    );
    assert_painted(&harness, "All roots");
    for name in &names {
        assert_painted(&harness, name);
    }
}

#[test]
fn clear_resets_every_filter_including_the_root_selection() {
    let mut harness = log_harness();
    {
        let state = harness.state_mut();
        state.ui.log_author_filter = Some("alice".to_owned());
        state.ui.log_date_filter = DateFilter::Last24Hours { since: 1 };
        state.ui.log_path_scope = Some("src/lib.rs".into());
        state.ui.log_root_filter = state.multi.roots.first().map(|r| r.id.clone());
    }
    settle_quiet(&mut harness);

    harness.get_by_label("Clear").click();
    settle_quiet(&mut harness);

    let state = harness.state();
    assert_eq!(state.ui.log_author_filter, None);
    assert!(state.ui.log_date_filter.is_all());
    assert_eq!(state.ui.log_path_scope, None);
    assert_eq!(state.ui.log_root_filter, None);
    assert_eq!(state.log_filter_key(), None);
}
