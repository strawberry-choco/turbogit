//! `Import to shelf` seen from the app: what parking a patch does to the shelf
//! index, to its payload file, and to the tree the row is allowed to touch.
//!
//! The two claims this suite exists to hold are the two the store's history says are
//! load-bearing. First, **a row is per root**: an unshelve puts work back in exactly one
//! working tree, so a shelf imported into repository A must not list under B, and after a
//! restart it must still name A rather than whichever directory the launch happened to
//! bind. Second, **the index write is the commit point**: a verb that painted a row before
//! the store accepted it would leave a name no launch can read back, and the work parked
//! beside it becomes unreachable — the bug this programme paid two agents to remove, which
//! a third way to create a record must not quietly reintroduce.
//!
//! The patch is real git content in a real repository, because the point of the deferral is
//! that the *apply* is what happens later: an import must leave the tree byte-identical.

use std::path::{Path, PathBuf};
use tempfile::TempDir;
use test_support::git_seed::{commit, git, pin_eol_lf};
use turbogit_app::state::AppState;
use turbogit_app::state::PatchSource;
use turbogit_domain::model::{PatchCreatedPolicy, ShelfSource, VcsSettings};
use turbogit_services::shelve_stash::{self, patch_payload_dir, patch_payload_path, shelf_path};

/// A patch that applies cleanly to `repo_at`'s committed `note.txt`.
const PATCH: &str = "diff --git a/note.txt b/note.txt\n\
                     --- a/note.txt\n\
                     +++ b/note.txt\n\
                     @@ -1 +1 @@\n\
                     -one\n\
                     +two\n";

fn repo_at(dir: &Path) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q", "-b", "main", dir.to_str().unwrap()]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "user.name", "Test"]);
    pin_eol_lf(dir);
    commit(dir, "note.txt", "one\n", "Seed the note");
    dir.to_path_buf()
}

/// A workspace of two repositories under a plain project directory — the multi-root shape,
/// in which a row that loses its root is not merely misfiled but applied to the wrong tree.
fn two_roots(project: &Path) -> (PathBuf, PathBuf) {
    (
        repo_at(&project.join("lib")),
        repo_at(&project.join("docs")),
    )
}

fn patch_file(dir: &Path) -> PathBuf {
    let p = dir.join("park-me.patch");
    std::fs::write(&p, PATCH).unwrap();
    p
}

fn names(rows: &[turbogit_domain::model::Shelf]) -> Vec<String> {
    rows.iter().map(|s| s.name.clone()).collect()
}

fn payload_files(project: &Path) -> Vec<String> {
    let dir = patch_payload_dir(project);
    if !dir.exists() {
        return Vec::new();
    }
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect();
    v.sort();
    v
}

/// Parking a patch is a deferral, not an apply: the row appears, its bytes sit beside the
/// index, and the working tree it belongs to is **byte-identical** to before.
#[test]
fn importing_a_patch_parks_it_without_touching_the_working_tree() {
    let project = TempDir::new().unwrap();
    let (lib, _docs) = two_roots(project.path());
    let mut state = AppState::for_roots(project.path(), std::slice::from_ref(&lib));
    let before = std::fs::read_to_string(lib.join("note.txt")).unwrap();

    let shelf = state
        .import_patch_to_shelf(&state.multi.roots[0].id.clone(), "align", PATCH)
        .expect("the import lands");

    assert_eq!(
        before,
        std::fs::read_to_string(lib.join("note.txt")).unwrap()
    );
    assert_eq!(
        shelf.changes.len(),
        1,
        "the row lists the file the patch names"
    );
    let ShelfSource::Patch { file } = &shelf.source else {
        panic!("an imported row is not a stash row: {:?}", shelf.source)
    };
    assert_eq!(
        std::fs::read_to_string(patch_payload_path(project.path(), file)).unwrap(),
        PATCH
    );
    // The durable feed gets the act (#6).
    assert!(
        state
            .ui
            .activity
            .entries
            .iter()
            .any(|e| e.message.contains("Patch imported to the shelf")),
        "{:?}",
        state
            .ui
            .activity
            .entries
            .iter()
            .map(|e| e.message.as_str())
            .collect::<Vec<_>>()
    );
}

