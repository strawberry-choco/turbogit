use std::path::Path;

use tempfile::TempDir;
use turbogit_app::persistence::load_ui_state;
use turbogit_app::state::{AppState, Tab};

const TAB_LABELS: [(Tab, &str); 5] = [
    (Tab::Commit, "Commit"),
    (Tab::Log, "Log"),
    (Tab::Branches, "Branches"),
    (Tab::Worktrees, "Worktrees"),
    (Tab::Submodules, "Submodules"),
];

fn write_ui_ron(project: &Path, tab: &str) {
    let dir = project.join(".turbogit");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("ui.ron"),
        format!("(tab: \"{tab}\", recent_repos: [], draft_message: \"\")"),
    )
    .unwrap();
}

#[test]
fn every_current_tab_label_round_trips_through_the_save_and_the_launch() {
    for (tab, label) in TAB_LABELS {
        let project = TempDir::new().unwrap();
        let recents_cfg = TempDir::new().unwrap();

        let mut state = AppState::for_roots(project.path(), &[]);
        state.ui.tab = tab;
        state.save_durable_ui();

        let written = std::fs::read_to_string(project.path().join(".turbogit/ui.ron"))
            .expect("ui.ron exists");
        assert!(
            written.contains(&format!("tab: \"{label}\"")),
            "{label}: written as the quoted label, not a variant name: {written}"
        );

        assert_eq!(
            load_ui_state(project.path()).tab,
            tab,
            "{label}: reads back as the same tab"
        );

        let reloaded = AppState::launch_in(
            Some(project.path().to_path_buf()),
            Some(recents_cfg.path().to_path_buf()),
        );
        assert_eq!(
            reloaded.ui.tab, tab,
            "{label}: and the shell lands where it was left"
        );
    }
}

#[test]
fn a_ui_ron_naming_the_removed_history_tab_lands_on_the_commit_tab() {
    let project = TempDir::new().unwrap();
    let recents_cfg = TempDir::new().unwrap();
    write_ui_ron(project.path(), "History");

    assert_eq!(
        load_ui_state(project.path()).tab,
        Tab::Commit,
        "the legacy \"History\" spelling falls back to the Commit tab"
    );

    let reloaded = AppState::launch_in(
        Some(project.path().to_path_buf()),
        Some(recents_cfg.path().to_path_buf()),
    );
    assert_eq!(
        reloaded.ui.tab,
        Tab::Commit,
        "and the rest of the file loads rather than the launch falling back to a whole default state"
    );
    assert!(!reloaded.ui.welcome_visible, "the project still opens");
}

#[test]
fn an_unrecognised_tab_string_lands_on_the_commit_tab() {
    let project = TempDir::new().unwrap();
    let recents_cfg = TempDir::new().unwrap();
    write_ui_ron(project.path(), "Compare");

    assert_eq!(
        load_ui_state(project.path()).tab,
        Tab::Commit,
        "a tab string nobody knows lands on the Commit tab"
    );

    let reloaded = AppState::launch_in(
        Some(project.path().to_path_buf()),
        Some(recents_cfg.path().to_path_buf()),
    );
    assert_eq!(
        reloaded.ui.tab,
        Tab::Commit,
        "and the launch is not the error path"
    );
    assert!(
        reloaded.ui.recent_repos.is_empty() && reloaded.ui.smart_group_rules.is_empty(),
        "with the rest of the state restored around it"
    );
}
