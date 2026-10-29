use std::collections::HashSet;
use std::path::PathBuf;

use tempfile::TempDir;
use turbogit_app::bulk_history::{BulkRunRecord, RepoOutcome, RepoRecord};
use turbogit_app::persistence::BranchesViewOptions;
use turbogit_app::pinned_views::PinnedView;
use turbogit_app::smart_rules::SmartGroupRule;
use turbogit_app::state::{AppState, Tab};
use turbogit_domain::model::{DiffHighlighting, DiffPolicies, DiffWhitespace};
use turbogit_services::bulk_ops::BulkOp;

const DURABLE_KEYS: [(&str, &str); 10] = [
    ("tab", "\"Worktrees\""),
    ("recent_repos", "/w/durable-keys/recent-root"),
    ("draft_message", "durable-keys draft body"),
    ("smart_group_rules", "durable-keys rule"),
    ("pinned_views", "durable-keys view"),
    ("bulk_history", "777777777"),
    ("recent_custom_commands", "git log --durable-keys"),
    ("sidebar_collapsed", "durable-keys/collapsed"),
    ("branches_view", "group_by_directory: false"),
    ("diff_policies", "IgnoreWhitespaceAndEmptyLines"),
];

fn state_holding_every_durable_value(project: &std::path::Path) -> AppState {
    let mut state = AppState::for_roots(project, &[]);
    state.ui.tab = Tab::Worktrees;
    state.ui.recent_repos = vec![PathBuf::from("/w/durable-keys/recent-root")];
    state.ui.commit_message = "durable-keys draft body".to_string();
    state.ui.smart_group_rules = vec![SmartGroupRule {
        label: "durable-keys rule".to_string(),
        branch_pattern: Some("durable-keys/*".to_string()),
        min_ahead: Some(2),
        ..SmartGroupRule::default()
    }];
    state.ui.pinned_views = vec![PinnedView {
        name: "durable-keys view".to_string(),
        repos: vec![PathBuf::from("/w/durable-keys/recent-root")],
    }];
    state.ui.bulk_history = vec![BulkRunRecord {
        id: 777_777_777,
        at: 1_791_234_000_001,
        op: BulkOp::FetchAll,
        branch: String::new(),
        repos: vec![RepoRecord {
            root: PathBuf::from("/w/durable-keys/recent-root"),
            name: "recent-root".to_string(),
            prior_branch: None,
            created: false,
            outcome: RepoOutcome::Skipped {
                reason: "durable-keys offline".to_string(),
            },
        }],
        rolled_back: false,
    }];
    state.ui.recent_custom_commands = vec!["git log --durable-keys".to_string()];
    state.ui.sidebar_collapsed = HashSet::from(["durable-keys/collapsed".to_string()]);
    state.ui.branches_view = BranchesViewOptions {
        group_by_directory: false,
        group_by_repository: false,
        show_recent: false,
        show_tags: true,
    };
    state.ui.diff_policies = DiffPolicies {
        whitespace: DiffWhitespace::IgnoreWhitespaceAndEmptyLines,
        highlighting: DiffHighlighting::Characters,
    };
    state
}

#[test]
fn every_durable_key_reaches_the_file_with_its_value_on_it() {
    let project = TempDir::new().unwrap();
    state_holding_every_durable_value(project.path()).save_durable_ui();

    let written =
        std::fs::read_to_string(project.path().join(".turbogit/ui.ron")).expect("ui.ron exists");

    let missing: Vec<&str> = DURABLE_KEYS
        .iter()
        .map(|(key, _)| *key)
        .filter(|key| !written.contains(&format!("{key}:")))
        .collect();
    assert!(
        missing.is_empty(),
        "ui.ron names none of these durable keys: {missing:?}\n{written}"
    );

    let defaulted: Vec<(&str, &str)> = DURABLE_KEYS
        .iter()
        .filter(|(_, marker)| !written.contains(marker))
        .copied()
        .collect();
    assert!(
        defaulted.is_empty(),
        "these durable keys reached the file without the value they were saved \
         with (key, expected value): {defaulted:?}\n{written}"
    );
}
