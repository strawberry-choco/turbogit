//! Issue #24 — acceptance-matrix verification with per-page screenshots.
//!
//! Renders every redesigned page through the headless harness and compares one
//! PNG per page against its committed baseline under
//! `tests/snapshots/` (egui_kittest's default snapshot directory, resolved
//! against this crate's root at test time). A plain run **writes nothing**: it
//! fails a page whose diff exceeds `kittest.toml`'s pixel budget and prints the
//! measured diff and its box for every page, over budget or not.
//!
//! A baseline changes only when the run is told to capture it, explicitly:
//!
//! ```text
//! UPDATE_SNAPSHOTS=force cargo test -p turbogit-ui --test acceptance_matrix
//! ```
//!
//! which repaints all eleven PNGs and leaves the previous ones beside them as
//! `{name}.old.png` for review (gitignored; the pair is what a reviewer compares
//! before committing). `force` is the only value this suite documents — see the
//! note on [`acceptance_matrix_screenshots`] for why `UPDATE_SNAPSHOTS=1` is not
//! a capture, and for the budget's floor.
//! Assertions here stay structural (the page paints its defining chrome); the
//! pixel-level check is the comparison above, and each page's own suite pins
//! its behaviour.
//!
//! The harness is [`CAPTURE_SIZE`] — 1440×900, the size `design-exports/` was
//! drawn and validated at. The frames are 2× exports of that box, so a capture
//! at any other size is not comparable with a frame at all, and a golden at the
//! wrong size is a golden nobody reviewed: it is a picture of the layout's
//! behaviour under a constraint the design never had.

use egui_kittest::Harness;
use std::path::PathBuf;
use test_support::git_seed::git;
use test_support::harness::shell_harness_over;
use turbogit_app::state::{AppState, Dialog};
use turbogit_domain::model::{RootId, Submodule, SubmoduleState, Worktree};

/// The box every page is captured at, in points: the frame size.
///
/// The `design-exports/` PNGs are 2880×1800, i.e. 2× exports of a 1440×900
/// frame, so this is the one size at which a capture and a frame are the same
/// picture. Stated as a named constant rather than as two literals so a
/// reviewer reading a capture's dimensions does not have to trust that the
/// number in the call site and the number in the commit message agree.
const CAPTURE_SIZE: egui::Vec2 = egui::vec2(1440.0, 900.0);

fn harness(state: AppState) -> Harness<'static, AppState> {
    shell_harness_over(state, CAPTURE_SIZE)
}

/// Run **exactly eight** frames.
///
/// Not either shared settle: a capture is a photograph of one frame, and which
/// frame is the whole content of the artifact. `settle` would run ten steps or
/// two, so two runs could photograph different frames and nothing would notice.
fn settle(h: &mut Harness<'_, AppState>) {
    for _ in 0..8 {
        h.step();
    }
}

// ---- the snapshot seam -----------------------------------------------------

/// How many pixels does [`dify`] count as failing at `threshold` — the same
/// call, with the same arguments, that egui_kittest makes to decide the verdict.
///
/// Recomputed here only to be *printed*: egui_kittest reports a count when a
/// snapshot fails and stays silent when it passes, so without this the suite
/// would show nothing about drift under the budget, which is the whole band the
/// stale captures live in.
fn diff_pixels(a: &image::RgbaImage, b: &image::RgbaImage, threshold: f32) -> i32 {
    dify::diff::get_results(a.clone(), b.clone(), threshold, true, None, &None, &None)
        .map_or(0, |(wrong, _)| wrong)
}

/// The box around every pixel pair that differs at all, at any tolerance.
///
/// Printed with the count because the count alone cannot be reviewed: 2 000 px
/// spread over a row of text and 2 000 px in one pane corner are different
/// findings. `(u32::MAX, ..)` when nothing differs.
fn differing_bbox(a: &image::RgbaImage, b: &image::RgbaImage) -> (u32, u32, u32, u32) {
    let mut box_ = (u32::MAX, u32::MAX, 0u32, 0u32);
    for (x, y, p) in a.enumerate_pixels() {
        if b.get_pixel(x, y) != p {
            box_.0 = box_.0.min(x);
            box_.1 = box_.1.min(y);
            box_.2 = box_.2.max(x);
            box_.3 = box_.3.max(y);
        }
    }
    box_
}

