//! The commit card and the commit view with CJK text, ported from the egui
//! `commit_view_cjk_text` suite: Chinese content previews, a Chinese subject
//! and body commit byte for byte, the multi-line well keeps its newlines, and
//! the log row and the recent chip paint the subject the developer typed.

use std::path::{Path, PathBuf};

use support::{Project, commit_tab, open_group, shows_part};

mod support;

use test_support::git_seed::git;

const SUBJECT: &str = "修复提交视图无法显示中文";
const BODY: &str = "第二行是一段说明";

/// A repository with a Chinese file name, committed, and a Chinese edit
/// already in the index: the commit card has a target without the changes
/// list's row being pressed.
fn chinese_repo(project: &Project) -> PathBuf {
    let repo = project.path().join("中文仓库");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    std::fs::write(repo.join("说明.md"), "第一行\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "初始提交"]);
    std::fs::write(repo.join("说明.md"), "第一行\n第二行\n").unwrap();
    git(&repo, &["add", "说明.md"]);
    repo
}

/// An ASCII-named file whose content is Chinese: a path the status layer
/// quotes like any other would otherwise hide the row (the quoting is an
/// engine-side finding, not a commit-card one).
fn ascii_repo(project: &Project) -> PathBuf {
    let repo = project.path().join("alpha");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    std::fs::write(repo.join("notes.txt"), "第一行\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "初始提交"]);
    std::fs::write(repo.join("notes.txt"), "第一行\n第二行\n").unwrap();
    repo
}

fn commit_app(project: &Project, repo: &Path) -> support::Mounted {
    let name = repo.file_name().unwrap().to_string_lossy().to_string();
    let mut app = commit_tab(project.path(), std::slice::from_ref(&repo.to_path_buf()));
    open_group(&mut app, &name);
    app
}

fn last_message(repo: &Path) -> String {
    git(repo, &["log", "-1", "--pretty=%B"])
}

fn type_and_commit(app: &mut support::Mounted, repo: &Path, message: &str) {
    app.type_text("id:commit:message", message).unwrap();
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
        |_| last_message(repo).contains(message.lines().next().unwrap_or("")),
    )
    .unwrap();
}

#[test]
fn chinese_content_paints_and_previews_its_own_lines() {
    let project = Project::new();
    let repo = ascii_repo(&project);
    let mut app = commit_app(&project, &repo);
    app.wait_for("id:changes:row:alpha:notes.txt")
        .expect("the changed row paints");
    app.press("id:changes:row:alpha:notes.txt")
        .expect("the row previews");
    app.wait_until(
        "the preview paints the file's own Chinese line",
        std::time::Duration::from_secs(15),
        |screen| screen.texts().iter().any(|text| text.contains("第二行")),
    )
    .unwrap();
}

#[test]
fn a_chinese_subject_and_body_commit_byte_for_byte_and_paint_in_the_log() {
    let project = Project::new();
    let repo = chinese_repo(&project);
    let mut app = commit_app(&project, &repo);
    app.wait_for("id:commit:message").unwrap();
    type_and_commit(&mut app, &repo, &format!("{SUBJECT}\n\n{BODY}"));

    let message = last_message(&repo);
    assert_eq!(
        message.lines().next(),
        Some(SUBJECT),
        "the subject is the first line, exactly: {message:?}"
    );
    assert!(
        message.contains(BODY),
        "the body keeps its own line: {message:?}"
    );
    assert_eq!(
        message.lines().nth(1),
        Some(""),
        "the blank line the developer typed survives: {message:?}"
    );

    app.press("id:tab:Log").unwrap();
    app.wait_for("text:Commits").unwrap();
    app.wait_until(
        "the log row paints the subject",
        std::time::Duration::from_secs(15),
        |screen| screen.texts().iter().any(|text| text.contains(SUBJECT)),
    )
    .unwrap();
}

#[test]
fn the_newlines_typed_into_the_well_survive_one_commit() {
    let project = Project::new();
    let repo = chinese_repo(&project);
    let mut app = commit_app(&project, &repo);
    app.wait_for("id:commit:message").unwrap();

    app.type_text("id:commit:message", "subject line\n\nbody paragraph")
        .unwrap();
    assert_eq!(
        app.screen().input_text("id:commit:message").unwrap(),
        Some("subject line\n\nbody paragraph"),
        "the well keeps the newlines the typist pressed"
    );
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
        |_| last_message(&repo).contains("body paragraph"),
    )
    .unwrap();
    let message = last_message(&repo);
    assert_eq!(
        message.lines().next(),
        Some("subject line"),
        "the subject stays one line: {message:?}"
    );
    assert_eq!(
        message.lines().nth(1),
        Some(""),
        "and the blank separator line survives: {message:?}"
    );
}

#[test]
fn the_recent_row_carries_the_chinese_message_back_into_the_well() {
    let project = Project::new();
    let repo = chinese_repo(&project);
    let mut app = commit_app(&project, &repo);
    app.wait_for("id:commit:message").unwrap();
    type_and_commit(&mut app, &repo, SUBJECT);
    app.wait_for("text:Recent:").unwrap();

    assert!(
        shows_part(&app, SUBJECT),
        "the recent row paints the Chinese subject it will put back: {:?}",
        app.screen().texts()
    );
}
