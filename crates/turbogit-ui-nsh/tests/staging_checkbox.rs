//! The changed-file checkbox, ported from the egui `staging_checkbox` suite:
//! the middle state stages the whole file, un-staging a new file returns it
//! to untracked, the box works on an unborn branch and a cold diff cache, the
//! list is one flat path-ordered set, ignored files stay out, the status
//! letters keep their rows, an unmerged path has no box, and the preview
//! stays pinned across a stage.

use std::path::{Path, PathBuf};

use support::{Project, commit_tab, open_group, seeded_repo};

mod support;

use test_support::git_seed::git;

fn repo(project: &Project, name: &str) -> PathBuf {
    let repo = seeded_repo(project.path(), name);
    git(&repo, &["checkout", "-q", "-b", "work"]);
    repo
}

fn stage_box(path: &str) -> String {
    format!("label:Stage {path} (unstaged)")
}

fn partial_box(path: &str) -> String {
    format!("label:Stage all of {path} (partially staged)")
}

fn unstage_box(path: &str) -> String {
    format!("label:Unstage {path} (staged)")
}

fn staged_paths(repo: &Path) -> Vec<String> {
    let mut out: Vec<String> = git(repo, &["diff", "--cached", "--name-only"])
        .lines()
        .map(str::to_owned)
        .collect();
    out.sort();
    out
}

fn app_for(project: &Project, repo: &Path, root: &str) -> support::Mounted {
    let mut app = commit_tab(project.path(), std::slice::from_ref(&repo.to_path_buf()));
    open_group(&mut app, root);
    app
}

#[test]
fn the_middle_state_clicks_to_the_whole_file_staged() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    std::fs::write(repo.join("base.txt"), "one\ntwo\nthree\n").unwrap();
    git(&repo, &["add", "base.txt"]);
    git(&repo, &["commit", "-q", "-m", "three lines"]);
    std::fs::write(repo.join("base.txt"), "ONE\ntwo\nthree\n").unwrap();
    git(&repo, &["add", "base.txt"]);
    std::fs::write(repo.join("base.txt"), "ONE\nTWO\nthree\n").unwrap();
    assert!(
        git(&repo, &["status", "--porcelain"]).starts_with("MM"),
        "the fixture must be genuinely half-staged, git says: {}",
        git(&repo, &["status", "--porcelain"])
    );

    let mut app = app_for(&project, &repo, "alpha");
    app.wait_for("id:changes:check:alpha:base.txt")
        .expect("the row paints");
    assert!(
        app.screen()
            .matches(&partial_box("base.txt"))
            .unwrap_or(false),
        "the half-staged row reads its middle state: {:?}",
        app.screen().texts()
    );

    app.press("id:changes:check:alpha:base.txt").unwrap();
    app.wait_until(
        "the middle state stages the whole file",
        std::time::Duration::from_secs(15),
        |_| {
            let staged = git(&repo, &["diff", "--cached", "--", "base.txt"]);
            staged.contains("+ONE") && staged.contains("+TWO")
        },
    )
    .unwrap();
    app.wait_until(
        "and the box becomes fully staged",
        std::time::Duration::from_secs(15),
        |screen| screen.matches(&unstage_box("base.txt")).unwrap_or(false),
    )
    .unwrap();
}

#[test]
fn unstaging_a_newly_added_file_returns_it_to_untracked() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    std::fs::write(repo.join("new.txt"), "new\n").unwrap();
    let mut app = app_for(&project, &repo, "alpha");
    app.wait_for("id:changes:check:alpha:new.txt").unwrap();
    assert!(app.screen().matches(&stage_box("new.txt")).unwrap_or(false));

    app.press("id:changes:check:alpha:new.txt").unwrap();
    app.wait_until(
        "the new file stages",
        std::time::Duration::from_secs(15),
        |_| staged_paths(&repo) == vec!["new.txt".to_owned()],
    )
    .unwrap();
    app.wait_until(
        "the box reads fully staged",
        std::time::Duration::from_secs(15),
        |screen| screen.matches(&unstage_box("new.txt")).unwrap_or(false),
    )
    .unwrap();

    app.press("id:changes:check:alpha:new.txt").unwrap();
    app.wait_until(
        "the index no longer holds it",
        std::time::Duration::from_secs(15),
        |_| staged_paths(&repo).is_empty(),
    )
    .unwrap();
    assert_eq!(
        git(&repo, &["status", "--porcelain"]).trim(),
        "?? new.txt",
        "git reports it untracked again — not deleted, not modified"
    );
    app.wait_until(
        "the row is still there with an empty box",
        std::time::Duration::from_secs(15),
        |screen| screen.matches(&stage_box("new.txt")).unwrap_or(false),
    )
    .unwrap();
}

