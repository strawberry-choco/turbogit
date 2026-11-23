//! The tag dialog, ported from the egui `tag_dialog` suite: the scope, the
//! live name validation, the annotated fields, the lightweight type, the
//! push option, and the dispatch the Create button makes.

use std::path::PathBuf;
use std::time::Duration;

use support::{Project, branches_tab, shows_part};
use test_support::git_seed::git;

mod support;

fn two_commit_repo(project: &Project, name: &str) -> PathBuf {
    let repo = support::seeded_repo(project.path(), name);
    support::commit_into(&repo, "second.txt", "second commit");
    repo
}

fn open_tag(app: &mut support::Mounted) {
    app.press("id:branches:tag").unwrap();
    app.wait_for("id:dialog:tag").unwrap();
}

#[test]
fn the_dialog_paints_the_scope_name_target_and_type() {
    let project = Project::new();
    let repo = two_commit_repo(&project, "alpha");
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    open_tag(&mut app);
    for id in [
        "tag:name",
        "tag:target",
        "tag:type-Lightweight",
        "tag:type-Annotated",
        "tag:message",
        "tag:tagger",
        "tag:sign",
        "tag:push",
    ] {
        assert!(
            app.screen()
                .matches(&format!("id:dialog:{id}"))
                .unwrap_or(false),
            "the dialog paints {id}"
        );
    }
    assert!(shows_part(&app, "Include alpha"));
}

#[test]
fn name_validation_is_live_and_a_duplicate_disables_the_create() {
    let project = Project::new();
    let repo = two_commit_repo(&project, "alpha");
    git(&repo, &["tag", "v1.0"]);
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    open_tag(&mut app);
    assert!(
        shows_part(&app, "A tag name is required"),
        "the empty reason"
    );

    app.type_text("id:dialog:tag:name", "bad name").unwrap();
    app.wait_until("the space is named", Duration::from_secs(10), |screen| {
        screen
            .texts()
            .iter()
            .any(|t| t.contains("cannot contain a space"))
    })
    .unwrap();

    app.clear("id:dialog:tag:name").unwrap();
    app.type_text("id:dialog:tag:name", "v1.0").unwrap();
    app.wait_until(
        "the duplicate is named",
        Duration::from_secs(10),
        |screen| screen.texts().iter().any(|t| t.contains("already exists")),
    )
    .unwrap();
}

#[test]
fn an_annotated_tag_reaches_git_with_its_message_and_tagger() {
    let project = Project::new();
    let repo = two_commit_repo(&project, "alpha");
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    open_tag(&mut app);
    app.type_text("id:dialog:tag:name", "v2.0").unwrap();
    app.type_text("id:dialog:tag:message", "release notes")
        .unwrap();
    app.type_text("id:dialog:tag:tagger", "Test <test@example.com>")
        .unwrap();
    app.press("id:dialog:tag:action").unwrap();

    for _ in 0..400 {
        if git(&repo, &["tag", "--list", "v2.0"]).contains("v2.0") {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let kind = git(&repo, &["cat-file", "-t", "refs/tags/v2.0"]);
    assert_eq!(kind.trim(), "tag", "an annotated tag object was created");
    let body = git(&repo, &["cat-file", "tag", "refs/tags/v2.0"]);
    assert!(body.contains("release notes"), "the message is carried");
    assert!(
        body.contains("Test <test@example.com>"),
        "the tagger is carried"
    );
}

#[test]
fn the_lightweight_type_hides_the_annotated_fields_and_creates_a_ref() {
    let project = Project::new();
    let repo = two_commit_repo(&project, "alpha");
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    open_tag(&mut app);
    app.press("id:dialog:tag:type-Lightweight").unwrap();
    app.wait_until(
        "the annotated fields leave",
        Duration::from_secs(10),
        |screen| {
            !screen.matches("id:dialog:tag:message").unwrap_or(false)
                && !screen.matches("id:dialog:tag:tagger").unwrap_or(false)
        },
    )
    .unwrap();
    app.type_text("id:dialog:tag:name", "light").unwrap();
    app.press("id:dialog:tag:action").unwrap();

    for _ in 0..400 {
        if git(&repo, &["tag", "--list", "light"]).contains("light") {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let kind = git(&repo, &["cat-file", "-t", "refs/tags/light"]);
    assert_eq!(
        kind.trim(),
        "commit",
        "a lightweight tag points at the commit"
    );
}

#[test]
fn push_immediately_pushes_to_origin() {
    let project = Project::new();
    let repo = two_commit_repo(&project, "alpha");
    let bare = project.path().join("origin.git");
    let bare_s = bare.to_string_lossy().to_string();
    git(
        project.path(),
        &["clone", "--bare", "-q", repo.to_str().unwrap(), &bare_s],
    );
    git(&repo, &["remote", "add", "origin", &bare_s]);
    git(&repo, &["push", "-q", "-u", "origin", "main"]);

    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    open_tag(&mut app);
    app.press("id:dialog:tag:type-Lightweight").unwrap();
    app.type_text("id:dialog:tag:name", "v3.0").unwrap();
    app.press("id:dialog:tag:push").unwrap();
    app.press("id:dialog:tag:action").unwrap();

    for _ in 0..600 {
        if git(&bare, &["tag", "--list", "v3.0"]).contains("v3.0") {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("the pushed tag never reached origin");
}
