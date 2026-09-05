//! Issue #07 — user-defined smart group rules: persistence with the
//! workspace. Rules live in `.turbogit/ui.ron` next to the other UI-only
//! state and must survive a round trip; files written before rules
//! existed (no `smart_group_rules` key) load with none, never failing.
use tempfile::TempDir;
use turbogit_app::persistence::{UiPersist, load_ui_state, save_ui_state};
use turbogit_app::smart_rules::SmartGroupRule;
use turbogit_app::state::AppState;

fn release_rule() -> SmartGroupRule {
    SmartGroupRule {
        label: "release branches".into(),
        branch_pattern: Some("release/*".into()),
        min_ahead: Some(1),
        ..SmartGroupRule::default()
    }
}

#[test]
fn rules_round_trip_through_the_workspace_ui_state() {
    let project = TempDir::new().unwrap();
    let ui = UiPersist {
        smart_group_rules: vec![release_rule()],
        ..Default::default()
    };

    save_ui_state(project.path(), &ui).unwrap();
    let loaded = load_ui_state(project.path());

    assert_eq!(loaded.smart_group_rules, vec![release_rule()]);
    // The pre-existing fields ride along unchanged.
    assert_eq!(loaded.tab, "");
}

#[test]
fn a_ui_ron_written_before_rules_existed_loads_with_no_rules() {
    let project = TempDir::new().unwrap();
    std::fs::create_dir_all(project.path().join(".turbogit")).unwrap();
    std::fs::write(
        project.path().join(".turbogit/ui.ron"),
        "(tab: \"Commit\", recent_repos: [], draft_message: \"hi\")",
    )
    .unwrap();

    let loaded = load_ui_state(project.path());

    assert!(loaded.smart_group_rules.is_empty());
    assert_eq!(loaded.draft_message, "hi");
    assert_eq!(loaded.tab, "Commit");
}

#[test]
fn rules_survive_a_restart_through_the_launch_path() {
    let project = TempDir::new().unwrap();
    let recents_cfg = TempDir::new().unwrap();

    let mut state = AppState::for_roots(project.path(), &[]);
    state.ui.smart_group_rules = vec![release_rule()];
    state.persist_ui();

    // Relaunching the same project restores the rules into live UI state.
    let reloaded = AppState::launch_in(
        Some(project.path().to_path_buf()),
        Some(recents_cfg.path().to_path_buf()),
    );
    assert_eq!(reloaded.ui.smart_group_rules, vec![release_rule()]);
}
