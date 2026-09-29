//! Issue #24 — acceptance-matrix verification with per-page screenshots.
//!
//! Renders every redesigned page through the headless harness and saves one
//! PNG per page under egui_kittest's default snapshot directory
//! (`tests/snapshots/`, resolved against this crate's root at test time).
//! Assertions here are deliberately structural (the page paints its defining
//! chrome); pixel-level checks live in each page's own suite.
//!
//! The harness is [`CAPTURE_SIZE`] — 1440×900, the size `design-exports/` was
//! drawn and validated at. The frames are 2× exports of that box, so a capture
//! at any other size is not comparable with a frame at all, and a golden at the
//! wrong size is a golden nobody reviewed: it is a picture of the layout's
//! behaviour under a constraint the design never had.

use egui_kittest::Harness;
use std::path::{Path, PathBuf};
use turbogit_app::state::{AppState, Dialog};
use turbogit_domain::model::{RootId, Submodule, SubmoduleState, Worktree};
use turbogit_ui::theme::{configure_style, install_fonts};

/// The box every page is captured at, in points: the frame size.
///
/// The `design-exports/` PNGs are 2880×1800, i.e. 2× exports of a 1440×900
/// frame, so this is the one size at which a capture and a frame are the same
/// picture. Stated as a named constant rather than as two literals so a
/// reviewer reading a capture's dimensions does not have to trust that the
/// number in the call site and the number in the commit message agree.
const CAPTURE_SIZE: egui::Vec2 = egui::vec2(1440.0, 900.0);

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
            turbogit_ui::ui::render(ui, state);
        },
        state,
    );
    h.set_size(CAPTURE_SIZE);
    h
}

