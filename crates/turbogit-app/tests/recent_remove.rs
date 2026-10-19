//! Removing a row from the global recents store (the Welcome row's trash
//! control).
//!
//! [`AppState::remove_recent`] is the one place a recents row leaves the list.
//! Three things have to happen together and are easy to separate by accident:
//! the row leaves the in-memory list the Welcome page paints, it leaves the
//! **file** (the list is rebuilt from disk at every launch, so an in-memory-only
//! removal comes straight back), and the row's cached branch indicator leaves
//! the in-memory cache too — otherwise every row a user ever removes stays
//! cached for the rest of the session.
//!
//! It is also deliberately non-destructive: the project on disk is untouched,
//! which is what lets the control be a single click with nothing to undo.
//!
//! Asserts only on the public surface: `AppState` fields and the recents file.

use std::path::{Path, PathBuf};
use turbogit_app::recents::{RecentKind, RecentProject, Recents, load, recents_file, record, save};
use turbogit_app::state::{AppState, ToastKind};

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

/// A temp config dir preloaded with `projects`, plus the bare-launch state that
/// loads it — the exact production wiring the Welcome page sees.
fn state_with(projects: Vec<RecentProject>) -> (tempfile::TempDir, AppState) {
    let config = tempfile::tempdir().expect("temp config dir");
    save(config.path(), &Recents { projects }).expect("seed recents");
    let state = AppState::launch_in(None, Some(config.path().to_path_buf()));
    (config, state)
}

/// Two listed projects: a single repo and a workspace root, so "only the named
/// row went" is a claim about kinds too, not just about names.
fn two_rows(alpha: &Path, ws: &Path) -> Vec<RecentProject> {
    vec![
        recent(alpha, RecentKind::Project, None),
        recent(ws, RecentKind::Workspace, Some(7)),
    ]
}

fn paths(state: &AppState) -> Vec<PathBuf> {
    state
        .ui
        .recent_projects
        .iter()
        .map(|p| p.path.clone())
        .collect()
}

// --- the removal itself --------------------------------------------------------

#[test]
fn removing_a_row_drops_it_from_memory_and_from_the_file() {
    let scratch = tempfile::tempdir().unwrap();
    let alpha = scratch.path().join("alpha");
    let ws = scratch.path().join("ws");
    let (config, mut state) = state_with(two_rows(&alpha, &ws));
    assert_eq!(paths(&state), vec![alpha.clone(), ws.clone()]);

    assert!(
        state.remove_recent(&alpha),
        "a listed row reports as removed"
    );

    // The list the Welcome page paints.
    assert_eq!(
        paths(&state),
        vec![ws.clone()],
        "the removed row leaves the in-memory list and its neighbour stays"
    );
    // The file the next launch reads. Without this the row returns on restart,
    // which is the failure an in-memory assertion cannot see.
    assert_eq!(
        load(config.path())
            .projects
            .iter()
            .map(|p| p.path.clone())
            .collect::<Vec<_>>(),
        vec![ws.clone()],
        "the removal must survive to the recents file"
    );
}

#[test]
fn the_surviving_row_keeps_its_kind_and_repo_count() {
    let scratch = tempfile::tempdir().unwrap();
    let (config, mut state) = state_with(two_rows(
        &scratch.path().join("alpha"),
        &scratch.path().join("ws"),
    ));

    state.remove_recent(&scratch.path().join("alpha"));

    let ws = load(config.path()).projects;
    let kept = ws.first().expect("the workspace row survives");
    assert_eq!(kept.kind, RecentKind::Workspace);
    assert_eq!(
        kept.repo_count,
        Some(7),
        "removing one row must not rewrite the other's fields"
    );
}

#[test]
fn removing_a_row_evicts_its_cached_branch_indicator() {
    let scratch = tempfile::tempdir().unwrap();
    let alpha = scratch.path().join("alpha");
    let ws = scratch.path().join("ws");
    let (_config, mut state) = state_with(two_rows(&alpha, &ws));

    // Both rows have been rendered at least once, so both hold a cached branch.
    state.ui.welcome_branch_cache.insert(
        alpha.clone(),
        (Some("main".into()), std::time::Instant::now()),
    );
    state.ui.welcome_branch_cache.insert(
        ws.clone(),
        (Some("trunk".into()), std::time::Instant::now()),
    );

    state.remove_recent(&alpha);

    assert!(
        !state.ui.welcome_branch_cache.contains_key(&alpha),
        "the removed row's branch indicator must not stay cached for the session"
    );
    assert!(
        state.ui.welcome_branch_cache.contains_key(&ws),
        "only the removed row's cache entry goes"
    );
}

