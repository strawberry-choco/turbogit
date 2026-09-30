//! Issue #34 — "Attach workspace root" (welcome-screen workspace upgrades).
//!
//! Headless suite over the real [`AppState`] + CLI executor: attaching a
//! workspace root deep-scans the chosen directory tree, registers every
//! repository found (including repos nested beyond `SCAN_MAX_DEPTH`), enters
//! the shell, and persists a [`RecentKind::Workspace`] recents row carrying the
//! indexed repo count so the Welcome screen can restore it later.
//!
//! Asserts only on the public surface: `AppState` transitions and the
//! persisted global recents store (ADR-0005).

use test_support::git_seed::repo_with_one_commit;
use turbogit_app::recents::{RecentKind, load};
use turbogit_app::state::AppState;

#[test]
fn attach_workspace_deep_scans_registers_all_roots_and_records_workspace_recent() {
    let root = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();

    // The workspace container is itself NOT a repo; its repos sit at various
    // depths, one strictly deeper than SCAN_MAX_DEPTH so only scan_deep finds
    // it. The recipe `git init`s *into* `project` rather than creating it, so
    // each level it is handed has to exist first.
    let workspace = root.path().join("ws");
    let deep_parent = workspace.join("a").join("b").join("c");
    std::fs::create_dir_all(&deep_parent).unwrap();
    let deep_repo = repo_with_one_commit(&deep_parent, "d");
    let shallow_repo = repo_with_one_commit(&workspace, "alpha");

    let mut state = AppState::launch_in(None, Some(config.path().to_path_buf()));
    state.attach_workspace(&workspace);

    // Attaching enters the shell and registers every discovered root.
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

    // Persisted as a workspace recent with the indexed repo count.
    let recents = load(config.path());
    let ws = recents
        .projects
        .iter()
        .find(|p| p.path == workspace)
        .expect("workspace must be recorded in the global store");
    assert_eq!(ws.kind, RecentKind::Workspace);
    assert_eq!(ws.repo_count, Some(2));
}
