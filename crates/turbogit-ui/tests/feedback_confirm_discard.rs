//! The discard confirm, ported from the egui `feedback_confirm_discard`
//! suite: the rollback names every affected file, offers the keep-and-park
//! path beside the destructive one, answers Cancel without touching the
//! worktree, reverts on Discard, and stashes on Shelve first.

use std::path::{Path, PathBuf};

use support::{Project, commit_tab, open_group, seeded_repo};

mod support;

use test_support::git_seed::git;

fn repo_with_modified_files(project: &Project) -> PathBuf {
    let repo = seeded_repo(project.path(), "alpha");
    for name in ["a.txt", "b.txt", "c.txt"] {
        std::fs::write(repo.join(name), "baseline\n").unwrap();
        git(&repo, &["add", name]);
    }
    git(&repo, &["commit", "-q", "-m", "init"]);
    for name in ["a.txt", "b.txt", "c.txt"] {
        std::fs::write(repo.join(name), "modified\n").unwrap();
    }
    repo
}

fn changes_app(project: &Project, repo: &Path) -> support::Mounted {
    let mut app = commit_tab(project.path(), std::slice::from_ref(&repo.to_path_buf()));
    open_group(&mut app, "alpha");
    for name in ["a.txt", "b.txt", "c.txt"] {
        app.wait_for(&format!("id:changes:row:alpha:{name}"))
            .expect("the file rows paint");
    }
    app
}

fn select_all(app: &mut support::Mounted) {
    app.press("id:changes:row:alpha:a.txt").unwrap();
    app.hold_keys("mod", true).unwrap();
    app.press("id:changes:row:alpha:b.txt").unwrap();
    app.press("id:changes:row:alpha:c.txt").unwrap();
    app.hold_keys("mod", false).unwrap();
}

fn read(repo: &Path, name: &str) -> String {
    std::fs::read_to_string(repo.join(name)).expect("the seeded file")
}

fn stash_list(repo: &Path) -> String {
    git(repo, &["stash", "list"])
}

#[test]
fn the_discard_confirm_lists_every_affected_file_and_its_verbs() {
    let project = Project::new();
    let repo = repo_with_modified_files(&project);
    let mut app = changes_app(&project, &repo);
    select_all(&mut app);

    app.press("id:changes:rollback").unwrap();
    app.wait_for("id:dialog:discard")
        .expect("the rollback raises the discard question");

    for name in ["a.txt", "b.txt", "c.txt"] {
        assert!(
            app.screen()
                .matches(&format!("id:dialog:discard >> text:{name}"))
                .unwrap_or(false),
            "the confirm names {name}"
        );
    }
    for verb in ["Shelve first", "Discard 3-file changes", "Cancel"] {
        assert!(
            app.screen()
                .matches(&format!("id:dialog:discard >> text:{verb}"))
                .unwrap_or(false),
            "the confirm offers `{verb}`"
        );
    }

    app.press("id:dialog:discard >> text:Cancel").unwrap();
    app.wait_for_gone("id:dialog:discard", std::time::Duration::from_secs(5))
        .unwrap();
    for name in ["a.txt", "b.txt", "c.txt"] {
        assert_eq!(
            read(&repo, name),
            "modified\n",
            "Cancel keeps {name} exactly as it was"
        );
    }
    assert!(
        stash_list(&repo).trim().is_empty(),
        "Cancel must not create a stash"
    );
}

#[test]
fn the_discard_verb_runs_the_destructive_operation() {
    let project = Project::new();
    let repo = repo_with_modified_files(&project);
    let mut app = changes_app(&project, &repo);
    select_all(&mut app);

    app.press("id:changes:rollback").unwrap();
    app.wait_for("id:dialog:discard").unwrap();
    app.press("id:dialog:discard >> text:Discard 3-file changes")
        .unwrap();
    app.wait_for_gone("id:dialog:discard", std::time::Duration::from_secs(10))
        .unwrap();
    app.wait_until(
        "the three files revert to baseline",
        std::time::Duration::from_secs(15),
        |_| {
            ["a.txt", "b.txt", "c.txt"]
                .iter()
                .all(|name| read(&repo, name) == "baseline\n")
        },
    )
    .unwrap();
    assert!(
        git(&repo, &["status", "--porcelain"]).trim().is_empty(),
        "the discarded work leaves no trace: {}",
        git(&repo, &["status", "--porcelain"])
    );
}

#[test]
fn shelve_first_parks_the_work_and_leaves_the_worktree_clean() {
    let project = Project::new();
    let repo = repo_with_modified_files(&project);
    let mut app = changes_app(&project, &repo);
    select_all(&mut app);

    app.press("id:changes:rollback").unwrap();
    app.wait_for("id:dialog:discard").unwrap();
    app.press("id:dialog:discard >> text:Shelve first").unwrap();
    app.wait_until(
        "the parked work leaves the worktree clean",
        std::time::Duration::from_secs(20),
        |_| {
            git(&repo, &["status", "--porcelain"]).trim().is_empty()
                && !stash_list(&repo).trim().is_empty()
        },
    )
    .unwrap();
    assert!(
        stash_list(&repo).contains("Shelved 3 file(s) before discard"),
        "Shelve first parks the selection under its own parking message: {:?}",
        stash_list(&repo)
    );
}

#[test]
fn a_rollback_with_nothing_selected_names_the_missing_selection() {
    let project = Project::new();
    let repo = repo_with_modified_files(&project);
    let mut app = changes_app(&project, &repo);

    app.press("id:changes:rollback").unwrap();
    app.wait_until(
        "the missing selection is named",
        std::time::Duration::from_secs(10),
        |screen| {
            screen
                .texts()
                .iter()
                .any(|text| text.contains("Select files to discard."))
        },
    )
    .unwrap();
    assert!(
        !app.screen().matches("id:dialog:discard").unwrap_or(true),
        "an empty selection asks nothing and destroys nothing"
    );
}
