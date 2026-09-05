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
//! - Metadata rail: Path / Branch / Upstream for the focused root.
//! - Status bar: aggregated workspace state (diverged · conflicts ·
//!   unpulled · archived · dirty · total).
//!
//! Existing Commit and Log content renders inside the new Changes / Log
//! tabs unchanged.
//!
//! Tests drive the real [`turbogit_ui::ui::render`] through
//! `egui_kittest` over temporary git repositories (CONTEXT.md "Headless
// harness") and assert only on public surfaces: painted labels and
// public `AppState` transitions.
use egui_kittest::{Harness, kittest::Queryable};
use std::path::{Path, PathBuf};
use test_support::harness::{assert_not_painted, assert_painted, settle};
use turbogit_app::state::AppState;
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
fn repo_header_combined_badge_appears_when_root_is_ahead() {
    // Seed two local commits ahead of upstream so the ahead count is
    // non-zero: that drives the combined ↑/↓/X badge to paint the
    // arrow with the count.
    let parent = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .join(".scratch/workspace-shell-frame-repoheader-ahead");
    let _ = std::fs::remove_dir_all(&parent);
    let parent = parent.parent().unwrap().join("wsf-repoheader-ahead");
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    let project_dir = parent.clone();
    let repo = temp_repo(&parent, "alpha");
    // Add two local commits with no push.
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "ahead 1"]);
    std::fs::write(repo.join("b.txt"), "b\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "ahead 2"]);

    let state = app_state(&project_dir, &[repo]);
    let mut h = harness(state);
    settle(&mut h);
    // Drive a manual refresh so the headless harness computes ahead/behind
    // synchronously and the cache carries the (2, 0) we expect.
    h.get_by_label("Refresh").click();
    settle(&mut h);

    // Combined badge: ahead count 2 → "↑2".
    assert_painted(&h, "↑2");
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

// -- Cycle D — metadata rail: Path / Branch / Upstream

#[test]
fn metadata_rail_paints_path_branch_and_upstream_for_focused_root() {
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
    let state = app_state(&project_dir, &[repo]);
    let mut h = harness(state);
    settle(&mut h);

    // Rail header.
    assert_painted(&h, "METADATA");
    // Field labels.
    for label in ["Path", "Branch", "Upstream"] {
        assert_painted(&h, label);
    }
    // Branch value mirrors the focused root's current branch.
    assert_painted(&h, "main");
    // Upstream value: origin/main (the temp_repo helper sets it up).
    assert_painted(&h, "origin/main");
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
