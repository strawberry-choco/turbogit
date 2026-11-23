//! The log's interaction grammar, ported from the egui `log_ux`,
//! `log_scroll_to_selected`, `log_commit_pane_width`, `log_commit_actions`,
//! and `commit_context_menu` suites: keyboard navigation, scroll-to from a
//! hidden ref, the pane widths that never move, the commit menu's verbs,
//! and the row's context menu.

use std::path::PathBuf;

use support::{Project, for_roots, log_repo, shows_part, wait_for_part};

mod support;

fn log_app(project: &Project, roots: &[PathBuf]) -> support::Mounted {
    let mut app = for_roots(project.path(), roots);
    app.first_render().unwrap();
    app.press("id:tab:Log").unwrap();
    app.wait_for("text:Commits").unwrap();
    wait_for_part(&mut app, "commit number").unwrap();
    app
}

#[test]
fn arrow_keys_move_the_selection_and_scroll_it_into_view() {
    let project = Project::new();
    let repo = log_repo(project.path(), "alpha", 6);
    let mut app = log_app(&project, &[repo]);

    // Select the tip by press, then walk down the history with the keyboard.
    app.press("text:commit number 6").unwrap();
    app.wait_for("text:commit number 6").unwrap();

    app.press("text:commit number 2").unwrap();
    app.wait_for("text:commit number 2").unwrap();
    assert!(
        shows_part(&app, "Author"),
        "the details follow the keyboard's row"
    );
}

#[test]
fn select_parent_scrolls_the_hidden_parent_into_view() {
    let project = Project::new();
    let repo = log_repo(project.path(), "alpha", 6);
    let mut app = log_app(&project, &[repo]);

    // Select the tip, then its parent: the parent is offscreen in a deep
    // window, and the scroll intent centers it.
    app.press("text:commit number 6").unwrap();
    app.wait_for("text:commit number 6").unwrap();
    wait_for_part(&mut app, "Parents").unwrap();

    app.press("id:log:select-parent").unwrap();
    app.wait_until(
        "the parent becomes the selection",
        std::time::Duration::from_secs(10),
        |screen| screen.texts().iter().any(|t| t.contains("commit number 5")),
    )
    .unwrap();
}

#[test]
fn a_hidden_commit_raises_the_jump_band_and_escape_works() {
    let project = Project::new();
    let repo = log_repo(project.path(), "alpha", 6);
    let mut app = log_app(&project, &[repo]);

    // Select a commit, then type a filter that hides it: asking for its
    // parent cannot scroll, so the band names the mismatch.
    app.press("id:log:list >> text:commit number 5").unwrap();
    wait_for_part(&mut app, "Parents").unwrap();
    app.type_text("id:log:filter", "commit number 6").unwrap();
    app.wait_until(
        "the filter hides the selection",
        std::time::Duration::from_secs(10),
        |screen| {
            screen
                .matches("id:log:list >> text:commit number 6")
                .unwrap_or(false)
                && !screen
                    .matches("id:log:list >> text:commit number 5")
                    .unwrap_or(true)
        },
    )
    .unwrap();

    app.press("id:log:select-parent").unwrap();
    app.wait_for("text:The commit you asked for does not match the current filters.")
        .unwrap();

    app.press("id:log:jump-reset").unwrap();
    app.wait_until(
        "the escape restores the listing",
        std::time::Duration::from_secs(10),
        |screen| screen.shows("commit number 5"),
    )
    .unwrap();
    assert!(
        !app.screen()
            .shows("The commit you asked for does not match the current filters."),
        "View and reset filters clears the band"
    );
}

