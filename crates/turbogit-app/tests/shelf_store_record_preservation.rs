//! The **Shelf** store seen from the app: what the Shelf pane lists after a
//! launch, and what a shelf verb does to `shelf.ron` when the launch could not
//! read it.
//!
//! Both are the user-facing half of the record-preservation invariant that
//! [`turbogit_services::shelve_stash`] now holds: a legacy store (written before
//! the [`Shelf`] `root` field became required) has to reach the pane, and a verb has to
//! leave the rows the launch never read exactly where they were. The second one
//! is the data loss — `.turbogit/shelf.ron` held every shelf's *name*, and
//! [`turbogit_services::shelve_stash::unshelve`] finds the parked content in git's
//! stash list **only by that name**, so an overwritten index strands work that is
//! still on disk.

use std::path::{Path, PathBuf};
use tempfile::TempDir;
use test_support::git_seed::{commit, git, pin_eol_lf};
use turbogit_app::state::AppState;
use turbogit_domain::model::{Change, ChangeStatus, RootId, Shelf, ShelfSource};
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
    pin_eol_lf(project.path());
    commit(project.path(), "README.md", "x\n", "init");
    project
}

/// A repository in its **own** directory — one root of a multi-root workspace whose
/// project directory is a plain folder, the shape the fix's stated cost describes.
fn repo_at(dir: &Path) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q", "-b", "main", dir.to_str().unwrap()]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "user.name", "Test"]);
    commit(dir, "README.md", "x\n", "init");
    dir.to_path_buf()
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

/// Parsed is not **usable**. The legacy row's whole point is that an unshelve can
/// reach the parked content again, and `unshelve` needs both halves of the record:
/// the *name* (which selects the stash entry in git's list) and the *root* (the
/// repository it runs in). This drives the real verb through the app on a real
/// repository, so the binding to `project_dir` is checked against git itself
/// rather than against the pane's filter.
#[test]
fn a_legacy_row_unshelves_its_parked_content_through_the_real_verb() {
    let project = project_that_is_a_repo();
    // Park work exactly as `shelve` does: a stash entry whose message is the shelf's
    // name, then the pre-ticket-12 record that names it.
    std::fs::write(project.path().join("README.md"), "shelved work\n").unwrap();
    git(
        project.path(),
        &["stash", "push", "-q", "-m", "wip: parser"],
    );
    write_legacy_store(project.path(), "wip: parser");

    let mut state = AppState::for_roots(
        project.path(),
        std::slice::from_ref(&project.path().to_path_buf()),
    );
    let root = state.multi.roots[0].id.clone();
    let rows = state.shelves_for(&root);
    assert_eq!(rows.len(), 1, "the legacy row reaches the pane");
    let row = rows[0].clone();

    state.unshelve(row.root.clone(), row.name.clone(), false);
    state.drain_events();
    assert!(
        state.last_error.is_none(),
        "unshelving a legacy row must not fail on the root it was bound to: {:?}",
        state.last_error
    );
    assert_eq!(
        std::fs::read_to_string(project.path().join("README.md")).unwrap(),
        "shelved work\n",
        "the row's name and bound root are what reach the parked content"
    );
    assert_eq!(
        git(project.path(), &["stash", "list"]).lines().count(),
        1,
        "`Unshelve` keeps, so the row and git's copy both stand"
    );
}

