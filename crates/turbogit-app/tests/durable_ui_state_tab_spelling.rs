//! The active tab's spelling in `.turbogit/ui.ron`, in both directions.
//!
//! The on-disk vocabulary is one label per tab and nothing else: `tab: "Log"`,
//! never a variant name, so a file written by any build reads back the same and
//! a build that renames a variant does not respell everybody's file. The legacy
//! `"History"` tab and an unrecognised string both land on the Commit tab rather
//! than on an empty pane, which is what keeps an older `ui.ron` from stranding
//! the developer.

use std::path::Path;

use tempfile::TempDir;
use turbogit_app::persistence::load_ui_state;
use turbogit_app::state::{AppState, Tab};

/// Every tab the strip offers and the label the file spells it with. Written out
/// here rather than read back from the spelling table under test: the point of
/// the case is that the two agree.
const TAB_LABELS: [(Tab, &str); 5] = [
    (Tab::Commit, "Commit"),
    (Tab::Log, "Log"),
    (Tab::Branches, "Branches"),
    (Tab::Worktrees, "Worktrees"),
    (Tab::Submodules, "Submodules"),
];

/// A hand-written `ui.ron`. `tab`, `recent_repos` and `draft_message` carry no
/// `serde(default)` — a file missing any of them is unparseable — so a fixture
/// has to spell all three out, exactly as a build of the day did.
fn write_ui_ron(project: &Path, tab: &str) {
    let dir = project.join(".turbogit");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("ui.ron"),
        format!("(tab: \"{tab}\", recent_repos: [], draft_message: \"\")"),
    )
    .unwrap();
}

/// Every label the entry writes reads back as the same tab — asserted twice per
/// tab, once against the **file** (so a derived or variant-shaped spelling is
/// caught even if it round-trips through this build) and once against the
/// relaunched state.
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

/// `History` is not an unknown string by accident: the tab was **removed in
/// issue #19** (file history lives in Git Log's path-scoped view) and files
/// naming it are still on disk. The fallback to the Commit tab is a deliberate
/// courtesy to those files, not the same route as a typo — so it is pinned here
/// on its own, with the rest of the state still loading around it.
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

/// A `ui.ron` written by a build whose tab this one has never heard of takes the
/// same route: the Commit tab, with the other eight restored fields intact.
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
