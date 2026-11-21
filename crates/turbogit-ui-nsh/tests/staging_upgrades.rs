//! The staged-state upgrades of the staging pane, ported from the egui
//! `staging_upgrades` suite: hunk headers name their own staged state, a
//! half-staged hunk names itself part, revealing context never changes which
//! hunk a stage button stages, the list carries no staging sections, and an
//! untracked file's gutter stages a hunk end to end.

use std::path::{Path, PathBuf};

use support::{Project, commit_tab, open_group, seeded_repo, staged_patch};

mod support;

use test_support::git_seed::git;

fn repo(project: &Project, name: &str) -> PathBuf {
    let repo = seeded_repo(project.path(), name);
    git(&repo, &["checkout", "-q", "-b", "work"]);
    repo
}

fn seed_two_hunk_edit(repo: &Path) {
    let gap = "\n".repeat(10);
    let v1 = format!("fn alpha() {{\n    let a = 1;\n}}\n{gap}fn omega() {{\n    let o = 2;\n}}\n");
    let v2 = format!(
        "fn alpha() {{\n    let alpha = 10;\n}}\n{gap}fn omega() {{\n    let omega = 20;\n}}\n"
    );
    std::fs::write(repo.join("code.rs"), v1).unwrap();
    git(repo, &["add", "code.rs"]);
    git(repo, &["commit", "-q", "-m", "code"]);
    std::fs::write(repo.join("code.rs"), v2).unwrap();
}

fn open_diff(project: &Project, repo: &Path) -> support::Mounted {
    let mut app = commit_tab(project.path(), std::slice::from_ref(&repo.to_path_buf()));
    open_group(&mut app, "alpha");
    app.wait_for("id:changes:row:alpha:code.rs")
        .expect("the changed row paints");
    app.press("id:changes:row:alpha:code.rs").unwrap();
    app.wait_for("id:diff:stage-hunk:0")
        .expect("the staging gutter paints");
    app
}

fn says(app: &support::Mounted, needle: &str) -> bool {
    app.screen()
        .texts()
        .iter()
        .any(|text| text.contains(needle))
}

#[test]
fn hunk_headers_name_their_own_staged_state() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    seed_two_hunk_edit(&repo);
    let mut app = open_diff(&project, &repo);

    app.press("id:diff:stage-hunk:0").unwrap();
    app.wait_until(
        "the first hunk reaches the index",
        std::time::Duration::from_secs(15),
        |_| staged_patch(&repo).contains("+    let alpha = 10;"),
    )
    .unwrap();
    app.wait_until(
        "the hunk headers name their own state",
        std::time::Duration::from_secs(15),
        |screen| {
            let text = screen.texts().join("\n");
            text.contains("· staged") && text.contains("· not staged")
        },
    )
    .unwrap();

    let text = app.screen().texts().join("\n");
    for removed in ["hunk staged", "hunks staged", "· local", "· index"] {
        assert!(
            !text.contains(removed),
            "`{removed}` is not a state the header may name, got:\n{text}"
        );
    }
}

#[test]
fn a_partially_staged_hunk_names_itself_part() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    std::fs::write(
        repo.join("code.rs"),
        "fn main() {\n    let alpha = 1;\n    let beta = 2;\n    let gamma = 3;\n}\n",
    )
    .unwrap();
    git(&repo, &["add", "code.rs"]);
    git(&repo, &["commit", "-q", "-m", "code"]);
    std::fs::write(
        repo.join("code.rs"),
        "fn main() {\n    let ALPHA = 1;\n    let beta = 2;\n    let GAMMA = 3;\n}\n",
    )
    .unwrap();

    let mut app = open_diff(&project, &repo);
    app.press("id:diff:del:2").unwrap();
    app.wait_until(
        "the line stage reaches the index",
        std::time::Duration::from_secs(15),
        |_| staged_patch(&repo).contains("-    let alpha = 1;"),
    )
    .unwrap();
    app.wait_until(
        "the one hunk on screen names itself part",
        std::time::Duration::from_secs(15),
        |screen| screen.texts().iter().any(|text| text.contains("· part")),
    )
    .unwrap();
    assert!(
        !says(&app, "· staged") || says(&app, "· part"),
        "a hunk holding both states is `part`, never wholly `staged`: {:?}",
        app.screen().texts()
    );
}

