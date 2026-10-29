//! Issue #24, acceptance-matrix verification with per-page screenshots.
//!
//! Renders every redesigned page through the headless harness and compares one
//! PNG per page against its committed baseline under
//! `tests/snapshots/` (egui_kittest's default snapshot directory, resolved
//! against this crate's root at test time). A plain run **writes nothing**: it
//! fails a page whose diff exceeds `kittest.toml`'s pixel budget and prints the
//! measured diff and its box for every page, over budget or not.
//!
//! Those printed numbers appear only under `-- --nocapture`, because `cargo test`
//! captures a *passing* test's stdout. A green run that says nothing therefore did
//! not measure nothing, it measured and stayed under the budget, which is the one
//! case a reviewer most needs to see, since a page can drift silently for weeks
//! inside a generous budget before it fails.
//!
//! A baseline changes only when the run is told to capture it, explicitly:
//!
//! ```text
//! UPDATE_SNAPSHOTS=force cargo test -p turbogit-ui --test acceptance_matrix
//! ```
//!
//! which repaints all eleven PNGs and leaves the previous ones beside them as
//! `{name}.old.png` for review (gitignored; the pair is what a reviewer compares
//! before committing). `force` is the only value this suite documents, see the
//! note on [`acceptance_matrix_screenshots`] for why `UPDATE_SNAPSHOTS=1` is not
//! a capture, and for the budget's floor.
//! Assertions here stay structural (the page paints its defining chrome); the
//! pixel-level check is the comparison above, and each page's own suite pins
//! its behaviour.
//!
//! Every page is a fixture of this file and not a fact about the machine it ran
//! on: the repos build under one fixed capture root ([`CAPTURE_ROOT`]) instead
//! of a random `tempfile` name, their commits carry one pinned date
//! ([`SEEDED_COMMIT_DATE`]) so their SHAs and their age labels cannot move, the
//! paths a pane titles with are the repo-relative form the changelist itself
//! carries, and the one host value the shell reads (`git --version`) is pinned
//! ([`CAPTURED_GIT_VERSION`]).
//!
//! One fact about the machine stays in the frames anyway, and it is named because
//! the mac budget is **0 px**, which makes it a red run rather than a quietly
//! stale capture: a `DATE` cell renders the pinned instant through the machine's
//! own zone. Measured with `TZ=Pacific/Auckland` against baselines captured at
//! `+0800`, `03-git-log` 26 px, `03-git-log-selected`, `05-branches-popup` and
//! `06-push-dialog` 52 px each, every box inside the date column at x 1015-1220,
//! and the other seven pages 0 px. So a diff confined to that column is the zone
//! doing this, not a layout change; re-capture after a daylight-saving transition
//! on the machine that owns the baselines.
//!
//! Apart from that, a page that moves between two runs here moved because the UI
//! changed.
//!
//! The harness is [`CAPTURE_SIZE`], 1440×900, the size `design-exports/` was
//! drawn and validated at. The frames are 2× exports of that box, so a capture
//! at any other size is not comparable with a frame at all, and a golden at the
//! wrong size is a golden nobody reviewed: it is a picture of the layout's
//! behaviour under a constraint the design never had.

use egui_kittest::Harness;
use std::path::{Path, PathBuf};
use test_support::git_seed::git;
use test_support::harness::shell_harness_over;
use turbogit_app::state::{AppState, Dialog};
use turbogit_domain::model::{ChangeRef, RootId, Submodule, SubmoduleState, Worktree};

const CAPTURE_SIZE: egui::Vec2 = egui::vec2(1440.0, 900.0);

fn harness(state: AppState) -> Harness<'static, AppState> {
    shell_harness_over(state, CAPTURE_SIZE)
}

fn settle(h: &mut Harness<'_, AppState>) {
    for _ in 0..8 {
        h.step();
    }
}

/// How many pixels does [`dify`] count as failing at `threshold`, the same
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

fn snapshot(h: &mut Harness<'_, AppState>, name: &str) {
    let options = egui_kittest::SnapshotOptions::default();
    let new = h
        .render()
        .unwrap_or_else(|err| panic!("{name}: the renderer produced no image: {err}"));

    let painted = test_support::harness::painted_text(&*h);
    let home = std::env::var("HOME").unwrap_or_default();
    for banned in [CAPTURE_ROOT, home.as_str()]
        .into_iter()
        .filter(|needle| !needle.is_empty())
    {
        if let Some(leaked) = painted.iter().find(|text| text.contains(banned)) {
            panic!("{name}: the capture paints a path outside its fixtures ({banned}): {leaked}");
        }
    }

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
        Err(_) => println!(
            "GOLDEN {name}: no committed baseline at {}",
            baseline.display()
        ),
    }

    h.snapshot_options(name, &options);
}

