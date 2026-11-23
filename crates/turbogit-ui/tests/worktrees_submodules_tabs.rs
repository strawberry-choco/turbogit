//! The Worktrees and Submodules tabs, ported from the egui
//! `worktrees_submodules_tabs` suite at reduced fidelity: the columns, the
//! probe states, and the add, remove, init, update, and deinit lifecycles.

use std::path::{Path, PathBuf};
use std::time::Duration;

use nsh_testing::DialogAnswer;
use support::{Mounted, Project, for_roots};
use test_support::git_seed::git;

mod support;

fn worktrees_tab(project: &Project, repo: &Path) -> Mounted {
    let mut app = for_roots(project.path(), std::slice::from_ref(&repo.to_path_buf()));
    app.first_render().unwrap();
    app.press("id:tab:Worktrees").unwrap();
    app.wait_for("id:route:worktrees").unwrap();
    app
}

fn submodules_tab(project: &Project, repo: &Path) -> Mounted {
    let mut app = for_roots(project.path(), std::slice::from_ref(&repo.to_path_buf()));
    app.first_render().unwrap();
    app.press("id:tab:Submodules").unwrap();
    app.wait_for("id:route:submodules").unwrap();
    app
}

#[test]
fn the_worktrees_pane_lists_a_linked_checkout_and_its_probe() {
    let project = Project::new();
    let repo = support::seeded_repo(project.path(), "alpha");
    git(&repo, &["branch", "linked"]);
    let linked = project.path().join("alpha-linked");
    git(
        &repo,
        &["worktree", "add", linked.to_str().unwrap(), "linked"],
    );
    let mut app = worktrees_tab(&project, &repo);
    app.wait_for("id:worktree:table").unwrap();
    app.wait_until(
        "the probe settles to clean",
        Duration::from_secs(10),
        |screen| screen.texts().contains(&"clean"),
    )
    .unwrap();
    for header in ["PATH", "BRANCH", "HEAD", "STATE", "ACTIONS"] {
        assert!(app.screen().shows(header), "the column {header} paints");
    }
    assert!(
        app.screen().shows("linked"),
        "the linked checkout's branch paints"
    );
}

#[test]
fn a_worktree_is_added_and_removed_end_to_end() {
    let project = Project::new();
    let repo = support::seeded_repo(project.path(), "alpha");
    let mut app = worktrees_tab(&project, &repo);
    app.wait_for("id:worktree:add").unwrap();

    app.press("id:worktree:add").unwrap();
    app.wait_for("id:dialog:new-worktree").unwrap();
    let path = project.path().join("alpha-worktree");
    app.type_text("id:dialog:new-worktree:path", path.to_str().unwrap())
        .unwrap();
    app.type_text("id:dialog:new-worktree:branch", "worktree-branch")
        .unwrap();
    app.press("id:dialog:new-worktree:action").unwrap();

    for _ in 0..400 {
        if path.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(path.exists(), "the worktree's directory was created");
    app.wait_for("id:worktree:remove-alpha-worktree").unwrap();

    app.on_dialog(|_| DialogAnswer::Press("OK".to_string()));
    app.press("id:worktree:remove-alpha-worktree").unwrap();
    for _ in 0..400 {
        if !path.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the worktree's directory survived the remove");
}

fn super_with_submodule(project: &Project) -> PathBuf {
    let parent = project.path().join("sub-src");
    std::fs::create_dir_all(&parent).unwrap();
    git(&parent, &["init", "-q", "-b", "main"]);
    git(&parent, &["config", "user.email", "t@t"]);
    git(&parent, &["config", "user.name", "t"]);
    std::fs::write(parent.join("README.md"), "x\n").unwrap();
    git(&parent, &["add", "."]);
    git(&parent, &["commit", "-q", "-m", "init"]);
    let superproject = support::seeded_repo(project.path(), "super");
    git(
        &superproject,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            parent.to_str().unwrap(),
            "vendor/sub",
        ],
    );
    git(&superproject, &["commit", "-q", "-m", "add submodule"]);
    superproject
}

#[test]
fn the_submodules_pane_lists_the_status_and_updates_it() {
    let project = Project::new();
    let repo = super_with_submodule(&project);
    let mut app = submodules_tab(&project, &repo);
    app.wait_for("id:submodule:table").unwrap();
    app.wait_until(
        "the submodule row arrives",
        Duration::from_secs(15),
        |screen| screen.texts().contains(&"Up to date"),
    )
    .unwrap();

    app.on_dialog(|_| DialogAnswer::Press("OK".to_string()));
    app.press("id:submodule:Deinit-sub").unwrap();
    app.wait_until(
        "the row reads uninitialized",
        Duration::from_secs(15),
        |screen| screen.texts().contains(&"Uninitialized"),
    )
    .unwrap();

    app.press("id:submodule:Init-sub").unwrap();
    app.wait_until(
        "the row reads up to date again",
        Duration::from_secs(20),
        |screen| screen.texts().contains(&"Up to date"),
    )
    .unwrap();
}
