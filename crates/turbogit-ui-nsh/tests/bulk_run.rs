//! The bulk live monitor, ported from the egui `bulk_run` suite: the fleet
//! progress, the failed row's Resolve, retry-skipped, and the close.

use std::path::PathBuf;
use std::time::Duration;

use support::{Mounted, Project, for_roots};
use test_support::git_seed::git;

mod support;

fn repo(project: &Project, name: &str, dirty: bool) -> PathBuf {
    let work = support::seeded_repo(project.path(), name);
    let bare = project.path().join(format!("{name}.origin"));
    let bare_s = bare.to_string_lossy().to_string();
    git(
        project.path(),
        &["init", "-q", "--bare", "-b", "main", &bare_s],
    );
    git(&work, &["remote", "add", "origin", &bare_s]);
    git(&work, &["push", "-q", "origin", "main"]);
    git(&work, &["branch", "--set-upstream-to=origin/main", "main"]);
    if dirty {
        std::fs::write(work.join("README.md"), "uncommitted\n").expect("dirty file");
    }
    work
}

fn bulk_tab(project: &Project, roots: &[PathBuf]) -> Mounted {
    let mut app = for_roots(project.path(), roots);
    app.first_render().expect("the shell mounts");
    app.press("id:toolbar:n5-bulk").expect("the bulk band");
    app.wait_for("id:route:bulk").expect("the bulk route");
    app
}

fn wait_text(app: &mut Mounted, needle: &str) {
    app.wait_until(
        &format!("text containing {needle}"),
        Duration::from_secs(20),
        |screen| screen.texts().iter().any(|text| text.contains(needle)),
    )
    .unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn the_monitor_tracks_the_fleet_and_closes_on_demand() {
    let project = Project::new();
    let alpha = repo(&project, "alpha", false);
    let mut app = bulk_tab(&project, std::slice::from_ref(&alpha));
    app.press("id:bulk:open:Pull all").expect("pull preflight");
    app.wait_for("text:Run on 1 of 1").unwrap();
    app.press("id:bulk:run").expect("run");
    app.wait_for("id:bulk:monitor").expect("the monitor opens");
    wait_text(&mut app, "Done");
    assert!(support::shows_part(&app, "1 done"));
    assert!(
        app.screen().matches("id:bulk:stop").unwrap_or(false),
        "Stop remaining paints"
    );
    app.press("id:bulk:close-monitor").expect("close");
    app.wait_until("the monitor closes", Duration::from_secs(5), |screen| {
        !screen.matches("id:bulk:monitor").unwrap_or(false)
    })
    .expect("the monitor closes");
}

#[test]
fn a_failed_row_exposes_resolve_that_jumps_to_the_repo() {
    let project = Project::new();
    let alpha = repo(&project, "alpha", false);
    git(
        &alpha,
        &["remote", "set-url", "origin", "/nonexistent/broken"],
    );
    let mut app = bulk_tab(&project, std::slice::from_ref(&alpha));
    app.press("id:bulk:open:Fetch all")
        .expect("fetch preflight");
    app.wait_for("text:Run on 1 of 1").unwrap();
    app.press("id:bulk:run").expect("run");
    app.wait_for("id:bulk:monitor").unwrap();
    wait_text(&mut app, "Failed");
    assert!(
        support::shows_part(&app, "Resolve alpha"),
        "the failed row offers Resolve"
    );
    app.press("id:bulk:resolve:alpha").expect("resolve");
    app.wait_until("the monitor closes", Duration::from_secs(5), |screen| {
        !screen.matches("id:bulk:monitor").unwrap_or(false)
    })
    .expect("Resolve closes the monitor");
}

#[test]
fn retry_skipped_redispatches_the_skipped_repo() {
    let project = Project::new();
    let alpha = repo(&project, "alpha", false);
    let ui = repo(&project, "ui", true);
    let mut app = bulk_tab(&project, &[alpha, ui]);
    app.press("id:bulk:open:Stash all")
        .expect("stash preflight");
    app.wait_for("text:Run on 1 of 2").unwrap();
    app.press("id:bulk:run").expect("run");
    app.wait_for("id:bulk:monitor").unwrap();
    wait_text(&mut app, "Skipped (clean tree)");
    wait_text(&mut app, "Retry 1 skipped");
    app.press("id:bulk:retry").expect("retry");
    wait_text(
        &mut app,
        "2 done · 0 running · 0 queued · 0 skipped · 0 failed",
    );
}

#[test]
fn resolve_on_a_conflicted_repo_opens_the_merge_editor() {
    let project = Project::new();
    let alpha = repo(&project, "alpha", false);
    git(&alpha, &["checkout", "-q", "-b", "other"]);
    std::fs::write(alpha.join("README.md"), "theirs\n").unwrap();
    git(&alpha, &["commit", "-qam", "theirs"]);
    git(&alpha, &["checkout", "-q", "main"]);
    std::fs::write(alpha.join("README.md"), "ours\n").unwrap();
    git(&alpha, &["commit", "-qam", "ours"]);
    let _ = std::process::Command::new("git")
        .args(["merge", "other"])
        .current_dir(&alpha)
        .output();
    git(
        &alpha,
        &["remote", "set-url", "origin", "/nonexistent/broken"],
    );

    let mut app = bulk_tab(&project, std::slice::from_ref(&alpha));
    app.press("id:bulk:open:Fetch all")
        .expect("fetch preflight");
    app.wait_for("text:Run on 1 of 1").unwrap();
    app.press("id:bulk:run").expect("run");
    app.wait_for("id:bulk:monitor").unwrap();
    wait_text(&mut app, "Failed");
    app.press("id:bulk:resolve:alpha").expect("resolve");
    app.wait_for("id:dialog:merge-editor")
        .expect("the merge editor opens");
    assert!(support::shows_part(&app, "Merge: README.md"));
}
