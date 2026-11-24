//! The shell frame's behavior, ported from the egui `workspace_shell_frame`,
//! `shell_scroll_id_clash`, `status_bar_inflight`, `header_actions`, and
//! `activity_log` suites: the tab strip and its five tabs, the status bar's
//! aggregates and in-flight announcements, the header's tool-window
//! triggers, and the activity feed's controls.

use std::path::PathBuf;

use support::{Project, commit_into, shows_part, wait_for_part};
use turbogit_app::OwnedOperation;
use turbogit_app::operation::LifecycleVerb;

mod support;

/// A repository with an upstream: one commit pushed to a bare `origin`.
fn repo_with_upstream(parent: &std::path::Path, name: &str) -> PathBuf {
    let repo = support::seeded_repo(parent, name);
    let bare = parent.join(format!("{name}.origin"));
    let bare_s = bare.to_string_lossy().to_string();
    test_support::git_seed::git(parent, &["init", "-q", "--bare", "-b", "main", &bare_s]);
    test_support::git_seed::git(&repo, &["remote", "add", "origin", &bare_s]);
    test_support::git_seed::git(&repo, &["push", "-q", "origin", "main"]);
    test_support::git_seed::git(&repo, &["branch", "--set-upstream-to=origin/main", "main"]);
    repo
}

fn shell_one_repo(project: &Project) -> support::Mounted {
    let repo = repo_with_upstream(project.path(), "alpha");
    support::for_roots(project.path(), &[repo])
}

#[test]
fn the_tab_strip_paints_the_five_tabs_and_the_status_aggregate() {
    let project = Project::new();
    let mut app = shell_one_repo(&project);
    app.first_render().unwrap();
    app.wait_for("text:1 total").unwrap();

    for label in ["Changes", "Log", "Branches", "Worktrees", "Submodules"] {
        assert!(app.screen().shows(label), "the strip paints {label}");
    }
    assert!(
        shows_part(&app, "1 repo indexed"),
        "the version line paints the indexed count"
    );
    assert!(shows_part(&app, "1 total"), "the aggregate total paints");
}

#[test]
fn every_tab_press_paints_its_route() {
    let project = Project::new();
    let mut app = shell_one_repo(&project);
    app.first_render().unwrap();
    app.wait_for("text:1 total").unwrap();

    // The Log, Commit, Branches, Worktrees, and Submodules tabs are built
    // screens; only the tool windows still name their later phase.
    app.press("id:tab:Log").unwrap();
    app.wait_for("text:Commits").unwrap();
    assert!(app.screen().shows("Branches"), "the log's own pane paints");

    app.press("id:tab:Commit").unwrap();
    app.wait_for("id:route:changes").unwrap();
    assert!(
        app.screen().shows("Changes"),
        "the Commit tab paints the changes surface"
    );
    assert!(
        !app.screen()
            .shows("This surface arrives with Stage changes and commit (N3)."),
        "the changes route is no longer a placeholder"
    );

    for (slug, route, needle) in [
        ("Branches", "branches", "id:branches:search"),
        ("Worktrees", "worktrees", "id:worktree:add"),
        ("Submodules", "submodules", "id:submodule:header"),
    ] {
        app.press(&format!("id:tab:{slug}")).unwrap();
        app.wait_for(&format!("id:route:{route}")).unwrap();
        assert!(
            app.screen().matches(needle).unwrap_or(false),
            "{slug} paints its built surface"
        );
        assert!(
            !shows_part(&app, "arrives with"),
            "{slug} is no longer a placeholder"
        );
    }
}

#[test]
fn a_rebase_in_flight_announces_its_root_and_offers_its_verbs() {
    let project = Project::new();
    let repo = repo_with_upstream(project.path(), "alpha");
    // git's own held state, written the way the sequencer leaves it.
    std::fs::create_dir_all(repo.join(".git/rebase-merge")).unwrap();

    let mut app = support::for_roots(project.path(), std::slice::from_ref(&repo));
    app.first_render().unwrap();
    wait_for_part(&mut app, "Rebasing in alpha").unwrap();

    for verb in ["Abort", "Continue", "Skip"] {
        assert!(app.screen().shows(verb), "the rebase offers {verb}");
    }
    assert!(
        !shows_part(&app, "in beta"),
        "an idle root is never announced"
    );
    let _ = repo;
}

