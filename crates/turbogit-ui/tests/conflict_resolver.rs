//! The conflict resolver, ported from the egui `conflict_resolver` suite:
//! the three panes, the per-conflict verbs and undo, auto-advance, bulk
//! resolve and its report, the overwrite gate, the file list split, and the
//! held merge's abort and continue.

use std::path::{Path, PathBuf};
use std::time::Duration;

use support::{Mounted, Project, for_roots_seeded};
use test_support::git_seed::git;
use turbogit_app::root_caches::Affected;
use turbogit_ui::screens::conflicts;

mod support;

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

fn resolve_over(project: &Project, repo: &Path) -> Mounted {
    let roots = vec![repo.to_path_buf()];
    let mut app = for_roots_seeded(project.path(), &roots, |s| {
        s.refresh(Affected::All);
        s.drain_events();
        let root = s.multi.roots[0].id.clone();
        conflicts::open_resolver_in(s, &root, Path::new("conf.txt"));
    });
    app.first_render().expect("the shell mounts");
    app.wait_for("id:dialog:resolve-conflicts")
        .expect("the resolver paints");
    app
}

#[test]
fn the_resolver_renders_three_panes_and_its_verbs() {
    let project = Project::new();
    let repo = repo(&project, "resolver");
    let app = resolve_over(&project, &repo);
    for text in [
        "Local (Yours)",
        "Result",
        "Incoming (Theirs)",
        "Abort merge",
        "Continue merge",
        "Take ours 1",
        "Take theirs 1",
        "Take both 1",
        "Edit manually",
        "Apply to all 2 remaining",
        "Auto-advance to next conflict",
    ] {
        assert!(support::shows_part(&app, text), "{text} paints");
    }
    assert!(support::shows_part(&app, "2 conflicts remaining"));
}

#[test]
fn take_both_keeps_theirs_before_ours() {
    let project = Project::new();
    let repo = repo(&project, "both");
    let mut app = resolve_over(&project, &repo);
    app.press("id:conflicts:take:3:1").expect("take both");
    app.wait_for("text:both, theirs first")
        .expect("the block reads its choice");
    app.wait_for("text:1 conflict remaining").unwrap();
}

#[test]
fn per_conflict_undo_reverts_its_resolution() {
    let project = Project::new();
    let repo = repo(&project, "undo");
    let mut app = resolve_over(&project, &repo);
    app.press("id:conflicts:take:0:1").expect("take ours");
    app.wait_for("text:1 conflict remaining").unwrap();
    app.press("id:conflicts:undo:1").expect("undo");
    app.wait_for("text:2 conflicts remaining")
        .expect("undo reverts the block");
}

#[test]
fn auto_advance_moves_focus_after_a_resolution() {
    let project = Project::new();
    let repo = repo(&project, "auto");
    let mut app = resolve_over(&project, &repo);
    app.press("id:conflicts:auto-advance")
        .expect("auto-advance on");
    app.press("id:conflicts:take:0:1").expect("take ours");
    app.wait_for("text:Conflict 2")
        .expect("the focus advances to the next conflict");
}

#[test]
fn apply_to_all_reports_all_resolved() {
    let project = Project::new();
    let repo = repo(&project, "bulk");
    let mut app = resolve_over(&project, &repo);
    app.press("id:conflicts:apply-all").expect("apply to all");
    app.wait_for("text:All conflicts were resolved automatically")
        .expect("the positive report paints");
    assert!(
        app.screen().enabled("id:conflicts:apply").unwrap_or(false),
        "the finish control arms at zero remaining"
    );
}