const CAPTURE_ROOT: &str = "turbogit-acceptance-goldens";

fn capture_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(CAPTURE_ROOT).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("capture dir");
    dir
}

/// The date every seeded commit carries, in both stamp fields.
///
/// Two separate leaks get plugged at once. `git` folds the committer timestamp
/// into the object it writes, so an unstamped seed has a **different SHA every
/// run**, and the abbreviated SHA is painted in the log rows, the selected
/// commit's details panel and the SHA chip, which is fixture content wearing a
/// render diff's costume. And every age label in the shell is computed against
/// the wall clock, so a seed stamped *now* paints "3s ago", which ticks between
/// two runs and across the log's `d < 60` boundary.
///
/// The date is then chosen, not just fixed: more than the 30 days at which the
/// log's relative format falls through to an absolute date
/// (`log_window::fmt_date`), and past the 365 days that put the branch badge
/// (`branch_widget::stale_badge`) in its yearly band, the one band whose value
/// moves once a year rather than once a minute, hour, day or week. `05-branches-popup`
/// therefore reads `1y` until this seed passes two years old, and that tick is
/// the suite's one scheduled re-capture that is not a UI change.
///
/// What is pinned here is the instant, not what a frame shows: a `DATE` cell
/// renders it through the machine's own zone, so the hour is still a host fact on
/// a daylight-saving machine. The file header names the four pages that carry it.
const SEEDED_COMMIT_DATE: &str = "2025-09-01T12:00:00+00:00";

fn seed_commit(repo: &Path, message: &str) {
    let out = std::process::Command::new("git")
        .args(["commit", "-q", "-m", message])
        .current_dir(repo)
        .env("GIT_AUTHOR_DATE", SEEDED_COMMIT_DATE)
        .env("GIT_COMMITTER_DATE", SEEDED_COMMIT_DATE)
        .output()
        .expect("git on PATH");
    assert!(
        out.status.success(),
        "seeded commit {message:?} in {} failed: {}",
        repo.display(),
        String::from_utf8_lossy(&out.stderr)
    );
}

const CAPTURED_GIT_VERSION: &str = "2.50.1";

fn seeded_repo() -> PathBuf {
    let repo = capture_dir("seeded").join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    std::fs::write(repo.join("lib.txt"), "alpha\nbeta\ngamma\n").unwrap();
    git(&repo, &["add", "."]);
    seed_commit(&repo, "initial import");
    let base = git(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["tag", "v1.0"]);
    git(&repo, &["checkout", "-q", "-b", "feature/tokens"]);
    std::fs::write(repo.join("lib.txt"), "alpha\nBETA\ngamma\n").unwrap();
    git(&repo, &["add", "."]);
    seed_commit(&repo, "feature: retokenize beta");
    git(&repo, &["checkout", "-q", "main"]);
    git(
        &repo,
        &["update-ref", "refs/remotes/origin/main", base.trim()],
    );
    std::fs::write(repo.join("lib.txt"), "alpha\nbeta\nGAMMA\n").unwrap();
    repo
}

fn conflicted_repo() -> PathBuf {
    let repo = capture_dir("merge").join("conflict");
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
    seed_commit(&repo, "base");
    git(&repo, &["checkout", "-q", "-b", "side"]);
    std::fs::write(
        repo.join("conf.txt"),
        "SIDE-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nSIDE-nine\nten\n",
    )
    .unwrap();
    git(&repo, &["add", "."]);
    seed_commit(&repo, "side");
    git(&repo, &["checkout", "-q", "main"]);
    std::fs::write(
        repo.join("conf.txt"),
        "MAIN-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nMAIN-nine\nten\n",
    )
    .unwrap();
    git(&repo, &["add", "."]);
    seed_commit(&repo, "main");
    let _ = std::process::Command::new("git")
        .args(["merge", "--no-edit", "side"])
        .current_dir(&repo)
        .output()
        .expect("git on PATH");
    repo
}

