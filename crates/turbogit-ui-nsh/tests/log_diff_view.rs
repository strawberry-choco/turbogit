//! The diff opened from the log, ported from the egui `log_diff_view` suite:
//! a commit's file opens its own historical comparison in the diff window,
//! offers no staging controls, never answers a staged-ness question, closes
//! on its own verb, and a root commit's diff does not carry the worktree's
//! later dirt.

use std::path::{Path, PathBuf};

use support::{Project, for_roots, log_repo};

mod support;

use test_support::git_seed::{commit, git};

const ADDED_LINE: &str = "zz-added-by-second-commit";

fn commit_file(dir: &Path, name: &str, body: &str, message: &str) {
    commit(dir, name, body, message);
}

fn seeded_project(project: &Project) -> PathBuf {
    let alpha = project.path().join("alpha");
    std::fs::create_dir_all(&alpha).unwrap();
    git(&alpha, &["init", "-q", "-b", "main"]);
    git(&alpha, &["config", "user.email", "test@example.com"]);
    git(&alpha, &["config", "user.name", "Test"]);
    commit_file(
        &alpha,
        "tracked.txt",
        "first line\n",
        "message of the first commit",
    );
    commit_file(
        &alpha,
        "tracked.txt",
        &format!("first line\n{ADDED_LINE}\n"),
        "message of the second commit",
    );
    commit_file(&alpha, "README.md", "docs", "message of the docs commit");
    alpha
}

fn log_app(project: &Project, repo: &Path) -> support::Mounted {
    let mut app = for_roots(project.path(), &[repo.to_path_buf()]);
    app.first_render().unwrap();
    app.press("id:tab:Log").unwrap();
    app.wait_for("text:Commits").unwrap();
    app
}

fn open_commit_file(app: &mut support::Mounted, subject: &str) {
    app.press(&format!("text:{subject}")).unwrap();
    app.wait_for("id:log:file-diff")
        .expect("the commit's files pane paints");
    app.press("id:log:file-diff").unwrap();
    app.wait_for("id:diff:window")
        .expect("the diff window opens");
}

#[test]
fn the_open_diff_verb_paints_the_selected_files_diff_and_offers_no_staging() {
    let project = Project::new();
    let repo = seeded_project(&project);
    let mut app = log_app(&project, &repo);
    open_commit_file(&mut app, "message of the second commit");

    app.wait_until(
        "the commit's added line paints",
        std::time::Duration::from_secs(15),
        |screen| screen.texts().iter().any(|text| text.contains(ADDED_LINE)),
    )
    .unwrap();

    for staging in ["id:diff:stage-hunk:0", "id:diff:unstage-hunk:0"] {
        assert!(
            !app.screen().matches(staging).unwrap_or(true),
            "a historical diff must not offer `{staging}`"
        );
    }
    assert!(
        !app.screen()
            .texts()
            .iter()
            .any(|text| text.contains("(staged-ness unknown)")),
        "a historical row must not answer the staged-ness question at all"
    );
}

#[test]
fn closing_the_diff_window_closes_the_window() {
    let project = Project::new();
    let repo = seeded_project(&project);
    let mut app = log_app(&project, &repo);
    open_commit_file(&mut app, "message of the second commit");
    app.wait_until(
        "the diff settles",
        std::time::Duration::from_secs(15),
        |screen| screen.texts().iter().any(|text| text.contains(ADDED_LINE)),
    )
    .unwrap();

    app.press("id:dialog:diff:action").unwrap();
    app.wait_for_gone("id:diff:window", std::time::Duration::from_secs(10))
        .unwrap();
}

#[test]
fn a_root_commit_file_paints_the_commits_own_diff_not_the_worktree() {
    let project = Project::new();
    let alpha = project.path().join("alpha");
    std::fs::create_dir_all(&alpha).unwrap();
    git(&alpha, &["init", "-q", "-b", "main"]);
    git(&alpha, &["config", "user.email", "test@example.com"]);
    git(&alpha, &["config", "user.name", "Test"]);
    let root_added = "zz-added-by-root-commit";
    let worktree_added = "zz-dirty-worktree-line";
    commit_file(
        &alpha,
        "tracked.txt",
        &format!("first line\n{root_added}\n"),
        "message of the only commit",
    );
    let mut body = std::fs::read_to_string(alpha.join("tracked.txt")).unwrap();
    body.push_str(worktree_added);
    body.push('\n');
    std::fs::write(alpha.join("tracked.txt"), body).unwrap();

    let mut app = log_app(&project, &alpha);
    open_commit_file(&mut app, "message of the only commit");
    app.wait_until(
        "the root commit's line paints",
        std::time::Duration::from_secs(15),
        |screen| screen.texts().iter().any(|text| text.contains(root_added)),
    )
    .unwrap();
    assert!(
        !app.screen()
            .texts()
            .iter()
            .any(|text| text.contains(worktree_added)),
        "the worktree's own dirt is not part of the commit's change"
    );
}

#[test]
fn a_log_checkout_stays_a_read_only_historical_view_of_the_two_commits() {
    let project = Project::new();
    let repo = log_repo(project.path(), "alpha", 3);
    let mut app = log_app(&project, &repo);

    app.press("text:commit number 2").unwrap();
    app.wait_for("id:log:file-diff").unwrap();
    assert!(
        !app.screen().shows("Select a commit…"),
        "the commit's own details replaced the empty state"
    );
    assert!(
        app.screen()
            .matches("id:log:select-parent")
            .unwrap_or(false),
        "the details pane names a revision to walk to"
    );
}