/// A shelf row is **one root's**. Two imports into two repositories of one workspace list
/// under their own root and never under the other's, and that separation survives a
/// restart — because an unshelve that ran in the wrong root moves work in the wrong tree.
#[test]
fn an_imported_row_lists_only_under_the_root_it_was_parked_in() {
    let project = TempDir::new().unwrap();
    let (lib, docs) = two_roots(project.path());
    let mut state = AppState::for_roots(project.path(), &[lib.clone(), docs.clone()]);
    let lib_id = state.multi.roots[0].id.clone();
    let docs_id = state.multi.roots[1].id.clone();
    state.selected_root = Some(lib_id.clone());
    state
        .import_patch_file_to_shelf(&patch_file(project.path()), "in the lib")
        .unwrap();
    state.selected_root = Some(docs_id.clone());
    state
        .import_patch_file_to_shelf(&patch_file(project.path()), "in the docs")
        .unwrap();

    assert_eq!(names(&state.shelves_for(&lib_id)), vec!["in the lib"]);
    assert_eq!(names(&state.shelves_for(&docs_id)), vec!["in the docs"]);

    let relaunched = AppState::for_roots(project.path(), &[lib.clone(), docs.clone()]);
    assert_eq!(names(&relaunched.shelves_for(&lib_id)), vec!["in the lib"]);
    assert_eq!(
        names(&relaunched.shelves_for(&docs_id)),
        vec!["in the docs"],
        "a relaunched row keeps the root it was written with, not the project directory"
    );
    assert_eq!(
        relaunched
            .shelves_for(&lib_id)
            .first()
            .map(|s| s.root.clone()),
        Some(lib_id.clone())
    );
}

/// The store's refusal is the verb's refusal, and it is **total**: no row in memory, no row
/// on disk, and no payload file left naming nothing. This is the case that used to strand
/// parked work — a row painted over a store nobody could read back disappears at the next
/// launch, and the bytes it addressed are then unreachable through the product.
#[test]
fn a_refused_index_write_adds_no_row_and_leaves_no_payload_behind() {
    let project = TempDir::new().unwrap();
    let (lib, _docs) = two_roots(project.path());
    std::fs::create_dir_all(project.path().join(".turbogit")).unwrap();
    std::fs::write(shelf_path(project.path()), "]]] not a shelf index").unwrap();
    let before = std::fs::read(shelf_path(project.path())).unwrap();

    let mut state = AppState::for_roots(project.path(), std::slice::from_ref(&lib));
    let id = state.multi.roots[0].id.clone();
    let err = state
        .import_patch_to_shelf(&id, "parked", PATCH)
        .expect_err("an unreadable store refuses to be rewritten");

    assert!(
        state.ui.shelves.is_empty(),
        "the refused row must never reach the pane"
    );
    assert!(payload_files(project.path()).is_empty(), "{err}");
    assert_eq!(
        std::fs::read(shelf_path(project.path())).unwrap(),
        before,
        "the store is left byte-for-byte alone"
    );
    assert!(
        state.last_error.is_some(),
        "the refusal is reported, not swallowed"
    );
}

/// An import whose bytes name no files is refused **before anything is written**: no row,
/// no payload, no index. A shelf row that lists nothing is a row nobody would keep.
#[test]
fn a_file_that_is_not_a_patch_imports_nothing() {
    let project = TempDir::new().unwrap();
    let (lib, _docs) = two_roots(project.path());
    let mut state = AppState::for_roots(project.path(), std::slice::from_ref(&lib));
    let id = state.multi.roots[0].id.clone();

    let err = state
        .import_patch_to_shelf(&id, "not a patch", "just some prose\n")
        .expect_err("prose is not a patch");
    assert!(err.to_string().contains("diff --git"), "{err}");
    assert!(state.ui.shelves.is_empty());
    assert!(payload_files(project.path()).is_empty());
}

