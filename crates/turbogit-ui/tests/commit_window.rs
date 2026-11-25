//! The Commit tab, ported from the egui `commit_window` suite: the grouped
//! row set, the file checkbox, the message well and its enable rule, the
//! commit and amend paths, the options popover's hook skip, the sub-tab
//! strip, the filter, and the clean-tree sentence.

use std::path::Path;

use support::{Project, commit_tab, dirty_files_repo, open_group, shows_part, staged_patch};

mod support;

use test_support::git_seed::git;

fn app_for(project: &Project, repo: &Path) -> support::Mounted {
    let mut app = commit_tab(project.path(), std::slice::from_ref(&repo.to_path_buf()));
    open_group(&mut app, "alpha");
    app.wait_for("id:changes:row:alpha:a.txt")
        .expect("the file rows paint");
    app
}

#[test]
fn canonical_groups_count_badges_and_status_rows_paint() {
    let project = Project::new();
    let repo = dirty_files_repo(project.path(), "alpha");
    let app = app_for(&project, &repo);

    assert!(app.screen().shows("Changes"), "the pane header paints");
    assert!(app.screen().shows("3"), "the header counts three changes");
    assert!(shows_part(&app, "alpha"), "the repository group paints");
    assert!(app.screen().shows("main"), "and its branch");
    for file in ["a.txt", "b.txt", "c.txt"] {
        assert!(
            app.screen()
                .matches(&format!("id:changes:row:alpha:{file}"))
                .unwrap_or(false),
            "{file} paints as its own row"
        );
    }
}

#[test]
fn the_file_row_checkbox_stages_and_unstages_the_whole_file() {
    let project = Project::new();
    let repo = dirty_files_repo(project.path(), "alpha");
    let mut app = app_for(&project, &repo);

    app.press("id:changes:check:alpha:a.txt").unwrap();
    app.wait_until(
        "a.txt lands in the index",
        std::time::Duration::from_secs(15),
        |_| staged_patch(&repo).contains("+modified"),
    )
    .unwrap();
    app.press("id:changes:refresh").unwrap();
    app.wait_for("text:Unstage").unwrap();

    app.press("id:changes:check:alpha:a.txt").unwrap();
    app.wait_until(
        "a.txt leaves the index again",
        std::time::Duration::from_secs(15),
        |_| staged_patch(&repo).is_empty(),
    )
    .unwrap();
}

#[test]
fn commit_is_disabled_until_the_index_holds_something_and_a_message_exists() {
    let project = Project::new();
    let repo = dirty_files_repo(project.path(), "alpha");
    let mut app = app_for(&project, &repo);

    assert!(
        !app.screen().enabled("id:commit:action").unwrap(),
        "nothing staged, nothing to commit"
    );

    app.press("id:changes:check:alpha:a.txt").unwrap();
    app.wait_until(
        "the index holds a.txt",
        std::time::Duration::from_secs(15),
        |_| !staged_patch(&repo).is_empty(),
    )
    .unwrap();
    assert!(
        !app.screen().enabled("id:commit:action").unwrap(),
        "a staged file with no message still cannot commit"
    );

    app.type_text("id:commit:message", "the first commit")
        .unwrap();
    app.wait_until(
        "a message enables the commit",
        std::time::Duration::from_secs(15),
        |screen| screen.enabled("id:commit:action").unwrap_or(false),
    )
    .unwrap();
}

#[test]
fn a_clean_tree_states_the_clean_tree_sentence() {
    let project = Project::new();
    let repo = support::seeded_repo(project.path(), "alpha");
    let app = commit_tab(project.path(), std::slice::from_ref(&repo));
    assert!(
        app.screen()
            .shows("Nothing to commit, working tree is clean."),
        "a clean repository says so"
    );
}