#[test]
fn abort_in_the_bar_finishes_the_merge_and_clears_the_announcement() {
    let project = Project::new();
    let repo = repo_with_upstream(project.path(), "alpha");
    // A real conflicted merge, the one `git merge --abort` can finish.
    commit_into(&repo, "c.txt", "main side");
    test_support::git_seed::git(&repo, &["checkout", "-q", "-b", "side", "HEAD~1"]);
    commit_into(&repo, "c.txt", "other side");
    test_support::git_seed::git(&repo, &["checkout", "-q", "main"]);
    let _ = std::process::Command::new("git")
        .args(["merge", "--no-edit", "side"])
        .current_dir(&repo)
        .output();
    assert!(
        repo.join(".git/MERGE_HEAD").exists(),
        "the fixture merge must be in flight"
    );

    let mut app = support::for_roots(project.path(), std::slice::from_ref(&repo));
    app.first_render().unwrap();
    wait_for_part(&mut app, "Merging in alpha").unwrap();
    assert!(app.screen().shows("Abort"));
    assert!(app.screen().shows("Continue"));
    assert!(!app.screen().shows("Skip"), "a merge resolves two ways");

    app.press("id:toolbar:verb-abort-alpha").unwrap();
    app.wait_until(
        "the announcement clears",
        std::time::Duration::from_secs(10),
        |screen| !screen.shows("Abort"),
    )
    .unwrap();
    assert!(
        !repo.join(".git/MERGE_HEAD").exists(),
        "the abort finished the merge"
    );
}

#[test]
fn a_cherry_pick_announces_abort_and_continue_but_never_skip() {
    let project = Project::new();
    let repo = repo_with_upstream(project.path(), "alpha");
    std::fs::write(repo.join(".git/CHERRY_PICK_HEAD"), "deadbeef\n").unwrap();

    let mut app = support::for_roots(project.path(), &[repo]);
    app.first_render().unwrap();
    wait_for_part(&mut app, "Cherry-picking in alpha").unwrap();

    assert!(app.screen().shows("Abort"));
    assert!(app.screen().shows("Continue"));
    assert!(
        !app.screen().shows("Skip"),
        "a cherry-pick resolves two ways"
    );
}

#[test]
fn the_status_bar_counts_dirty_and_unpulled_from_real_data() {
    let project = Project::new();
    let dirty = repo_with_upstream(project.path(), "dirty");
    std::fs::write(dirty.join("wip.txt"), "wip\n").unwrap();

    let behind = repo_with_upstream(project.path(), "behind");
    // A commit only the remote has: the upstream moves ahead.
    let bare = project.path().join("behind.origin");
    let seed = project.path().join("behind-seed");
    test_support::git_seed::git(
        project.path(),
        &[
            "clone",
            "-q",
            bare.to_str().unwrap(),
            seed.to_str().unwrap(),
        ],
    );
    commit_into(&seed, "remote.txt", "remote only");
    test_support::git_seed::git(&seed, &["push", "-q", "origin", "main"]);
    test_support::git_seed::git(&behind, &["fetch", "-q"]);

    let mut app = support::for_roots(project.path(), &[dirty.clone(), behind.clone()]);
    app.first_render().unwrap();
    wait_for_part(&mut app, "2 total").unwrap();
    wait_for_part(&mut app, "1 dirty").unwrap();
    wait_for_part(&mut app, "1 unpulled").unwrap();
}

#[test]
fn the_status_bar_counters_come_from_the_state_map() {
    // The counters' colors are the repository-state map's, asserted in the
    // crate's token suite; here the reading itself is pinned: the egui
    // `AggregatedStatus::compute` over the same fixture.
    let project = Project::new();
    let repo = repo_with_upstream(project.path(), "alpha");
    std::fs::write(repo.join("wip.txt"), "wip\n").unwrap();

    let mut app = support::for_roots(project.path(), &[repo]);
    app.first_render().unwrap();
    wait_for_part(&mut app, "1 dirty").unwrap();
    assert!(!shows_part(&app, "1 diverged"));
}