/// The cost the fix accepts, pinned rather than asserted in prose: in a
/// **multi-root** project whose `project_dir` is not itself a repository, a legacy
/// row binds to a root no pane lists. It must therefore read as absent (never as
/// applied to the wrong tree), and it must still **survive** the writes the panes
/// can make — the row rides in the in-memory slice, and because the binding is
/// read-time its record goes back rootless, so the store gains no phantom root.
/// Invisible is the deal; destroyed, or re-labelled as a recorded root, would not be.
#[test]
fn a_legacy_row_in_a_multi_root_project_is_invisible_but_never_destroyed() {
    let parent = tempfile::tempdir().unwrap();
    // A plain directory holding two repositories: the workspace shape where no root
    // is the project directory.
    let roots = vec![
        repo_at(&parent.path().join("alpha")),
        repo_at(&parent.path().join("beta")),
    ];
    let project_dir = parent.path().to_path_buf();
    write_legacy_store(&project_dir, "wip: parser");

    let mut state = AppState::for_roots(&project_dir, &roots);
    assert!(
        state.last_error.is_none(),
        "a legacy store must not fail the launch even where its row lists under nothing: {:?}",
        state.last_error
    );
    assert_eq!(
        state.ui.shelves.len(),
        1,
        "the row is loaded — it is nowhere to *list*, not lost"
    );
    for root in state.multi.roots.iter().map(|r| r.id.clone()) {
        assert!(
            state.shelves_for(&root).is_empty(),
            "a row bound to a non-repo project dir must not appear under a real root"
        );
    }

    // A shelve for a listed root rewrites the whole store; the invisible row is in
    // the slice, so it survives the truncation.
    let first_root = state.multi.roots[0].id.clone();
    state
        .ui
        .shelves
        .push(shelve_stash::make_shelf("wip: cli", &[], first_root));
    shelve_stash::save_shelves(&project_dir, &state.ui.shelves)
        .expect("a store this build reads is a store it may write");

    let names: Vec<String> = shelve_stash::load_shelves(&project_dir)
        .unwrap()
        .into_iter()
        .map(|s| s.name)
        .collect();
    assert_eq!(names, vec!["wip: parser", "wip: cli"]);
}

