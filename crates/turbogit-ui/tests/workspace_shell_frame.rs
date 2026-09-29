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
use egui::{Color32, Key, Modifiers, Rect};
use egui_kittest::Harness;
use std::path::{Path, PathBuf};
use test_support::harness::{
    assert_not_painted, assert_painted, filled_rects, galley_origin, painted_galleys, painted_ink,
    settle,
};
use turbogit_app::state::AppState;
use turbogit_ui::theme::{Palette, RAIL_WIDTH, RepoState};
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

/// The narrowest harness width at which the **sidebar** actually renders.
///
/// `shell::MIN_SIDEBAR_WINDOW_WIDTH` is 1000, but the shell measures its work
/// rect *inside* the window frame, so the outer width that clears the
/// threshold is higher than the constant: measured on the real render, the rail
/// first paints between 1030 and 1040. A suite that assumed "the 1024 default
/// shows the sidebar" would have asserted against nothing — which is the
/// vacuous-pass trap this ticket's criterion names, and why the number is
/// measured here rather than reasoned about.
pub const SIDEBAR_VISIBLE_FROM: f32 = 1040.0;

/// The width the acceptance criterion is about: the headless harness's own
/// documented default, where the shell renders *without* the rail.
const HARNESS_DEFAULT: (f32, f32) = (1024.0, 768.0);

/// A size wide of the rail's threshold, so the sidebar is really on screen.
/// Base width plus the rail's own 280, the arithmetic several existing suites
/// already use, so it cannot drift when the threshold moves.
fn rail_visible_size() -> (f32, f32) {
    (
        HARNESS_DEFAULT.0 + turbogit_ui::ui::sidebar::SIDEBAR_WIDTH,
        HARNESS_DEFAULT.1,
    )
}

/// Headless harness driving the full app UI; mirrors `commit_window`'s.
fn harness(state: AppState) -> Harness<'static, AppState> {
    harness_at(state, HARNESS_DEFAULT.0, HARNESS_DEFAULT.1)
}

