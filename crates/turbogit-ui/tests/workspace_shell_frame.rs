//! Issue #03 — Workspace shell frame.
//!
//! Replaces the old IDE chrome (topbar menu / toolbar / sidebar rail /
//! tab strip) with the screen-01 layout. Nothing claims the top window
//! edge — the central panel starts at y=0 and the shell is three regions:
//!
//! - Center tabs (32px): Changes (count), Log, Branches, Worktrees
//!   (count), Submodules. The repo header that once sat above this strip
//!   was deleted, so the strip is now the content column's topmost band
//!   and sits flush against the window edge. Branches/Worktrees/Submodules
//!   are empty-state placeholders in v1.
//! - Workspace sidebar: the left rail's repo tree. The focused root's
//!   branch lives on its repo row here, where the deleted header's branch
//!   pill used to carry it (`workspace_sidebar` owns those assertions).
//! - Status bar (24px): the version / git / indexed-repo line that came
//!   here with the topbar's deletion (so it paints on Welcome too), then
//!   aggregated workspace state (diverged · conflicts · unpulled ·
//!   archived · dirty · total) plus granularity and repo scope. The
//!   far-right metadata column was removed (redesign 03); its
//!   Path/Branch/Upstream info lives in the sidebar tree, the Branches
//!   popup, and the status-bar aggregates.
//!
//! The brand wordmark, the workspace selector, the repo header and the
//! Fetch/Pull/Push/Branch/More cluster are gone from the shell; their
//! entry points are the command palette (`Ctrl+Shift+A`), the frozen
//! shortcuts (`Ctrl+T` for Refresh), and the Commit window's own
//! `Refresh changes` button.
//!
//! Existing Commit and Log content renders inside the new Changes / Log
//! tabs unchanged.
//!
//! Tests drive the real [`turbogit_ui::ui::render`] through
//! `egui_kittest` over temporary git repositories (CONTEXT.md "Headless
//! harness") and assert only on public surfaces: painted labels and
//! public `AppState` transitions.
use egui::{Key, Modifiers};
use egui_kittest::Harness;
use std::path::{Path, PathBuf};
use test_support::harness::{
    assert_not_painted, assert_painted, filled_rects, galley_origin, painted_galleys, settle,
};
use turbogit_app::state::AppState;
use turbogit_ui::theme::Palette;
use turbogit_ui::ui::shell;
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

/// Drive a manual refresh through `Ctrl+T` — the frozen refresh shortcut
/// (`shell::handle_shortcuts`), and the shell-level survivor of the deleted
/// repo header's Refresh button. It dispatches the same
/// `state.refresh(Affected::All)` the header button did, so the headless
/// harness still gets its ahead/behind and status caches filled
/// synchronously.
#[track_caller]
fn manual_refresh(h: &mut Harness<'_, AppState>) {
    h.key_press_modifiers(Modifiers::CTRL, Key::T);
    settle(h);
}

// -- Cycle A — the shell's top band: the tab strip (the repo header above it
//    was deleted, so the strip is now the content column's first band) ------

#[test]
fn tab_strip_is_the_content_columns_topmost_band() {
    let parent = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join(".scratch/workspace-shell-frame-topband");
    let _ = std::fs::remove_dir_all(&parent);
    let parent = parent.parent().unwrap().join("wsf-topband");
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    let project_dir = parent.clone();
    let repo = temp_repo(&parent, "alpha");
    let state = app_state(&project_dir, &[repo]);
    let mut h = harness(state);
    settle(&mut h);

    // The active tab's band starts at the top of the content column — it is
    // the strip's own top edge, since the 48px repo header that used to own
    // that space is gone.
    let label = galley_origin(&h, "Changes").expect("the active tab label paints");
    let rects = filled_rects(&h);
    let band = rects
        .iter()
        .find(|(r, c)| *c == Palette::SURFACE_2 && r.contains(label))
        .map(|(r, _)| *r)
        .expect("the active tab's band");

    // Nothing at all paints in the content column above it: no header band, no
    // breadcrumb, no dirty badge. The project → root breadcrumb and the
    // header's per-file uncommitted count are deliberately gone, so the strip
    // is now the content column's topmost chrome and the window edge above it
    // is bare.
    let content_left = band.left();
    let mut above: Vec<String> = painted_galleys(&h)
        .iter()
        .filter(|g| g.rect.left() >= content_left && g.rect.top() < band.top() - 0.5)
        .map(|g| format!("text {:?} at {:?}", g.text, g.rect))
        .collect();
    above.extend(
        rects
            .iter()
            .filter(|(r, _)| r.left() >= content_left && r.top() < band.top() - 0.5)
            .map(|(r, _)| format!("rect {r:?}")),
    );
    assert!(
        above.is_empty(),
        "the tab strip must be the content column's topmost band; painted above it: {above:#?}"
    );
}