/// A refused rewrite is a refused **verb**. The removal above reported the refusal
/// and kept the file, but it had already dropped the row from the in-memory list:
/// the pane stopped painting a shelf whose record still sits on disk, so the user
/// is told to shelve again *and* sees the row come back at the next launch. The
/// two halves of one verb have to move together — either both or neither.
#[test]
fn a_refused_removal_keeps_the_row_the_file_still_holds() {
    let project = project_that_is_a_repo();
    let root = RootId(std::sync::Arc::from(project.path()));
    let shelf = shelve_stash::make_shelf("wip: parser", &[], root.clone());
    shelve_stash::save_shelves(project.path(), std::slice::from_ref(&shelf)).unwrap();

    let mut state = AppState::for_roots(
        project.path(),
        std::slice::from_ref(&project.path().to_path_buf()),
    );
    assert_eq!(state.ui.shelves.len(), 1, "a current store loads");

    let unreadable = "[not RON";
    std::fs::write(shelf_path(project.path()), unreadable).unwrap();

    state.remove_shelf(&root, "wip: parser");

    assert_eq!(
        state
            .shelves_for(&root)
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        vec!["wip: parser"],
        "the refusal leaves the pane painting exactly what the store still holds"
    );
    assert_eq!(
        std::fs::read_to_string(shelf_path(project.path())).unwrap(),
        unreadable,
        "and the file is untouched"
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

/// The create half of the same hazard, at the seam the shelve dialog goes through.
/// A row the store refused is a row the next launch drops — and by then the work it
/// named has already been parked in git's stash list, where the Shelf pane cannot
/// reach it because only this file carries the name that selects it. So a refused
/// record is a refused shelf: nothing is added to the list.
#[test]
fn a_refused_creation_adds_no_row_that_the_next_launch_would_drop() {
    let project = project_that_is_a_repo();
    let root = RootId(std::sync::Arc::from(project.path()));
    let shelf = shelve_stash::make_shelf("wip: parser", &[], root.clone());
    shelve_stash::save_shelves(project.path(), std::slice::from_ref(&shelf)).unwrap();

    let mut state = AppState::for_roots(
        project.path(),
        std::slice::from_ref(&project.path().to_path_buf()),
    );
    assert_eq!(state.ui.shelves.len(), 1, "a current store loads");

    let unreadable = "[not RON";
    std::fs::write(shelf_path(project.path()), unreadable).unwrap();

    let refused = state.add_shelf(shelve_stash::make_shelf("wip: cli", &[], root.clone()));

    assert!(
        refused.is_err(),
        "the caller learns the record did not land, so it parks the work in git no more"
    );
    assert_eq!(
        state
            .ui
            .shelves
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        vec!["wip: parser"],
        "the pane paints no phantom row"
    );
    assert_eq!(
        std::fs::read_to_string(shelf_path(project.path())).unwrap(),
        unreadable,
        "and the untouched file still holds the rows this session never read"
    );
    assert!(
        state
            .last_error
            .as_deref()
            .is_some_and(|m| m.contains("Refused to rewrite the shelf store")),
        "the user is told: {:?}",
        state.last_error
    );
}

/// The other direction, so the guard above is not mistaken for a shelf verb that
/// never happens: an accepted record is both painted and durable, and the row the
/// store got is the row the pane lists.
#[test]
fn an_accepted_creation_lands_in_the_pane_and_on_disk_together() {
    let project = project_that_is_a_repo();
    let root = RootId(std::sync::Arc::from(project.path()));
    let mut state = AppState::for_roots(
        project.path(),
        std::slice::from_ref(&project.path().to_path_buf()),
    );

    state
        .add_shelf(shelve_stash::make_shelf("wip: cli", &[], root.clone()))
        .expect("a store this build can read is a store it may write");

    assert_eq!(
        state
            .shelves_for(&root)
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        vec!["wip: cli"]
    );
    assert_eq!(
        shelve_stash::load_shelves(project.path())
            .unwrap()
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        vec!["wip: cli"],
        "a restart finds the row the pane is painting"
    );
    assert!(state.last_error.is_none(), "{:?}", state.last_error);
}

/// A per-row reading of the **wire**: which rows recorded a `root`.
///
/// The same instrument as the services suite's `WireRow` (integration tests cannot
/// share a helper across crates), and deliberately not the store's own row type,
/// which invents a root for a keyless row: `#[serde(default)]` on a plain `String`
/// leaves the absent spelling empty, so "recorded" and "omitted" stay separately
/// observable while the pane is being checked.
#[derive(serde::Deserialize)]
struct WireRow {
    name: String,
    #[serde(default)]
    root: String,
}

fn recorded_roots(project_dir: &Path) -> Vec<(String, Option<PathBuf>)> {
    let text = std::fs::read_to_string(shelf_path(project_dir)).unwrap();
    ron::from_str::<Vec<WireRow>>(&text)
        .unwrap_or_else(|e| panic!("the store this build wrote must parse: {e}\n{text}"))
        .into_iter()
        .map(|row| {
            let root = (!row.root.is_empty()).then(|| PathBuf::from(row.root));
            (row.name, root)
        })
        .collect()
}

/// A per-row reading of the **wire**: which rows recorded a `source`, and the payload file
/// a recorded one names.
///
/// The same reason as [`WireRow`]: `ShelfSource` itself reads a missing key as `Stash`, so
/// presence and absence would collapse. `ron::Value` sees the key and its fields (RON's
/// enum tag is dropped in the generic value), so the tag is asserted from the file's text
/// beside it, and any other spelling of a `source` fails loudly here.
fn source_keys(project_dir: &Path) -> Vec<(String, Option<String>)> {
    let text = std::fs::read_to_string(shelf_path(project_dir)).unwrap();
    let value: ron::Value = ron::from_str(&text)
        .unwrap_or_else(|e| panic!("the store this build wrote must parse: {e}\n{text}"));
    let ron::Value::Seq(rows) = value else {
        panic!("the shelf store is a list of rows, got: {value:?}");
    };
    rows.iter()
        .map(|row| {
            let ron::Value::Map(fields) = row else {
                panic!("each row is a map, got: {row:?}");
            };
            let name = match fields.get(&ron::Value::String("name".to_string())) {
                Some(ron::Value::String(name)) => name.clone(),
                other => panic!("a row's name is a string, got: {other:?}"),
            };
            let file = fields
                .get(&ron::Value::String("source".to_string()))
                .map(payload_file);
            (name, file)
        })
        .collect()
}

/// The payload field a recorded `source` holds.
fn payload_file(value: &ron::Value) -> String {
    match value {
        ron::Value::Map(fields) => match fields.get(&ron::Value::String("file".to_string())) {
            Some(ron::Value::String(file)) => file.clone(),
            other => panic!("a recorded `source` must name its payload file, got: {other:?}"),
        },
        other => panic!("unexpected `source` spelling on the wire: {other:?}"),
    }
}

/// A patch that applies cleanly to the `README.md` this suite's `repo_at` commits.
const PATCH: &str = "diff --git a/README.md b/README.md\n\
                    --- a/README.md\n\
                    +++ b/README.md\n\
                    @@ -1 +1 @@\n\
                    -x\n\
                    +y\n";

/// An **imported** row's `source` is the only fact that names the file its parked bytes
/// live in, and [`AppState::unshelve`] branches on it: read back as `Stash`, the verb goes
/// looking for a git stash entry that was never made and fails over content the store is
/// holding. So the relaunched row has to come back as the same variant addressing the same
/// file — and the stash row written beside it must still go back keyless, because the store
/// is rewritten whole on every verb.
#[test]
fn a_relaunched_imported_row_is_still_a_patch_row_addressing_its_own_payload() {
    let project = TempDir::new().unwrap();
    let lib = repo_at(&project.path().join("lib"));
    let mut state = AppState::for_roots(project.path(), std::slice::from_ref(&lib));
    let root = state.multi.roots[0].id.clone();

    let imported = state
        .import_patch_to_shelf(&root, "align the api", PATCH)
        .expect("the import lands on a store this build can read");
    let ShelfSource::Patch { file } = imported.source.clone() else {
        panic!("an imported row is a patch row: {:?}", imported.source)
    };
    state
        .add_shelf(shelve_stash::make_shelf("wip: parser", &[], root.clone()))
        .expect("and a shelved row sits beside it");

    assert_eq!(
        source_keys(project.path()),
        vec![
            ("align the api".to_string(), Some(file.clone())),
            ("wip: parser".to_string(), None),
        ],
        "one row records where its bytes are; the other is given no key it does not need"
    );

    let relaunched = AppState::for_roots(project.path(), std::slice::from_ref(&lib));
    assert!(
        relaunched.last_error.is_none(),
        "a store with a `source` key launches clean: {:?}",
        relaunched.last_error
    );
    let rows = relaunched.shelves_for(&root);
    let again = rows
        .iter()
        .find(|s| s.name == "align the api")
        .unwrap_or_else(|| panic!("the imported row relaunches: {rows:?}"));
    assert_eq!(
        again.source,
        ShelfSource::Patch { file: file.clone() },
        "the variant and the file it addresses both survive the relaunch"
    );
    assert_eq!(
        rows.iter()
            .find(|s| s.name == "wip: parser")
            .map(|s| s.source.clone()),
        Some(ShelfSource::Stash),
        "and the neighbouring stash row is not mistaken for one"
    );
    assert_eq!(
        shelve_stash::read_patch_payload(project.path(), &file).ok(),
        Some(PATCH.to_string()),
        "the relaunched row still addresses the bytes it parked"
    );
}

/// A store this build cannot read must not launch into an **empty pane**: the Shelf pane
/// would then answer "nothing is shelved" over rows it never read, and the only reclaim
/// left — the next shelve — is the write the store itself refuses. The launch therefore
/// reports the failure, and reports it while the file is still byte-for-byte what it was.
#[test]
fn a_store_that_will_not_parse_launches_with_a_report_not_an_empty_pane() {
    let project = project_that_is_a_repo();
    std::fs::create_dir_all(project.path().join(".turbogit")).unwrap();
    let unreadable = "[{ name: \"wip: parser\", changes: [], created_at: ";
    std::fs::write(shelf_path(project.path()), unreadable).unwrap();

    let state = AppState::for_roots(
        project.path(),
        std::slice::from_ref(&project.path().to_path_buf()),
    );

    assert!(
        state.ui.shelves.is_empty(),
        "rows that were never read are not invented: {:?}",
        state.ui.shelves
    );
    assert!(
        state.last_error.is_some(),
        "a launch that could not read the store says so instead of painting an absence"
    );
    assert_eq!(
        std::fs::read_to_string(shelf_path(project.path())).unwrap(),
        unreadable,
        "and reporting it costs nothing on disk"
    );

    // The report is the session's, and it is not a licence to write: the first verb that
    // rewrites the store is refused over the same file.
    let mut state = state;
    let refused = state.add_shelf(shelve_stash::make_shelf(
        "wip: cli",
        &[],
        RootId(std::sync::Arc::from(project.path())),
    ));
    assert!(
        refused.is_err(),
        "the launch's report and the writer's guard agree about this file"
    );
    assert_eq!(
        std::fs::read_to_string(shelf_path(project.path())).unwrap(),
        unreadable,
        "so the rows the launch never read are still there"
    );
}
/// The **multi-root coincidence**, checked through the real create seam: a project
/// whose roots are its own directory *and* a nested repository. `save_shelves`
/// omits the `root` key for the row rooted at `project_dir` — the shape a legacy
/// row arrives in — so the question this answers is whether that omission is a
/// loss of information for a row that genuinely recorded the root.
///
/// It is not, and the reason is structural: the store lives at
/// `<project_dir>/.turbogit/shelf.ron`, so the root it erases is always the very
/// directory the store sits in, which is what [`shelve_stash::load_shelves`] binds
/// an absent root to. The two spellings therefore carry one value, and the row
/// still lists under the root `for_roots` registered — while the second, genuinely
/// different root is recorded and lists under its own pane.
#[test]
fn a_project_dir_that_is_also_one_of_several_roots_round_trips_as_the_same_root() {
    let project = project_that_is_a_repo();
    let nested = repo_at(&project.path().join("nested"));
    let roots = vec![project.path().to_path_buf(), nested.clone()];
    let outer_root = RootId(std::sync::Arc::from(project.path()));
    let nested_root = RootId(std::sync::Arc::from(nested.as_path()));

    let mut state = AppState::for_roots(project.path(), &roots);
    state
        .add_shelf(shelve_stash::make_shelf(
            "wip: outer",
            &[],
            outer_root.clone(),
        ))
        .expect("a store this build can read is a store it may write");
    state
        .add_shelf(shelve_stash::make_shelf(
            "wip: nested",
            &[],
            nested_root.clone(),
        ))
        .expect("and a second write of the same store is refused no more");

    assert_eq!(
        recorded_roots(project.path()),
        vec![
            ("wip: outer".to_string(), None),
            ("wip: nested".to_string(), Some(nested.clone())),
        ],
        "only the row whose root IS the store's own directory goes back rootless"
    );

    // A relaunch reads the store and registers the roots again: each row comes
    // back under the root it was shelved in, as the same `RootId` value.
    let relaunched = AppState::for_roots(project.path(), &roots);
    assert!(
        relaunched.last_error.is_none(),
        "the omitted key must not fail the launch: {:?}",
        relaunched.last_error
    );
    assert_eq!(
        relaunched
            .ui
            .shelves
            .iter()
            .map(|s| s.root.clone())
            .collect::<Vec<RootId>>(),
        vec![outer_root.clone(), nested_root.clone()],
        "identity: the rebound root is the value that was written, not another one"
    );
    for (root, name) in [(&outer_root, "wip: outer"), (&nested_root, "wip: nested")] {
        assert_eq!(
            relaunched
                .shelves_for(root)
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            vec![name],
            "and the row lists under exactly one root"
        );
    }
}