#[test]
fn commit_executes_for_valid_input_and_clears_the_well() {
    let project = Project::new();
    let repo = dirty_files_repo(project.path(), "alpha");
    let mut app = app_for(&project, &repo);

    app.press("id:changes:check:alpha:a.txt").unwrap();
    app.wait_until("a.txt stages", std::time::Duration::from_secs(15), |_| {
        !staged_patch(&repo).is_empty()
    })
    .unwrap();
    app.type_text("id:commit:message", "first real commit")
        .unwrap();
    app.wait_until(
        "the commit enables",
        std::time::Duration::from_secs(15),
        |screen| screen.enabled("id:commit:action").unwrap_or(false),
    )
    .unwrap();
    app.press("id:commit:action").unwrap();
    app.wait_until(
        "the commit lands",
        std::time::Duration::from_secs(20),
        |_| git(&repo, &["log", "-1", "--pretty=%s"]).trim() == "first real commit",
    )
    .unwrap();
    app.wait_until(
        "the index is spent",
        std::time::Duration::from_secs(15),
        |_| staged_patch(&repo).is_empty(),
    )
    .unwrap();
}

#[test]
fn amend_commits_with_the_amend_flag_and_keeps_one_commit() {
    let project = Project::new();
    let repo = dirty_files_repo(project.path(), "alpha");
    let mut app = app_for(&project, &repo);

    app.press("id:changes:check:alpha:a.txt").unwrap();
    app.wait_until("a.txt stages", std::time::Duration::from_secs(15), |_| {
        !staged_patch(&repo).is_empty()
    })
    .unwrap();
    app.type_text("id:commit:message", "original subject")
        .unwrap();
    app.wait_until(
        "the commit enables",
        std::time::Duration::from_secs(15),
        |screen| screen.enabled("id:commit:action").unwrap_or(false),
    )
    .unwrap();
    app.press("id:commit:action").unwrap();
    app.wait_until(
        "the first commit lands",
        std::time::Duration::from_secs(20),
        |_| git(&repo, &["log", "-1", "--pretty=%s"]).trim() == "original subject",
    )
    .unwrap();

    std::fs::write(repo.join("a.txt"), "amended\n").unwrap();
    app.press("id:changes:refresh").unwrap();
    app.wait_until(
        "the new work reaches the list",
        std::time::Duration::from_secs(15),
        |screen| {
            screen
                .matches("id:changes:row:alpha:a.txt")
                .unwrap_or(false)
        },
    )
    .unwrap();
    app.wait(std::time::Duration::from_millis(400)).unwrap();
    app.press("id:changes:check:alpha:a.txt").unwrap();
    app.wait_until(
        "the amended work stages",
        std::time::Duration::from_secs(15),
        |_| staged_patch(&repo).contains("+amended"),
    )
    .unwrap();

    app.type_text("id:commit:message", "amended subject")
        .unwrap();
    app.press("id:commit:amend").unwrap();
    app.wait_until(
        "amend enables the commit",
        std::time::Duration::from_secs(15),
        |screen| screen.enabled("id:commit:action").unwrap_or(false),
    )
    .unwrap();
    app.press("id:commit:action").unwrap();
    app.wait_until(
        "the amend replaces the commit",
        std::time::Duration::from_secs(20),
        |_| git(&repo, &["log", "-1", "--pretty=%s"]).trim() == "amended subject",
    )
    .unwrap();
    assert_eq!(
        git(&repo, &["rev-list", "--count", "HEAD"]).trim(),
        "3",
        "an amend rewrites the commit instead of adding one"
    );
}

#[test]
fn the_options_popover_paints_the_hook_skip_as_one_row() {
    let project = Project::new();
    let repo = dirty_files_repo(project.path(), "alpha");
    let mut app = app_for(&project, &repo);

    app.press("id:commit:options").unwrap();
    app.wait_for("id:commit:no-verify").unwrap();

    // The popover stacks its children in a column, so a box and its words
    // added as two children paint as an empty band above the label.
    let mut box_center = None;
    let mut box_left = None;
    let mut label_center = None;
    let mut label_left = None;
    for node in app.screen().layout_nodes() {
        let center = node.bounds.origin.y.0 + node.bounds.size.height.0 / 2.0;
        if node.test_id.as_deref() == Some("commit:no-verify") {
            box_center = Some(center);
            box_left = Some(node.bounds.origin.x.0);
        }
        if node
            .text
            .as_ref()
            .is_some_and(|text| text.text.text == "Do not run git commit hooks")
        {
            label_center = Some(center);
            label_left = Some(node.bounds.origin.x.0);
        }
    }
    let (box_center, label_center) = (
        box_center.expect("the hook-skip box paints"),
        label_center.expect("its label paints"),
    );
    assert!(
        (box_center - label_center).abs() <= 3.0,
        "the label shares the box's line: box {box_center}, label {label_center}"
    );
    assert!(
        label_left.unwrap() > box_left.unwrap(),
        "the label sits right of the box"
    );
}

