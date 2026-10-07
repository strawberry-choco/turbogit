//! The **Shelf** store's read/write discipline: a record the app never read may
//! never be erased by a record it did write.
//!
//! `.turbogit/shelf.ron` is the IDE's own list of named shelves — the pane's
//! rows, and the only place a shelved bucket's *name* lives. Its content sits in
//! git's stash list, which [`turbogit_services::shelve_stash::unshelve`] can only
//! reach **by that name**, so a lost record strands work that is still on disk:
//! unreachable through the UI even though git still holds it.
//!
//! Two ways that happened, both from `save_shelves` writing the whole in-memory
//! slice as one `fs::write` truncation:
//!
//! 1. ticket 12 gave [`Shelf`] a required `root`, so every `shelf.ron` written by
//!    an earlier build stopped parsing; the launch load reported the failure and
//!    left the list empty, and the next shelve wrote that empty list over the
//!    file. The first test pins the legacy shape reading back.
//! 2. Any store this build cannot parse at all (corrupt, hand-edited, a future
//!    required field) still truncates on the next write. The second test pins
//!    that a write after a failed read cannot swallow the unreadable rows.

use std::path::{Path, PathBuf};

use turbogit_domain::model::{Change, ChangeStatus, RootId, Shelf};
use turbogit_services::shelve_stash::{self, shelf_path};

/// What an **older build** wrote: the same row without the `root` field ticket 12
/// added. Serialising this shape rather than typing RON by hand keeps the legacy
/// artefact coming from an independent source of truth — the old field list — so
/// the test cannot drift from what the previous release actually emitted.
#[derive(serde::Serialize)]
struct LegacyShelf {
    name: String,
    changes: Vec<Change>,
    created_at: chrono::DateTime<chrono::Utc>,
}

fn change(path: &str) -> Change {
    Change {
        path: PathBuf::from(path),
        status: ChangeStatus::Modified,
        chunks: Vec::new(),
        staged: false,
        unstaged: false,
        orig_path: None,
    }
}

/// Write `shelf.ron` in the pre-ticket-12 shape, as an earlier build left it.
fn write_legacy_store(project_dir: &Path, names: &[&str]) {
    let legacy: Vec<LegacyShelf> = names
        .iter()
        .map(|name| LegacyShelf {
            name: (*name).to_string(),
            changes: vec![change("src/main.rs")],
            created_at: chrono::DateTime::parse_from_rfc3339("2026-03-01T09:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
        })
        .collect();
    let text = ron::ser::to_string_pretty(&legacy, ron::ser::PrettyConfig::new()).unwrap();
    std::fs::create_dir_all(project_dir.join(".turbogit")).unwrap();
    std::fs::write(shelf_path(project_dir), text).unwrap();
}

/// A `shelf.ron` from before ticket 12 — no `root` on any row — **loads** instead
/// of failing, and each legacy row is bound to the project directory (the one
/// root a legacy store could only ever have meant), so the pane lists it and an
/// unshelve can find it by name again.
#[test]
fn a_legacy_shelf_row_without_a_root_loads_bound_to_the_project_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let project_dir = tmp.path().to_path_buf();
    write_legacy_store(&project_dir, &["wip: parser", "wip: cli"]);

    let shelves = shelve_stash::load_shelves(&project_dir)
        .expect("a legacy shelf store must load, not fail the launch");

    let names: Vec<&str> = shelves.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["wip: parser", "wip: cli"]);
    // The row's own content rode along intact.
    assert_eq!(shelves[0].changes.len(), 1);
    assert_eq!(shelves[0].changes[0].path, PathBuf::from("src/main.rs"));
    for shelf in &shelves {
        assert_eq!(
            shelf.root.as_path(),
            project_dir.as_path(),
            "a legacy row carries no root, so it must bind to the project's own root"
        );
    }
}

/// After a store this build cannot parse, a write must not erase what it never
/// read. Both app-side writers used to discard `save_shelves`' result, so a
/// failed launch load followed by the first shelve or unshelve-overwrote the
/// file with a list that had never contained the unreadable records.
#[test]
fn a_write_after_an_unreadable_load_leaves_the_unreadable_records_intact() {
    let tmp = tempfile::tempdir().unwrap();
    let project_dir = tmp.path().to_path_buf();
    std::fs::create_dir_all(project_dir.join(".turbogit")).unwrap();
    let unreadable = "[this is not RON";
    std::fs::write(shelf_path(&project_dir), unreadable).unwrap();

    let load = shelve_stash::load_shelves(&project_dir);
    assert!(
        load.is_err(),
        "an unparseable store reports itself rather than painting an empty pane"
    );

    // The in-memory list is empty — exactly what the launch left behind — and a
    // shelve now tries to write it.
    let written = shelve_stash::save_shelves(&project_dir, &[] as &[Shelf]);
    assert!(
        written.is_err(),
        "save must refuse: its slice does not contain the records on disk"
    );

    assert_eq!(
        std::fs::read_to_string(shelf_path(&project_dir)).unwrap(),
        unreadable,
        "the unreadable records survived"
    );
}

/// The refusal must not turn into a permanent lockout over a store that holds
/// **nothing**: a 0-byte `shelf.ron` is what an interrupted write leaves behind,
/// and it carries no records to lose, so it reads as an empty shelf and the next
/// shelve writes through it normally.
#[test]
fn an_empty_store_is_an_empty_shelf_not_an_unreadable_one() {
    let tmp = tempfile::tempdir().unwrap();
    let project_dir = tmp.path().to_path_buf();
    std::fs::create_dir_all(project_dir.join(".turbogit")).unwrap();
    std::fs::write(shelf_path(&project_dir), "").unwrap();

    assert!(
        shelve_stash::load_shelves(&project_dir)
            .expect("a blank store reads as an empty shelf")
            .is_empty()
    );

    let root = RootId(std::sync::Arc::from(project_dir.as_path()));
    let shelf = shelve_stash::make_shelf("wip: parser", &[], root);
    shelve_stash::save_shelves(&project_dir, std::slice::from_ref(&shelf))
        .expect("a blank store holds no records, so writing it is not a loss");
    assert_eq!(
        shelve_stash::load_shelves(&project_dir)
            .unwrap()
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        vec!["wip: parser"]
    );
}
