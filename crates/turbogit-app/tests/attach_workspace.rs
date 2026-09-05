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

use std::path::Path;
use turbogit_app::recents::{RecentKind, load};
use turbogit_app::state::AppState;

fn run_git(dir: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git should be on PATH");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// Seed a minimal repo at `dir`: one branch, one commit touching file.txt.
fn seed_repo(dir: &Path) {
    std::fs::create_dir_all(dir).expect("repo dir");
    run_git(dir, &["init", "-q", "-b", "main"]);
    run_git(dir, &["config", "user.email", "test@example.com"]);
    run_git(dir, &["config", "user.name", "Test"]);
    std::fs::write(dir.join("file.txt"), "v1\n").expect("work file");
    run_git(dir, &["add", "."]);
    run_git(dir, &["commit", "-q", "-m", "initial"]);
}

#[test]
fn attach_workspace_deep_scans_registers_all_roots_and_records_workspace_recent() {
    let root = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();

    // The workspace container is itself NOT a repo; its repos sit at various
    // depths, one strictly deeper than SCAN_MAX_DEPTH so only scan_deep finds
    // it.
    let workspace = root.path().join("ws");
    std::fs::create_dir_all(&workspace).unwrap();
    let deep_repo = workspace.join("a").join("b").join("c").join("d");
    seed_repo(&deep_repo);
    let shallow_repo = workspace.join("alpha");
    seed_repo(&shallow_repo);

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
