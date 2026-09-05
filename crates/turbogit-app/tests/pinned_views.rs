//! Issue #08 — "Pin as view": the current multi-repo selection saves as a
//! named view in `.turbogit/ui.ron` next to the other UI-only state and
//! restores — filtered to the roots still registered — on recall or
//! relaunch.

use std::path::{Path, PathBuf};
use tempfile::TempDir;
use turbogit_app::persistence::{UiPersist, load_ui_state, save_ui_state};
use turbogit_app::pinned_views::{PinnedView, next_name, selection_from_view};
use turbogit_app::state::AppState;
use turbogit_domain::model::RootId;

fn view(name: &str, repos: &[&str]) -> PinnedView {
    PinnedView {
        name: name.to_string(),
        repos: repos.iter().map(PathBuf::from).collect(),
    }
}

#[test]
fn views_round_trip_through_the_workspace_ui_state() {
    let project = TempDir::new().unwrap();
    let ui = UiPersist {
        pinned_views: vec![view("View 1", &["/w/frontend/app", "/w/oss/lib"])],
        ..Default::default()
    };

    save_ui_state(project.path(), &ui).unwrap();
    let loaded = load_ui_state(project.path());

    assert_eq!(
        loaded.pinned_views,
        vec![view("View 1", &["/w/frontend/app", "/w/oss/lib"])]
    );
}

#[test]
fn a_ui_ron_written_before_views_existed_loads_with_none() {
    let project = TempDir::new().unwrap();
    std::fs::create_dir_all(project.path().join(".turbogit")).unwrap();
    std::fs::write(
        project.path().join(".turbogit/ui.ron"),
        "(tab: \"Commit\", recent_repos: [], draft_message: \"hi\")",
    )
    .unwrap();

    let loaded = load_ui_state(project.path());

    assert!(loaded.pinned_views.is_empty());
    assert_eq!(loaded.draft_message, "hi");
}

#[test]
fn views_survive_a_restart_through_the_launch_path() {
    let project = TempDir::new().unwrap();
    let recents_cfg = TempDir::new().unwrap();

    let mut state = AppState::for_roots(project.path(), &[]);
    state.ui.pinned_views = vec![view("View 1", &["/w/oss/lib"])];
    state.persist_ui();

    let reloaded = AppState::launch_in(
        Some(project.path().to_path_buf()),
        Some(recents_cfg.path().to_path_buf()),
    );
    assert_eq!(
        reloaded.ui.pinned_views,
        vec![view("View 1", &["/w/oss/lib"])]
    );
}

#[test]
fn next_name_sequences_view_labels() {
    assert_eq!(next_name(&[]), "View 1");
    assert_eq!(next_name(&[view("View 1", &[])]), "View 2");
    assert_eq!(
        next_name(&[view("View 1", &[]), view("release sweep", &[])]),
        "View 2",
        "numbering follows the highest existing View N"
    );
    assert_eq!(next_name(&[view("View 2", &[])]), "View 3");
}

#[test]
fn restoring_a_view_keeps_only_roots_still_registered() {
    let registered = vec![
        RootId(Path::new("/w/frontend/app").into()),
        RootId(Path::new("/w/frontend/ui").into()),
    ];
    let v = view("View 1", &["/w/frontend/app", "/w/oss/gone"]);

    let selection = selection_from_view(&v, &registered);

    assert_eq!(selection.len(), 1, "the vanished root drops out");
    assert!(selection.contains(&RootId(Path::new("/w/frontend/app").into())));
}

#[test]
fn pinning_the_selection_saves_a_named_view_and_persists() {
    let base = TempDir::new().unwrap();
    let repo_dir = base.path().join("app");
    std::fs::create_dir_all(&repo_dir).unwrap();
    for args in [
        ["init", "-q", "-b", "main"].as_slice(),
        ["config", "user.email", "t@e.com"].as_slice(),
        ["config", "user.name", "T"].as_slice(),
        ["commit", "-q", "--allow-empty", "-m", "init"].as_slice(),
    ] {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(&repo_dir)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?} failed");
    }

    let mut state = AppState::for_roots(base.path(), std::slice::from_ref(&repo_dir));
    let root_id = state.multi.roots[0].id.clone();
    state.ui.repo_selection.insert(root_id.clone());

    state.pin_selection();

    assert_eq!(state.ui.pinned_views.len(), 1);
    assert_eq!(state.ui.pinned_views[0].name, "View 1");
    assert_eq!(state.ui.pinned_views[0].repos, vec![repo_dir.clone()]);

    // The view reaches disk through the same persist path as the rules.
    let loaded = load_ui_state(base.path());
    assert_eq!(loaded.pinned_views, state.ui.pinned_views);
}

#[test]
fn pinning_with_nothing_selected_pins_nothing() {
    let project = TempDir::new().unwrap();
    let mut state = AppState::for_roots(project.path(), &[]);

    state.pin_selection();

    assert!(state.ui.pinned_views.is_empty());
}