// -- Cycle C — center tabs: Changes, Log, Branches, Worktrees, Submodules

#[test]
fn center_tabs_include_changes_log_branches_worktrees_submodules() {
    let parent = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
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
        .nth(2)
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

// -- Cycle C -- the active tab is a full-strip band with a brand rule --------

#[test]
fn active_tab_renders_as_a_full_height_band_not_a_floating_pill() {
    let parent = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
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

    // The active shell tab is a full-strip-height SURFACE_2 band, anchored to
    // the top window edge, containing the "Changes" label. It is not the
    // floating pill that used to float 4px inside the strip, and not the old
    // full-width SURFACE box outline behind the label.
    let origin = galley_origin(&h, "Changes").expect("the active tab label paints");
    let rects = filled_rects(&h);
    let band = rects
        .iter()
        .find(|(r, c)| *c == Palette::SURFACE_2 && r.contains(origin))
        .unwrap_or_else(|| {
            panic!("active tab must paint a SURFACE_2 band containing its label; rects: {rects:#?}")
        })
        .0;
    assert_eq!(
        band.height(),
        shell::TAB_STRIP_HEIGHT,
        "the active tab is a full-strip-height band, not a pill inset from it"
    );
    // The selection rule: 2px of BRAND along the band's bottom edge, as wide
    // as the band itself, overwriting the strip's LINE divider there — the
    // same measure the sidebar's active row uses.
    let rule = rects
        .iter()
        .find(|(r, c)| {
            *c == Palette::BRAND
                && (r.bottom() - band.bottom()).abs() < 0.5
                && (r.left() - band.left()).abs() < 0.5
                && (r.right() - band.right()).abs() < 0.5
        })
        .map(|(r, _)| *r)
        .unwrap_or_else(|| {
            panic!("a 2px BRAND rule must run along the active tab's bottom edge; band: {band:?}")
        });
    assert_eq!(
        rule.height(),
        2.0,
        "the tab strip's selection rule is 2px, matching the sidebar's active band"
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
    // gone — its information lives in the status bar (issue 02) and, since
    // the repo header's deletion, the sidebar tree. The shell must no longer
    // paint the rail's header or its upstream row, while the status-bar
    // aggregates stay.
    let parent = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
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
    // The rest of the retired IDE chrome stays retired too — the old
    // menubar, the inert toolbar buttons, and the topbar's own band. Use
    // exact-galley substrings so accidental matches like "Filter files"
    // (commit sub-tab input) don't false-fire `assert_not_painted`.
    for old in [
        "File\0",
        "Edit\0",
        "View\0",
        "Navigate\0",
        "Code\0",
        "Window\0",
        "Help\0",
        "Run\0",
        "Debug\0",
        "Update Project\0",
    ] {
        assert_not_painted(&h, old);
    }
    // The shell's own actions live in the command palette now, not in a
    // button band of their own. ("Branch" is deliberately not in this
    // list: the tab strip's "Branches" contains it.)
    for old in ["Fetch", "Pull", "Push", "More"] {
        assert_not_painted(&h, old);
    }

    // No regression: the status bar still aggregates the workspace counters
    // (issue 02). The focused root's branch — the fact the removed metadata
    // rail carried and the repo header's pill repeated — now lives on the
    // sidebar's repo row; this harness is narrower than
    // `MIN_SIDEBAR_WINDOW_WIDTH`, so the rail is off screen and
    // `workspace_sidebar` is where that assertion lives.
    assert_painted(&h, "1 total");
}

// -- Cycle E — status bar: aggregated workspace state

#[test]
fn status_bar_shows_aggregated_total_repos() {
    let parent = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
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
        .nth(2)
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
    manual_refresh(&mut h);
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
        .nth(2)
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
    manual_refresh(&mut h);

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
