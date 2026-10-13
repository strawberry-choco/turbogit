//! The whole durable UI set, walked through the production launch path: every
//! durable field is set to a distinct non-default value, the save entry runs,
//! the project is relaunched over the same directory, and what comes back is
//! read field by field.
//!
//! Nine of the ten rows come back. The tenth — the commit draft — does not, and
//! [`the_commit_draft_is_written_and_never_restored`] says so by name rather
//! than leaving the field out of a list: writing a key and reading it back are
//! two contracts, and the one place they disagree is asserted, not omitted.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use tempfile::TempDir;
use turbogit_app::bulk_history::{BulkRunRecord, RepoOutcome, RepoRecord};
use turbogit_app::persistence::BranchesViewOptions;
use turbogit_app::pinned_views::PinnedView;
use turbogit_app::smart_rules::SmartGroupRule;
use turbogit_app::state::{AppState, Tab};
use turbogit_domain::model::{DiffHighlighting, DiffPolicies, DiffWhitespace};
use turbogit_services::bulk_ops::BulkOp;

/// One distinct value per durable row, none of them its default and none of them
/// reachable from another row — a row wired to the wrong source cannot pass by
/// coincidence.
const TAB: Tab = Tab::Worktrees;
const RECENT_REPO: &str = "/w/durable-ui/frontend";
const DRAFT: &str = "durable-ui: a draft no other row could have produced";
const RULE_LABEL: &str = "durable-ui rules";
const VIEW_NAME: &str = "durable-ui view";
const BULK_RUN_ID: u64 = 4_242_424;
const COMMAND: &str = "git log --durable-ui";
const COLLAPSED_KEY: &str = "durable-ui/frontend/src";
const BRANCHES_VIEW: BranchesViewOptions = BranchesViewOptions {
    group_by_directory: false,
    group_by_repository: true,
    show_recent: false,
    show_tags: true,
};
const POLICIES: DiffPolicies = DiffPolicies {
    whitespace: DiffWhitespace::TrimWhitespace,
    highlighting: DiffHighlighting::Characters,
};

fn rule() -> SmartGroupRule {
    SmartGroupRule {
        label: RULE_LABEL.to_string(),
        branch_pattern: Some("durable-*".to_string()),
        min_ahead: Some(3),
        ..SmartGroupRule::default()
    }
}

fn view() -> PinnedView {
    PinnedView {
        name: VIEW_NAME.to_string(),
        repos: vec![PathBuf::from(RECENT_REPO)],
    }
}

fn run_record() -> BulkRunRecord {
    BulkRunRecord {
        id: BULK_RUN_ID,
        at: 1_791_234_567_890,
        op: BulkOp::FetchAll,
        branch: String::new(),
        repos: vec![RepoRecord {
            root: PathBuf::from(RECENT_REPO),
            name: "frontend".to_string(),
            prior_branch: None,
            created: false,
            outcome: RepoOutcome::Done,
        }],
        rolled_back: false,
    }
}

/// The headless harness (`AppState::for_roots` over a temp project) holding all
/// ten durable values at once, so every field is exercised in the same session.
fn state_holding_the_durable_set(project: &Path) -> AppState {
    let mut state = AppState::for_roots(project, &[]);
    state.ui.tab = TAB;
    state.ui.recent_repos = vec![PathBuf::from(RECENT_REPO)];
    state.ui.commit_message = DRAFT.to_string();
    state.ui.smart_group_rules = vec![rule()];
    state.ui.pinned_views = vec![view()];
    state.ui.bulk_history = vec![run_record()];
    state.ui.recent_custom_commands = vec![COMMAND.to_string()];
    state.ui.sidebar_collapsed = HashSet::from([COLLAPSED_KEY.to_string()]);
    state.ui.branches_view = BRANCHES_VIEW;
    state.ui.diff_policies = POLICIES;
    state
}

#[test]
fn nine_of_the_ten_durable_fields_come_back_through_the_launch_path() {
    let project = TempDir::new().unwrap();
    let recents_cfg = TempDir::new().unwrap();

    let state = state_holding_the_durable_set(project.path());
    state.save_durable_ui();

    let reloaded = AppState::launch_in(
        Some(project.path().to_path_buf()),
        Some(recents_cfg.path().to_path_buf()),
    );

    assert_eq!(
        reloaded.ui.tab, TAB,
        "tab: the strip lands where it was left"
    );
    assert_eq!(
        reloaded.ui.recent_repos,
        vec![PathBuf::from(RECENT_REPO)],
        "recent_repos: the recently opened roots survive"
    );
    assert_eq!(
        reloaded.ui.smart_group_rules,
        vec![rule()],
        "smart_group_rules: a user-defined group is still a group"
    );
    assert_eq!(
        reloaded.ui.pinned_views,
        vec![view()],
        "pinned_views: a pinned selection is still selectable"
    );
    assert_eq!(
        reloaded.ui.bulk_history,
        vec![run_record()],
        "bulk_history: a recorded run is still listed"
    );
    assert_eq!(
        reloaded.ui.recent_custom_commands,
        vec![COMMAND.to_string()],
        "recent_custom_commands: a recent command is still re-selectable"
    );
    assert_eq!(
        reloaded.ui.sidebar_collapsed,
        HashSet::from([COLLAPSED_KEY.to_string()]),
        "sidebar_collapsed: a collapsed folder is still collapsed"
    );
    assert_eq!(
        reloaded.ui.branches_view, BRANCHES_VIEW,
        "branches_view: the four gear toggles keep their positions"
    );
    assert_eq!(
        reloaded.ui.diff_policies, POLICIES,
        "diff_policies: the viewer's two policies keep their rung"
    );
}

/// The tenth row, asserted rather than skipped.
///
/// The draft reaches `.turbogit/ui.ron` from **every** trigger, not only from a
/// commit: collapsing a sidebar folder, applying settings and settling a bulk
/// run each write the whole projection with the draft in it. No route reads it
/// back, so a draft typed before a restart does not survive one — the asymmetry
/// the durable declaration states in the `draft_message` row, and the reason
/// this assertion exists instead of an omission.
///
/// Restoring the draft is user-perceivable and a product ruling, so this test
/// is written to FAIL the moment somebody implements the restore: that failure
/// is the place the ruling lands.
#[test]
fn the_commit_draft_is_written_and_never_restored() {
    let project = TempDir::new().unwrap();
    let recents_cfg = TempDir::new().unwrap();

    let state = state_holding_the_durable_set(project.path());
    state.save_durable_ui();

    // The write half, first: the draft is on disk, so this is not a dead row.
    let written =
        std::fs::read_to_string(project.path().join(".turbogit/ui.ron")).expect("ui.ron exists");
    assert!(
        written.contains(DRAFT),
        "the draft reaches the file: {written}"
    );

    let reloaded = AppState::launch_in(
        Some(project.path().to_path_buf()),
        Some(recents_cfg.path().to_path_buf()),
    );

    assert!(
        reloaded.ui.commit_message != DRAFT,
        "the durable-UI declaration's tenth row is write-only: the commit draft \
         is captured from UiState::commit_message and written to ui.ron by all \
         fourteen triggers, but no row restores it. If a draft now survives a \
         restart, the draft-restore asymmetry declared in \
         durable_ui_state!'s draft_message row is gone — update that row and \
         this assertion together."
    );
}