#[test]
fn removing_a_row_leaves_the_project_on_disk() {
    let scratch = tempfile::tempdir().unwrap();
    let alpha = scratch.path().join("alpha");
    let ws = scratch.path().join("ws");
    std::fs::create_dir_all(&alpha).expect("create project dir");
    let (_config, mut state) = state_with(two_rows(&alpha, &ws));

    state.remove_recent(&alpha);

    assert!(
        alpha.is_dir(),
        "removal forgets the path only — the directory is not touched"
    );
    // And the row comes back the ordinary way: opening the project re-records
    // it as the newest entry, which is why the control needs no undo.
    state.open_project(&alpha);
    assert_eq!(
        paths(&state),
        vec![alpha, ws],
        "reopening a removed project records it again, newest-first"
    );
}

// --- the paths that must do nothing -------------------------------------------

#[test]
fn removing_an_unlisted_path_changes_nothing() {
    let scratch = tempfile::tempdir().unwrap();
    let alpha = scratch.path().join("alpha");
    let (config, mut state) = state_with(two_rows(&alpha, &scratch.path().join("ws")));
    let before = paths(&state);
    let file_before = std::fs::read(recents_file(config.path())).expect("read seeded file");
    state.ui.toast = None;

    assert!(
        !state.remove_recent(&scratch.path().join("never-listed")),
        "an unlisted path reports that nothing was removed"
    );

    assert_eq!(paths(&state), before, "the list is untouched");
    assert_eq!(
        std::fs::read(recents_file(config.path())).expect("read recents file"),
        file_before,
        "an unlisted path must not rewrite the store"
    );
    assert!(
        state.ui.toast.is_none(),
        "nothing was removed, so there is nothing to report"
    );
}

#[test]
fn removing_a_row_reports_which_project_it_dropped() {
    let scratch = tempfile::tempdir().unwrap();
    let alpha = scratch.path().join("alpha");
    let (_config, mut state) = state_with(two_rows(&alpha, &scratch.path().join("ws")));

    state.remove_recent(&alpha);

    let toast = state.ui.toast.as_ref().expect("a removal reports itself");
    assert_eq!(toast.kind, ToastKind::Info);
    assert!(
        toast.message.contains("alpha"),
        "the toast names the row that vanished, since the list no longer does; got {:?}",
        toast.message
    );
}

/// A removal must never be a control that silently does nothing when the store
/// cannot be written: the screen's own list is the source of truth for what the
/// user sees, so the row goes from it regardless, and the best-effort write
/// fails the way every other recents write does.
#[test]
fn removal_drops_the_in_memory_row_even_when_the_store_cannot_be_written() {
    let scratch = tempfile::tempdir().unwrap();
    let alpha = scratch.path().join("alpha");
    let ws = scratch.path().join("ws");
    let config = tempfile::tempdir().expect("temp config dir");
    save(
        config.path(),
        &Recents {
            projects: two_rows(&alpha, &ws),
        },
    )
    .expect("seed recents");

    let mut state = AppState::launch_in(None, Some(config.path().to_path_buf()));
    assert_eq!(paths(&state).len(), 2);

    // A regular file where the config dir's parent should be: `create_dir_all`
    // fails with ENOTDIR on every platform, so `save` cannot succeed. (A
    // read-only directory would need POSIX modes to say the same thing.)
    let blocked = scratch.path().join("blocked");
    std::fs::write(&blocked, "x").expect("write blocking file");
    state.recents_config_dir = Some(blocked);

    assert!(
        state.remove_recent(&alpha),
        "the control still reports the removal it made"
    );
    assert_eq!(
        paths(&state),
        vec![ws],
        "the row is gone from the screen even when the file could not be written"
    );
}

/// The recorded-open path and the removal path must agree: a project opened
/// after an earlier removal is listed again (the cap still applies), so the two
/// directions cannot drift into separate stores.
#[test]
fn a_recorded_open_re_appears_after_a_removal() {
    let config = tempfile::tempdir().expect("temp config dir");
    let mut state = AppState::launch_in(None, Some(config.path().to_path_buf()));
    let alpha = config.path().join("alpha");
    let beta = config.path().join("beta");

    state.record_recent(&alpha);
    state.record_recent(&beta);
    assert_eq!(paths(&state), vec![beta.clone(), alpha.clone()]);

    state.remove_recent(&beta);

    state.record_recent(&beta);

    assert_eq!(
        paths(&state),
        vec![beta, alpha],
        "a re-opened project is the newest row again, ahead of the untouched one"
    );
    assert_eq!(
        load(config.path()).projects.len(),
        2,
        "and the file agrees with the screen"
    );
}

/// A store seeded only by the recording path still loads: the remove control
/// must not depend on how a row got into the list.
#[test]
fn a_row_recorded_through_the_open_path_can_be_removed() {
    let config = tempfile::tempdir().expect("temp config dir");
    let mut state = AppState::launch_in(None, Some(config.path().to_path_buf()));
    let alpha = config.path().join("alpha");

    record(config.path(), &alpha);
    state.ui.recent_projects = load(config.path()).projects;
    assert_eq!(paths(&state), vec![alpha.clone()]);

    assert!(state.remove_recent(&alpha));
    assert!(state.ui.recent_projects.is_empty(), "the list is empty");
    assert!(
        load(config.path()).projects.is_empty(),
        "the file is empty too, so the next launch starts clean"
    );
}
