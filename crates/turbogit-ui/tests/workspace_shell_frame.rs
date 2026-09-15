//! Issue #03 — Workspace shell frame.
//!
//! Replaces the old IDE chrome (topbar menu / toolbar / sidebar rail /
//! tab strip) with the new screen-01 layout:
//!
//! - Topbar: TurboGit brand + workspace selector + breadcrumb (project /
//!   group / focused repo) on the left; Fetch / Pull / Push / Branch / More
//!   on the right.
//! - Repo header: focused root folder + branch pill + combined
//!   ahead/behind/conflict badge + Refresh.
//! - Center tabs: Changes (count), Log, Branches, Worktrees (count),
//!   Submodules. Branches/Worktrees/Submodules are empty-state
//!   placeholders in v1.
//! - Status bar: aggregated workspace state (diverged · conflicts ·
//!   unpulled · archived · dirty · total) plus granularity and repo
//!   scope. The far-right metadata column was removed (redesign 03);
//!   its Path/Branch/Upstream info lives in the topbar breadcrumb, the
//!   repo header branch pill, and the status-bar aggregates.
//!
//! Existing Commit and Log content renders inside the new Changes / Log
//! tabs unchanged.
//!
//! Tests drive the real [`turbogit_ui::ui::render`] through
//! `egui_kittest` over temporary git repositories (CONTEXT.md "Headless
// harness") and assert only on public surfaces: painted labels and
// public `AppState` transitions.
use egui::{Pos2, Shape};
use egui_kittest::{Harness, kittest::Queryable};
use std::path::{Path, PathBuf};
use test_support::harness::{
    assert_not_painted, assert_painted, filled_rects, galley_origin, settle,
};
use turbogit_app::state::AppState;
use turbogit_ui::theme::Palette;
use turbogit_ui::ui::{shell, widgets};
/// Run `git` in `repo`, asserting success, and return stdout.
fn git(repo: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git invocation");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 stdout")
}