/// The same harness at an explicit size.
///
/// **Sized, not defaulted.** `egui_kittest::Harness::builder()` lands on
/// 800×600, which is *below* the sidebar's own threshold — so a suite that
/// never says what size it is renders a shell with no left rail and every
/// sidebar assertion in it passes vacuously. Tests that mean to look at the
/// sidebar pass [`rail_visible_size`]; tests about the shell itself pass
/// [`HARNESS_DEFAULT`].
fn harness_at(state: AppState, width: f32, height: f32) -> Harness<'static, AppState> {
    let mut h = Harness::builder().with_max_steps(1024).build_ui_state(
        |ui, state| {
            state.drain_events();
            turbogit_ui::ui::render(ui, state);
        },
        state,
    );
    h.set_size(egui::vec2(width, height));
    h
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

    // The active tab's label sits at the top of the content column — the strip
    // is the content column's topmost band, since the 48px repo header that
    // used to own that space is gone. The strip itself is located through the
    // active tab's own accent underline (see `the_active_tab_is_an_accent_
    // underline_and_a_brighter_label`), not through a raised fill: with the
    // band gone there is no band to find, and the underline is the mark that
    // says which tab this is.
    let label = galley_origin(&h, "Changes").expect("the active tab label paints");

    // Nothing at all paints in the content column above it: no header band, no
    // breadcrumb, no dirty badge. The project → root breadcrumb and the
    // header's per-file uncommitted count are deliberately gone, so the strip
    // is now the content column's topmost chrome and the window edge above it
    // is bare.
    let content_left = tab_underlines(&h)
        .into_iter()
        .map(|rect| rect.left())
        .fold(f32::INFINITY, f32::min);
    assert!(
        content_left.is_finite(),
        "the active tab's accent underline locates the content column's leading \
         edge; nothing brand-filled painted in the strip"
    );
    let mut above: Vec<String> = painted_galleys(&h)
        .iter()
        .filter(|g| g.rect.left() >= content_left && g.rect.top() < label.y - 0.5)
        .map(|g| format!("text {:?} at {:?}", g.text, g.rect))
        .collect();
    above.extend(
        filled_rects(&h)
            .iter()
            .filter(|(r, _)| r.left() >= content_left && r.top() < label.y - 0.5)
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

// -- Cycle C -- the active tab: an accent underline and a brighter label ------
//
// The tab strip's selection mark is exactly two things, and both are asserted
// here from painted output: a 2px `BRAND` underline on the active tab's bottom
// edge, and a label one step brighter on the ramp than every inactive tab's.
// "Exactly one tab carries the underline" is its own test, because the failure
// it guards against — two tabs marked — is invisible in a test that only looks
// for the one that is marked.

/// The tab strip's own band.
///
/// **Centred on the tab labels, not on the viewport.** The strip does not start
/// at the window's top edge: the window margin and the panel's own frame put
/// the content area some points in, and hard-coding either the inset or the top
/// would make this a test of a constant rather than of the render. The caller
/// places every tab item at the strip's top and the strip's height centres it,
/// so the active tab's label centre *is* the strip's centre — and the band's
/// height is the spec's own `TAB_STRIP_HEIGHT`.
///
/// The strip used to be findable as a `SURFACE_2` fill behind the active label.
/// With that fill gone the label is what locates the band, which is the more
/// honest anchor: it is the tab itself, not its decoration.
fn tab_strip_band(harness: &Harness<'_, AppState>) -> Rect {
    // The label's own **painted rect**, not its origin: `galley_origin` is the
    // text's top-left, and the strip is centred on the text's middle, so
    // centring on the origin puts the band eight points high and the underline
    // falls outside it. This is the same trap the harness module warns about —
    // a galley's position is not its extent.
    let label: Vec<_> = painted_galleys(harness)
        .into_iter()
        .filter(|g| g.text == "Changes")
        .collect();
    assert_eq!(label.len(), 1, "one `Changes` label; found {label:#?}");
    let centre = label[0].rect.center();
    let viewport = harness.ctx.content_rect();
    Rect::from_center_size(
        egui::pos2(viewport.center().x, centre.y),
        egui::vec2(viewport.width(), shell::TAB_STRIP_HEIGHT),
    )
}

/// The `BRAND`-filled rects the strip paints that are exactly the accent width
/// tall — the tab strip's selection underlines, and nothing else in the band.
fn tab_underlines(harness: &Harness<'_, AppState>) -> Vec<Rect> {
    let strip = tab_strip_band(harness);
    filled_rects(harness)
        .into_iter()
        .filter(|(rect, color)| {
            *color == Palette::BRAND
                && (rect.height() - RAIL_WIDTH).abs() < 0.01
                // `grow(1.0)` rather than `intersect`: the band is derived
                // from a *text* rect, and a galley's height rounds to a whole
                // number of points, so the derived band can land half a point
                // off the underline it is meant to contain. A point of slack
                // absorbs that rounding and nothing else — the filter is still
                // "brand, one accent-width tall, in the strip".
                && strip.expand(1.0).intersect(*rect) == *rect
        })
        .map(|(rect, _)| rect)
        .collect()
}

#[test]
fn the_active_tab_is_an_accent_underline_and_a_brighter_label() {
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

    // (1) The underline. 2px — the token layer's accent width, the same one
    //     the sidebar's selected-row rail uses — sitting on the strip's bottom
    //     edge and spanning the active tab's own width, so it overwrites the
    //     strip's `LINE` divider and the selection mark and the divider read as
    //     one edge.
    let underlines = tab_underlines(&h);
    assert_eq!(
        underlines.len(),
        1,
        "the active tab paints one accent underline in the strip: {underlines:?}"
    );
    let rule = underlines[0];
    let strip = tab_strip_band(&h);
    assert_eq!(
        rule.height(),
        RAIL_WIDTH,
        "the underline is the token layer's accent width, not a literal at the call site"
    );
    // A point of slack, not half a point: the band is derived from a text
    // galley's rect, whose height rounds to whole points, so the band's own
    // bottom can sit a half-point off the rule it is checked against. The
    // tolerance is stated here rather than hidden by widening the band, so a
    // rule that drifted a whole point off the edge would still fail.
    assert!(
        (rule.bottom() - strip.bottom()).abs() <= 1.0,
        "the underline runs along the strip's bottom edge: rule {rule:?}, strip {strip:?}"
    );

    // (2) The brighter label, and quieter inactive ones. Both inks come from
    //     the shared ramp and both are legal where they land: the strip's own
    //     fill is `Palette::BG`, on which `INK` measures 12.59:1 and `INK_3`
    //     5.02:1. Scoped by position, never by first match — a tab label is
    //     also reachable through other surfaces' text in other frames.
    let strip_ink = |harness: &Harness<'_, AppState>, label: &str| -> Color32 {
        let hits: Vec<_> = painted_galleys(harness)
            .into_iter()
            .filter(|g| g.text == label && strip.contains_rect(g.rect))
            .collect();
        assert_eq!(hits.len(), 1, "one `{label}` in the strip, found {hits:#?}");
        hits.into_iter().next().expect("one hit").color
    };
    let active = strip_ink(&h, "Changes");
    assert_eq!(
        active,
        Palette::INK,
        "the active tab's label is the brightest ink on the ramp"
    );
    for quiet in ["Log", "Branches", "Worktrees", "Submodules"] {
        assert_eq!(
            strip_ink(&h, quiet),
            Palette::INK_3,
            "an inactive tab is quiet on the shared ramp, one step down from the active one"
        );
    }

    // (3) And nothing else marks the active tab. A raised fill behind the
    //     label is the second vocabulary this strip used to have, and it is
    //     also the wrong rung of the raised ladder: the strip sits on `BG`,
    //     where raised is `SURFACE`, not `SURFACE_2`.
    //
    //     Scoped to the fills the **strip itself** put down — a rect that
    //     begins inside the band. The panel the strip lives in fills the whole
    //     content area behind every label, and that fill is the strip's own
    //     surface rather than a mark on the tab; asserting against it would be
    //     asserting that the strip paints nothing at all.
    let label_rect = painted_galleys(&h)
        .into_iter()
        .find(|g| g.text == "Changes")
        .expect("the active tab label")
        .rect;
    let strip = tab_strip_band(&h);
    let marks: Vec<(Rect, Color32)> = filled_rects(&h)
        .into_iter()
        .filter(|(rect, _)| {
            rect.intersect(strip.expand(1.0)) == *rect && rect.intersect(label_rect).is_positive()
        })
        .collect();
    for (rect, color) in &marks {
        assert_eq!(
            *color,
            Palette::BRAND,
            "the active tab is marked by its accent underline and a brighter \
             label, and by nothing else; the strip painted {color:?} across the \
             label in {rect:?}"
        );
        assert_eq!(
            rect.height(),
            RAIL_WIDTH,
            "the only thing the strip paints across a tab is the underline: a \
             full-height mark would be a second treatment. Painted {rect:?}"
        );
    }
    assert!(
        !filled_rects(&h).iter().any(|(r, c)| {
            *c == Palette::SURFACE && r.contains(label_rect.center()) && r.height() > 28.0
        }),
        "active tab must not paint a full-width box outline behind its label"
    );
}

