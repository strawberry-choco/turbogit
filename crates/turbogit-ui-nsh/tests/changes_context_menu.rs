//! The changed-file row menu, ported from the egui `changes_context_menu`
//! suite: the designed order, the staging verb naming the row's own state,
//! the gates a tracked, untracked, half-staged, or unmerged row answers with,
//! the selection a right-click keeps or replaces, and the verbs that reach
//! git.

use std::path::{Path, PathBuf};

use support::{Project, commit_tab, open_group, seeded_repo};

mod support;

use test_support::git_seed::git;

const STAGE: &str = "Stage file";
const UNSTAGE: &str = "Unstage file";
const STAGE_ALL: &str = "Stage all of file";
const DISCARD: &str = "Discard changes…";
const SHELVE: &str = "Shelve…";
const PATCH: &str = "Create Patch from Local Changes";
const COPY_PATCH: &str = "Copy as Patch to Clipboard";
const BLAME: &str = "Show blame";
const RESOLVE: &str = "Resolve…";
const OURS: &str = "Take ours";
const THEIRS: &str = "Take theirs";

fn make_dirty(repo: &Path) {
    std::fs::write(repo.join("base.txt"), "work\n").unwrap();
    std::fs::write(repo.join("new.txt"), "fresh\n").unwrap();
}

fn commit_base(repo: &Path) {
    std::fs::write(repo.join("base.txt"), "base\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "base"]);
}

/// A seeded repository with a committed `base.txt` for the menus to answer
/// for.
fn repo(project: &Project, name: &str) -> PathBuf {
    let repo = seeded_repo(project.path(), name);
    commit_base(&repo);
    repo
}

fn app_for(project: &Project, repo: &PathBuf) -> support::Mounted {
    let name = repo.file_name().unwrap().to_string_lossy().to_string();
    let mut app = commit_tab(project.path(), std::slice::from_ref(repo));
    open_group(&mut app, &name);
    app.wait_for(&format!("id:changes:row:{name}:base.txt"))
        .expect("the file rows paint");
    app
}

fn labels(app: &support::Mounted) -> Vec<String> {
    app.menu()
        .expect("the context menu opened")
        .into_iter()
        .map(|entry| entry.label)
        .collect()
}

fn gated(app: &support::Mounted, label: &str) -> bool {
    app.menu()
        .expect("the context menu opened")
        .into_iter()
        .find(|entry| entry.label == label)
        .map(|entry| entry.disabled)
        .unwrap_or_else(|| panic!("the menu offers {label}"))
}

fn unmerged_paths(repo: &Path) -> Vec<String> {
    git(repo, &["diff", "--name-only", "--diff-filter=U"])
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn right_clicking_a_changed_row_raises_the_designed_order() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    make_dirty(&repo);
    let mut app = app_for(&project, &repo);

    app.right_click("id:changes:row:alpha:base.txt").unwrap();
    assert_eq!(
        labels(&app),
        vec![STAGE, DISCARD, SHELVE, PATCH, COPY_PATCH, BLAME],
        "the staging verb leads alone, the destructive and parking verbs share a group, \
         the patch pair shares one, and the read-only verb closes the menu"
    );
    app.close_menu().unwrap();
}

#[test]
fn the_resolution_verbs_are_absent_from_an_ordinary_rows_menu() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    make_dirty(&repo);
    let mut app = app_for(&project, &repo);

    app.right_click("id:changes:row:alpha:base.txt").unwrap();
    let labels = labels(&app);
    for absent in [OURS, THEIRS, RESOLVE] {
        assert!(
            !labels.iter().any(|label| label == absent),
            "an ordinary row never offers `{absent}`: {labels:?}"
        );
    }
    app.close_menu().unwrap();
}

#[test]
fn the_staging_verb_names_the_state_the_row_is_in() {
    let project = Project::new();
    let unstaged = repo(&project, "alpha");
    make_dirty(&unstaged);
    let mut app = app_for(&project, &unstaged);
    app.right_click("id:changes:row:alpha:base.txt").unwrap();
    let offered = labels(&app);
    assert!(offered.iter().any(|label| label == STAGE));
    assert!(!offered.iter().any(|label| label == UNSTAGE));
    app.close_menu().unwrap();

    let staged = repo(&project, "staged");
    make_dirty(&staged);
    git(&staged, &["add", "base.txt"]);
    let mut app = app_for(&project, &staged);
    app.right_click("id:changes:row:staged:base.txt").unwrap();
    let offered = labels(&app);
    assert!(offered.iter().any(|label| label == UNSTAGE));
    assert!(!offered.iter().any(|label| label == STAGE));
    app.close_menu().unwrap();
}