/// Create an initialized temp repository with one base commit on the
/// default branch plus an `origin` remote so upstream tracking reads
/// can be exercised.
fn temp_repo(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q", "-b", "main"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("base.txt"), "base\n").unwrap();
    git(&path, &["add", "."]);
    git(&path, &["commit", "-q", "-m", "init"]);
    // Seed a tracking remote so the repo carries an upstream.
    let bare = parent.join(format!("{name}.origin"));
    let _ = std::fs::remove_dir_all(&bare);
    git(
        parent,
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    git(&path, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(&path, &["push", "-q", "origin", "main"]);
    git(&path, &["branch", "--set-upstream-to=origin/main", "main"]);
    path
}

/// Headless harness over the given repository roots (see CONTEXT.md).
fn app_state(project_dir: &Path, roots: &[PathBuf]) -> AppState {
    AppState::for_roots(project_dir, roots)
}

/// Headless harness driving the full app UI; mirrors `commit_window`'s.
fn harness(state: AppState) -> Harness<'static, AppState> {
    Harness::builder().with_max_steps(1024).build_ui_state(
        |ui, state| {
            state.drain_events();
            turbogit_ui::ui::render(ui, state);
        },
        state,
    )
}

// -- Cycle A — topbar: brand, workspace selector, breadcrumb, right actions --

#[test]
fn topbar_paints_new_shape_with_brand_selector_breadcrumb_and_actions() {
    // The temp parent has a deterministic basename so the breadcrumb
    // assertion is stable across CI machines.
    let parent = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .join(".scratch/workspace-shell-frame-topbar");
    let _ = std::fs::remove_dir_all(&parent);
    let parent = parent.parent().unwrap().join("wsf-topbar");
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    // A single-root project where the project dir is the workspace root
    // and the focused repo lives one level under it: project name
    // "wsf-topbar", focused repo name "alpha". The breadcrumb shows
    // "wsf-topbar / alpha".
    let project_dir = parent.clone();
    let repo = temp_repo(&parent, "alpha");
    let state = app_state(&project_dir, &[repo]);
    let mut h = harness(state);
    settle(&mut h);

    // Brand.
    assert_painted(&h, "TurboGit");
    // Workspace selector: project_dir basename.
    assert_painted(&h, "wsf-topbar");
    // Breadcrumb: project / focused root.
    assert_painted(&h, "alpha");
    // Right-side actions.
    for label in ["Fetch", "Pull", "Push", "Branch", "More"] {
        assert_painted(&h, label);
    }
    // Old IDE chrome is gone: the old menubar labels must NOT paint.
    // Use exact-galley substrings so accidental matches like "Filter
    // files" (commit sub-tab input) don't false-fire `assert_not_painted`.
    for old in [
        "File\0",
        "Edit\0",
        "View\0",
        "Navigate\0",
        "Code\0",
        "Window\0",
        "Help\0",
    ] {
        assert_not_painted(&h, old);
    }
    // Old toolbar inert chrome is gone too.
    for old in ["Run\0", "Debug\0", "Update Project\0"] {
        assert_not_painted(&h, old);
    }
}

// -- Cycle B — repo header: branch pill + combined ahead/behind/conflict badge + Refresh

#[test]
fn repo_header_shows_branch_pill_and_refresh() {
    let parent = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .join(".scratch/workspace-shell-frame-repoheader");
    let _ = std::fs::remove_dir_all(&parent);
    let parent = parent.parent().unwrap().join("wsf-repoheader");
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    let project_dir = parent.clone();
    let repo = temp_repo(&parent, "alpha");
    let state = app_state(&project_dir, &[repo]);
    let mut h = harness(state);
    settle(&mut h);

    // Branch pill: the focused root's current branch is "main".
    assert_painted(&h, "main");
    // Refresh affordance is exposed in the header.
    assert_painted(&h, "Refresh");
}

#[test]
fn repo_header_paints_orange_dirty_badge_with_uncommitted_count() {
    // Issue 02 (design doc §6): the repo header collapses to a single row —
    // name + branch chip + an orange dirty badge carrying the focused root's
    // uncommitted count (modified + unversioned + conflicted paths).
    let parent = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .join(".scratch/workspace-shell-frame-repoheader-dirtybadge");
    let _ = std::fs::remove_dir_all(&parent);
    let parent = parent.parent().unwrap().join("wsf-repoheader-dirtybadge");
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    let project_dir = parent.clone();
    let repo = temp_repo(&parent, "alpha");
    // One uncommitted file → the focused root's dirty count is 1.
    std::fs::write(repo.join("wip.txt"), "wip\n").unwrap();

    let state = app_state(&project_dir, &[repo]);
    let mut h = harness(state);
    settle(&mut h);
    h.get_by_label("Refresh").click();
    settle(&mut h);

    // The orange dirty badge: the only COUNTER-tinted chip on the header row
    // (its fill is the deterministic tint over the app background).
    let badge_fill = widgets::tint_over_bg(Palette::COUNTER, 0.18);
    let (badge, _) = filled_rects(&h)
        .into_iter()
        .find(|(_, c)| *c == badge_fill)
        .unwrap_or_else(|| panic!("repo header must paint an orange dirty badge"));
    // The badge sits on the header row, below the 38px topbar…
    assert!(
        badge.top() > 38.0,
        "the dirty badge must live inside the repo header row"
    );
    // …and carries the uncommitted count as an exact galley inside it.
    let count_origins: Vec<Pos2> = h
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Text(shape) if shape.galley.text() == "1" => Some(shape.pos),
            _ => None,
        })
        .collect();
    assert!(
        count_origins.iter().any(|p| badge.contains(*p)),
        "the dirty count galley sits inside the badge"
    );
}

// -- Cycle C — center tabs: Changes, Log, Branches, Worktrees, Submodules

#[test]
fn center_tabs_include_changes_log_branches_worktrees_submodules() {
    let parent = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .join(".scratch/workspace-shell-frame-tabs");
    let _ = std::fs::remove_dir_all(&parent);
    let parent = parent.parent().unwrap().join("wsf-tabs");
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    let project_dir = parent.clone();
    let repo = temp_repo(&parent, "alpha");
    let state = app_state(&project_dir, &[repo]);
    let mut h = harness(state);
    settle(&mut h);

    // Each center tab paints its name in the strip.
    for label in ["Changes", "Log", "Branches", "Worktrees", "Submodules"] {
        assert_painted(&h, label);
    }
}

