//! Bulk operations, ported from the egui `bulk_operations` suite: the fleet
//! strip, the preflight matrix with its skip reasons, the protected-branch
//! refusals, and the confirmed run's per-root outcomes.

use std::path::PathBuf;

use support::{Mounted, Project, for_roots, for_roots_seeded};

mod support;

fn repo(project: &Project, name: &str, dirty: bool) -> (PathBuf, PathBuf) {
    let work = support::seeded_repo(project.path(), name);
    let bare = project.path().join(format!("{name}.origin"));
    let bare_s = bare.to_string_lossy().to_string();
    test_support::git_seed::git(
        project.path(),
        &["init", "-q", "--bare", "-b", "main", &bare_s],
    );
    test_support::git_seed::git(&work, &["remote", "add", "origin", &bare_s]);
    test_support::git_seed::git(&work, &["push", "-q", "origin", "main"]);
    test_support::git_seed::git(&work, &["branch", "--set-upstream-to=origin/main", "main"]);
    if dirty {
        std::fs::write(work.join("README.md"), "uncommitted\n").expect("dirty file");
    }
    (work, bare)
}

fn bulk_tab(project: &Project, roots: &[PathBuf]) -> Mounted {
    let mut app = for_roots(project.path(), roots);
    app.first_render().expect("the shell mounts");
    support::open_surface(&mut app, "Bulk Operations");
    app.wait_for("id:route:bulk").expect("the bulk route");
    app
}

#[test]
fn the_fleet_strip_offers_the_operations_for_every_root() {
    let project = Project::new();
    let (alpha, _) = repo(&project, "alpha", false);
    let (ui, _) = repo(&project, "ui", true);
    let mut app = bulk_tab(&project, &[alpha, ui]);
    for label in [
        "Fetch all",
        "Pull all",
        "Push all",
        "Create branch",
        "Stash all",
        "Custom command",
        "Fleet history",
    ] {
        assert!(support::shows_part(&app, label), "{label} paints");
    }
    app.press("id:bulk:open:Fetch all")
        .expect("the fetch preflight");
    app.wait_for("text:Run on 2 of 2")
        .expect("the run control counts both roots");
    app.press("id:bulk:cancel").expect("cancel");
    app.wait_until(
        "the preflight closes",
        std::time::Duration::from_secs(5),
        |screen| !screen.matches("id:bulk:preflight").unwrap_or(false),
    )
    .expect("the preflight closes");
}

#[test]
fn the_preflight_matrix_shows_per_repo_status_and_skip_reasons() {
    let project = Project::new();
    let (alpha, _) = repo(&project, "alpha", false);
    let (ui, _) = repo(&project, "ui", true);
    let mut app = bulk_tab(&project, &[alpha, ui]);
    app.press("id:bulk:open:Pull all").expect("pull preflight");
    app.wait_for("id:bulk:preflight").unwrap();
    for text in [
        "1 of 2 will run",
        "1 will be skipped",
        "REPO",
        "CURRENT BRANCH",
        "STATUS",
        "OUTCOME",
        "Skipped (dirty worktree) — commit or stash first",
        "Merge upstream",
        "Dirty",
        "Clean",
        "Rebase onto upstream instead of merging",
    ] {
        assert!(support::shows_part(&app, text), "{text} paints");
    }
}

#[test]
fn a_rebasing_pull_refuses_every_protected_root() {
    let project = Project::new();
    let (alpha, _) = repo(&project, "alpha", false);
    let (ui, _) = repo(&project, "ui", true);
    let mut app = bulk_tab(&project, &[alpha, ui]);
    app.press("id:bulk:open:Pull all").expect("pull preflight");
    app.wait_for("id:bulk:preflight").unwrap();
    app.press("id:bulk:rebase-policy")
        .expect("the rebase policy control");
    app.wait_for("text:/Skipped \\(protected branch\\)/")
        .expect("the refusal paints");
    assert_eq!(
        app.screen()
            .texts()
            .iter()
            .filter(|text| text.contains("Skipped (protected branch) — the run may not touch it"))
            .count(),
        2,
        "one refusal row per root"
    );
}

#[test]
fn confirming_the_preflight_runs_the_fleet_and_reports_outcomes() {
    let project = Project::new();
    let (alpha, _) = repo(&project, "alpha", false);
    let (ui, _) = repo(&project, "ui", true);
    let mut app = bulk_tab(&project, &[alpha.clone(), ui.clone()]);
    app.press("id:bulk:open:Stash all")
        .expect("stash preflight");
    app.wait_for("text:Run on 1 of 2").expect("one root runs");
    app.press("id:bulk:run").expect("run");
    app.wait_for("text:/Stash all · 1 of 1 ok/")
        .expect("the run reports its outcome");
    assert!(
        test_support::git_seed::git(&ui, &["status", "--porcelain"])
            .trim()
            .is_empty(),
        "ui was stashed"
    );
    assert!(
        !test_support::git_seed::git(&ui, &["stash", "list"])
            .trim()
            .is_empty(),
        "ui's changes are in the stash"
    );
    assert!(
        test_support::git_seed::git(&alpha, &["stash", "list"])
            .trim()
            .is_empty(),
        "the clean root was skipped, not stashed"
    );
}

#[test]
fn a_destructive_custom_command_arms_before_it_runs() {
    let project = Project::new();
    let (alpha, _) = repo(&project, "alpha", false);
    let mut app = for_roots_seeded(project.path(), std::slice::from_ref(&alpha), |s| {
        s.open_bulk_preflight(turbogit_app::bulk_ops::BulkOp::Custom, &[]);
        s.ui.bulk_command = "git reset --hard".to_string();
    });
    app.first_render().expect("the shell mounts");
    app.wait_for("id:bulk:preflight").unwrap();
    assert!(
        support::shows_part(&app, "Destructive command"),
        "the warning paints"
    );
}