#[test]
fn a_half_staged_row_is_asked_about_as_the_remainder_it_is() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    std::fs::write(repo.join("base.txt"), "one\ntwo\nthree\n").unwrap();
    git(&repo, &["add", "base.txt"]);
    git(&repo, &["commit", "-q", "-m", "three lines"]);
    std::fs::write(repo.join("base.txt"), "ONE\ntwo\nthree\n").unwrap();
    git(&repo, &["add", "base.txt"]);
    std::fs::write(repo.join("base.txt"), "ONE\nTWO\nthree\n").unwrap();

    let mut app = app_for(&project, &repo);
    app.right_click("id:changes:row:alpha:base.txt").unwrap();
    assert!(
        labels(&app).iter().any(|label| label == STAGE_ALL),
        "a row with one of its two edits already in the index stages the remainder: {:?}",
        labels(&app)
    );
    app.close_menu().unwrap();
}

#[test]
fn an_untracked_row_gates_discard_blame_and_the_parking_verbs() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    make_dirty(&repo);
    let mut app = app_for(&project, &repo);

    app.right_click("id:changes:row:alpha:new.txt").unwrap();
    assert!(!gated(&app, STAGE), "an untracked file still stages");
    for gated_label in [DISCARD, BLAME, SHELVE, PATCH, COPY_PATCH] {
        assert!(
            gated(&app, gated_label),
            "`{gated_label}` answers a row git cannot diff or restore, and says so"
        );
    }
    app.close_menu().unwrap();
}

#[test]
fn a_tracked_row_gates_nothing() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    make_dirty(&repo);
    let mut app = app_for(&project, &repo);

    app.right_click("id:changes:row:alpha:base.txt").unwrap();
    for label in [STAGE, DISCARD, SHELVE, PATCH, COPY_PATCH, BLAME] {
        assert!(!gated(&app, label), "`{label}` answers a tracked row");
    }
    app.close_menu().unwrap();
}

#[test]
fn right_clicking_a_row_already_in_the_selection_keeps_the_other_rows() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    make_dirty(&repo);
    let mut app = app_for(&project, &repo);

    app.press("id:changes:row:alpha:base.txt").unwrap();
    app.hold_keys("mod", true).unwrap();
    app.press("id:changes:row:alpha:new.txt").unwrap();
    app.hold_keys("mod", false).unwrap();

    // The untracked row joins the selection beside the tracked one, so the
    // selection-scoped verbs stay live. A right-click that replaced the set
    // would leave only the untracked row and gate them.
    app.right_click("id:changes:row:alpha:new.txt").unwrap();
    assert!(
        !gated(&app, SHELVE),
        "the menu of a selected row must not drop the rows its selection-scoped \
         verbs were about to act on: {:?}",
        labels(&app)
    );
    app.close_menu().unwrap();
}

#[test]
fn right_clicking_a_row_outside_the_selection_replaces_it() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    make_dirty(&repo);
    let mut app = app_for(&project, &repo);

    app.press("id:changes:row:alpha:base.txt").unwrap();
    app.right_click("id:changes:row:alpha:new.txt").unwrap();
    assert!(
        gated(&app, SHELVE),
        "a right-click outside the set selects the row it landed on, and that row \
         alone has nothing for git to park: {:?}",
        labels(&app)
    );
    app.close_menu().unwrap();
}

#[test]
fn right_clicking_another_row_retargets_the_open_menu() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    make_dirty(&repo);
    let mut app = app_for(&project, &repo);

    app.right_click("id:changes:row:alpha:new.txt").unwrap();
    assert!(gated(&app, DISCARD), "the untracked row's discard is shut");

    app.right_click("id:changes:row:alpha:base.txt").unwrap();
    assert!(
        !gated(&app, DISCARD),
        "the menu follows the row under the pointer, and the tracked row's \
         discard is live: {:?}",
        labels(&app)
    );
    app.close_menu().unwrap();
}