#[test]
fn staging_a_hunk_after_revealing_around_it_stages_what_staging_alone_stages() {
    let quiet_project = Project::new();
    let loud_project = Project::new();
    let quiet = repo(&quiet_project, "alpha");
    let loud = repo(&loud_project, "alpha");
    seed_two_hunk_edit(&quiet);
    seed_two_hunk_edit(&loud);

    let mut quiet_app = open_diff(&quiet_project, &quiet);
    let mut loud_app = open_diff(&loud_project, &loud);

    loud_app.wait_for("text:6 unchanged lines hidden").unwrap();
    loud_app
        .press("label:6 unchanged lines hidden")
        .expect("the gap between the hunks expands");
    loud_app
        .wait_for_gone(
            "text:6 unchanged lines hidden",
            std::time::Duration::from_secs(10),
        )
        .unwrap();

    quiet_app.press("id:diff:stage-hunk:0").unwrap();
    loud_app.press("id:diff:stage-hunk:0").unwrap();
    quiet_app
        .wait_until(
            "the quiet pane stages",
            std::time::Duration::from_secs(15),
            |_| staged_patch(&quiet).contains("+    let alpha = 10;"),
        )
        .unwrap();
    loud_app
        .wait_until(
            "the revealed pane stages",
            std::time::Duration::from_secs(15),
            |_| staged_patch(&loud).contains("+    let alpha = 10;"),
        )
        .unwrap();
    assert_eq!(
        staged_patch(&quiet),
        staged_patch(&loud),
        "opening six lines of context must not change which hunk the button stages"
    );
}

#[test]
fn the_file_list_carries_no_staging_sections_or_hunk_counts() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    std::fs::write(repo.join("base.txt"), "modified\n").unwrap();
    std::fs::write(repo.join("other.txt"), "staged edit\n").unwrap();
    git(&repo, &["add", "other.txt"]);
    std::fs::write(repo.join("untracked.txt"), "new\n").unwrap();

    let app = {
        let mut app = commit_tab(project.path(), std::slice::from_ref(&repo));
        open_group(&mut app, "alpha");
        app
    };
    let text = app.screen().texts().join("\n");
    assert!(
        !text.contains("UNSTAGED") && !text.contains("STAGED"),
        "staging section headers must be gone, got:\n{text}"
    );
    assert!(
        !text.contains("Unversioned Files"),
        "untracked files share the flat list; their own group is gone"
    );
    assert!(
        !text.contains("1 hunk") && !text.contains("1/1"),
        "no row paints a hunk count or a staged/total ratio, got:\n{text}"
    );
    for row in ["base.txt", "other.txt", "untracked.txt"] {
        assert!(
            app.screen()
                .matches(&format!("id:changes:row:alpha:{row}"))
                .unwrap_or(false),
            "{row} keeps a flat row"
        );
    }
}

#[test]
fn an_untracked_files_gutter_stages_a_hunk_end_to_end() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    std::fs::write(repo.join("new.txt"), "one\ntwo\nthree\nfour\nfive\nsix\n").unwrap();
    let mut app = commit_tab(project.path(), std::slice::from_ref(&repo));
    open_group(&mut app, "alpha");
    app.wait_for("id:changes:row:alpha:new.txt").unwrap();
    app.press("id:changes:row:alpha:new.txt").unwrap();
    app.wait_for("id:diff:stage-hunk:0")
        .expect("an untracked file's preview offers the gutter");

    app.press("id:diff:stage-hunk:0").unwrap();
    app.wait_until(
        "the hunk reaches the index",
        std::time::Duration::from_secs(15),
        |_| git(&repo, &["diff", "--cached", "--name-only"]).contains("new.txt"),
    )
    .unwrap();
    assert!(
        staged_patch(&repo).contains("+one"),
        "the staged hunk carries the file's lines:\n{}",
        staged_patch(&repo)
    );
    assert!(
        !git(&repo, &["status", "--porcelain"]).contains("?? new.txt"),
        "the file is no longer wholly untracked"
    );
}