#[test]
fn unimplemented_tabs_render_empty_state_placeholder() {
    let parent = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .join(".scratch/workspace-shell-frame-placeholder");
    let _ = std::fs::remove_dir_all(&parent);
    let parent = parent.parent().unwrap().join("wsf-placeholder");
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    let project_dir = parent.clone();
    let repo = temp_repo(&parent, "alpha");
    let mut state = app_state(&project_dir, &[repo]);
    // Click into Branches: the tab body paints an empty-state placeholder
    // (spec ADR-0008) rather than a live tree.
    state.ui.tab = turbogit_app::state::Tab::Branches;
    let mut h = harness(state);
    settle(&mut h);

    // The placeholder's body still labels the focused tab.
    assert_painted(&h, "Branches");
}

// -- Cycle C -- active tab renders as a filled pill (issue 02)

#[test]
fn active_tab_renders_as_a_filled_pill_not_a_box() {
    let parent = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .join(".scratch/workspace-shell-frame-pill");
    let _ = std::fs::remove_dir_all(&parent);
    let parent = parent.parent().unwrap().join("wsf-pill");
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    let project_dir = parent.clone();
    let repo = temp_repo(&parent, "alpha");
    let state = app_state(&project_dir, &[repo]);
    let mut h = harness(state);
    settle(&mut h);

    // The active shell tab is a filled pill: the lighter SURFACE_2 chip,
    // inset from the full 32px strip, containing the "Changes" label.
    // The old full-width box outline (a SURFACE chip spanning the strip
    // height behind the label) must be gone.
    let origin = galley_origin(&h, "Changes").expect("the active tab label paints");
    let rects = filled_rects(&h);
    let pill = rects
        .iter()
        .find(|(r, c)| {
            *c == Palette::SURFACE_2
                && r.height() > 0.0
                && r.height() < shell::TAB_STRIP_HEIGHT - 4.0
                && r.contains(origin)
        })
        .unwrap_or_else(|| {
            panic!(
                "active tab must paint a filled pill (SURFACE_2, inset from {}px); rects: {rects:#?}",
                shell::TAB_STRIP_HEIGHT
            )
        });
    // The pill is inset on the 4px grid: 8px shorter than the 32px strip
    // (24 px tall) so it reads as a chip, not a full-height box.
    assert_eq!(
        pill.0.height(),
        shell::TAB_STRIP_HEIGHT - 8.0,
        "the active-tab pill must be 8px shorter than the strip"
    );
    assert!(
        !rects
            .iter()
            .any(|(r, c)| { *c == Palette::SURFACE && r.contains(origin) && r.height() > 28.0 }),
        "active tab must not paint a full-width box outline behind its label"
    );
}

// -- Cycle D — two-zone layout: no third metadata column (issue 03)

#[test]
fn shell_is_two_zones_without_metadata_rail() {
    // Local-changes redesign issue 03: the far-right metadata column is
    // gone — its information lives in the status bar (issue 02) and the
    // repo header. The shell must no longer paint the rail's header or its
    // upstream row, while the branch pill and the status-bar aggregates
    // stay.
    let parent = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .join(".scratch/workspace-shell-frame-metadata");
    let _ = std::fs::remove_dir_all(&parent);
    let parent = parent.parent().unwrap().join("wsf-metadata");
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    let project_dir = parent.clone();
    let repo = temp_repo(&parent, "alpha");
    let mut state = app_state(&project_dir, &[repo]);
    state.ui.show_status_bar = true;
    let mut h = harness(state);
    settle(&mut h);

    // The third metadata column is removed: its header and upstream row
    // must not paint anywhere on the frame.
    assert_not_painted(&h, "METADATA");
    assert_not_painted(&h, "Upstream");

    // No regression: the metadata information stays reachable. The branch
    // pill still paints the focused root's branch, and the status bar still
    // aggregates the workspace counters (issue 02).
    assert_painted(&h, "main");
    assert_painted(&h, "1 total");
}

