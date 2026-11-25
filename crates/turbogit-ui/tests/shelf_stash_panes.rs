//! The Shelf and Stash panes, ported from the egui `shelf_stash_panes`
//! suite: both roots' shelves list together, the keep and remove unshelve
//! verbs do what their names say, the stash pane lists git's own entries and
//! applies or pops the one its row names, both deletes ask first, and each
//! empty state names its own mechanism.

use std::path::{Path, PathBuf};

use support::{Project, for_roots_seeded, seeded_repo};

mod support;

use chrono::Utc;
use nsh_testing::DialogAnswer;
use test_support::git_seed::git;
use turbogit_domain::model::{RootId, Shelf, ShelfSource};

fn repo(project: &Project, name: &str) -> PathBuf {
    let repo = seeded_repo(project.path(), name);
    std::fs::write(repo.join("base.txt"), "base\n").unwrap();
    git(&repo, &["add", "base.txt"]);
    git(&repo, &["commit", "-q", "-m", "base"]);
    repo
}

fn shelf(name: &str, root: &Path) -> Shelf {
    Shelf {
        name: name.to_string(),
        changes: Vec::new(),
        created_at: Utc::now(),
        root: RootId(root.to_path_buf().into()),
        source: ShelfSource::Stash,
    }
}

/// Parks the working tree's `base.txt` edit in a stash named `name`, the
/// entry a shelf record points at.
fn park_edit(repo: &Path, name: &str, body: &str) {
    std::fs::write(repo.join("base.txt"), body).unwrap();
    git(repo, &["stash", "push", "-q", "-m", name]);
}

fn changes_app(project: &Project, roots: &[PathBuf]) -> support::Mounted {
    let mut app = for_roots_seeded(project.path(), roots, |_| {});
    app.first_render().unwrap();
    app.press("id:tab:Commit").unwrap();
    app.wait_for("id:route:changes").unwrap();
    app
}

fn shelf_app(project: &Project, roots: &[PathBuf], shelves: Vec<Shelf>) -> support::Mounted {
    let mut app = for_roots_seeded(project.path(), roots, move |state| {
        state.ui.shelves.extend(shelves);
    });
    app.first_render().unwrap();
    app.press("id:tab:Commit").unwrap();
    app.wait_for("id:route:changes").unwrap();
    app.press("id:changes:subtab:shelf").unwrap();
    app.wait_for("id:route:shelf").unwrap();
    app
}

fn stash_app(project: &Project, roots: &[PathBuf]) -> support::Mounted {
    let mut app = changes_app(project, roots);
    app.press("id:changes:subtab:stash").unwrap();
    app.wait_for("text:STASHES").unwrap();
    app
}

fn stash_list(repo: &Path) -> Vec<String> {
    git(repo, &["stash", "list"])
        .lines()
        .map(str::to_owned)
        .collect()
}

fn worktree(repo: &Path) -> String {
    std::fs::read_to_string(repo.join("base.txt")).unwrap_or_default()
}

fn answer_pending(app: &mut support::Mounted, label: &str) {
    for _ in 0..200 {
        if app.pending_dialog().is_some() {
            break;
        }
        app.wait(std::time::Duration::from_millis(20)).unwrap();
    }
    app.answer_dialog(DialogAnswer::Press(label.to_string()))
        .expect("the platform alert is pending");
}

fn in_pane(app: &support::Mounted, needle: &str) -> bool {
    app.screen()
        .texts()
        .iter()
        .any(|text| text.contains(needle))
}

#[test]
fn the_shelf_pane_lists_every_roots_shelves() {
    let project = Project::new();
    let alpha = repo(&project, "alpha");
    let beta = repo(&project, "beta");
    let mut app = shelf_app(
        &project,
        &[alpha.clone(), beta.clone()],
        vec![shelf("weather", &alpha), shelf("tides", &beta)],
    );

    app.wait_for("id:shelf:row:alpha:weather")
        .expect("one root's shelf paints");
    app.wait_for("id:shelf:row:beta:tides")
        .expect("and another root's shelf paints too");
    for name in ["alpha", "beta", "weather", "tides"] {
        assert!(in_pane(&app, name), "the pane names {name}");
    }
}

