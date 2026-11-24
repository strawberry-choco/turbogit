//! The blame surface, ported from the egui `blame_view` suite at the
//! fidelity the nsh ports keep: the waiting, empty, and failed verdicts, the
//! per-line attribution and its age, the log jump, and the footer trigger.

use std::path::{Path, PathBuf};
use std::time::Duration;

use support::{Mounted, Project, for_roots_seeded, shows_part, wait_for_enabled, wait_for_part};
use test_support::git_seed::{commit, git};
use turbogit_app::state::{AppState, BlameTarget, Tab};

mod support;

fn commit_file(dir: &Path, name: &str, body: &str, message: &str) -> String {
    commit(dir, name, body, message);
    git(dir, &["rev-parse", "HEAD"]).trim().to_owned()
}

struct Seed {
    project: PathBuf,
    repo: PathBuf,
    c1: String,
    c2: String,
}

fn seeded(project: &Project) -> Seed {
    let repo = project.path().join("alpha");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    let c1 = commit_file(
        &repo,
        "file.txt",
        "alpha: initial commit\n",
        "alpha: initial commit",
    );
    let c2 = commit_file(
        &repo,
        "file.txt",
        "alpha: initial commit\nalpha: second commit\n",
        "alpha: second commit",
    );
    Seed {
        project: project.path().to_path_buf(),
        repo,
        c1,
        c2,
    }
}

fn blame_app(
    seed: &Seed,
    target: impl FnOnce(&AppState) -> BlameTarget + Send + 'static,
) -> Mounted {
    let project = seed.project.clone();
    let repo = seed.repo.clone();
    for_roots_seeded(&project, &[repo], move |s| {
        s.ui.tab = Tab::Log;
        let target = target(s);
        s.ui.selected_commit = Some((target.root.clone(), target.rev.clone()));
        s.ui.log_selected_file = Some((target.root.clone(), target.path.clone()));
        s.ui.blame = Some(target);
    })
}

fn target(
    seed: &Seed,
    rev: &str,
    path: &str,
) -> impl FnOnce(&AppState) -> BlameTarget + Send + 'static {
    let rev = rev.to_string();
    let path = seed.repo.join(path);
    move |s| BlameTarget {
        root: s.multi.roots[0].id.clone(),
        path: path.clone(),
        rev: rev.clone(),
    }
}

#[test]
fn blame_paints_the_waiting_presentation_on_the_first_frame() {
    let project = Project::new();
    let seed = seeded(&project);
    let mut app = blame_app(&seed, target(&seed, &seed.c2, "file.txt"));
    app.first_render().unwrap();
    assert!(
        shows_part(&app, "Computing blame…"),
        "the keyed read's waiting text paints before any answer is drained"
    );
}

#[test]
fn blame_paints_a_failed_reads_message() {
    let project = Project::new();
    let seed = seeded(&project);
    let mut app = blame_app(&seed, target(&seed, &seed.c2, "missing.txt"));
    app.first_render().unwrap();
    wait_for_part(&mut app, "no such path").unwrap();
    assert!(
        !shows_part(&app, "Computing blame…"),
        "the failure replaces the waiting text"
    );
    assert!(
        !shows_part(&app, "No blame lines for this revision."),
        "a failure is not an empty answer"
    );
}

#[test]
fn blame_paints_a_page_owned_empty_message_for_an_empty_file() {
    let project = Project::new();
    let seed = seeded(&project);
    let empty = commit_file(&seed.repo, "empty.txt", "", "empty file");
    let mut app = blame_app(&seed, target(&seed, &empty, "empty.txt"));
    app.first_render().unwrap();
    wait_for_part(&mut app, "No blame lines for this revision.").unwrap();
}

#[test]
fn blame_paints_per_line_attribution_and_its_age() {
    let project = Project::new();
    let seed = seeded(&project);
    let mut app = blame_app(&seed, target(&seed, &seed.c2, "file.txt"));
    app.first_render().unwrap();
    app.wait_for("id:dialog:blame").unwrap();
    app.wait_for("id:blame:row:1").unwrap();
    app.wait_for("id:blame:row:2").unwrap();

    for label in ["HASH", "AUTHOR", "LINE", "AGE"] {
        assert!(app.screen().shows(label), "the {label} column paints");
    }
    assert!(shows_part(&app, "file.txt @"));
    assert!(shows_part(&app, "alpha: second commit"));
    assert!(shows_part(&app, "Test"));
    assert!(
        app.screen()
            .texts()
            .iter()
            .any(|text| text.ends_with("s ago") || text.ends_with("m ago")),
        "a relative age paints: {:?}",
        app.screen().texts()
    );
    assert!(
        app.screen()
            .texts()
            .iter()
            .any(|text| text.starts_with(&seed.c1[..7])),
        "the line from the older commit names its hash"
    );
}

#[test]
fn clicking_a_blamed_line_closes_blame_and_selects_its_commit_in_the_log() {
    let project = Project::new();
    let seed = seeded(&project);
    let mut app = blame_app(&seed, target(&seed, &seed.c2, "file.txt"));
    app.first_render().unwrap();
    app.wait_for("id:blame:row:1").unwrap();
    app.press("id:blame:row:1").unwrap();
    app.wait_for_gone("id:dialog:blame", Duration::from_secs(5))
        .unwrap();
    app.wait_for("text:Commits").unwrap();
}

#[test]
fn the_log_file_footer_opens_blame_for_the_selection() {
    let project = Project::new();
    let seed = seeded(&project);
    let mut app = support::for_roots(&seed.project, std::slice::from_ref(&seed.repo));
    app.first_render().unwrap();
    app.press("id:tab:Log").unwrap();
    app.wait_for("id:log:list").unwrap();
    let row = format!("id:row:alpha:{}", &seed.c2[..7]);
    app.wait_for(&row).unwrap();
    app.press(&row).unwrap();
    let file = "id:file:alpha:file.txt";
    app.wait_for(file).unwrap();
    app.press(file).unwrap();
    wait_for_enabled(&mut app, "id:log:file-blame").unwrap();
    app.press("id:log:file-blame").unwrap();
    app.wait_for("id:dialog:blame").unwrap();
    app.wait_for("id:blame:row:2").unwrap();
}

#[test]
fn the_route_paints_the_body_when_a_target_is_seeded() {
    let project = Project::new();
    let seed = seeded(&project);
    let mut app = blame_app(&seed, target(&seed, &seed.c2, "file.txt"));
    app.first_render().unwrap();
    support::open_surface(&mut app, "Blame");
    app.wait_for("id:route:blame").unwrap();
    app.wait_for("id:blame:body").unwrap();
}