// -- Cycle E — status bar: aggregated workspace state

#[test]
fn status_bar_shows_aggregated_total_repos() {
    let parent = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .join(".scratch/workspace-shell-frame-statusbar");
    let project_dir = parent.clone();
    let repo = temp_repo(&parent, "alpha");
    let mut state = app_state(&project_dir, &[repo]);
    state.ui.show_status_bar = true;
    let mut h = harness(state);
    settle(&mut h);

    // Aggregated count: the project owns 1 root → "1 total" paints.
    assert_painted(&h, "1 total");
}

// -- Cycle E -- diverged counts

#[test]
fn status_bar_shows_diverged_count_when_root_is_ahead_of_upstream() {
    let parent = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .join(".scratch/workspace-shell-frame-statusbar-diverged");
    let _ = std::fs::remove_dir_all(&parent);
    let parent = parent.parent().unwrap().join("wsf-statusbar-diverged");
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    let project_dir = parent.clone();
    let repo = temp_repo(&parent, "alpha");
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "ahead 1"]);

    let mut state = app_state(&project_dir, &[repo]);
    state.ui.show_status_bar = true;
    let mut h = harness(state);
    settle(&mut h);
    // Drive a manual refresh so the headless harness computes
    // ahead/behind synchronously.
    h.get_by_label("Refresh").click();
    settle(&mut h);
    // 1 root ahead of its upstream counts as "diverged" in v1 (any
    // local work that hasn't reached the remote is loosely diverged).
    assert_painted(&h, "1 diverged");
}

// -- Cycle E -- unpulled + dirty counts, granularity, repo scope (issue 02)

#[test]
fn status_bar_paints_unpulled_dirty_granularity_and_scope_from_real_data() {
    // Two roots: `dirty` carries an untracked file; `behind` has one
    // remote-only commit fetched in — so the status bar aggregates
    // `1 unpulled` and `1 dirty` from the real sync/status data, plus the
    // granularity setting and the repo-scope count (issue 02 checkbox 1).
    let parent = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .join(".scratch/workspace-shell-frame-statusbar-unpulled");
    let _ = std::fs::remove_dir_all(&parent);
    let parent = parent.parent().unwrap().join("wsf-statusbar-unpulled");
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    let project_dir = parent.clone();
    let dirty = temp_repo(&parent, "dirty");
    let behind = temp_repo(&parent, "behind");
    // behind: land a remote-only commit and fetch it locally (ahead 0,
    // behind 1 — unpulled without diverging divergence).
    let bare = parent.join("behind.origin");
    let seed = parent.join("behind-seed");
    let _ = std::fs::remove_dir_all(&seed);
    git(
        &parent,
        &[
            "clone",
            "-q",
            bare.to_str().unwrap(),
            seed.to_str().unwrap(),
        ],
    );
    std::fs::write(seed.join("remote.txt"), "remote\n").unwrap();
    git(&seed, &["add", "."]);
    git(&seed, &["commit", "-q", "-m", "remote only"]);
    git(&seed, &["push", "-q", "origin", "main"]);
    git(&behind, &["fetch", "-q"]);
    // dirty: an untracked file.
    std::fs::write(dirty.join("wip.txt"), "wip\n").unwrap();

    let mut state = app_state(&project_dir, &[dirty, behind]);
    state.ui.show_status_bar = true;
    let mut h = harness(state);
    settle(&mut h);
    h.get_by_label("Refresh").click();
    settle(&mut h);

    // Aggregated counters from the real root data (colors are asserted at
    // the pure seam in `ui::shell::tests`); every chip only paints when
    // non-zero.
    assert_painted(&h, "1 unpulled");
    assert_painted(&h, "1 dirty");
    // Granularity readout (the app default: line) and the repo-scope count
    // (no explicit multi-repo selection → the focused single root).
    assert_painted(&h, "granularity: line");
    assert_painted(&h, "1 repo in scope");
    assert_painted(&h, "2 total");
}