/// **Exactly one** tab carries the accent underline.
///
/// Its own test because the failure it catches is not visible from the
/// "the active tab is marked" assertion: a second underline still leaves the
/// first one exactly where it was, and the active tab still reads correctly.
/// "Two tabs are marked" is only visible as a count, and it is the failure a
/// `state.ui.tab == tab` comparison in a refactor invites.
#[test]
fn exactly_one_tab_carries_the_accent_underline() {
    let parent = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join(".scratch/workspace-shell-frame-one-underline");
    let _ = std::fs::remove_dir_all(&parent);
    let parent = parent.parent().unwrap().join("wsf-one-underline");
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    let project_dir = parent.clone();
    let repo = temp_repo(&parent, "alpha");

    // Every tab in the strip, one fresh app state per tab: five renders, one
    // underline each. A strip that marked two, or none, or that left a stale
    // underline behind a tab switch, fails here. (`AppState` is deliberately
    // not `Clone` — a test that rebuilt it per tab is also a test that cannot
    // accidentally share a cached frame between renders.)
    for tab in [
        turbogit_app::state::Tab::Commit,
        turbogit_app::state::Tab::Log,
        turbogit_app::state::Tab::Branches,
        turbogit_app::state::Tab::Worktrees,
        turbogit_app::state::Tab::Submodules,
    ] {
        let mut state = app_state(&project_dir, std::slice::from_ref(&repo));
        state.ui.tab = tab;
        let mut h = harness(state);
        settle(&mut h);
        let underlines = tab_underlines(&h);
        assert_eq!(
            underlines.len(),
            1,
            "with {:?} active exactly one tab may carry the accent underline, found \
             {underlines:?}",
            tab
        );
    }
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

// -- Cycle F — the status bar is on the shared ink ramp ----------------------
//
// The bar is one raised band (`Palette::SURFACE`) and every line on it is quiet
// text, so the whole surface has one legality answer: the muted step is NOT
// legal there (4.20:1, under AA) and the ink must step *up* to `INK_2`
// (6.58:1). These two tests say that from painted output — one per direction,
// so a failure says which half of the contract broke.

/// A two-root workspace with a dirty root and a behind root, so every status
/// bar line paints: the version line, the `·` separators, the two aggregated
/// counters, the granularity and scope readouts, and the total.
fn busy_status_bar(tag: &str) -> (Harness<'static, AppState>, PathBuf) {
    let parent = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join(format!(".scratch/workspace-shell-frame-ramp-{tag}"));
    let _ = std::fs::remove_dir_all(&parent);
    let parent = parent.parent().unwrap().join(format!("wsf-ramp-{tag}"));
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    let project_dir = parent.clone();
    let dirty = temp_repo(&parent, "dirty");
    let behind = temp_repo(&parent, "behind");
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
    std::fs::write(dirty.join("wip.txt"), "wip\n").unwrap();

    let mut state = app_state(&project_dir, &[dirty, behind]);
    state.ui.show_status_bar = true;
    let mut h = harness(state);
    settle(&mut h);
    manual_refresh(&mut h);
    (h, parent)
}

/// The aggregated workspace counters, by the words they end in.
///
/// They are the one part of the bar that is **not** quiet text: each is a fact
/// about repository state and wears the one state map's colour for it. The
/// state colours are checked by their own test
/// ([`the_status_bar_counters_read_the_one_repository_state_colour_map`]),
/// so this one is about the *quiet* lines only. Listed as words rather than as
/// positions because the bar's contents move as the workspace changes and a
/// position-based exemption would silently stop covering them.
fn is_an_aggregated_counter(text: &str) -> bool {
    text.ends_with(" diverged")
        || text.ends_with(" conflict")
        || text.ends_with(" unpulled")
        || text.ends_with(" archived")
        || text.ends_with(" dirty")
}

/// The status bar's own band: the frame painted in `Palette::SURFACE` at the
/// bottom of the viewport. Found from the painted frame rather than assumed, so
/// it self-calibrates.
fn status_bar_band(harness: &Harness<'_, AppState>) -> Rect {
    // Anchored on the bar's own right-cluster total, which is the one line that
    // paints on every project and on Welcome, and located from the frame rather
    // than from a restated height.
    let origin = galley_origin(harness, "2 total").expect("the status bar paints");
    let bottom = harness.ctx.content_rect().bottom();
    Rect::from_min_max(egui::pos2(0.0, origin.y - 8.0), egui::pos2(bottom, bottom))
}

/// Every text ink painted inside the status bar, with the text that carries it.
fn status_bar_inks(harness: &Harness<'_, AppState>) -> Vec<(String, Color32)> {
    let band = status_bar_band(harness);
    painted_galleys(harness)
        .into_iter()
        .filter(|g| band.contains_rect(g.rect))
        .map(|g| (g.text.clone(), g.color))
        .collect()
}

#[test]
fn the_status_bar_sits_one_step_up_the_ink_ramp_on_its_raised_band() {
    let (h, _parent) = busy_status_bar("ink");
    let inks = status_bar_inks(&h);
    assert!(
        inks.len() >= 6,
        "the fixture must exercise the whole bar, not one line: {inks:#?}"
    );
    for (text, ink) in &inks {
        // The version line, the separators, the granularity/scope readouts and
        // the total are all quiet text and all take the step-up. The two
        // aggregated counters are state-coloured and are checked separately,
        // below, against the one state map.
        if is_an_aggregated_counter(text) {
            continue;
        }
        assert_eq!(
            *ink,
            Palette::INK_2,
            "`{text}` is quiet text on the status bar's raised `SURFACE` band, \
             where the muted step is illegal (4.20:1) and the ink steps up to \
             INK_2 (6.58:1); it painted {ink:?}"
        );
    }
    // The negative, stated as a negative over the whole bar: the muted step
    // appears nowhere on it. This is the half that catches a *new* line added
    // later with the quiet-looking ink, which the loop above cannot see
    // because it has no expectation for a text it has never heard of.
    for (text, ink) in &inks {
        assert_ne!(
            *ink,
            Palette::INK_3,
            "the muted step is not legal on the status bar's raised band, and \
             `{text}` painted it"
        );
        assert_ne!(
            *ink,
            Palette::INK_4,
            "the dim step is for placeholders and hatches only; `{text}` painted \
             it on the status bar"
        );
    }
}

/// The aggregated counters read the **one** repository-state colour map, and
/// the reserved counter orange appears on this surface for dirt and unpushed
/// and nothing else.
#[test]
fn the_status_bar_counters_read_the_one_repository_state_colour_map() {
    let (h, _parent) = busy_status_bar("state");
    let inks = status_bar_inks(&h);
    let ink_of = |needle: &str| -> Color32 {
        let hits: Vec<_> = inks.iter().filter(|(text, _)| text == needle).collect();
        assert_eq!(hits.len(), 1, "one `{needle}` on the bar, found {inks:#?}");
        hits[0].1
    };
    // Both of these roots are behind their upstream, so the bar shows the
    // unpulled counter — and it wears the map's unpulled colour, which *is*
    // the reserved counter orange, because unpulled is one of the two states
    // the reservation names.
    assert_eq!(
        ink_of("1 unpulled"),
        RepoState::Unpulled.color(),
        "the unpulled counter wears the one state map's unpulled colour"
    );
    assert_eq!(
        ink_of("1 dirty"),
        RepoState::Dirty.color(),
        "the dirty counter wears the one state map's dirty colour"
    );
    assert_eq!(
        RepoState::Unpulled.color(),
        Palette::COUNTER,
        "and the map's unpulled colour IS the reserved counter orange"
    );
    // The negative half: no *quiet* line on the bar is orange. A version line
    // or a separator in the reserved orange would spend the reservation on
    // something that is not a dirt or unpushed count, which is precisely what
    // the reservation exists to prevent.
    for (text, ink) in &inks {
        if *ink == Palette::COUNTER {
            assert!(
                text.ends_with("unpulled") || text.ends_with("dirty"),
                "the reserved counter orange is for dirt and unpushed counts \
                 only, and `{text}` painted it"
            );
        }
    }
}

// -- Cycle G — the shell still renders at the default size, and the narrow
//    window is a second render rather than a scaled one -------------------

#[test]
fn the_shell_renders_at_the_harness_default_size_without_clipping() {
    // The headless harness's documented default, 1024×768, and the size the
    // criterion is about. **The rail is not on screen here** — measured, not
    // assumed: the shell measures its work rect inside the window frame, so
    // `MIN_SIDEBAR_WINDOW_WIDTH`'s 1000 needs an outer width above 1030 (see
    // `SIDEBAR_VISIBLE_FROM`). This test is about the shell fitting its own
    // default; [`the_harness_renders_the_sidebar_at_a_width_that_shows_it`] is
    // about the rail. Splitting them is what keeps neither vacuous.
    let parent = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join(".scratch/workspace-shell-frame-noclip");
    let _ = std::fs::remove_dir_all(&parent);
    let parent = parent.parent().unwrap().join("wsf-noclip");
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    let project_dir = parent.clone();
    let repo = temp_repo(&parent, "alpha");
    let mut state = app_state(&project_dir, &[repo]);
    state.ui.show_status_bar = true;
    let mut h = harness(state);
    settle(&mut h);

    let viewport = h.ctx.content_rect();
    assert_eq!(
        (viewport.width(), viewport.height()),
        HARNESS_DEFAULT,
        "this test is about the harness's own default size"
    );
    // Nothing the shell paints escapes the viewport's content area, and nothing
    // lands outside it. The frame carries an 8-point margin, so the inner rect
    // is the one content has to stay inside.
    let inner = viewport.shrink(8.0);
    for (rect, color) in filled_rects(&h) {
        assert!(
            inner.intersect(rect) == rect,
            "a filled rect escapes the shell's content area: {rect:?} in {color:?}, \
             inner {inner:?}"
        );
    }
    for galley in painted_galleys(&h) {
        assert!(
            inner.intersect(galley.rect) == galley.rect,
            "text clips or escapes at the default size: {:?} at {:?}, inner {inner:?}",
            galley.text,
            galley.rect
        );
    }
    // …and the navigation is all there at that size. The tab strip is a
    // top band whose width does not depend on the rail's, so all five tabs
    // paint and exactly one is marked at the default size too.
    for label in ["Changes", "Log", "Branches", "Worktrees", "Submodules"] {
        assert_painted(&h, label);
    }
    assert_painted(&h, "1 total");
    assert_eq!(
        tab_underlines(&h).len(),
        1,
        "the default size marks exactly one active tab"
    );
}

/// The harness renders the sidebar **at a width that shows it**, and says so
/// with painted geometry rather than with a label that might be matching
/// something else.
///
/// Its own test because the failure it guards is a *vacuous* one: at the
/// default 1024 the rail is not on screen, so "PROJECTS is not painted" and
/// "the selected row has a rail" both succeed for the wrong reason. Widening
/// the harness past the measured threshold is what makes every sidebar
/// assertion in the workspace mean something, and asserting the rail's own
/// surface here is what proves the widening worked.
#[test]
fn the_harness_renders_the_sidebar_at_a_width_that_shows_it() {
    let parent = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join(".scratch/workspace-shell-frame-railing");
    let _ = std::fs::remove_dir_all(&parent);
    let parent = parent.parent().unwrap().join("wsf-railing");
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    let project_dir = parent.clone();
    let repo = temp_repo(&parent, "alpha");
    let (w, h_) = rail_visible_size();
    let mut state = app_state(&project_dir, std::slice::from_ref(&repo));
    state.ui.show_status_bar = true;
    let mut h = harness_at(state, w, h_);
    settle(&mut h);

    // The rail's own surface, at its own width, at the content area's leading
    // edge. Three facts in one assertion: the surface token has a consumer
    // (ticket 08), the consumer is the sidebar, and the sidebar is really on
    // screen at this size.
    let rail = filled_rects(&h)
        .into_iter()
        .find(|(rect, color)| {
            *color == Palette::SIDEBAR
                && (rect.width() - turbogit_ui::ui::sidebar::SIDEBAR_WIDTH).abs() < 0.5
        })
        .map(|(rect, _)| rect)
        .expect("the sidebar paints its own surface at the rail's width");
    let viewport = h.ctx.content_rect();
    // The rail starts at the content area's leading edge. The exact inset is
    // the window margin plus the panel's own frame, which is not this suite's
    // to restate — what matters is that the rail is flush left (nothing to its
    // left) and nowhere near the middle.
    assert!(
        rail.left() - viewport.left() < 32.0,
        "the rail is flush with the content area's leading edge: {rail:?} vs {viewport:?}"
    );
    assert!(
        rail.right() < viewport.center().x,
        "the rail occupies the left of the content area, not the middle: {rail:?}"
    );
    // And the rail is not merely a fill: its two section headers and its
    // filter field are on screen, which is what a developer actually reads.
    assert_painted(&h, "PROJECTS");
    assert_painted(&h, "Filter repos / branches…");

    // The threshold itself, from both sides, so the constant above is pinned
    // against the real render rather than against a note in a comment. The
    // narrow side is stated as "below `SIDEBAR_VISIBLE_FROM`", not as a
    // specific number: the claim is that this harness size shows the rail and
    // the default does not, and the boundary between them is the shell's
    // business, not this suite's.
    let mut narrow = harness_at(
        {
            let mut s = app_state(&project_dir, std::slice::from_ref(&repo));
            s.ui.show_status_bar = true;
            s
        },
        HARNESS_DEFAULT.0,
        HARNESS_DEFAULT.1,
    );
    settle(&mut narrow);
    assert!(
        !filled_rects(&narrow)
            .iter()
            .any(|(_, c)| *c == Palette::SIDEBAR),
        "the default width is below the rail's threshold — if this ever changes, \
         `SIDEBAR_VISIBLE_FROM` and the no-clip test above need re-measuring"
    );
    let mut just_under = harness_at(
        {
            let mut s = app_state(&project_dir, std::slice::from_ref(&repo));
            s.ui.show_status_bar = true;
            s
        },
        SIDEBAR_VISIBLE_FROM - 20.0,
        HARNESS_DEFAULT.1,
    );
    settle(&mut just_under);
    assert!(
        !filled_rects(&just_under)
            .iter()
            .any(|(_, c)| *c == Palette::SIDEBAR),
        "20 points below the measured threshold the rail is still hidden"
    );
    let mut just_over = harness_at(
        {
            let mut s = app_state(&project_dir, &[repo]);
            s.ui.show_status_bar = true;
            s
        },
        SIDEBAR_VISIBLE_FROM,
        HARNESS_DEFAULT.1,
    );
    settle(&mut just_over);
    assert!(
        filled_rects(&just_over)
            .iter()
            .any(|(_, c)| *c == Palette::SIDEBAR),
        "{SIDEBAR_VISIBLE_FROM} is the narrowest width that shows the rail"
    );
}

#[test]
fn the_narrow_window_collapse_is_a_second_render_that_keeps_the_navigation() {
    // The narrow path is a *different render*, not a scaled one: the sidebar
    // disappears and the shell's minimum pane sizes take the whole width. So
    // it is asserted at its own size, after a full-width render in the same
    // test — resizing the first harness would only prove the frame tolerates a
    // `set_size` call.
    let (mut h, _parent) = busy_status_bar("narrow");
    // Wide first — and *wide of the rail's threshold*, which is the whole
    // point: the claim is that the rail is there before the collapse, so the
    // fixture has to be a size where it is.
    h.set_size(egui::vec2(rail_visible_size().0, rail_visible_size().1));
    settle(&mut h);
    assert_painted(&h, "PROJECTS");
    let wide_viewport = h.ctx.content_rect();
    assert!(wide_viewport.width() >= shell::MIN_SIDEBAR_WINDOW_WIDTH);

    // Then the collapse.
    h.set_size(egui::vec2(SIDEBAR_VISIBLE_FROM - 300.0, 560.0));
    settle(&mut h);
    let narrow = h.ctx.content_rect();
    assert!(
        narrow.width() < shell::MIN_SIDEBAR_WINDOW_WIDTH,
        "the second render must actually be narrow, or this test proves nothing: \
         {} wide",
        narrow.width()
    );
    // The rail is gone…
    assert_not_painted(&h, "PROJECTS");
    assert_not_painted(&h, "Filter repos / branches…");
    // …and the tab strip survives intact. Shrinking the window must not hide
    // the navigation: all five tabs still paint, and exactly one of them still
    // carries the accent underline, because the strip is a top band that does
    // not depend on the rail's width.
    for label in ["Changes", "Log", "Branches", "Worktrees", "Submodules"] {
        assert_painted(&h, label);
    }
    assert_eq!(
        tab_underlines(&h).len(),
        1,
        "the collapsed render marks exactly one active tab too"
    );
    // The repository switcher survives as the *other* route: the sidebar's
    // header row is inside the rail and goes with it, so the command palette
    // is what keeps workspace switching reachable at a narrow width. Opening
    // it here and finding the action is the assertion — "the palette still
    // lists it" is the same claim, but driving the open state is what proves
    // the route is not merely declared.
    {
        // The frozen find shortcut, `Ctrl+Shift+A` — opened the way a user
        // opens it, so the assertion is about the route rather than about a
        // test poking the open flag. At this width the sidebar's header row
        // (the other trigger) is inside the rail and therefore gone, which is
        // what makes the palette the only remaining door to a workspace switch.
        h.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::A);
        settle(&mut h);
        assert!(
            h.state().ui.command_palette,
            "the palette route to switching workspaces is still reachable at a \
             narrow width"
        );
        assert_painted(&h, "Switch Workspace…");
        h.key_press(egui::Key::Escape);
        settle(&mut h);
    }
    // And the content area still holds everything it painted wide.
    let inner = narrow.shrink(8.0);
    for (rect, _color) in filled_rects(&h) {
        assert!(
            inner.intersect(rect) == rect,
            "the collapsed render clips: {rect:?} escapes {inner:?}"
        );
    }
    for galley in painted_galleys(&h) {
        assert!(
            inner.intersect(galley.rect) == galley.rect,
            "the collapsed render clips text: {:?} at {:?} escapes {inner:?}",
            galley.text,
            galley.rect
        );
    }
    // Painted ink, resolved at paint time, is part of the same check: a galley
    // laid out in white and never overridden would be legible to a reader of
    // the source and invisible to a user.
    assert!(
        painted_ink(&h, "2 total").is_some_and(|ink| ink == Palette::INK_2),
        "the status bar's total keeps the step-up ink in the collapsed render too"
    );
}
