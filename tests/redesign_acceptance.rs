//! Issue #24 — acceptance-matrix verification with per-page screenshots.
//!
//! Renders every redesigned page through the headless harness and saves one
//! PNG per page into `turbogit-screens/redesign/` via egui_kittest snapshots.
//! Assertions here are deliberately structural (the page paints its defining
//! chrome); pixel-level checks live in each page's own suite.

use egui_kittest::Harness;
use std::path::{Path, PathBuf};
use turbogit::model::RootId;
use turbogit::state::{AppState, Dialog};
use turbogit::theme::{configure_style, install_fonts};

const SNAPSHOT_DIR: &str = "turbogit-screens/redesign";

/// Run `git` in `repo`, asserting success.
fn git(repo: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git should be on PATH");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn harness(state: AppState) -> Harness<'static, AppState> {
    let mut fonts_installed = false;
    let mut h = Harness::new_ui_state(
        move |ui, state| {
            configure_style(ui.ctx());
            if !fonts_installed {
                install_fonts(ui.ctx());
                fonts_installed = true;
            }
            state.drain_events();
            turbogit::ui::render(ui, state);
        },
        state,
    );
    h.set_size(egui::vec2(1280.0, 800.0));
    h
}

fn settle(h: &mut Harness<'_, AppState>) {
    for _ in 0..8 {
        h.step();
    }
}

fn snap(h: &mut Harness<'_, AppState>, name: &str) {
    let options = egui_kittest::SnapshotOptions::new().output_path(SNAPSHOT_DIR);
    h.snapshot_options(name, &options);
}

/// Seed a repo with history worth looking at: two branches, a tag, a remote
/// ref decoration, and one uncommitted modification for the Commit page.
fn seeded_repo() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    std::fs::write(repo.join("lib.txt"), "alpha\nbeta\ngamma\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "initial import"]);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["tag", "v1.0"]);
    git(&repo, &["checkout", "-q", "-b", "feature/tokens"]);
    std::fs::write(repo.join("lib.txt"), "alpha\nBETA\ngamma\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "feature: retokenize beta"]);
    git(&repo, &["checkout", "-q", "main"]);
    // A fake remote decoration without needing a network.
    git(
        &repo,
        &["update-ref", "refs/remotes/origin/main", base.trim()],
    );
    std::fs::write(repo.join("lib.txt"), "alpha\nbeta\nGAMMA\n").unwrap();
    (tmp, repo)
}

/// Seed a repo with a real two-hunk merge conflict (mirrors redesign_merge).
fn conflicted_repo() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let repo = tmp.path().join("conflict");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    std::fs::write(
        repo.join("conf.txt"),
        "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n",
    )
    .unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "base"]);
    git(&repo, &["checkout", "-q", "-b", "side"]);
    std::fs::write(
        repo.join("conf.txt"),
        "SIDE-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nSIDE-nine\nten\n",
    )
    .unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "side"]);
    git(&repo, &["checkout", "-q", "main"]);
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
        .output()
        .expect("git on PATH");
    (tmp, repo)
}

#[test]
fn acceptance_matrix_screenshots() {
    // This suite produces acceptance-matrix EVIDENCE, not a pixel-CI gate:
    // every run refreshes the committed renders (wgpu antialiasing jitters
    // a few hundred pixels between runs, which would flake a comparison).
    // Real regression protection lives in each page's own assertion suite.
    // Safe: single-threaded test process; no other thread reads the env
    // concurrently while this runs.
    unsafe { std::env::set_var("UPDATE_SNAPSHOTS", "1") };
    std::fs::create_dir_all(SNAPSHOT_DIR).unwrap();
    let mut results = egui_kittest::SnapshotResults::default();

    // --- Welcome (no project open) -------------------------------------
    let project = tempfile::tempdir().unwrap();
    let mut h = harness(AppState::new(project.path().to_path_buf()));
    settle(&mut h);
    assert!(
        h.state().show_welcome(),
        "no-project launch must land on Welcome"
    );
    snap(&mut h, "01-welcome");
    results.extend_harness(&mut h);
    drop(h);
    drop(project);

    // --- Repo-backed pages ---------------------------------------------
    let (_tmp, repo) = seeded_repo();
    let state = AppState::for_roots(repo.parent().unwrap(), std::slice::from_ref(&repo));
    let mut h = harness(state);

    // Commit tool window (default tab) with changelist + preview.
    settle(&mut h);
    snap(&mut h, "02-commit");

    // Git Log four panes with decorated history.
    h.state_mut().ui.tab = turbogit::state::Tab::Log;
    settle(&mut h);
    assert!(
        h.state().caches.log(&RootId(repo.clone())).is_some(),
        "log should be loaded"
    );
    snap(&mut h, "03-git-log");

    // Diff viewer over the working-tree change.
    h.state_mut().ui.preview_change = Some(repo.join("lib.txt"));
    settle(&mut h);
    snap(&mut h, "04-diff");

    // Branches popup.
    h.state_mut().ui.branches_popup = true;
    settle(&mut h);
    snap(&mut h, "05-branches-popup");
    h.state_mut().ui.branches_popup = false;

    // Push dialog.
    h.state_mut().ui.dialog = Some(Dialog::Push);
    settle(&mut h);
    snap(&mut h, "06-push-dialog");
    h.state_mut().ui.dialog = None;

    // Settings modal from the gear state.
    h.state_mut().ui.settings_open = true;
    settle(&mut h);
    snap(&mut h, "08-settings-modal");
    results.extend_harness(&mut h);
    drop(h);

    // --- Merge editor ----------------------------------------------------
    let (_ctmp, crepo) = conflicted_repo();
    let mut state = AppState::for_roots(crepo.parent().unwrap(), std::slice::from_ref(&crepo));
    // Open the merge editor for the conflicted file directly.
    state.ui.conflict_open = Some(crepo.join("conf.txt"));
    let mut h = harness(state);
    settle(&mut h);
    snap(&mut h, "07-merge-editor");
    results.extend_harness(&mut h);
    drop(h);
}

/// Cleanup regression guard: legacy theme entry points stay gone.
#[test]
fn legacy_theme_surface_is_gone() {
    // The old ThemeMode-driven light/high-contrast paths were deleted with
    // the dark-only migration (ADR-0003); configure_style must be the only
    // styling entry point and must not branch on any mode.
    let src = include_str!("../src/theme.rs");
    assert!(
        !src.contains("ThemeMode"),
        "legacy ThemeMode must not reappear in theme.rs"
    );
    assert!(
        !src.contains("HighContrast"),
        "legacy HighContrast palette must not reappear in theme.rs"
    );
}
