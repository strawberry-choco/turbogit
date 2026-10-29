use test_support::git_seed::repo_with_one_commit;
use turbogit_app::recents::{RecentKind, load};
use turbogit_app::state::AppState;

#[test]
fn attach_workspace_deep_scans_registers_all_roots_and_records_workspace_recent() {
    let root = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();

    let workspace = root.path().join("ws");
    let deep_parent = workspace.join("a").join("b").join("c");
    std::fs::create_dir_all(&deep_parent).unwrap();
    let deep_repo = repo_with_one_commit(&deep_parent, "d");
    let shallow_repo = repo_with_one_commit(&workspace, "alpha");

    let mut state = AppState::launch_in(None, Some(config.path().to_path_buf()));
    state.attach_workspace(&workspace);

    assert!(
        !state.ui.welcome_visible,
        "attaching a workspace must enter the shell"
    );
    assert_eq!(
        state.multi.roots.len(),
        2,
        "both repos in the tree must be registered"
    );
    assert!(
        state
            .multi
            .roots
            .iter()
            .any(|r| r.id.as_path() == deep_repo),
        "deep-nested repo must be registered by the deep scan"
    );
    assert!(
        state
            .multi
            .roots
            .iter()
            .any(|r| r.id.as_path() == shallow_repo)
    );
    assert!(
        state.selected_root.is_some(),
        "a focused root is selected after attaching"
    );

    let recents = load(config.path());
    let ws = recents
        .projects
        .iter()
        .find(|p| p.path == workspace)
        .expect("workspace must be recorded in the global store");
    assert_eq!(ws.kind, RecentKind::Workspace);
    assert_eq!(ws.repo_count, Some(2));
}