#[test]
fn unstaging_works_in_a_repository_with_no_commits() {
    let project = Project::new();
    let repo = project.path().join("alpha");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    std::fs::write(repo.join("base.txt"), "base\n").unwrap();
    assert!(
        !std::process::Command::new("git")
            .args(["rev-parse", "--verify", "HEAD"])
            .current_dir(&repo)
            .output()
            .is_ok_and(|out| out.status.success()),
        "the fixture must have no HEAD"
    );

    let mut app = app_for(&project, &repo, "alpha");
    app.wait_for("id:changes:check:alpha:base.txt").unwrap();
    app.press("id:changes:check:alpha:base.txt").unwrap();
    app.wait_until(
        "the file stages on an unborn branch",
        std::time::Duration::from_secs(15),
        |_| staged_paths(&repo) == vec!["base.txt".to_owned()],
    )
    .unwrap();
    app.wait_for(&unstage_box("base.txt")).unwrap();

    app.press("id:changes:check:alpha:base.txt").unwrap();
    app.wait_until(
        "and it un-stages there too",
        std::time::Duration::from_secs(15),
        |_| staged_paths(&repo).is_empty(),
    )
    .unwrap();
    assert_eq!(
        git(&repo, &["status", "--porcelain"]).trim(),
        "?? base.txt",
        "the click behaves the same on a fresh project"
    );
}

#[test]
fn the_checkbox_works_with_no_previewed_diff_behind_it() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    std::fs::write(repo.join("base.txt"), "modified\n").unwrap();
    let mut app = app_for(&project, &repo, "alpha");

    app.press("id:changes:check:alpha:base.txt").unwrap();
    app.wait_until(
        "a checkbox click reaches the index with no cached diff behind it",
        std::time::Duration::from_secs(15),
        |_| staged_paths(&repo) == vec!["base.txt".to_owned()],
    )
    .unwrap();
}

#[test]
fn the_list_is_one_flat_path_sorted_list_with_no_staging_headings() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("zzz.txt"), "modified\n").unwrap();
    std::fs::write(repo.join("aaa.txt"), "staged\n").unwrap();
    std::fs::write(repo.join("mmm.txt"), "untracked\n").unwrap();
    std::fs::write(repo.join("src/bbb.rs"), "nested\n").unwrap();
    git(&repo, &["add", "src/bbb.rs"]);
    git(&repo, &["commit", "-q", "-m", "nested"]);
    std::fs::write(repo.join("src/bbb.rs"), "nested edited\n").unwrap();
    git(&repo, &["add", "aaa.txt"]);

    let app = app_for(&project, &repo, "alpha");
    let texts = app.screen().texts().join("\n");
    for heading in ["UNSTAGED", "STAGED", "Unversioned Files"] {
        assert!(
            !texts.contains(heading),
            "`{heading}` must not structure the list any more, got:\n{texts}"
        );
    }

    let top_of = |path: &str| -> f32 {
        app.screen()
            .layout_nodes()
            .iter()
            .find(|node| node.test_id.as_deref() == Some(&format!("changes:row:alpha:{path}")))
            .map(|node| node.bounds.origin.y.0)
            .unwrap_or_else(|| panic!("the {path} row paints"))
    };
    let rows: Vec<f32> = ["aaa.txt", "mmm.txt", "src/bbb.rs", "zzz.txt"]
        .iter()
        .map(|path| top_of(path))
        .collect();
    assert!(
        rows.windows(2).all(|window| window[0] <= window[1]),
        "the flat list is sorted by path, got row tops {rows:?}"
    );
}

#[test]
fn ignored_files_stay_out_of_the_flat_list() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    std::fs::write(repo.join(".gitignore"), "build/\n").unwrap();
    git(&repo, &["add", ".gitignore"]);
    git(&repo, &["commit", "-q", "-m", "ignore"]);
    std::fs::create_dir_all(repo.join("build")).unwrap();
    std::fs::write(repo.join("build/out.bin"), "noise\n").unwrap();
    std::fs::write(repo.join("real.txt"), "work\n").unwrap();

    let app = app_for(&project, &repo, "alpha");
    assert!(
        app.screen()
            .matches("id:changes:row:alpha:real.txt")
            .unwrap_or(false),
        "the real change paints"
    );
    assert!(
        !app.screen()
            .matches("id:changes:row:alpha:build/out.bin")
            .unwrap_or(true),
        "an ignored file must not reappear in the list"
    );
    assert!(
        !app.screen().texts().join("\n").contains("out.bin"),
        "and must not paint at all"
    );
}

