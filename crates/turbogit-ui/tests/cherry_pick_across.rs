//! Cherry-pick across repositories, ported from the egui
//! `cherry_pick_across` suite: the source checklist, the patch preview, the
//! target forecasts, the search, and the run that lands in the target root.

use std::path::PathBuf;
use std::time::Duration;

use support::{Mounted, Project, for_roots};
use test_support::git_seed::{commit, git};

mod support;

fn project_with_commits(project: &Project) -> (PathBuf, PathBuf) {
    let alpha = project.path().join("alpha");
    std::fs::create_dir_all(&alpha).unwrap();
    git(&alpha, &["init", "-q", "-b", "main"]);
    git(&alpha, &["config", "user.email", "t@t"]);
    git(&alpha, &["config", "user.name", "t"]);
    commit(&alpha, "a.txt", "one\n", "alpha: first commit");
    commit(&alpha, "b.txt", "two\n", "alpha: second commit");
    commit(&alpha, "c.txt", "three\n", "alpha: third commit");
    let beta = project.path().join("beta");
    std::fs::create_dir_all(&beta).unwrap();
    git(&beta, &["init", "-q", "-b", "main"]);
    git(&beta, &["config", "user.email", "t@t"]);
    git(&beta, &["config", "user.name", "t"]);
    commit(&beta, "base.txt", "base\n", "beta: base commit");
    (alpha, beta)
}

fn across(project: &Project, roots: &[PathBuf]) -> Mounted {
    let mut app = for_roots(project.path(), roots);
    app.first_render().expect("the shell mounts");
    support::open_surface(&mut app, "Cherry-pick");
    app.wait_for("id:route:cherry_pick")
        .expect("the cherry route");
    app.press("id:cherry:open-across").expect("open across");
    app.wait_for("id:dialog:cherry-pick-across")
        .expect("the picker paints");
    app
}

#[test]
fn the_dialog_lists_commits_targets_and_predictions_with_a_preview_rail() {
    let project = Project::new();
    let (alpha, beta) = project_with_commits(&project);
    let app = across(&project, &[alpha, beta]);
    for text in [
        "alpha: first commit",
        "alpha: second commit",
        "alpha: third commit",
        "will apply in order",
        "APPLIES",
        "RISK",
        "OUTCOME",
        "beta",
        "Stop on first conflict",
        "PATCH PREVIEW",
        "TARGETS",
    ] {
        assert!(support::shows_part(&app, text), "{text} paints");
    }
}

#[test]
fn selecting_a_commit_shows_its_patch_and_the_target_forecast() {
    let project = Project::new();
    let (alpha, beta) = project_with_commits(&project);
    let mut app = across(&project, &[alpha, beta]);
    app.press("text:/alpha: second commit/")
        .expect("the commit row selects");
    app.wait_for("text:/1 of 3 selected/")
        .expect("the selection counter follows");
    app.wait_for("text:b.txt")
        .expect("the patch preview names its file");
    assert!(support::shows_part(&app, "+two"), "the added line paints");
    app.wait_for("text:1/1")
        .expect("the target applies the pick");
}

#[test]
fn the_search_filters_the_commit_checklist() {
    let project = Project::new();
    let (alpha, beta) = project_with_commits(&project);
    let mut app = across(&project, &[alpha, beta]);
    app.paste("id:cherry:search", &nsh_core::PasteEvent::new("second"))
        .expect("the search takes text");
    app.wait_until(
        "the non-matching commit leaves",
        Duration::from_secs(5),
        |screen| !screen.matches("text:/alpha: first commit/").unwrap_or(true),
    )
    .expect("non-matching commits leave the checklist");
    assert!(
        app.screen()
            .matches("text:/alpha: second commit/")
            .unwrap_or(false),
        "the matching commit remains"
    );
}

#[test]
fn executing_lands_the_pick_in_the_target_root_only() {
    let project = Project::new();
    let (alpha, beta) = project_with_commits(&project);
    let mut app = across(&project, &[alpha.clone(), beta.clone()]);
    app.press("text:/alpha: second commit/")
        .expect("the commit row selects");
    app.wait_for("text:/1 of 3 selected/").unwrap();
    app.press("id:cherry:run").expect("run");
    app.wait_for("id:bulk:cherry-monitor")
        .expect("the live monitor opens");
    assert!(
        test_support::wait_until(20_000, || {
            git(&beta, &["log", "--format=%s"]).contains("alpha: second commit")
        }),
        "the target gained the picked commit"
    );
    assert!(
        !git(&alpha, &["log", "--format=%s"]).contains("alpha: second commit\nbeta"),
        "the source root was not rewritten"
    );
    app.wait_until("the row settles Done", Duration::from_secs(20), |screen| {
        screen.texts().contains(&"Done")
    })
    .expect("the monitor reports Done");
}