fn context_repo() -> PathBuf {
    let repo = capture_dir("panes").join("workbench");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    std::fs::write(repo.join("README.md"), "# workbench\n").unwrap();
    git(&repo, &["add", "."]);
    seed_commit(&repo, "initial import");
    git(&repo, &["checkout", "-q", "-b", "feature/tokens"]);
    std::fs::write(repo.join("tokens.txt"), "accent\n").unwrap();
    git(&repo, &["add", "."]);
    seed_commit(&repo, "feature: add tokens");
    git(&repo, &["checkout", "-q", "main"]);
    repo
}

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
            path: "vendor/design-tokens".into(),
            head: Some("7c1e0aa".into()),
            recorded: Some("40dd7e8".into()),
            state: SubmoduleState::NeedsUpdate,
            root: root.clone(),
        },
        Submodule {
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
    // Two renders of the same frame inside one process are byte-identical on all
    // eleven pages. That is the fact that says what this suite's run-to-run diff
    // actually was: the noise was never wgpu antialiasing, the claim a
    // force-write-every-run harness was rested on for as long as it took eleven
    // baselines to stop being evidence for anything, it was the *fixtures*,
    // painting a random temp directory name, a per-run commit SHA, a live
    // `git --version` and an age label that ticks every second. Rasterization
    // here is deterministic, so a page that moves between two runs moved because
    // the UI changed, which is the only thing a golden is for.
    //
    // The consequence for the budget in `kittest.toml` is that it buys tolerance
    // for nothing, and a budget sized to absorb *anything* hides real change with
    // it: the diff viewer's whitespace control becoming a menu once went unseen
    // inside a generous one.
    //
    // `UPDATE_SNAPSHOTS=1` is deliberately NOT the documented capture: egui_kittest
    // maps it to `UpdateFailing`, which rewrites only a golden already over the
    // budget, so an under-budget visual change lands as a silent refresh, which
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

    let project = capture_dir("welcome");
    let recents_config = capture_dir("welcome-config");
    let mut state = AppState::launch_in(Some(project.clone()), Some(recents_config.clone()));
    state.git_version = CAPTURED_GIT_VERSION.to_string();
    let mut h = harness(state);
    settle(&mut h);
    assert!(
        h.state().show_welcome(),
        "no-project launch must land on Welcome"
    );
    let painted = test_support::harness::painted_text(&h).join("\n");
    let home = std::env::var("HOME").unwrap_or_default();
    assert!(
        (home.is_empty() || !painted.contains(&home))
            && !painted.contains(&*recents_config.to_string_lossy()),
        "the welcome capture must not photograph the developer's recent \
         projects; point `recents_config_dir` at the capture dir. Painted:\n{painted}"
    );
    snapshot(&mut h, "01-welcome");
    results.extend_harness(&mut h);
    drop(h);

    let repo = seeded_repo();
    let state = AppState::for_roots(repo.parent().unwrap(), std::slice::from_ref(&repo));
    let mut h = harness(state);

    settle(&mut h);
    snapshot(&mut h, "02-commit");

    h.state_mut().ui.tab = turbogit_app::state::Tab::Log;
    settle(&mut h);
    assert!(
        h.state().caches.log(&RootId(repo.clone().into())).is_some(),
        "log should be loaded"
    );
    snapshot(&mut h, "03-git-log");

    let head = git(&repo, &["rev-parse", "HEAD"]).trim().to_owned();
    h.state_mut().ui.selected_commit = Some(head);
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(25));
        h.step();
    }
    snapshot(&mut h, "03-git-log-selected");

    h.state_mut().ui.preview_change = Some(ChangeRef::new(RootId(repo.clone().into()), "lib.txt"));
    h.state_mut().ui.tab = turbogit_app::state::Tab::Commit;
    settle(&mut h);
    snapshot(&mut h, "04-diff");
    h.state_mut().ui.tab = turbogit_app::state::Tab::Log;
    settle(&mut h);

    h.state_mut().ui.branches_popup = true;
    settle(&mut h);
    snapshot(&mut h, "05-branches-popup");
    h.state_mut().ui.branches_popup = false;

    h.state_mut().ui.dialog = Some(Dialog::Push);
    settle(&mut h);
    snapshot(&mut h, "06-push-dialog");
    h.state_mut().ui.dialog = None;

    h.state_mut().ui.settings_open = true;
    settle(&mut h);
    snapshot(&mut h, "08-settings-modal");
    results.extend_harness(&mut h);
    drop(h);

    let crepo = conflicted_repo();
    let mut state = AppState::for_roots(crepo.parent().unwrap(), std::slice::from_ref(&crepo));
    state.ui.conflict_open = Some(PathBuf::from("conf.txt"));
    let mut h = harness(state);
    settle(&mut h);
    snapshot(&mut h, "07-merge-editor");
    results.extend_harness(&mut h);
    drop(h);

    let wrepo = context_repo();
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

#[test]
fn legacy_theme_surface_is_gone() {
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
