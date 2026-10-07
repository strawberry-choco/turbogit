//! The **Shelf** store seen from the app: what the Shelf pane lists after a
//! launch, and what a shelf verb does to `shelf.ron` when the launch could not
//! read it.
//!
//! Both are the user-facing half of the record-preservation invariant that
//! [`turbogit_services::shelve_stash`] now holds: a legacy store (written before
//! ticket 12 gave [`Shelf`] a `root`) has to reach the pane, and a verb has to
//! leave the rows the launch never read exactly where they were. The second one
//! is the data loss — `.turbogit/shelf.ron` held every shelf's *name*, and
//! [`turbogit_services::shelve_stash::unshelve`] finds the parked content in git's
//! stash list **only by that name**, so an overwritten index strands work that is
//! still on disk.

use std::path::{Path, PathBuf};
use tempfile::TempDir;
use test_support::git_seed::{commit, git};
use turbogit_app::state::AppState;
use turbogit_domain::model::{Change, ChangeStatus, RootId, Shelf};
use turbogit_services::shelve_stash::{self, shelf_path};

/// A repository **at the project directory itself** — the single-root layout
/// every legacy `shelf.ron` was written by, and the one
/// [`crate::state`] discovers as `project_dir`. Returns that path.
fn project_that_is_a_repo() -> TempDir {
    let project = TempDir::new().unwrap();
    git(
        project.path(),
        &["init", "-q", "-b", "main", project.path().to_str().unwrap()],
    );
    git(
        project.path(),
        &["config", "user.email", "test@example.com"],
    );
    git(project.path(), &["config", "user.name", "Test"]);
    commit(project.path(), "README.md", "x\n", "init");
    project
}

/// What an **older build** wrote, reproduced by serialising the old field list
/// rather than typing RON by hand, so the artefact is the real legacy shape.
#[derive(serde::Serialize)]
struct LegacyShelf {
    name: String,
    changes: Vec<Change>,
    created_at: chrono::DateTime<chrono::Utc>,
}

fn write_legacy_store(project_dir: &Path, name: &str) {
    let legacy = vec![LegacyShelf {
        name: name.to_string(),
        changes: vec![Change {
            path: PathBuf::from("README.md"),
            status: ChangeStatus::Modified,
            chunks: Vec::new(),
            staged: false,
            unstaged: false,
            orig_path: None,
        }],
        created_at: chrono::DateTime::parse_from_rfc3339("2026-03-01T09:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc),
    }];
    std::fs::create_dir_all(project_dir.join(".turbogit")).unwrap();
    std::fs::write(
        shelf_path(project_dir),
        ron::ser::to_string_pretty(&legacy, ron::ser::PrettyConfig::new()).unwrap(),
    )
    .unwrap();
}

/// A `shelf.ron` from before `Shelf::root` existed still **launches into the
/// pane**: the launch load reads it, the row binds to the project's own root, and
/// `shelves_for` lists it under the root the app registered — which is what makes
/// the name available to an unshelve.
#[test]
fn a_legacy_shelf_store_launches_into_the_pane_of_its_own_root() {
    let project = project_that_is_a_repo();
    write_legacy_store(project.path(), "wip: parser");

    let state = AppState::for_roots(
        project.path(),
        std::slice::from_ref(&project.path().to_path_buf()),
    );

    assert!(
        state.last_error.is_none(),
        "the launch load must not report a legacy store: {:?}",
        state.last_error
    );
    let root = state.multi.roots[0].id.clone();
    let rows = state.shelves_for(&root);
    assert_eq!(
        rows.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        vec!["wip: parser"],
        "the legacy shelf is listed under the root the app actually registered"
    );
}

/// After a launch that could not read the store, the shelf verbs must not
/// truncate it. This is the exact hole: the load failed, the in-memory list is
/// what the session later built up, and a remove wrote that list over the file —
/// discarding rows this process never read. Now the write is refused, the file is
/// left byte-for-byte alone, and the refusal is reported rather than discarded.
#[test]
fn a_shelf_remove_after_an_unreadable_store_reports_and_preserves_the_file() {
    let project = project_that_is_a_repo();
    let root = RootId(std::sync::Arc::from(project.path()));
    let shelf = shelve_stash::make_shelf("wip: parser", &[], root.clone());
    shelve_stash::save_shelves(project.path(), std::slice::from_ref(&shelf)).unwrap();

    let mut state = AppState::for_roots(
        project.path(),
        std::slice::from_ref(&project.path().to_path_buf()),
    );
    assert_eq!(state.ui.shelves.len(), 1, "a current store loads");

    // Rows the process has never read now sit on disk — a hand-edit or a corrupt
    // write from outside this session.
    let unreadable = "[not RON";
    std::fs::write(shelf_path(project.path()), unreadable).unwrap();

    state.remove_shelf(&root, "wip: parser");

    assert_eq!(
        std::fs::read_to_string(shelf_path(project.path())).unwrap(),
        unreadable,
        "a remove must not overwrite rows the launch never read"
    );
    assert!(
        state
            .last_error
            .as_deref()
            .is_some_and(|m| m.contains("Refused to rewrite the shelf store")),
        "the refused write is reported, not discarded: {:?}",
        state.last_error
    );
}

/// The shape an unshelved row has to keep for the verb to work at all: a shelf
/// written by this build round-trips through the store with its root intact, so
/// the legacy tolerance above did not quietly become the only shape that loads.
#[test]
fn a_current_shelf_round_trips_with_its_root() {
    let project = project_that_is_a_repo();
    let other = project.path().join("sub");
    std::fs::create_dir_all(&other).unwrap();
    let root = RootId(std::sync::Arc::from(other.as_path()));
    let shelf: Shelf = shelve_stash::make_shelf("wip: cli", &[], root.clone());

    shelve_stash::save_shelves(project.path(), std::slice::from_ref(&shelf)).unwrap();
    let loaded = shelve_stash::load_shelves(project.path()).unwrap();

    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].name, "wip: cli");
    assert_eq!(
        loaded[0].root, root,
        "a non-default root survives the write"
    );
}