/// Compare one page's render against its committed baseline, print the measured
/// diff, and record the verdict with the harness.
///
/// The budget is `kittest.toml`'s, read through [`egui_kittest::SnapshotOptions`]
/// rather than restated here, so the number a reader sees and the number the
/// verdict is taken against cannot drift apart.
fn snapshot(h: &mut Harness<'_, AppState>, name: &str) {
    let options = egui_kittest::SnapshotOptions::default();
    let new = h
        .render()
        .unwrap_or_else(|err| panic!("{name}: the renderer produced no image: {err}"));

    let baseline = options.output_path.join(format!("{name}.png"));
    match image::open(&baseline) {
        Ok(previous) => {
            let previous = previous.to_rgba8();
            if previous.dimensions() == new.dimensions() {
                println!(
                    "GOLDEN {name}: {} differing px of {} budget, box {:?}",
                    diff_pixels(&previous, &new, options.threshold),
                    options.max_failed_pixels,
                    differing_bbox(&previous, &new),
                );
            } else {
                println!(
                    "GOLDEN {name}: no pixel count, baseline is {:?} and this render is {:?}",
                    previous.dimensions(),
                    new.dimensions(),
                );
            }
        }
        // The comparison itself reports a missing baseline; this only keeps the
        // printed line from being the silence a reviewer reads as "unchanged".
        Err(_) => println!(
            "GOLDEN {name}: no committed baseline at {}",
            baseline.display()
        ),
    }

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
    // A plain run COMPARES and writes nothing; capture is the explicit
    // `UPDATE_SNAPSHOTS=force` invocation documented at the top of this file.
    //
    // The budget's floor is not anti-aliasing. Two renders of the same frame
    // inside one process are byte-identical here (0 px on all eleven pages), so
    // the run-to-run noise the old comment blamed on wgpu is not what forces a
    // budget above zero. What forces it is the fixtures: every repo-backed page
    // paints the `tempfile` directory's random name in the shell header, the
    // seeded repo's two abbreviated commit SHAs change every run because git
    // stamps the commit time, and two pages paint the full temp path in a title
    // (`04-diff`, `07-merge-editor`). Measured on macOS (Apple GPU, 2026-10-10)
    // across separate processes, that is 0 px on `01-welcome` and 109-2 033 px
    // on the rest, worst on `07-merge-editor` — hence `kittest.toml`'s mac
    // budget of 4 000 (≈2× the floor) instead of the 8 000 that sat above it
    // with no measurement behind it.
    //
    // The same fact has a sharper edge than the number: a baseline that bakes
    // in a machine-local path and a per-run SHA can only ever match on the
    // machine and the fixtures that produced it. `04-diff` and
    // `07-merge-editor` are the two pages that paint a full `/var/folders/…`
    // path into their frame, which is the shape of the pair on record as
    // failing on macOS against a Windows-taken baseline. No threshold fixes
    // that; pinning the painted path and the seeded commit timestamps would,
    // and that is a fixture change, not one to make by editing a budget. The
    // printed per-page diff is what makes the band below the budget readable
    // in the meantime.
    //
    // `UPDATE_SNAPSHOTS=1` is deliberately NOT the documented capture: egui_kittest
    // maps it to `UpdateFailing`, which rewrites only a golden already over the
    // budget, so an under-budget visual change lands as a silent refresh — which
    // is exactly how the committed renders went stale once before, when the
    // 0.35 → 0.36 upgrade changed the meaning of "1".
    //
    // Headless CI runners (ubuntu-latest) expose no GPU adapter, so
    // egui_kittest's wgpu renderer cannot initialize there ("No adapter
    // found"). Upstream egui also only runs snapshot tests on GPU-backed
    // runners. This is the workspace's only `snapshot` caller, so under CI the
    // comparison above never runs anywhere: the goldens are human evidence on a
    // developer machine, not a gate.
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
    snapshot(&mut h, "01-welcome");
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
    snapshot(&mut h, "02-commit");

    // Git Log four panes with decorated history.
    h.state_mut().ui.tab = turbogit_app::state::Tab::Log;
    settle(&mut h);
    assert!(
        h.state().caches.log(&RootId(repo.clone().into())).is_some(),
        "log should be loaded"
    );
    snapshot(&mut h, "03-git-log");

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
    snapshot(&mut h, "03-git-log-selected");

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
    snapshot(&mut h, "04-diff");
    h.state_mut().ui.tab = turbogit_app::state::Tab::Log;
    settle(&mut h);

    // Branches popup.
    h.state_mut().ui.branches_popup = true;
    settle(&mut h);
    snapshot(&mut h, "05-branches-popup");
    h.state_mut().ui.branches_popup = false;

    // Push dialog.
    h.state_mut().ui.dialog = Some(Dialog::Push);
    settle(&mut h);
    snapshot(&mut h, "06-push-dialog");
    h.state_mut().ui.dialog = None;

    // Settings modal from the gear state.
    h.state_mut().ui.settings_open = true;
    settle(&mut h);
    snapshot(&mut h, "08-settings-modal");
    results.extend_harness(&mut h);
    drop(h);

    // --- Merge editor ----------------------------------------------------
    let (_ctmp, crepo) = conflicted_repo();
    let mut state = AppState::for_roots(crepo.parent().unwrap(), std::slice::from_ref(&crepo));
    // Open the merge editor for the conflicted file directly.
    state.ui.conflict_open = Some(crepo.join("conf.txt"));
    let mut h = harness(state);
    settle(&mut h);
    snapshot(&mut h, "07-merge-editor");
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
    snapshot(&mut h, "09-worktrees");

    h.state_mut().ui.tab = turbogit_app::state::Tab::Submodules;
    settle(&mut h);
    snapshot(&mut h, "10-submodules");
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