#[test]
fn the_shelf_offers_both_unshelve_verbs() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    park_edit(&repo, "weather", "shelved work\n");
    let app = shelf_app(
        &project,
        std::slice::from_ref(&repo),
        vec![shelf("weather", &repo)],
    );

    assert!(
        app.screen()
            .matches("id:shelf:unshelve:weather")
            .unwrap_or(false),
        "the keeping verb paints"
    );
    assert!(
        app.screen()
            .matches("id:shelf:unshelve-remove:weather")
            .unwrap_or(false),
        "the destructive sibling paints beside it"
    );
}

#[test]
fn unshelve_puts_the_work_back_and_keeps_the_shelf_row() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    park_edit(&repo, "weather", "shelved work\n");
    let mut app = shelf_app(
        &project,
        std::slice::from_ref(&repo),
        vec![shelf("weather", &repo)],
    );

    app.press("id:shelf:unshelve:weather").unwrap();
    app.wait_until(
        "the shelved change returns to the working tree",
        std::time::Duration::from_secs(15),
        |_| worktree(&repo) == "shelved work\n",
    )
    .unwrap();
    assert_eq!(
        stash_list(&repo).len(),
        1,
        "Unshelve keeps git's copy of the content"
    );
    assert!(
        app.screen()
            .matches("id:shelf:row:alpha:weather")
            .unwrap_or(false),
        "and keeps the shelf row"
    );
}

#[test]
fn unshelve_and_remove_takes_the_row_and_gits_copy() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    park_edit(&repo, "weather", "shelved work\n");
    let mut app = shelf_app(
        &project,
        std::slice::from_ref(&repo),
        vec![shelf("weather", &repo)],
    );

    app.press("id:shelf:unshelve-remove:weather").unwrap();
    app.wait_until(
        "the destructive verb empties git's stash list",
        std::time::Duration::from_secs(15),
        |_| stash_list(&repo).is_empty(),
    )
    .unwrap();
    assert_eq!(
        worktree(&repo),
        "shelved work\n",
        "and the work itself lands back in the tree"
    );
    app.wait_for_gone(
        "id:shelf:row:alpha:weather",
        std::time::Duration::from_secs(10),
    )
    .unwrap();
}

#[test]
fn the_stash_pane_lists_the_roots_entries() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    park_edit(&repo, "half-done", "half-done work\n");
    let mut app = stash_app(&project, std::slice::from_ref(&repo));
    app.wait_for("text:stash@{0}")
        .expect("the entry's own git ref paints as a row");

    assert!(in_pane(&app, "stash@{0}"));
    assert!(
        in_pane(&app, "half-done"),
        "and so does the message it was stashed under: {:?}",
        app.screen().texts()
    );
}

#[test]
fn the_stash_pane_states_keep_and_drop_once() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    park_edit(&repo, "half-done", "half-done work\n");
    let mut app = stash_app(&project, std::slice::from_ref(&repo));
    app.wait_for("text:stash@{0}")
        .expect("the entry paints before its verbs are read");

    assert!(
        in_pane(&app, "Apply keeps the entry"),
        "the header states the keep/drop rule once: {:?}",
        app.screen().texts()
    );
    assert!(
        !in_pane(&app, "Apply Stash"),
        "and no row repeats it: {:?}",
        app.screen().texts()
    );
    assert!(
        app.screen().texts().contains(&"Pop"),
        "the row's middle verb is the bare word: {:?}",
        app.screen().texts()
    );
}

#[test]
fn apply_stash_puts_the_work_back_and_keeps_the_entry() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    park_edit(&repo, "half-done", "half-done work\n");
    let mut app = stash_app(&project, std::slice::from_ref(&repo));
    app.wait_for("text:stash@{0}")
        .expect("the entry paints before its verb is pressed");

    app.press("id:shelf:apply:0").unwrap();
    app.wait_until(
        "the click puts the entry's content back",
        std::time::Duration::from_secs(15),
        |_| worktree(&repo) == "half-done work\n",
    )
    .unwrap();
    assert_eq!(
        stash_list(&repo).len(),
        1,
        "the keeping half of the pair really keeps the entry"
    );
    assert!(git(&repo, &["stash", "list"]).contains("half-done"));
}