#[test]
fn each_status_paints_its_expected_letter() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    std::fs::write(repo.join("gone.txt"), "doomed\n").unwrap();
    std::fs::write(repo.join("kept.txt"), "kept\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "two files"]);
    std::fs::write(repo.join("kept.txt"), "edited\n").unwrap();
    git(&repo, &["rm", "-q", "gone.txt"]);
    std::fs::write(repo.join("fresh.txt"), "untracked\n").unwrap();

    let app = app_for(&project, &repo, "alpha");
    let texts = app.screen().texts();
    for letter in ["N", "M", "D"] {
        assert!(
            texts.iter().any(|text| text.trim() == letter),
            "the list paints the {letter} badge: {texts:?}"
        );
    }
    assert!(
        app.screen()
            .matches("id:changes:row:alpha:fresh.txt")
            .unwrap_or(false)
            && app
                .screen()
                .matches("id:changes:row:alpha:gone.txt")
                .unwrap_or(false),
        "every status keeps a row"
    );
}

#[test]
fn a_new_and_modified_file_renders_m() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    std::fs::write(repo.join("new.txt"), "first\n").unwrap();
    git(&repo, &["add", "new.txt"]);
    std::fs::write(repo.join("new.txt"), "second\n").unwrap();
    assert!(
        git(&repo, &["status", "--porcelain"]).starts_with('A'),
        "the fixture must be added-with-unstaged-work, git says: {}",
        git(&repo, &["status", "--porcelain"])
    );

    let app = app_for(&project, &repo, "alpha");
    assert!(
        app.screen().texts().iter().any(|text| text.trim() == "M"),
        "\"new and modified\" is one letter, and it is the modified one: {:?}",
        app.screen().texts()
    );
}

#[test]
fn conflicted_rows_have_no_checkbox_and_keep_the_name_column() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    let branch = git(&repo, &["rev-parse", "--abbrev-ref", "HEAD"])
        .trim()
        .to_string();
    std::fs::write(repo.join("plain.txt"), "v1\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "base"]);
    git(&repo, &["checkout", "-q", "-b", "side"]);
    std::fs::write(repo.join("conf.txt"), "side\n").unwrap();
    git(&repo, &["add", "conf.txt"]);
    git(&repo, &["commit", "-q", "-m", "side"]);
    git(&repo, &["checkout", "-q", &branch]);
    std::fs::write(repo.join("conf.txt"), "main line\n").unwrap();
    git(&repo, &["add", "conf.txt"]);
    git(&repo, &["commit", "-q", "-m", "main"]);
    let _ = std::process::Command::new("git")
        .args(["merge", "--no-edit", "side"])
        .current_dir(&repo)
        .output();
    std::fs::write(repo.join("plain.txt"), "v2\n").unwrap();

    let app = app_for(&project, &repo, "alpha");
    assert!(
        app.screen().shows("Merge conflicts (1)"),
        "the unmerged path has its own bucket: {:?}",
        app.screen().texts()
    );
    assert!(
        app.screen()
            .matches("id:changes:row:alpha:conf.txt")
            .unwrap_or(false),
        "and its own row"
    );
    assert!(
        !app.screen()
            .matches("id:changes:check:alpha:conf.txt")
            .unwrap_or(true),
        "an unmerged path must have no staging box"
    );
}

#[test]
fn the_preview_stays_pinned_across_a_stage_and_an_unstage() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    std::fs::write(repo.join("base.txt"), "modified\n").unwrap();
    let mut app = app_for(&project, &repo, "alpha");
    app.press("id:changes:row:alpha:base.txt").unwrap();
    app.wait_until(
        "the unstaged edit is what the pinned read shows",
        std::time::Duration::from_secs(15),
        |screen| screen.texts().iter().any(|text| text.trim() == "modified"),
    )
    .unwrap();

    app.press("id:changes:check:alpha:base.txt").unwrap();
    app.wait_until(
        "ticking stages the whole file",
        std::time::Duration::from_secs(15),
        |_| staged_paths(&repo) == vec!["base.txt".to_owned()],
    )
    .unwrap();
    app.wait_for(&unstage_box("base.txt"))
        .expect("the box reads fully staged before the un-tick");
    app.wait(std::time::Duration::from_millis(400)).unwrap();
    app.wait_for(&unstage_box("base.txt"))
        .expect("and stays staged while the status scans settle");
    assert!(
        git(&repo, &["diff", "--", "base.txt"]).is_empty(),
        "the unstaged answer is now empty"
    );
    app.wait_until(
        "the pane stays on the one comparison instead of jumping away",
        std::time::Duration::from_secs(15),
        |screen| {
            screen.texts().iter().any(|text| text.trim() == "modified")
                && !screen
                    .texts()
                    .iter()
                    .any(|text| text.contains("(no differences)"))
        },
    )
    .unwrap();

    app.press("id:changes:check:alpha:base.txt").unwrap();
    app.wait_until(
        "un-ticking returns the pane to the same pinned answer",
        std::time::Duration::from_secs(15),
        |screen| {
            staged_paths(&repo).is_empty()
                && screen.texts().iter().any(|text| text.trim() == "modified")
        },
    )
    .unwrap();
}