fn settle(h: &mut Harness<'_, AppState>) {
    for _ in 0..8 {
        h.step();
    }
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

/// Seed a repo with a real two-hunk merge conflict (mirrors `merge_editor`).
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

/// A repo with a little history, for the two captures whose subject is a pane
/// the shell fills from the root caches rather than from a git read of its own.
fn context_repo() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let repo = tmp.path().join("workbench");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    std::fs::write(repo.join("README.md"), "# workbench\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "initial import"]);
    git(&repo, &["checkout", "-q", "-b", "feature/tokens"]);
    std::fs::write(repo.join("tokens.txt"), "accent\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "feature: add tokens"]);
    git(&repo, &["checkout", "-q", "main"]);
    (tmp, repo)
}

/// Fill the Worktrees and Submodules caches with one row per state each pane can
/// render, so the two new captures are not photographs of two empty tables.
///
/// **The caches are seeded, not fetched, and that is a choice about the
/// evidence rather than about the panes.** Both panes are fed by an async
/// worker round-trip, and both read a value a golden cannot afford to race:
///
/// - The Worktrees pane's PATH cell prints a worktree's **absolute** path,
///   truncated to the column's width. A linked worktree created under a
///   temp directory therefore paints the same machine-local prefix
///   (`/var/folders/<hash>/…` on macOS) in every row, and the capture stops
///   being readable as a picture of three distinct worktrees.
/// - The Worktrees pane's STATE cell is `probing…` until a per-worktree dirty
///   probe settles, so a fetched list photographs whichever frame the pump
///   happened to be on.
///
/// Neither value is invented here: both are what git reports, and the fetch
/// path — the actual `git worktree list` / `git submodule status` round trip,
/// the badge counts and the dirty probes — has its own suite in
/// `worktrees_submodules_tabs.rs`. A golden that varies with a worker's
/// schedule is not a golden. Seeding `dirty` on every row also means
/// `ensure_worktree_probes` finds no unknown row to dispatch, so the capture
/// cannot photograph a probe in flight.
///
/// The rows cover the arms a reviewer needs to see at once: three worktrees
/// (one dirty, two clean — the main worktree is deliberately absent, because
/// the engine's `worktree_list` filters `path == root` and it can never appear)
/// and four submodules covering all but one lifecycle state, including the
/// uninitialised one whose action is the `Init` verb rather than `Update`.
fn seed_worktree_and_submodule_panes(state: &mut AppState) {
    let root = state.selected_root.clone().expect("a focused root");
    let worktrees: Vec<Worktree> = [
        ("~/dev/wt-hotfix", "hotfix/4412", Some(true)),
        ("~/dev/wt-review", "review/qa", Some(false)),
        ("~/dev/wt-release", "release/2026.09", Some(false)),
    ]
    .into_iter()
    .map(|(path, branch, dirty)| Worktree {
        path: PathBuf::from(path),
        branch: branch.to_string(),
        dirty,
        root: root.clone(),
    })
    .collect();
    state.caches.store_worktrees(root.clone(), worktrees);

    let submodules = vec![
        Submodule {
            path: "vendor/analytics-sdk".into(),
            head: Some("a60d73a".into()),
            recorded: Some("a60d73a".into()),
            state: SubmoduleState::UpToDate,
            root: root.clone(),
        },
        Submodule {
            // The checkout moved off the record, so this is the row that shows
            // CHECKED OUT wearing the divergence colour while RECORDED stays in
            // secondary ink, and the row that shows the two columns disagreeing.
            path: "vendor/design-tokens".into(),
            head: Some("7c1e0aa".into()),
            recorded: Some("40dd7e8".into()),
            state: SubmoduleState::NeedsUpdate,
            root: root.clone(),
        },
        Submodule {
            // No working copy: CHECKED OUT empty, the muted mark, and `Init` in
            // the action column rather than `Update`.
            path: "vendor/proto-schemas".into(),
            head: None,
            recorded: Some("40dd7e8".into()),
            state: SubmoduleState::Uninitialized,
            root: root.clone(),
        },
        Submodule {
            path: "vendor/legacy-ui".into(),
            head: Some("5b8c7e7".into()),
            recorded: Some("9a1c4f2".into()),
            state: SubmoduleState::Conflicted,
            root,
        },
    ];
    state.caches.store_submodules(
        state.selected_root.clone().expect("a focused root"),
        submodules,
    );
}

#[test]
fn acceptance_matrix_screenshots() {
    // This suite produces acceptance-matrix EVIDENCE, not a pixel-CI gate:
    // every run refreshes the committed renders (wgpu antialiasing jitters
    // a few hundred pixels between runs, which would flake a comparison).
    // Real regression protection lives in each page's own assertion suite.
    // Safe: single-threaded test process; no other thread reads the env
    // concurrently while this runs.
    //
    // "force", not "1": egui_kittest maps `UPDATE_SNAPSHOTS=1` to
    // `UpdateFailing`, which only rewrites a golden that already exceeds
    // `kittest.toml`'s pixel budget. A small visual change stays under that
    // budget, so the run reports green and the committed render goes quietly
    // stale — which is exactly what happened after the 0.35 → 0.36 upgrade
    // changed the meaning of "1". `force` is `UpdateAll`.
    unsafe { std::env::set_var("UPDATE_SNAPSHOTS", "force") };
    // Headless CI runners (ubuntu-latest) expose no GPU adapter, so
    // egui_kittest's wgpu renderer cannot initialize there ("No adapter
    // found"). Upstream egui also only runs snapshot tests on GPU-backed
    // runners. Renders refreshed here are never consumed by CI, so skip.
    if std::env::var_os("CI").is_some() {
        eprintln!("skipping acceptance-matrix screenshots: no GPU adapter on CI runners");
        return;
    }
    let mut results = egui_kittest::SnapshotResults::default();

    // --- Welcome (no project open) -------------------------------------
    let project = tempfile::tempdir().unwrap();
    // Recents live in ONE global file under the OS config dir (ADR-0005), and
    // they are loaded *eagerly* inside `launch_in`, so the config dir has to be
    // injected at construction — setting `recents_config_dir` afterwards is
    // write-only and changes nothing. Without this the Welcome card photographs
    // the *developer's* recent projects: real absolute paths and last-opened
    // timestamps committed into a tracked binary asset, and a capture that can
    // never match on another machine.
    let recents_config = tempfile::tempdir().unwrap();
    let state = AppState::launch_in(
        Some(project.path().to_path_buf()),
        Some(recents_config.path().to_path_buf()),
    );
    let mut h = harness(state);
    settle(&mut h);
    assert!(
        h.state().show_welcome(),
        "no-project launch must land on Welcome"
    );
    // The capture must not name a path outside its own fixtures.
    let painted = test_support::harness::painted_text(&h).join("\n");
    let home = std::env::var("HOME").unwrap_or_default();
    assert!(
        (home.is_empty() || !painted.contains(&home))
            && !painted.contains(&*recents_config.path().to_string_lossy()),
        "the welcome capture must not photograph the developer's recent \
         projects; point `recents_config_dir` at a temp dir. Painted:\n{painted}"
    );
    h.snapshot("01-welcome");
    results.extend_harness(&mut h);
    drop(h);
    drop(project);
    drop(recents_config);

    // --- Repo-backed pages ---------------------------------------------
    let (_tmp, repo) = seeded_repo();
    let state = AppState::for_roots(repo.parent().unwrap(), std::slice::from_ref(&repo));
    let mut h = harness(state);

    // Commit tool window (default tab) with changelist + preview.
    settle(&mut h);
    h.snapshot("02-commit");

    // Git Log four panes with decorated history.
    h.state_mut().ui.tab = turbogit_app::state::Tab::Log;
    settle(&mut h);
    assert!(
        h.state().caches.log(&RootId(repo.clone().into())).is_some(),
        "log should be loaded"
    );
    h.snapshot("03-git-log");

    // The same page with a commit selected, so the redesigned changed-files
    // rows and the commit-details blocks carry content (logs-panels redesign).
    // The line counts are a worker round-trip, so the pump pauses for them
    // rather than racing them.
    let head = git(&repo, &["rev-parse", "HEAD"]).trim().to_owned();
    h.state_mut().ui.selected_commit = Some(head);
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(25));
        h.step();
    }
    h.snapshot("03-git-log-selected");

    // Diff viewer over the working-tree change.
    //
    // **The tab matters, and it used to be wrong.** The diff preview pane is
    // painted by `commit_window`, so `ui.preview_change` is only rendered on
    // the **Changes** tab. This page was captured on the Log tab the three
    // snapshots above left open, where `preview_change` renders nothing at all
    // — so `04-diff` was a second picture of the log page with no diff in it,
    // and every claim about the diff pane's own background, text and row fills
    // had nothing to be read off. The tab is switched here and switched back
    // below, because the three captures after this one are overlays taken over
    // the Log page and must stay over the Log page.
    h.state_mut().ui.preview_change = Some(repo.join("lib.txt"));
    h.state_mut().ui.tab = turbogit_app::state::Tab::Commit;
    settle(&mut h);
    h.snapshot("04-diff");
    h.state_mut().ui.tab = turbogit_app::state::Tab::Log;
    settle(&mut h);

    // Branches popup.
    h.state_mut().ui.branches_popup = true;
    settle(&mut h);
    h.snapshot("05-branches-popup");
    h.state_mut().ui.branches_popup = false;

    // Push dialog.
    h.state_mut().ui.dialog = Some(Dialog::Push);
    settle(&mut h);
    h.snapshot("06-push-dialog");
    h.state_mut().ui.dialog = None;

    // Settings modal from the gear state.
    h.state_mut().ui.settings_open = true;
    settle(&mut h);
    h.snapshot("08-settings-modal");
    results.extend_harness(&mut h);
    drop(h);

    // --- Merge editor ----------------------------------------------------
    let (_ctmp, crepo) = conflicted_repo();
    let mut state = AppState::for_roots(crepo.parent().unwrap(), std::slice::from_ref(&crepo));
    // Open the merge editor for the conflicted file directly.
    state.ui.conflict_open = Some(crepo.join("conf.txt"));
    let mut h = harness(state);
    settle(&mut h);
    h.snapshot("07-merge-editor");
    results.extend_harness(&mut h);
    drop(h);

    // --- Worktrees & Submodules panes ------------------------------------
    // Their own AppState, for the same reason the merge editor has one: these
    // are whole tool windows over a second root, and a second root is a second
    // shell, not a mutation of the first. Numbered 09 and 10 — appended rather
    // than inserted, so the nine existing captures keep the names a reviewer
    // has already looked at, and so the log's two variants under one number
    // stays the only place the scheme repeats itself.
    let (_ctmp2, wrepo) = context_repo();
    let mut state = AppState::for_roots(wrepo.parent().unwrap(), std::slice::from_ref(&wrepo));
    seed_worktree_and_submodule_panes(&mut state);
    let mut h = harness(state);
    h.state_mut().ui.tab = turbogit_app::state::Tab::Worktrees;
    settle(&mut h);
    assert!(
        h.state()
            .caches
            .worktrees(&h.state().selected_root.clone().expect("a focused root"))
            .is_some(),
        "the Worktrees pane's rows should be in the cache before it is captured"
    );
    h.snapshot("09-worktrees");

    h.state_mut().ui.tab = turbogit_app::state::Tab::Submodules;
    settle(&mut h);
    h.snapshot("10-submodules");
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