#[test]
fn pop_stash_removes_the_entry_its_own_row_names() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    park_edit(&repo, "older", "older work\n");
    park_edit(&repo, "newer", "newer work\n");
    assert_eq!(stash_list(&repo).len(), 2, "two entries to tell apart");

    let mut app = stash_app(&project, std::slice::from_ref(&repo));
    app.wait_for("text:stash@{1}").unwrap();
    app.press("id:shelf:pop:1").unwrap();
    app.wait_until(
        "the pop takes exactly one entry",
        std::time::Duration::from_secs(15),
        |_| stash_list(&repo).len() == 1,
    )
    .unwrap();
    let left = stash_list(&repo);
    assert!(
        left[0].contains("newer"),
        "the survivor is the entry that was NOT clicked: {left:?}"
    );
    assert_eq!(
        worktree(&repo),
        "older work\n",
        "the work that came back is the clicked row's own content"
    );
}

#[test]
fn the_shelf_and_stash_deletes_ask_before_they_destroy() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    park_edit(&repo, "weather", "shelved work\n");
    let mut app = shelf_app(
        &project,
        std::slice::from_ref(&repo),
        vec![shelf("weather", &repo)],
    );

    app.answer_dialogs_myself();
    app.press("id:shelf:delete:weather").unwrap();
    answer_pending(&mut app, "OK");
    app.wait_for_gone(
        "id:shelf:row:alpha:weather",
        std::time::Duration::from_secs(15),
    )
    .expect("the confirmed answer removes the record");
    assert!(
        stash_list(&repo).is_empty(),
        "and the stash entry the shelf's content lived in"
    );

    park_edit(&repo, "half-done", "half-done work\n");
    app.press("id:changes:subtab:stash").unwrap();
    app.wait_for("text:STASHES").unwrap();
    app.wait_for("id:shelf:row:alpha:stash@{0}")
        .expect("the fresh entry paints");
    app.press("id:shelf:delete:0").unwrap();
    answer_pending(&mut app, "OK");
    app.wait_until(
        "the confirmed answer drops the entry",
        std::time::Duration::from_secs(15),
        |_| stash_list(&repo).is_empty(),
    )
    .unwrap();
}

#[test]
fn cancelling_a_delete_keeps_the_row_and_the_entry() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    park_edit(&repo, "half-done", "half-done work\n");
    let mut app = stash_app(&project, std::slice::from_ref(&repo));
    app.wait_for("text:stash@{0}").unwrap();

    app.answer_dialogs_myself();
    app.press("id:shelf:delete:0").unwrap();
    answer_pending(&mut app, "Cancel");
    app.wait(std::time::Duration::from_millis(500)).unwrap();
    assert_eq!(
        stash_list(&repo).len(),
        1,
        "Cancel must leave the entry alone"
    );
}

#[test]
fn the_two_empty_states_name_their_own_mechanism() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    let mut app = shelf_app(&project, std::slice::from_ref(&repo), Vec::new());
    assert!(
        in_pane(&app, "Nothing is shelved in this repository"),
        "the shelf's empty state says what a shelf holds and where to make one: {:?}",
        app.screen().texts()
    );

    app.press("id:changes:subtab:stash").unwrap();
    app.wait_for("text:STASHES").unwrap();
    app.wait_until(
        "the stash's empty state paints",
        std::time::Duration::from_secs(10),
        |screen| {
            screen
                .texts()
                .iter()
                .any(|text| text.contains("Nothing is stashed in this repository"))
        },
    )
    .unwrap();
}

#[test]
fn the_shelf_header_reaches_its_own_verb() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    let app = shelf_app(&project, std::slice::from_ref(&repo), Vec::new());

    assert!(
        app.screen().matches("id:shelf:shelve").unwrap_or(false),
        "the pane's header offers its Shelve verb"
    );
    assert!(
        app.screen().matches("id:shelf:import").unwrap_or(false),
        "and the import path"
    );
}
