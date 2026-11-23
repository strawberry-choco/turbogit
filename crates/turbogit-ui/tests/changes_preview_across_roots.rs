//! The changes preview across workspace roots, ported from the egui
//! `changes_preview_across_roots` suite: a row previews the change its own
//! repository owns, even when another root holds a file of the same name,
//! and a nested file in a non-focused root still answers its own diff.

use std::path::{Path, PathBuf};

use support::{Project, commit_tab, open_group, shows_part};

mod support;

use test_support::git_seed::git;

fn init_repo(parent: &Path, name: &str) -> PathBuf {
    let repo = parent.join(name);
    std::fs::create_dir_all(&repo).expect("repo dir");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    repo
}

fn commit_file(repo: &Path, name: &str, content: &str) {
    if let Some(parent) = repo.join(name).parent() {
        std::fs::create_dir_all(parent).expect("dir");
    }
    std::fs::write(repo.join(name), content).expect("write");
    git(repo, &["add", name]);
    git(repo, &["commit", "-q", "-m", "seed"]);
}

fn workspace(tmp: &Path) -> (PathBuf, PathBuf) {
    let alpha = init_repo(tmp, "alpha");
    let beta = init_repo(tmp, "beta");
    commit_file(&alpha, "shared.txt", "alpha version\n");
    commit_file(&beta, "shared.txt", "beta version\n");
    commit_file(&beta, "deep/nested-beta-file.txt", "one\n");
    std::fs::write(alpha.join("shared.txt"), "alpha version CHANGED\n").unwrap();
    std::fs::write(beta.join("shared.txt"), "beta version CHANGED\n").unwrap();
    std::fs::write(beta.join("deep/nested-beta-file.txt"), "one\ntwo\n").unwrap();
    (alpha, beta)
}

fn shell(project: &Project, roots: &[PathBuf]) -> support::Mounted {
    let mut app = commit_tab(project.path(), roots);
    for root in roots {
        let name = root.file_name().unwrap().to_string_lossy().to_string();
        open_group(&mut app, &name);
    }
    app
}

#[test]
fn a_change_owned_by_another_root_previews_that_change() {
    let project = Project::new();
    let (alpha, beta) = workspace(project.path());
    let mut app = shell(&project, &[alpha, beta]);

    app.press("id:changes:row:beta:deep/nested-beta-file.txt")
        .unwrap();
    app.wait_until(
        "beta's edit paints",
        std::time::Duration::from_secs(15),
        |screen| screen.texts().iter().any(|text| text.trim() == "two"),
    )
    .unwrap();

    assert!(
        !shows_part(&app, "(no differences)"),
        "beta's edit is real; the pane answered for the focused root instead"
    );
    assert!(
        shows_part(&app, "two"),
        "the pane shows the line beta's worktree added: {:?}",
        app.screen().texts()
    );
}

#[test]
fn two_roots_with_the_same_relative_path_preview_separately() {
    let project = Project::new();
    let (alpha, beta) = workspace(project.path());
    let mut app = shell(&project, &[alpha, beta]);

    app.press("id:changes:row:beta:shared.txt").unwrap();
    app.wait_until(
        "beta's row answers",
        std::time::Duration::from_secs(15),
        |screen| {
            screen
                .texts()
                .iter()
                .any(|text| text.contains("beta version CHANGED"))
        },
    )
    .unwrap();
    assert!(
        !shows_part(&app, "alpha version CHANGED"),
        "alpha's line belongs to alpha's row: {:?}",
        app.screen().texts()
    );

    app.press("id:changes:row:alpha:shared.txt").unwrap();
    app.wait_until(
        "alpha's row answers",
        std::time::Duration::from_secs(15),
        |screen| {
            screen
                .texts()
                .iter()
                .any(|text| text.contains("alpha version CHANGED"))
        },
    )
    .unwrap();
    assert!(
        !shows_part(&app, "beta version CHANGED"),
        "the same relative path in the other root is a different change: {:?}",
        app.screen().texts()
    );
}