#[cfg(unix)]
#[test]
fn the_options_popover_skips_a_hook_that_would_block_the_commit() {
    use std::os::unix::fs::PermissionsExt;

    let project = Project::new();
    let repo = dirty_files_repo(project.path(), "alpha");
    // A hook that refuses every commit: only the option gets past it.
    let hook = repo.join(".git/hooks/pre-commit");
    std::fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut app = app_for(&project, &repo);

    app.press("id:changes:check:alpha:a.txt").unwrap();
    app.wait_until("a.txt stages", std::time::Duration::from_secs(15), |_| {
        !staged_patch(&repo).is_empty()
    })
    .unwrap();
    app.type_text("id:commit:message", "past the hook").unwrap();
    app.wait_until(
        "the commit enables",
        std::time::Duration::from_secs(15),
        |screen| screen.enabled("id:commit:action").unwrap_or(false),
    )
    .unwrap();

    app.press("id:commit:options").unwrap();
    app.wait_for("id:commit:no-verify").unwrap();
    app.press("id:commit:no-verify").unwrap();
    // The card re-renders on the toggle, and the popover's own open state
    // does not outlive that render: it may already be closed.
    app.press_key("escape").unwrap();
    app.press("id:commit:action").unwrap();
    app.wait_until(
        "the commit lands despite the hook",
        std::time::Duration::from_secs(20),
        |_| git(&repo, &["log", "-1", "--pretty=%s"]).trim() == "past the hook",
    )
    .unwrap();
}

#[test]
fn the_sub_tab_strip_switches_between_local_changes_shelf_and_stash() {
    let project = Project::new();
    let repo = dirty_files_repo(project.path(), "alpha");
    let mut app = commit_tab(project.path(), std::slice::from_ref(&repo));

    app.press("id:changes:subtab:shelf").unwrap();
    app.wait_for("id:route:shelf").unwrap();
    app.wait_for("text:SHELVES").unwrap();
    app.press("id:changes:subtab:stash").unwrap();
    app.wait_for("text:STASHES").unwrap();
    app.press("id:changes:subtab:local").unwrap();
    app.wait_for("id:changes:header").unwrap();
}

#[test]
fn the_filter_narrows_the_rows_and_names_its_miss() {
    let project = Project::new();
    let repo = dirty_files_repo(project.path(), "alpha");
    let mut app = app_for(&project, &repo);

    app.type_text("id:changes:filter", "a.txt").unwrap();
    app.wait_until(
        "only a.txt remains",
        std::time::Duration::from_secs(10),
        |screen| {
            screen
                .matches("id:changes:row:alpha:a.txt")
                .unwrap_or(false)
                && !screen.matches("id:changes:row:alpha:b.txt").unwrap_or(true)
        },
    )
    .unwrap();

    app.clear("id:changes:filter").unwrap();
    app.type_text("id:changes:filter", "no-such-file").unwrap();
    app.wait_for("text:No files match 'no-such-file'.").unwrap();
}

#[test]
fn a_commit_seeds_the_recent_messages_row() {
    let project = Project::new();
    let repo = dirty_files_repo(project.path(), "alpha");
    let mut app = app_for(&project, &repo);

    app.press("id:changes:check:alpha:a.txt").unwrap();
    app.wait_until("a.txt stages", std::time::Duration::from_secs(15), |_| {
        !staged_patch(&repo).is_empty()
    })
    .unwrap();
    app.type_text("id:commit:message", "recent one").unwrap();
    app.wait_until(
        "the commit enables",
        std::time::Duration::from_secs(15),
        |screen| screen.enabled("id:commit:action").unwrap_or(false),
    )
    .unwrap();
    app.press("id:commit:action").unwrap();
    app.wait_for("text:Recent:").unwrap();
}