#[test]
fn closing_the_row_menu_dismisses_it() {
    // Escape is the platform popup's own dismissal; the harness closes the
    // menu the way the platform does (`close_menu`), which is the same
    // surface the app hands the items to.
    let project = Project::new();
    let repo = repo(&project, "alpha");
    make_dirty(&repo);
    let mut app = app_for(&project, &repo);

    app.right_click("id:changes:row:alpha:base.txt").unwrap();
    assert!(app.menu().is_some(), "the menu is open");
    app.close_menu().unwrap();
    assert!(
        app.menu().is_none(),
        "a dismissed menu leaves nothing armed"
    );
}

#[test]
fn picking_the_staging_verb_stages_the_whole_file() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    make_dirty(&repo);
    let mut app = app_for(&project, &repo);

    app.right_click("id:changes:row:alpha:base.txt").unwrap();
    app.choose_menu_item(STAGE).unwrap();
    app.wait_until(
        "the item reaches the same write the checkbox makes",
        std::time::Duration::from_secs(15),
        |_| {
            git(&repo, &["diff", "--cached", "--name-only"])
                .lines()
                .any(|path| path == "base.txt")
        },
    )
    .unwrap();
}

#[test]
fn picking_discard_names_the_row_it_was_opened_on() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    make_dirty(&repo);
    let mut app = app_for(&project, &repo);

    app.right_click("id:changes:row:alpha:base.txt").unwrap();
    app.choose_menu_item(DISCARD).unwrap();
    app.wait_for("id:dialog:discard")
        .expect("the item raises the discard question rather than answering it");
    assert!(
        app.screen()
            .matches("id:dialog:discard >> text:base.txt")
            .unwrap_or(false),
        "the confirm names the right-clicked row"
    );
    assert!(
        !app.screen()
            .matches("id:dialog:discard >> text:new.txt")
            .unwrap_or(true),
        "and nothing else"
    );
    app.press("id:dialog:discard >> text:Cancel").unwrap();
    app.wait_for_gone("id:dialog:discard", std::time::Duration::from_secs(5))
        .unwrap();
}

#[test]
fn a_conflicted_rows_menu_offers_the_resolution_verbs_and_one_resolves_the_path() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    let branch = git(&repo, &["rev-parse", "--abbrev-ref", "HEAD"])
        .trim()
        .to_string();
    std::fs::write(repo.join("clash.txt"), "one\n").unwrap();
    git(&repo, &["add", "clash.txt"]);
    git(&repo, &["commit", "-q", "-m", "base"]);
    git(&repo, &["checkout", "-q", "-b", "side"]);
    std::fs::write(repo.join("clash.txt"), "side\n").unwrap();
    git(&repo, &["commit", "-q", "-am", "side"]);
    git(&repo, &["checkout", "-q", &branch]);
    std::fs::write(repo.join("clash.txt"), "main\n").unwrap();
    git(&repo, &["commit", "-q", "-am", "main"]);
    let _ = std::process::Command::new("git")
        .args(["merge", "--no-edit", "side"])
        .current_dir(&repo)
        .output();
    assert_eq!(
        unmerged_paths(&repo),
        vec!["clash.txt".to_owned()],
        "the fixture is an unresolved clash before the menu is opened"
    );

    let mut app = commit_tab(project.path(), std::slice::from_ref(&repo));
    open_group(&mut app, "alpha");
    app.wait_for("id:changes:row:alpha:clash.txt")
        .expect("the conflicted row paints");
    assert!(app.screen().shows("Merge conflicts (1)"));

    app.right_click("id:changes:row:alpha:clash.txt").unwrap();
    assert_eq!(
        labels(&app),
        vec![RESOLVE, OURS, THEIRS, DISCARD, BLAME],
        "the conflicted order, and no staging or parking verb"
    );
    assert!(gated(&app, RESOLVE), "resolution has no dialog yet");
    assert!(gated(&app, DISCARD), "resolve the conflict first");
    assert!(
        !labels(&app)
            .iter()
            .any(|label| label == STAGE || label == STAGE_ALL),
        "an unmerged path has no index state to toggle"
    );

    app.choose_menu_item(OURS).unwrap();
    app.wait_until(
        "the row's own resolution verb resolves the row",
        std::time::Duration::from_secs(15),
        |_| unmerged_paths(&repo).is_empty(),
    )
    .unwrap();
}
