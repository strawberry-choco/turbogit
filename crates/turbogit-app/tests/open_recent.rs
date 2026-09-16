//! Issue #34 follow-up — the shared recents dispatcher behind the topbar
//! workspace picker and the Welcome screen.
//!
//! `AppState::open_recent` is the single place that decides what a recents row
//! *means*: a [`RecentKind::Workspace`] row deep-scans and re-indexes, a
//! [`RecentKind::Project`] row takes the bounded `rescan` path. Both the
//! Welcome screen's recent rows and the new topbar picker route through it, so
//! there is exactly one copy of the kind dispatch to keep honest.
//!
//! It also owns the missing-path guard: `open_project` on a deleted directory
//! rescans to zero roots, and `show_welcome()` then returns true — the user is
//! silently dumped on Welcome with no explanation. `open_recent` checks
//! `is_dir()` first and surfaces a toast instead.
//!
//! Asserts only on the public surface: `AppState` transitions and the toast.

use std::path::{Path, PathBuf};
use turbogit_app::recents::{RecentKind, RecentProject};
use turbogit_app::state::{AppState, ToastKind};

fn run_git(dir: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git should be on PATH");
    assert!(
        output.status.success(),
        "git {args:?} failed in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Seed a minimal repo at `dir`: one branch (`main`), one commit.
fn seed_repo(dir: &Path) {
    std::fs::create_dir_all(dir).expect("repo dir");
    run_git(dir, &["init", "-q", "-b", "main"]);
    run_git(dir, &["config", "user.email", "test@example.com"]);
    run_git(dir, &["config", "user.name", "Test"]);
    std::fs::write(dir.join("file.txt"), "v1\n").expect("work file");
    run_git(dir, &["add", "."]);
    run_git(dir, &["commit", "-q", "-m", "initial"]);
}

/// A workspace container holding two repos, one of them strictly deeper than
/// the bounded scanner's `SCAN_MAX_DEPTH` so only the deep scan can find it.
fn seed_workspace(base: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let ws = base.join("ws");
    std::fs::create_dir_all(&ws).expect("workspace dir");
    let shallow = ws.join("alpha");
    seed_repo(&shallow);
    let deep = ws.join("a").join("b").join("c").join("d");
    seed_repo(&deep);
    (ws, shallow, deep)
}

fn recent(path: &Path, kind: RecentKind, repo_count: Option<usize>) -> RecentProject {
    RecentProject {
        path: path.to_path_buf(),
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        last_opened: 1_755_000_000_000,
        kind,
        repo_count,
    }
}

// --- Cycle 1: the kind dispatch -----------------------------------------------

#[test]
fn open_recent_deep_scans_a_workspace_row() {
    let scratch = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let (ws, shallow, deep) = seed_workspace(scratch.path());

    let mut state = AppState::launch_in(None, Some(config.path().to_path_buf()));
    assert!(state.show_welcome(), "a bare launch lands on Welcome");

    state.open_recent(&recent(&ws, RecentKind::Workspace, Some(2)));

    assert_eq!(
        state.project_dir, ws,
        "the workspace root becomes the active project dir"
    );
    assert!(
        !state.show_welcome(),
        "restoring a workspace must enter the shell"
    );
    assert_eq!(
        state.multi.roots.len(),
        2,
        "a Workspace row deep-scans: both the shallow sibling and the \
         beyond-SCAN_MAX_DEPTH repo must register"
    );
    assert!(
        state.multi.roots.iter().any(|r| r.id.as_path() == shallow),
        "the shallow repo must be registered"
    );
    assert!(
        state.multi.roots.iter().any(|r| r.id.as_path() == deep),
        "the deep-nested repo must be registered by the deep scan"
    );
    assert!(
        state.selected_root.is_some(),
        "a focused root is selected after restoring"
    );
}

#[test]
fn open_recent_takes_the_bounded_path_for_a_project_row() {
    let scratch = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();

    // The project dir is itself a repo that also nests a repo deeper than the
    // bounded scanner walks: a Project row must NOT reach it, or the two kinds
    // would be indistinguishable.
    let repo = scratch.path().join("repo");
    seed_repo(&repo);
    let deep = repo.join("a").join("b").join("c").join("d");
    seed_repo(&deep);

    let mut state = AppState::launch_in(None, Some(config.path().to_path_buf()));
    state.open_recent(&recent(&repo, RecentKind::Project, None));

    assert_eq!(
        state.project_dir, repo,
        "a Project row retargets the project dir at the row's path"
    );
    assert!(
        !state.show_welcome(),
        "opening a project must enter the shell"
    );
    assert_eq!(
        state.multi.roots.len(),
        1,
        "a Project row takes the bounded scan: only the top-level repo registers"
    );
    assert_eq!(state.multi.roots[0].id.as_path(), repo.as_path());
}

// --- Cycle 2: the missing-path guard (D6) -------------------------------------

#[test]
fn open_recent_on_a_missing_path_toasts_and_dispatches_nothing() {
    let scratch = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let open = scratch.path().join("open");
    seed_repo(&open);

    let mut state = AppState::launch_in(Some(open.clone()), Some(config.path().to_path_buf()));
    assert!(!state.show_welcome(), "launched straight into the shell");

    // A recents row whose directory is gone by the time it is clicked.
    let gone = scratch.path().join("gone");
    seed_repo(&gone);
    std::fs::remove_dir_all(&gone).expect("delete the recent's directory");

    let before_roots = state.multi.roots.len();
    let before_selected = state.selected_root.clone();
    state.ui.toast = None;

    state.open_recent(&recent(&gone, RecentKind::Project, None));

    assert_eq!(
        state.project_dir, open,
        "a missing path must not retarget the project dir"
    );
    assert_eq!(
        state.multi.roots.len(),
        before_roots,
        "a missing path must not re-register roots"
    );
    assert_eq!(
        state.selected_root, before_selected,
        "a missing path must not disturb the focused root"
    );
    assert!(
        !state.show_welcome(),
        "a missing path must not bounce the user to Welcome"
    );
    let toast = state
        .ui
        .toast
        .as_ref()
        .expect("a missing recents path must surface an error toast");
    assert_eq!(toast.kind, ToastKind::Error);
    assert!(
        toast.message.contains("gone"),
        "the toast must name the missing path; got {:?}",
        toast.message
    );
}

#[test]
fn open_recent_on_a_missing_workspace_row_also_toasts_instead_of_dispatching() {
    let scratch = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let open = scratch.path().join("open");
    seed_repo(&open);

    let mut state = AppState::launch_in(Some(open.clone()), Some(config.path().to_path_buf()));
    let gone = scratch.path().join("gone-ws");
    std::fs::create_dir_all(&gone).unwrap();
    std::fs::remove_dir_all(&gone).unwrap();

    state.ui.toast = None;
    state.open_recent(&recent(&gone, RecentKind::Workspace, Some(3)));

    assert_eq!(state.project_dir, open, "project dir untouched");
    assert_eq!(state.multi.roots.len(), 1, "roots untouched");
    assert!(!state.show_welcome(), "never bounced to Welcome");
    assert_eq!(
        state.ui.toast.as_ref().map(|t| t.kind),
        Some(ToastKind::Error),
        "a missing workspace path is an error, not a silent no-op"
    );
}

// --- Cycle 3: the picker's transient flag is invalidated by its transitions ---

#[test]
fn open_recent_closes_the_workspace_picker() {
    let scratch = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let (ws, _, _) = seed_workspace(scratch.path());

    let mut state = AppState::launch_in(None, Some(config.path().to_path_buf()));
    state.ui.workspace_picker_open = true;

    state.open_recent(&recent(&ws, RecentKind::Workspace, Some(2)));

    assert!(
        !state.ui.workspace_picker_open,
        "switching workspace must close the picker"
    );
}

#[test]
fn open_project_closes_the_workspace_picker() {
    let scratch = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let repo = scratch.path().join("repo");
    seed_repo(&repo);

    let mut state = AppState::launch_in(None, Some(config.path().to_path_buf()));
    state.ui.workspace_picker_open = true;

    state.open_project(&repo);

    assert!(
        !state.ui.workspace_picker_open,
        "entering a project must close the picker"
    );
}

#[test]
fn attach_workspace_closes_the_workspace_picker() {
    let scratch = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let (ws, _, _) = seed_workspace(scratch.path());

    let mut state = AppState::launch_in(None, Some(config.path().to_path_buf()));
    state.ui.workspace_picker_open = true;

    state.attach_workspace(&ws);

    assert!(
        !state.ui.workspace_picker_open,
        "attaching a workspace must close the picker"
    );
}

#[test]
fn close_all_projects_closes_the_workspace_picker() {
    let scratch = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let repo = scratch.path().join("repo");
    seed_repo(&repo);

    let mut state = AppState::launch_in(Some(repo), Some(config.path().to_path_buf()));
    state.ui.workspace_picker_open = true;

    state.close_all_projects();

    assert!(
        !state.ui.workspace_picker_open,
        "returning to Welcome must close the picker"
    );
    assert!(state.show_welcome());
}

// --- Cycle 4: the picker state itself -----------------------------------------

#[test]
fn workspace_picker_state_defaults_closed_and_unanchored() {
    // A temp config dir: the picker's flags are session-only and never
    // persisted, but the launch path still loads the real recents file, so
    // keep it off the user's machine.
    let config = tempfile::tempdir().unwrap();
    let state = AppState::launch_in(None, Some(config.path().to_path_buf()));
    assert!(!state.ui.workspace_picker_open, "the picker starts closed");
    assert!(
        state.ui.workspace_picker_anchor.is_none(),
        "no anchor until the selector or the palette opens it"
    );
}
