//! The merge editor, ported from the egui `merge_editor` suite: three
//! panes, the read-only result with its placeholder, the per-block verbs,
//! the apply gate, and the composed file Apply writes.

use std::path::{Path, PathBuf};

use support::{Mounted, Project, for_roots_seeded};
use test_support::git_seed::git;
use turbogit_app::root_caches::Affected;
use turbogit_ui::screens::conflicts;

mod support;

const COMPOSED: &str = "MAIN-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nSIDE-nine\nten\n";

fn repo(project: &Project, name: &str) -> PathBuf {
    let repo = support::seeded_repo(project.path(), name);
    std::fs::write(
        repo.join("conf.txt"),
        "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n",
    )
    .unwrap();
    git(&repo, &["add", "conf.txt"]);
    git(&repo, &["commit", "-q", "-m", "base conf"]);
    git(&repo, &["checkout", "-q", "-b", "side"]);
    std::fs::write(
        repo.join("conf.txt"),
        "SIDE-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nSIDE-nine\nten\n",
    )
    .unwrap();
    git(&repo, &["add", "conf.txt"]);
    git(&repo, &["commit", "-q", "-m", "side commit"]);
    git(&repo, &["checkout", "-q", "main"]);
    std::fs::write(
        repo.join("conf.txt"),
        "MAIN-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nMAIN-nine\nten\n",
    )
    .unwrap();
    git(&repo, &["add", "conf.txt"]);
    git(&repo, &["commit", "-q", "-m", "main commit"]);
    let _ = std::process::Command::new("git")
        .args(["merge", "--no-edit", "side"])
        .current_dir(&repo)
        .output();
    repo
}

fn editor(project: &Project, repo: &Path) -> Mounted {
    let roots = vec![repo.to_path_buf()];
    let mut app = for_roots_seeded(project.path(), &roots, |s| {
        s.refresh(Affected::All);
        s.drain_events();
        let root = s.multi.roots[0].id.clone();
        conflicts::open_merge_editor_in(s, &root, Path::new("conf.txt"));
    });
    app.first_render().expect("the shell mounts");
    app.wait_for("id:dialog:merge-editor")
        .expect("the merge editor paints");
    app
}

#[test]
fn the_editor_renders_three_panes_with_the_placeholder() {
    let project = Project::new();
    let repo = repo(&project, "panes");
    let app = editor(&project, &repo);
    for text in [
        "Local (Yours)",
        "Result",
        "Incoming (Theirs)",
        "MAIN-one",
        "SIDE-one",
        "<< unresolved >>",
        "2 conflicts remaining",
    ] {
        assert!(support::shows_part(&app, text), "{text} paints");
    }
}

#[test]
fn the_accept_buttons_resolve_blocks_and_the_counter() {
    let project = Project::new();
    let repo = repo(&project, "resolve");
    let mut app = editor(&project, &repo);
    app.press("id:conflicts:merge-take:0:1")
        .expect("accept yours 1");
    app.wait_for("text:/1 conflict remaining/").unwrap();
    app.press("id:conflicts:merge-take:1:2")
        .expect("accept theirs 2");
    app.wait_for("text:/0 conflicts remaining/").unwrap();
    assert!(
        app.screen()
            .enabled("id:conflicts:merge-apply")
            .unwrap_or(false),
        "Apply arms at zero remaining"
    );
}

#[test]
fn ignore_keeps_both_sides_and_counts_as_resolved() {
    let project = Project::new();
    let repo = repo(&project, "ignore");
    let mut app = editor(&project, &repo);
    app.press("id:conflicts:merge-take:2:1").expect("ignore 1");
    app.wait_for("text:/1 conflict remaining/").unwrap();
    assert!(support::shows_part(&app, "MAIN-one"));
    assert!(support::shows_part(&app, "SIDE-one"));
}

#[test]
fn apply_writes_the_composed_result_and_clears_the_conflict() {
    let project = Project::new();
    let repo = repo(&project, "apply");
    let mut app = editor(&project, &repo);
    app.press("id:conflicts:merge-take:0:1").expect("ours 1");
    app.press("id:conflicts:merge-take:1:2").expect("theirs 2");
    app.press("id:conflicts:merge-apply").expect("apply");
    assert!(
        test_support::wait_until(15_000, || {
            std::fs::read_to_string(repo.join("conf.txt"))
                .map(|text| text == COMPOSED)
                .unwrap_or(false)
                && !git(&repo, &["status", "--porcelain"])
                    .lines()
                    .any(|line| line.starts_with("UU"))
        }),
        "the composed result lands: {}",
        git(&repo, &["status", "--porcelain"])
    );
}