#[test]
fn apply_writes_every_hunk_and_clears_the_conflict() {
    let project = Project::new();
    let repo = repo(&project, "apply");
    let mut app = resolve_over(&project, &repo);
    app.press("id:conflicts:take:1:1").expect("take theirs 1");
    app.press("id:conflicts:next").expect("next");
    app.press("id:conflicts:take:0:2").expect("take ours 2");
    app.wait_for("id:conflicts:overwrite")
        .expect("the overwrite gate arms");
    app.press("id:conflicts:discard-accept")
        .expect("discard and accept");
    app.press("id:conflicts:apply").expect("apply");
    assert!(
        test_support::wait_until(15_000, || {
            std::fs::read_to_string(repo.join("conf.txt"))
                .map(|text| {
                    text == "SIDE-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nMAIN-nine\nten\n"
                })
                .unwrap_or(false)
                && !git(&repo, &["status", "--porcelain"])
                    .lines()
                    .any(|line| line.starts_with("UU"))
        }),
        "the resolver wrote the composed result: {}",
        git(&repo, &["status", "--porcelain"])
    );
}

#[test]
fn abort_merge_restores_the_pre_merge_tree() {
    let project = Project::new();
    let repo = repo(&project, "abort");
    let mut app = resolve_over(&project, &repo);
    app.press("id:conflicts:abort-merge").expect("abort");
    assert!(
        test_support::wait_until(15_000, || {
            std::fs::read_to_string(repo.join("conf.txt"))
                .map(|text| text.starts_with("MAIN-one"))
                .unwrap_or(false)
                && !git(&repo, &["status", "--porcelain"])
                    .lines()
                    .any(|line| line.starts_with("UU"))
        }),
        "the merge is aborted and the tree restored"
    );
}

#[test]
fn continue_merge_completes_once_every_conflict_is_resolved() {
    let project = Project::new();
    let repo = repo(&project, "continue");
    let mut app = resolve_over(&project, &repo);
    app.press("id:conflicts:apply-all").expect("apply to all");
    app.press("id:conflicts:apply").expect("apply");
    app.wait_for("text:/0 conflicts remaining|2 conflicts remaining/")
        .unwrap();
    app.press("id:conflicts:continue-merge")
        .expect("continue merge");
    assert!(
        test_support::wait_until(15_000, || {
            git(&repo, &["status", "--porcelain"]).trim().is_empty()
        }),
        "the merge completes: {}",
        git(&repo, &["status", "--porcelain"])
    );
}

#[test]
fn the_file_list_splits_conflicted_from_auto_merged() {
    let project = Project::new();
    let repo = support::seeded_repo(project.path(), "auto-merged");
    std::fs::write(repo.join("auto.txt"), "shared-0\n").unwrap();
    std::fs::write(
        repo.join("conf.txt"),
        "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n",
    )
    .unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "base"]);
    git(&repo, &["checkout", "-q", "-b", "side"]);
    std::fs::write(repo.join("auto.txt"), "shared-1\n").unwrap();
    std::fs::write(
        repo.join("conf.txt"),
        "SIDE-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nSIDE-nine\nten\n",
    )
    .unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "side"]);
    git(&repo, &["checkout", "-q", "main"]);
    std::fs::write(repo.join("auto.txt"), "shared-1\n").unwrap();
    std::fs::write(
        repo.join("conf.txt"),
        "MAIN-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nMAIN-nine\nten\n",
    )
    .unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "main"]);
    let _ = std::process::Command::new("git")
        .args(["merge", "--no-edit", "side"])
        .current_dir(&repo)
        .output();

    let mut app = for_roots_seeded(project.path(), std::slice::from_ref(&repo), |s| {
        s.refresh(Affected::All);
        s.drain_events();
        let root = s.multi.roots[0].id.clone();
        conflicts::open_resolver_in(s, &root, Path::new("conf.txt"));
    });
    app.first_render().expect("the shell mounts");
    app.wait_for("id:dialog:resolve-conflicts").unwrap();
    for text in ["CONFLICTED FILES", "AUTO-MERGED", "conf.txt", "auto.txt"] {
        assert!(support::shows_part(&app, text), "{text} paints");
    }
}

#[test]
fn file_rows_paint_their_conflict_kind() {
    let project = Project::new();
    let repo = repo(&project, "kinds");
    let app = resolve_over(&project, &repo);
    assert!(support::shows_part(&app, "both modified"));
}

#[allow(dead_code)]
fn quiet() {
    std::thread::sleep(Duration::from_millis(1));
}
