//! The branch kit's words, ported from the egui `branch_component_kit`
//! suite at the data layer: pluralization, middle truncation, sync badges,
//! stale age labels, and the repository-state vocabulary.

use turbogit_ui::screens::branches::{middle_truncate, stale_badge, sync_words};
use turbogit_ui::screens::shelf::counted;
use turbogit_ui::theme::RepoState;

#[test]
fn counted_pluralizes_zero_one_and_many() {
    assert_eq!(counted(0, "remote", "remotes"), "0 remotes");
    assert_eq!(counted(1, "remote", "remotes"), "1 remote");
    assert_eq!(counted(2, "remote", "remotes"), "2 remotes");
    assert_eq!(counted(0, "branch", "branches"), "0 branches");
    assert_eq!(counted(1, "branch", "branches"), "1 branch");
}

#[test]
fn the_sync_badge_speaks_only_for_drift_or_a_gone_upstream() {
    assert_eq!(sync_words(0, 0, false), None);
    assert_eq!(sync_words(2, 1, false).as_deref(), Some("↑2 ↓1"));
    assert_eq!(sync_words(0, 3, false).as_deref(), Some("↑0 ↓3"));
    assert_eq!(
        sync_words(0, 0, true).as_deref(),
        Some("gone"),
        "a gone upstream outranks the drift that no longer exists"
    );
}

#[test]
fn stale_ages_compact_to_one_unit() {
    let secs = |n: u64| std::time::Duration::from_secs(n);
    assert_eq!(stale_badge(secs(30)), "now");
    assert_eq!(stale_badge(secs(90)), "1m");
    assert_eq!(stale_badge(secs(3 * 3600)), "3h");
    assert_eq!(stale_badge(secs(2 * 86_400)), "2d");
    assert_eq!(stale_badge(secs(21 * 86_400)), "3w");
    assert_eq!(stale_badge(secs(100 * 86_400)), "3mo");
    assert_eq!(stale_badge(secs(400 * 86_400)), "1y");
}

#[test]
fn middle_truncation_keeps_identifying_ends() {
    let long = "feature/pdv-2.7.62-global-redesign";
    let cut = middle_truncate(long, 18);
    assert!(cut.starts_with("feature"), "the head survives: {cut}");
    assert!(cut.ends_with("design"), "the tail survives: {cut}");
    assert!(cut.contains('…'), "the cut is marked: {cut}");
    assert_eq!(middle_truncate("main", 18), "main");
}

#[test]
fn the_repository_state_vocabulary_is_one_word_per_state() {
    assert_eq!(RepoState::Clean.words(), "");
    assert_eq!(RepoState::Dirty.words(), "dirty");
    assert_eq!(RepoState::Conflict.words(), "has conflicts");
    assert_eq!(RepoState::Diverged.words(), "diverged");
    assert_eq!(RepoState::Unpushed.words(), "unpushed commits");
    assert_eq!(RepoState::Unpulled.words(), "unpulled commits");
    assert_eq!(RepoState::Uninitialized.words(), "not initialized");
}
