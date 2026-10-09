//! The **failure half of `Unshelve and Remove`**, seen from the app.
//!
//! `AppState::unshelve` arms `shelf_removal_pending` *before* it dispatches, and the shelf
//! record drops on the `OpKind::Unshelve` **success** arm only. That ordering is the whole
//! safety property of the destructive verb: a pop that did not land left the shelved content
//! in git's stash list, so the row that points at it has to stay listed, and the armed
//! removal has to be disarmed — otherwise the *next*, unrelated unshelve spends it and
//! destroys a record whose content nothing ever applied.
//!
//! Both arms are driven through the real state over a real repository, and the outcomes are
//! read back from git's own stash list and from the store on disk.

use std::path::{Path, PathBuf};
use tempfile::TempDir;
use test_support::git_seed::{commit, git};
use turbogit_app::state::AppState;
use turbogit_domain::model::{RootId, Shelf, ShelfSource};
use turbogit_services::shelve_stash;

/// A repository with one commit and repo-local identity, so `git stash push` works.
fn repo_at(dir: &Path) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q", "-b", "main", dir.to_str().unwrap()]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "user.name", "Test"]);
    commit(dir, "base.txt", "base\n", "init");
    dir.to_path_buf()
}

/// A shelf record whose content is git's stash entry (the shape every `shelve` makes).
fn shelved(name: &str, root: &RootId) -> Shelf {
    Shelf {
        name: name.to_string(),
        changes: Vec::new(),
        created_at: chrono::Utc::now(),
        root: root.clone(),
        source: ShelfSource::Stash,
    }
}

/// The rows the pane lists, by name.
fn names(rows: &[Shelf]) -> Vec<String> {
    rows.iter().map(|s| s.name.clone()).collect()
}

/// A pop that never landed keeps the row. The shelf names content in git's stash list, and
/// this row names none: the verb fails, and neither the record nor its row may follow the
/// content it did not restore.
#[test]
fn a_failed_unshelve_and_remove_keeps_the_row_and_disarms_the_removal() {
    let project = TempDir::new().unwrap();
    let lib = repo_at(&project.path().join("lib"));
    let mut state = AppState::for_roots(project.path(), std::slice::from_ref(&lib));
    let root = state.multi.roots[0].id.clone();
    state
        .add_shelf(shelved("phantom", &root))
        .expect("a readable store takes the row");

    // The stash entry the shelf was supposed to have parked is not there — deleted
    // elsewhere, or never made.
    assert!(git(&lib, &["stash", "list"]).trim().is_empty());
    state.unshelve(root.clone(), "phantom", true);
    assert!(
        state.ui.shelf_removal_pending.is_some(),
        "the removal is armed at dispatch, before the verb has run"
    );
    state.drain_events();

    assert!(
        state.ui.shelf_removal_pending.is_none(),
        "a failure disarms it: {:?}",
        state.ui.shelf_removal_pending
    );
    assert_eq!(
        names(&state.shelves_for(&root)),
        vec!["phantom"],
        "the row the pane lists is the row whose content is still parked"
    );
    assert!(
        state
            .last_error
            .as_deref()
            .is_some_and(|m| m.contains("phantom")),
        "git's own refusal is what the user is told: {:?}",
        state.last_error
    );
    assert_eq!(
        shelve_stash::load_shelves(project.path())
            .expect("the store this build wrote parses")
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        vec!["phantom"],
        "and the record on disk agrees with the one on screen"
    );
}

/// The armed removal belongs to **one** verb run. After a failed `Unshelve and Remove`, a
/// later one that lands drops only its own row: were the pending left armed, this success
/// would spend it and erase a record whose content nothing ever applied — the row would
/// vanish from the pane while git still held the work under a name nothing lists.
#[test]
fn a_later_unshelve_that_lands_does_not_spend_a_failed_verbs_pending_removal() {
    let project = TempDir::new().unwrap();
    let lib = repo_at(&project.path().join("lib"));
    let mut state = AppState::for_roots(project.path(), std::slice::from_ref(&lib));
    let root = state.multi.roots[0].id.clone();
    state
        .add_shelf(shelved("phantom", &root))
        .expect("the store takes the first row");

    // The second row has real content parked under its name, so its verb can land.
    std::fs::write(lib.join("base.txt"), "shelved work\n").unwrap();
    git(&lib, &["stash", "push", "-q", "-m", "weather"]);
    state
        .add_shelf(shelved("weather", &root))
        .expect("and the store takes the second");

    state.unshelve(root.clone(), "phantom", true);
    state.drain_events();
    state.unshelve(root.clone(), "weather", true);
    state.drain_events();

    assert_eq!(
        names(&state.shelves_for(&root)),
        vec!["phantom"],
        "only the row whose apply landed was removed"
    );
    assert!(
        state.ui.shelf_removal_pending.is_none(),
        "the success arm takes the pending it was armed with"
    );
    assert!(
        git(&lib, &["stash", "list"]).trim().is_empty(),
        "the entry that landed is popped from git"
    );
    assert_eq!(
        std::fs::read_to_string(lib.join("base.txt")).unwrap(),
        "shelved work\n",
        "and its work is in the tree"
    );
    assert_eq!(
        shelve_stash::load_shelves(project.path())
            .expect("a store this build rewrote still parses")
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        vec!["phantom"],
        "the disk keeps the row the pane keeps"
    );
}