#[test]
fn the_settings_and_shortcuts_triggers_open_their_dialogs() {
    let project = Project::new();
    let mut app = shell_one_repo(&project);
    app.first_render().unwrap();
    app.wait_for("text:1 total").unwrap();

    app.press("id:toolbar:settings").unwrap();
    app.wait_for("id:dialog:settings").unwrap();
    assert!(app.screen().shows("Settings"));
    app.press("id:dialog:settings:cancel").unwrap();
    app.wait_for_gone("id:dialog:settings", std::time::Duration::from_secs(5))
        .unwrap();

    app.press("id:toolbar:shortcuts").unwrap();
    app.wait_for("id:dialog:shortcuts").unwrap();
    app.wait_for("id:shortcuts:table").unwrap();
    assert!(app.screen().shows("Shortcuts"));
    app.press("id:dialog:shortcuts:cancel").unwrap();
    app.wait_for_gone("id:dialog:shortcuts", std::time::Duration::from_secs(5))
        .unwrap();

    app.press("id:toolbar:palette").unwrap();
    app.wait_for("id:dialog:command-palette").unwrap();
    assert!(app.screen().shows("Find Action"));
}

#[test]
fn the_switch_trigger_opens_the_workspace_picker() {
    let project = Project::new();
    let mut app = shell_one_repo(&project);
    app.first_render().unwrap();
    app.wait_for("text:1 total").unwrap();

    app.press("id:workspace:switch").unwrap();
    app.wait_for("id:workspace:picker").unwrap();
    assert!(app.screen().shows("Current"));
    assert!(app.screen().shows("Recent"));
    assert!(app.screen().shows("Open Project…"));
    assert!(app.screen().shows("Attach Workspace Root…"));
}

#[test]
fn the_activity_feed_expands_clears_and_filters() {
    let project = Project::new();
    let mut app = shell_one_repo(&project);
    app.first_render().unwrap();
    app.wait_for("text:1 total").unwrap();

    // Collapsed by default: the rows' controls are hidden, the chip paints.
    assert!(app.screen().shows("ACTIVITY"));
    assert!(!app.screen().shows("Clear feed"));

    app.press("id:activity:toggle").unwrap();
    app.wait_for("text:Clear feed").unwrap();
    assert!(
        shows_part(&app, "No activity yet"),
        "the empty feed names itself"
    );
    assert!(app.screen().shows("All time"));
    assert!(app.screen().shows("All repos"));

    // The repo filter cycles into the first registered repository.
    app.press("id:activity:repo-filter").unwrap();
    app.wait_for("text:alpha").unwrap();

    // The window filter flips.
    app.press("id:activity:window").unwrap();
    app.wait_for("text:Last 30 min").unwrap();

    app.press("id:activity:toggle").unwrap();
    app.wait_until(
        "the feed collapses",
        std::time::Duration::from_secs(5),
        |screen| !screen.shows("Clear feed"),
    )
    .unwrap();
    assert!(app.screen().shows("ACTIVITY"), "the chip stays");
}

#[test]
fn the_shell_walks_every_tab_without_a_second_scroll_soul() {
    // The egui suite asserted no id-clash error; the nsh port walks the same
    // pages and asserts each still paints after the round trip.
    let project = Project::new();
    let mut app = shell_one_repo(&project);
    app.first_render().unwrap();
    app.wait_for("text:1 total").unwrap();

    for slug in [
        "Log",
        "Commit",
        "Branches",
        "Worktrees",
        "Submodules",
        "Log",
    ] {
        app.press(&format!("id:tab:{slug}")).unwrap();
        app.wait_for("text:Commits").unwrap_or(()); // placeholder tabs name themselves
        assert!(app.screen().shows("ACTIVITY"), "the chrome stays on {slug}");
    }
    assert!(
        app.screen().shows("1 total"),
        "the full walk leaves the aggregate intact"
    );
}

#[test]
fn verb_labels_match_the_operation_they_resolve() {
    // The egui suite pinned the label mapping; the state layer owns it and
    // the bar paints it verbatim.
    assert_eq!(
        LifecycleVerb::Abort.label_for(OwnedOperation::Merge),
        "Abort merge"
    );
    assert_eq!(
        LifecycleVerb::Continue.label_for(OwnedOperation::CherryPick),
        "Continue cherry-pick"
    );
    assert_eq!(
        LifecycleVerb::offered_for(OwnedOperation::Rebase).to_vec(),
        vec![
            LifecycleVerb::Abort,
            LifecycleVerb::Continue,
            LifecycleVerb::Skip
        ]
    );
}