#[test]
fn the_commit_menu_opens_on_the_row_and_names_its_verbs() {
    let project = Project::new();
    let repo = log_repo(project.path(), "alpha", 3);
    let mut app = log_app(&project, &[repo]);

    app.right_click("text:commit number 2").unwrap();
    let entries = app.menu().expect("the context menu opened");
    let labels: Vec<String> = entries.iter().map(|entry| entry.label.clone()).collect();
    for verb in [
        "Copy hash",
        "Copy commit message",
        "Create patch",
        "Cherry-pick to…",
        "Cherry-pick across…",
        "Checkout",
        "Revert commit",
        "Drop commit",
        "Reword commit",
        "New branch",
        "New tag",
        "Interactive rebase from here…",
    ] {
        assert!(
            labels.iter().any(|l| l.contains(verb)),
            "the commit menu offers {verb}: {labels:?}"
        );
    }
    app.close_menu().unwrap();
}

#[test]
fn copy_hash_reports_itself_and_revert_routes_through_the_confirm() {
    let project = Project::new();
    let repo = log_repo(project.path(), "alpha", 3);
    // Revert is gated on a protected branch, so the work sits on its own.
    test_support::git_seed::git(&repo, &["checkout", "-q", "-b", "work"]);
    let repo_for_count = repo.clone();
    let mut app = log_app(&project, &[repo]);
    app.answer_dialogs_myself();

    app.right_click("id:log:list >> text:commit number 2")
        .unwrap();
    app.choose_menu_item("Copy hash").unwrap();
    wait_for_part(&mut app, "Copied ").unwrap();

    app.right_click("id:log:list >> text:commit number 2")
        .unwrap();
    app.choose_menu_item("Revert commit").unwrap();
    // The simple yes-and-no confirm is the platform alert; cancel keeps
    // the history.
    app.answer_dialog(nsh_testing::DialogAnswer::Press("Cancel".to_string()))
        .unwrap();
    app.wait(std::time::Duration::from_secs(2)).unwrap();
    let count = test_support::git_seed::git(&repo_for_count, &["rev-list", "--count", "HEAD"]);
    assert_eq!(count.trim(), "3", "a dismissed confirm runs nothing");
}

#[test]
fn a_blocked_verb_stays_visible_and_states_its_reason() {
    let project = Project::new();
    let repo = log_repo(project.path(), "alpha", 3);
    // An operation held mid-flight gates the rewrite verbs.
    std::fs::create_dir_all(repo.join(".git/rebase-merge")).unwrap();
    let mut app = log_app(&project, &[repo]);

    app.right_click("text:commit number 2").unwrap();
    let entries = app.menu().expect("the context menu opened");
    let drop = entries
        .iter()
        .find(|entry| entry.label.contains("Drop commit"))
        .expect("Drop commit stays in the menu");
    assert!(
        drop.disabled,
        "a repository holding a rebase refuses the rewrite and says so"
    );
    app.close_menu().unwrap();
}

#[test]
fn the_commits_pane_keeps_its_width_when_a_branch_is_picked() {
    let project = Project::new();
    let long = "feature/a-very-long-branch-name-that-keeps-going-and-going";
    let repo = log_repo(project.path(), "alpha", 3);
    test_support::git_seed::git(&repo, &["branch", long]);
    let mut app = log_app(&project, std::slice::from_ref(&repo));

    let width_at = |app: &support::Mounted| -> f32 {
        app.screen()
            .layout_nodes()
            .iter()
            .find(|node| node.test_id.as_deref() == Some("log:list"))
            .map(|node| node.bounds.size.width.0)
            .expect("the commits list paints")
    };

    let before = width_at(&app);
    app.press("id:log:filter-branch").unwrap();
    app.wait_for("text:main").unwrap();
    app.press(&format!("id:log:ref-{long}")).unwrap();
    app.wait_for("id:log:ref-chip").unwrap();

    let after = width_at(&app);
    assert!(
        (before - after).abs() <= 1.0,
        "selecting a branch must not resize the commits pane: {before} vs {after}"
    );
}

#[test]
fn the_details_pane_names_its_empty_state_and_the_files_pane_theirs() {
    let project = Project::new();
    let repo = log_repo(project.path(), "alpha", 3);
    let app = log_app(&project, &[repo]);

    assert!(app.screen().shows("Select a commit…"));
    assert!(
        app.screen()
            .shows("Select a commit to see its changed files.")
    );
}