/// What the deferral is FOR: `Unshelve` puts the parked bytes into **its own** tree through
/// the apply engine, and the captured default keeps the row. The other root's file is not
/// touched — the same row, unshelved after a restart, still cannot mean the wrong tree.
#[test]
fn unshelving_an_imported_row_applies_it_into_its_own_root_and_keeps_the_row() {
    let project = TempDir::new().unwrap();
    let (lib, docs) = two_roots(project.path());
    let mut state = AppState::for_roots(project.path(), &[lib.clone(), docs.clone()]);
    let lib_id = state.multi.roots[0].id.clone();
    state
        .import_patch_to_shelf(&lib_id, "align the note", PATCH)
        .unwrap();
    let files = payload_files(project.path());
    assert_eq!(files.len(), 1, "{files:?}");

    state.unshelve(lib_id.clone(), "align the note", false);
    state.drain_events();

    assert_eq!(
        std::fs::read_to_string(lib.join("note.txt")).unwrap(),
        "two\n"
    );
    assert_eq!(
        std::fs::read_to_string(docs.join("note.txt")).unwrap(),
        "one\n",
        "the other root never saw this patch"
    );
    assert_eq!(
        names(&state.shelves_for(&lib_id)),
        vec!["align the note"],
        "`Unshelve` keeps by default"
    );
    assert_eq!(payload_files(project.path()), files);
}

/// The destructive half of the pair reclaims everything the row owns: the record and the
/// payload file, and **only for that root**. A row whose apply failed keeps both, which is
/// what makes a retry possible at all.
#[test]
fn unshelve_and_remove_takes_the_imported_payload_with_it() {
    let project = TempDir::new().unwrap();
    let (lib, docs) = two_roots(project.path());
    let mut state = AppState::for_roots(project.path(), &[lib.clone(), docs.clone()]);
    let lib_id = state.multi.roots[0].id.clone();
    let docs_id = state.multi.roots[1].id.clone();
    state
        .import_patch_to_shelf(&lib_id, "remove me", PATCH)
        .unwrap();
    state
        .import_patch_to_shelf(&docs_id, "keep me", PATCH)
        .unwrap();
    assert_eq!(payload_files(project.path()).len(), 2);

    state.unshelve(lib_id.clone(), "remove me", true);
    // `for_roots` runs the production threaded pump: the completion has to be drained
    // before settlement has happened, or this asserts the state of a verb still running.
    state.drain_events();

    assert_eq!(
        std::fs::read_to_string(lib.join("note.txt")).unwrap(),
        "two\n",
        "the removal is of the copy, not of the work"
    );
}

/// A patch that cannot land leaves the tree exactly as it was and **keeps the row**: an
/// unshelve is the same apply engine with the same all-or-nothing guarantee, so failing to
/// apply cannot be the act that silently discards what was parked.
#[test]
fn an_imported_row_that_fails_to_apply_keeps_its_row_and_its_bytes() {
    let project = TempDir::new().unwrap();
    let (lib, _docs) = two_roots(project.path());
    let mut state = AppState::for_roots(project.path(), std::slice::from_ref(&lib));
    let id = state.multi.roots[0].id.clone();
    state
        .import_patch_to_shelf(&id, "conflicts", PATCH)
        .unwrap();
    // The tree the patch expects is no longer there, so git refuses the section.
    std::fs::write(lib.join("note.txt"), "utterly different\n").unwrap();

    state.unshelve(id.clone(), "conflicts", true);
    state.drain_events();

    assert_eq!(
        std::fs::read_to_string(lib.join("note.txt")).unwrap(),
        "utterly different\n",
        "a refused apply wrote nothing"
    );
    assert_eq!(
        names(&state.shelves_for(&id)),
        vec!["conflicts"],
        "the row survives the failure, with its bytes"
    );
    assert_eq!(payload_files(project.path()).len(), 1);
}

