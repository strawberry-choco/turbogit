//! The diff viewer, ported from the egui `diff_viewer` suite at the reduced
//! fidelity the nsh surface supports: the preview answers HEAD against the
//! working tree with both the staged and the unstaged halves, a fully staged
//! file still answers, the whitespace policy removes the pair it calls equal,
//! and the toolbar's counts and hunk navigation follow the comparison.

use std::path::{Path, PathBuf};

use support::{Project, commit_tab, open_group};

mod support;

use test_support::git_seed::git;

fn repo(project: &Project, name: &str) -> PathBuf {
    let repo = support::seeded_repo(project.path(), name);
    git(&repo, &["checkout", "-q", "-b", "work"]);
    repo
}

fn write_file(repo: &Path, name: &str, content: &str) {
    std::fs::write(repo.join(name), content).expect("write the fixture");
}

fn repo_mixed(project: &Project) -> PathBuf {
    let repo = repo(project, "alpha");
    write_file(
        &repo,
        "file.txt",
        "alpha\nbeta\ndelta\nepsilon\nzeta\neta\ntheta\ngamma\niota\n",
    );
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "c1"]);
    write_file(
        &repo,
        "file.txt",
        "alpha\nBETA\ndelta\nepsilon\nzeta\neta\ntheta\ngamma\niota\n",
    );
    git(&repo, &["add", "file.txt"]);
    write_file(
        &repo,
        "file.txt",
        "alpha\nBETA\ndelta\nepsilon\nzeta\neta\ntheta\nGAMMA\niota\n",
    );
    repo
}

fn repo_staged_only(project: &Project) -> PathBuf {
    let repo = repo(project, "alpha");
    write_file(&repo, "one.txt", "one\ntwo\n");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "c1"]);
    write_file(&repo, "one.txt", "one\nTWO\n");
    git(&repo, &["add", "one.txt"]);
    repo
}

fn repo_two_hunks(project: &Project) -> PathBuf {
    let repo = repo(project, "alpha");
    let base: Vec<String> = (1..=16).map(|i| format!("l{i:02}")).collect();
    write_file(&repo, "nav.txt", &format!("{}\n", base.join("\n")));
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "c1"]);
    let mut edited = base;
    edited[1] = "X2".to_string();
    edited[11] = "X12".to_string();
    write_file(&repo, "nav.txt", &format!("{}\n", edited.join("\n")));
    repo
}

fn repo_whitespace(project: &Project) -> PathBuf {
    let repo = repo(project, "alpha");
    write_file(&repo, "ws.txt", "alpha\nbeta\ngamma\n");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "c1"]);
    write_file(&repo, "ws.txt", "alpha\nbeta\ng amma\n");
    repo
}

fn preview_app(project: &Project, repo: &Path, file: &str) -> support::Mounted {
    let mut app = commit_tab(project.path(), std::slice::from_ref(&repo.to_path_buf()));
    open_group(&mut app, "alpha");
    app.wait_for(&format!("id:changes:row:alpha:{file}"))
        .expect("the changed file row paints");
    app.press(&format!("id:changes:row:alpha:{file}"))
        .expect("the preview follows the row");
    app.wait_for("id:diff:toolbar")
        .expect("the diff toolbar paints");
    app
}

fn wait_for_line(app: &mut support::Mounted, line: &str) {
    app.wait_until(
        &format!("{line} paints"),
        std::time::Duration::from_secs(15),
        |screen| screen.texts().iter().any(|text| text.trim() == line),
    )
    .unwrap();
}

#[test]
fn the_pane_answers_git_diff_head_and_shows_both_halves() {
    let project = Project::new();
    let repo = repo_mixed(&project);
    let mut app = preview_app(&project, &repo, "file.txt");
    wait_for_line(&mut app, "BETA");

    let texts = app.screen().texts();
    for line in ["BETA", "GAMMA"] {
        assert!(
            texts.iter().any(|text| text.contains(line)),
            "the combined read carries {line}: {texts:?}"
        );
    }
    assert!(
        !texts.iter().any(|text| text.contains("(no differences)")),
        "the combined read is not the unstaged-only answer"
    );
    assert_eq!(
        git(&repo, &["diff", "--name-only"]),
        "file.txt\n",
        "the fixture's premise: only GAMMA is unstaged, and the pane shows BETA too"
    );
}

#[test]
fn a_fully_staged_file_still_answers_the_combined_read() {
    let project = Project::new();
    let repo = repo_staged_only(&project);
    let mut app = preview_app(&project, &repo, "one.txt");
    wait_for_line(&mut app, "TWO");

    let texts = app.screen().texts();
    assert!(
        texts.iter().any(|text| text.contains("TWO"))
            && texts.iter().any(|text| text.contains("two")),
        "the staged row still paints both sides: {texts:?}"
    );

    assert!(
        !texts.iter().any(|text| text.contains("(no differences)")),
        "the read that hides staged content is gone"
    );
    assert!(
        git(&repo, &["diff", "--", "one.txt"]).is_empty(),
        "the fixture leaves nothing for the unstaged answer, and the pane shows it anyway"
    );
}

#[test]
fn the_toolbar_counts_the_changes_and_steps_between_the_hunks() {
    let project = Project::new();
    let repo = repo_two_hunks(&project);
    let mut app = preview_app(&project, &repo, "nav.txt");
    app.wait_until(
        "the read settles",
        std::time::Duration::from_secs(15),
        |screen| screen.shows("+2"),
    )
    .unwrap();

    assert!(app.screen().shows("+2"), "the toolbar counts two additions");
    assert!(
        app.screen().shows("−2"),
        "and two deletions, with the stats' own minus"
    );
    assert!(
        app.screen().shows("hunk 1/2"),
        "the toolbar names the first of two hunks: {:?}",
        app.screen().texts()
    );

    app.press("id:diff:next").unwrap();
    app.wait_for("text:hunk 2/2")
        .expect("Next hunk moves the toolbar's hunk");
    app.press("id:diff:prev").unwrap();
    app.wait_for("text:hunk 1/2")
        .expect("Previous hunk moves it back");
}

#[test]
fn the_whitespace_policy_removes_the_pair_it_calls_equal() {
    let project = Project::new();
    let repo = repo_whitespace(&project);
    let mut app = preview_app(&project, &repo, "ws.txt");
    wait_for_line(&mut app, "g amma");

    assert!(app.screen().shows("gamma"), "the pair starts painted");
    assert!(app.screen().shows("g amma"));

    app.press("id:diff:options").unwrap();
    app.wait_for("id:diff:options-popover")
        .expect("the policy menu opens");
    app.press("id:diff:whitespace:Ignore whitespaces").unwrap();
    app.wait_until(
        "the pair the policy calls equal drops out",
        std::time::Duration::from_secs(15),
        |screen| {
            !screen.texts().iter().any(|text| text.trim() == "gamma")
                && screen.texts().iter().any(|text| text.trim() == "g amma")
        },
    )
    .unwrap();

    if !app
        .screen()
        .matches("id:diff:whitespace:None")
        .unwrap_or(false)
    {
        app.press("id:diff:options").unwrap();
        app.wait_for("id:diff:whitespace:None").unwrap();
    }
    app.press("id:diff:whitespace:None").unwrap();
    app.wait_until(
        "the pair comes back with the policy off",
        std::time::Duration::from_secs(15),
        |screen| screen.texts().iter().any(|text| text.trim() == "gamma"),
    )
    .unwrap();
}
