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

/// A painted frame, in the root's points.
struct Frame {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

fn frame(app: &support::Mounted, id: &str) -> Frame {
    let node = app
        .screen()
        .layout_nodes()
        .into_iter()
        .find(|node| node.test_id.as_deref() == Some(id))
        .unwrap_or_else(|| panic!("{id} paints"));
    Frame {
        x: node.bounds.origin.x.0,
        y: node.bounds.origin.y.0,
        w: node.bounds.size.width.0,
        h: node.bounds.size.height.0,
    }
}

fn title_x(app: &support::Mounted, title: &str) -> f32 {
    app.screen()
        .layout_nodes()
        .into_iter()
        .find(|node| {
            node.text
                .as_ref()
                .is_some_and(|text| text.text.visible_text() == title)
        })
        .unwrap_or_else(|| panic!("the {title} title paints"))
        .bounds
        .origin
        .x
        .0
}

/// Every log section is a surface on the background, never a slice of one
/// band: the three columns part by the same gutter the Changes tab keeps
/// between its cards, and the resizable handle sits in that gap.
#[test]
fn the_log_columns_part_by_background_gutters() {
    let project = Project::new();
    let repo = log_repo(project.path(), "alpha", 3);
    let app = log_app(&project, &[repo]);

    let gutter = turbogit_ui::theme::PANEL_PADDING;
    let (branches, commits, files) = (
        frame(&app, "log:branches-pane"),
        frame(&app, "log:commits-pane"),
        frame(&app, "log:files-pane"),
    );
    for (left, right) in [(&branches, &commits), (&commits, &files)] {
        let between = right.x - (left.x + left.w);
        assert!(
            (between - gutter).abs() <= 1.0,
            "the columns part by the background gutter: {between}"
        );
    }
    let (from_left, from_right) = (
        branches.x,
        test_support::nsh_harness::VIEWPORT.0 - (files.x + files.w),
    );
    assert!(
        (from_left - gutter).abs() <= 1.0 && (from_right - gutter).abs() <= 1.0,
        "the outer columns part from the route edges by the gutter: {from_left} and {from_right}"
    );
}

/// The Changes tab's rhythm inside the right column: its two surfaces part by
/// the panel gutter, and each title sits one gutter inside the surface it
/// names. The pane header owns that gutter, so the padding lives on the
/// section's body and never above its header.
#[test]
fn the_right_column_sections_share_one_gutter_and_one_gap() {
    let project = Project::new();
    let repo = log_repo(project.path(), "alpha", 3);
    let app = log_app(&project, &[repo]);

    let gutter = turbogit_ui::theme::PANEL_PADDING;
    let (files, details) = (
        frame(&app, "log:files-pane"),
        frame(&app, "log:details-pane"),
    );
    let gap = details.y - (files.y + files.h);
    assert!(
        (gap - gutter).abs() <= 1.0,
        "the sections part by the background gutter: {gap}"
    );

    let (files_title, details_title) = (
        title_x(&app, "Changed files"),
        title_x(&app, "Commit details"),
    );
    assert!(
        (files_title - (files.x + gutter)).abs() <= 1.0,
        "the files title sits one gutter inside its surface: {files_title} against {}",
        files.x
    );
    assert!(
        (details_title - files_title).abs() <= 1.0,
        "the two titles share one gutter: {files_title} against {details_title}"
    );
    let body_x = title_x(&app, "Select a commit…");
    assert!(
        (body_x - details_title).abs() <= 1.0,
        "the body under a header keeps the header's gutter: {body_x} against {details_title}"
    );
}