/// The apply window's footer verb parks **the patch the window holds** under the root the
/// window points at — the same mechanism from the apply flow, so the deferral is reachable
/// wherever an apply is, and it stays as per-root as the window itself.
#[test]
fn the_apply_window_can_park_the_patch_it_holds_instead_of_applying_it() {
    let project = TempDir::new().unwrap();
    let (lib, _docs) = two_roots(project.path());
    let mut state = AppState::for_roots(project.path(), std::slice::from_ref(&lib))
        .with_settings(VcsSettings::default());
    state.selected_root = Some(state.multi.roots[0].id.clone());
    state.open_apply_patch(PatchSource::File);
    let path = patch_file(project.path());
    state.ui.dlg.apply_patch_path = path.to_string_lossy().into_owned();
    state.sync_apply_patch();
    state.ui.dlg.apply_patch_message = "Align the note".to_string();

    state.import_patch_from_apply_window();

    assert!(
        state
            .ui
            .dlg
            .apply_patch_error
            .as_deref()
            .unwrap_or_default()
            .is_empty(),
        "{:?}",
        state.ui.dlg.apply_patch_error
    );
    let id = state.multi.roots[0].id.clone();
    assert_eq!(names(&state.shelves_for(&id)), vec!["Align the note"]);
    assert_eq!(
        std::fs::read_to_string(lib.join("note.txt")).unwrap(),
        "one\n"
    );
    assert_eq!(
        state.ui.dialog, None,
        "the deferral is an ending to the flow"
    );
}

/// The `When patch is created: Ask` follow-up's first verb reaches the same shelf, under the
/// root the patch was generated from — the create route and the deferral are one story, and
/// the ask must not be a dead end that only knows how to apply.
#[test]
fn the_created_patch_ask_can_park_the_file_it_just_named() {
    let project = TempDir::new().unwrap();
    let (lib, _docs) = two_roots(project.path());
    let out = TempDir::new().unwrap();
    let mut state = AppState::for_roots(project.path(), std::slice::from_ref(&lib)).with_settings(
        VcsSettings {
            when_patch_is_created: PatchCreatedPolicy::Ask,
            ..VcsSettings::default()
        },
    );
    let id = state.multi.roots[0].id.clone();
    let d = &mut state.ui.dlg;
    d.create_patch_root = Some(id.clone());
    d.create_patch_raw = Some(PATCH.as_bytes().to_vec());
    d.create_patch_location = out.path().to_string_lossy().into_owned();
    d.create_patch_name = "align.patch".to_string();
    state.finish_create_patch();
    assert!(state.ui.patch_created.is_some(), "Ask armed the follow-up");

    state.import_created_patch_to_shelf();

    assert_eq!(names(&state.shelves_for(&id)), vec!["align"]);
    assert!(
        state.ui.patch_created.is_none(),
        "answering the ask ends it"
    );
    assert_eq!(
        std::fs::read_to_string(lib.join("note.txt")).unwrap(),
        "one\n"
    );
}

/// `Delete Shelf` on an imported row reclaims its payload too — the only reclaim an
/// imported shelf has — while leaving the other root's rows and bytes untouched.
#[test]
fn deleting_an_imported_row_reclaims_its_payload() {
    let project = TempDir::new().unwrap();
    let (lib, docs) = two_roots(project.path());
    let mut state = AppState::for_roots(project.path(), &[lib.clone(), docs.clone()]);
    let lib_id = state.multi.roots[0].id.clone();
    let docs_id = state.multi.roots[1].id.clone();
    state.import_patch_to_shelf(&lib_id, "gone", PATCH).unwrap();
    state
        .import_patch_to_shelf(&docs_id, "kept", PATCH)
        .unwrap();

    state.remove_shelf(&lib_id, "gone");

    assert_eq!(payload_files(project.path()).len(), 1);
    assert_eq!(names(&state.shelves_for(&docs_id)), vec!["kept"]);
    let relaunched = AppState::for_roots(project.path(), &[lib.clone(), docs]);
    assert_eq!(
        names(&relaunched.shelves_for(&lib_id)),
        Vec::<String>::new()
    );
}

