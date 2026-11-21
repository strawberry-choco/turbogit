//! Line granularity in the staging pane, ported from the egui
//! `line_granularity` suite: a press on a changed row stages that line and
//! only it, and a drag across an addition stages exactly the character range
//! it names, once.

use std::path::{Path, PathBuf};

use support::{Project, commit_tab, open_group, seeded_repo, staged_patch};

mod support;

use test_support::git_seed::git;

const OLD_LINE: &str = "    let x = 1;";
const NEW_LINE: &str = "    let calculated_value = compute(x);";

const WIDE_ADD: &str = concat!(
    "    let calculated_value = compute(x);",
    " ------------------------------------------------------------",
    " keep going with the same long addition so a character drag has room",
);

fn repo(project: &Project) -> PathBuf {
    let repo = seeded_repo(project.path(), "alpha");
    git(&repo, &["checkout", "-q", "-b", "work"]);
    repo
}

fn seed_single_line_edit(repo: &Path, new_line: &str) {
    std::fs::write(
        repo.join("code.rs"),
        format!("fn main() {{\n{OLD_LINE}\n}}\n"),
    )
    .unwrap();
    git(repo, &["add", "code.rs"]);
    git(repo, &["commit", "-q", "-m", "code"]);
    std::fs::write(
        repo.join("code.rs"),
        format!("fn main() {{\n{new_line}\n}}\n"),
    )
    .unwrap();
}

fn open_diff(project: &Project, repo: &Path) -> support::Mounted {
    let mut app = commit_tab(project.path(), std::slice::from_ref(&repo.to_path_buf()));
    open_group(&mut app, "alpha");
    app.wait_for("id:changes:row:alpha:code.rs")
        .expect("the changed row paints");
    app.press("id:changes:row:alpha:code.rs").unwrap();
    app.wait_for("id:diff:list")
        .expect("the staging rows paint");
    app
}

fn added_lines(patch: &str) -> Vec<String> {
    patch
        .lines()
        .filter(|line| line.starts_with('+') && !line.starts_with("+++"))
        .map(|line| line.trim_start_matches('+').to_owned())
        .collect()
}

#[test]
fn a_row_click_stages_the_line_in_git_and_keeps_its_partner() {
    let project = Project::new();
    let repo = repo(&project);
    seed_single_line_edit(&repo, NEW_LINE);
    let mut app = open_diff(&project, &repo);
    app.wait_for("id:diff:del:2")
        .expect("the deletion row paints");

    app.press("id:diff:del:2").unwrap();
    app.wait_until(
        "the clicked deletion lands in the index",
        std::time::Duration::from_secs(15),
        |_| {
            let staged = staged_patch(&repo);
            staged.contains(&format!("-{OLD_LINE}"))
        },
    )
    .unwrap();
    assert!(
        !staged_patch(&repo).contains(&format!("+{NEW_LINE}")),
        "the unclicked addition stays out of the index:\n{}",
        staged_patch(&repo)
    );

    app.wait_until(
        "both rows of the edit stay in the preview",
        std::time::Duration::from_secs(15),
        |screen| {
            screen
                .matches(&format!("label:{OLD_LINE} (staged)"))
                .unwrap_or(false)
                && screen
                    .matches(&format!("label:{NEW_LINE} (unstaged)"))
                    .unwrap_or(false)
        },
    )
    .unwrap();
}

#[test]
fn a_drag_stages_the_character_range_and_nothing_else() {
    let project = Project::new();
    let repo = repo(&project);
    seed_single_line_edit(&repo, WIDE_ADD);
    let mut app = open_diff(&project, &repo);
    app.wait_for("id:diff:add:2")
        .expect("the wide addition row paints");

    // Start near the row's middle and drag left: the row maps the pointer's
    // x through the monospace advance, so the draft names a substring, not
    // the whole line.
    app.drag(
        "id:diff:add:2",
        nsh_core::geometry::point(nsh_core::Points(-300.0), nsh_core::Points(0.0)),
    )
    .expect("the row hears the drag");
    app.wait_until(
        "the dragged bytes reach the index",
        std::time::Duration::from_secs(15),
        |_| !added_lines(&staged_patch(&repo)).is_empty(),
    )
    .unwrap();
    app.wait(std::time::Duration::from_millis(400)).unwrap();

    let staged = staged_patch(&repo);
    let staged_additions = added_lines(&staged);
    assert_eq!(
        staged_additions.len(),
        1,
        "the release stages exactly once:\n{staged}"
    );
    let staged_line = &staged_additions[0];
    assert_ne!(
        staged_line, WIDE_ADD,
        "the drag must stage a range, not the whole line:\n{staged}"
    );
    assert!(
        WIDE_ADD.contains(staged_line) && !staged_line.is_empty(),
        "the staged bytes are a slice of the dragged line, got {staged_line:?}"
    );
}

#[test]
fn an_untracked_files_gutter_offers_the_same_granular_rows() {
    let project = Project::new();
    let repo = repo(&project);
    std::fs::write(repo.join("new.txt"), "one\ntwo\nthree\nfour\nfive\nsix\n").unwrap();
    let mut app = open_diff_new(&project, &repo, "new.txt");
    app.wait_for("id:diff:add:1")
        .expect("an untracked file's rows are granular");
    app.wait_for("id:diff:stage-hunk:0")
        .expect("and its gutter stages a hunk");
}

fn open_diff_new(project: &Project, repo: &Path, file: &str) -> support::Mounted {
    let mut app = commit_tab(project.path(), std::slice::from_ref(&repo.to_path_buf()));
    open_group(&mut app, "alpha");
    app.wait_for(&format!("id:changes:row:alpha:{file}"))
        .expect("the untracked row paints");
    app.press(&format!("id:changes:row:alpha:{file}")).unwrap();
    app
}