/// Two imports whose names slug alike still own **separate** payload files: a shelf that
/// silently overwrote another shelf's parked bytes would be a deferral that destroys what
/// it deferred, and the index would list two rows addressing one file.
#[test]
fn two_imports_never_share_one_payload_file() {
    let project = TempDir::new().unwrap();
    let (lib, _docs) = two_roots(project.path());
    let mut state = AppState::for_roots(project.path(), std::slice::from_ref(&lib));
    let id = state.multi.roots[0].id.clone();
    for text in [PATCH, &PATCH.replace("+two", "+three")] {
        state.import_patch_to_shelf(&id, "same name", text).unwrap();
    }
    let files = payload_files(project.path());
    assert_eq!(files.len(), 2, "{files:?}");
    let rows = state.shelves_for(&id);
    let addrs: Vec<&str> = rows
        .iter()
        .map(|r| match &r.source {
            ShelfSource::Patch { file } => file.as_str(),
            ShelfSource::Stash => "",
        })
        .collect();
    assert_eq!(addrs.len(), 2);
    assert_ne!(addrs[0], addrs[1], "each row addresses its own bytes");
    assert!(shelve_stash::read_patch_payload(project.path(), addrs[0]).is_ok());
}

/// What the row's `source` is **for**, read back after a restart: `AppState::unshelve`
/// branches on that field, so an imported row that came back as a stash row would hand git
/// a lookup for a stash entry that was never made — the parked bytes would stay parked while
/// the verb reported failure over content the store is holding. The repository has no stash
/// entry at all, which is what makes this a routing assertion rather than just an apply one.
#[test]
fn a_relaunched_imported_row_unshelves_its_parked_bytes_and_not_a_stash_entry() {
    let project = TempDir::new().unwrap();
    let (lib, _docs) = two_roots(project.path());
    let mut state = AppState::for_roots(project.path(), std::slice::from_ref(&lib));
    let id = state.multi.roots[0].id.clone();
    state
        .import_patch_to_shelf(&id, "align", PATCH)
        .expect("the import parks the patch");
    assert!(
        git(&lib, &["stash", "list"]).trim().is_empty(),
        "deferral: nothing was ever parked in git's stash stack"
    );

    let mut relaunched = AppState::for_roots(project.path(), std::slice::from_ref(&lib));
    assert!(
        relaunched
            .shelves_for(&id)
            .iter()
            .any(|s| s.name == "align" && matches!(s.source, ShelfSource::Patch { .. })),
        "the relaunched row is still the imported kind, not a stash row: {:?}",
        relaunched.shelves_for(&id)
    );

    relaunched.unshelve(id.clone(), "align", false);
    relaunched.drain_events();

    assert!(
        relaunched.last_error.is_none(),
        "unshelving a relaunched imported row has nothing to fail on: {:?}",
        relaunched.last_error
    );
    assert_eq!(
        std::fs::read_to_string(lib.join("note.txt")).unwrap(),
        "two\n",
        "the parked bytes landed in the tree the row names"
    );
    assert_eq!(
        names(&relaunched.shelves_for(&id)),
        vec!["align"],
        "`Unshelve` keeps, so the row its verb came from is still listed"
    );
    assert!(
        git(&lib, &["stash", "list"]).trim().is_empty(),
        "and git's stash list never became part of the route"
    );
}
