//! Issue #9 — IDE shell frame: topbar, toolbar, rail, tab strip, status bar.
//!
//! Headless egui_kittest harness driving [`turbogit::ui::render`] end-to-end
//! over synthetic raw input (no GPU / window / display server). Asserts only
//! on public surfaces:
//!
//! - **Painted output** — the frame's shapes from `FullOutput`: text galleys
//!   carry their strings; filled rects carry geometry + token color.
//! - **State transitions** — public `AppState` fields after the frames.
//!
//! Covered here (spec §4.2/§6, ADR-0009):
//! - shell regions render at spec dimensions ±4px
//! - rail clicks switch the active tool window; tab strip reflects + controls it
//! - inert Run/Debug/Search chrome changes nothing on click
//! - the five frozen shortcuts dispatch unchanged
//! - no project open → Welcome placeholder page

mod common;

use common::{
    assert_not_painted, assert_painted, filled_rects, galley_origin, settle, shell_harness,
};
use egui::{Color32, Key, Modifiers, Rect};
use egui_kittest::{kittest::Queryable, Harness};
use turbogit::state::{AppState, Dialog, Tab};
use turbogit::theme::Palette;

// --- Region finders (geometry over painted filled rects) --------------------
//
// All regions are located RELATIVE to each other (the harness wraps content
// in an 8px outer margin, and production windows may too), so the assertions
// measure spec dimensions, not absolute screen coordinates.

#[track_caller]
fn expect_region(
    harness: &Harness<'_, AppState>,
    what: &str,
    pred: impl Fn(Rect, Color32) -> bool,
) -> Rect {
    filled_rects(harness)
        .into_iter()
        .find(|(r, c)| pred(*r, *c))
        .map(|(r, _)| r)
        .unwrap_or_else(|| panic!("{what} region was not painted"))
}

/// Topbar: topmost full-width SURFACE band.
fn topbar_rect(harness: &Harness<'_, AppState>) -> Rect {
    expect_region(harness, "topbar", |r, c| {
        c == Palette::SURFACE && r.width() >= 900.0
    })
}

/// Toolbar: full-width BG band directly below the topbar.
fn toolbar_rect(harness: &Harness<'_, AppState>) -> Rect {
    let topbar = topbar_rect(harness);
    expect_region(harness, "toolbar", |r, c| {
        c == Palette::BG && r.width() >= 900.0 && (r.top() - topbar.bottom()).abs() <= 2.0
    })
}

/// The active tab paints a SURFACE rect: ~31px tall, narrow, just below the
/// toolbar (spec §6.2: top-rounded INK-on-SURFACE entry).
fn active_tab_rect(harness: &Harness<'_, AppState>) -> Rect {
    let toolbar_top = toolbar_rect(harness).bottom();
    expect_region(harness, "active tab", |r, c| {
        c == Palette::SURFACE
            && r.height() >= 27.0
            && r.height() <= 35.0
            && r.width() < 400.0
            && r.top() >= toolbar_top - 2.0
            && r.top() <= toolbar_top + 8.0
    })
}

#[derive(Debug, PartialEq)]
struct ModalState {
    tab: Tab,
    dialog: Option<Dialog>,
    vcs_popup: bool,
    command_palette: bool,
    branches_popup: bool,
    settings_open: bool,
}

fn modal_state(harness: &Harness<'_, AppState>) -> ModalState {
    let s = harness.state();
    ModalState {
        tab: s.ui.tab,
        dialog: s.ui.dialog,
        vcs_popup: s.ui.vcs_popup,
        command_palette: s.ui.command_palette,
        branches_popup: s.ui.branches_popup,
        settings_open: s.ui.settings_open,
    }
}

// --- Cycle 1: initial render paints the full shell frame ---------------------

#[test]
fn initial_render_paints_the_shell_over_an_empty_project() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    // Topbar: eight menu labels.
    for label in [
        "File", "Edit", "View", "Navigate", "Code", "Git", "Window", "Help",
    ] {
        assert_painted(&harness, label);
    }
    // Toolbar: inert chrome + functional VCS actions.
    for label in [
        "Run", "Debug", "Search", "Commit", "Pull", "Fetch", "Push", "Branches",
    ] {
        assert_painted(&harness, label);
    }
    // Tab strip: the legacy History tab was deleted in issue #19; file
    // history lives in Git Log's path-scoped view.
    for label in ["Commit", "Log", "Settings"] {
        assert_painted(&harness, label);
    }
    assert_not_painted(&harness, "History");
    // No project open → central body routes to the Welcome page (issue #10).
    assert_painted(&harness, "TurboGit");
}

#[test]
fn no_project_renders_welcome_placeholder_instead_of_old_panels() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    assert_painted(&harness, "A fast, keyboard-friendly Git client");
    // The old panel layout is replaced outright — its left-pane copy is gone.
    assert_not_painted(&harness, "No Git repositories detected.");
    assert_not_painted(&harness, "Clone:");
}

// --- Cycle 2: shell regions at spec dimensions ±4px --------------------------

#[test]
fn shell_regions_render_at_spec_dimensions() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    // Topbar: SURFACE band across the very top (spec §4.2: 38px).
    let topbar = topbar_rect(&harness);
    assert!(
        (topbar.height() - 38.0).abs() <= 4.0,
        "topbar height {} deviates from 38px ±4",
        topbar.height()
    );

    // Toolbar: BG band directly below the topbar (spec §4.2: 34px).
    let toolbar = toolbar_rect(&harness);
    assert!(
        (toolbar.height() - 34.0).abs() <= 4.0,
        "toolbar height {} deviates from 34px ±4",
        toolbar.height()
    );

    // Sidebar rail: SURFACE column on the left edge (spec §4.2: 48px wide),
    // starting where the toolbar ends.
    let rail = expect_region(&harness, "rail", |r, c| {
        c == Palette::SURFACE
            && r.width() >= 40.0
            && r.width() <= 60.0
            && (r.top() - toolbar.bottom()).abs() <= 2.0
    });
    assert!(
        (rail.width() - 48.0).abs() <= 4.0,
        "rail width {} deviates from 48px ±4",
        rail.width()
    );

    // Tab strip: measured via the active tab item (31px in a 32px strip).
    let tab = active_tab_rect(&harness);
    assert!(
        (tab.height() - 31.0).abs() <= 4.0,
        "tab item height {} deviates from 31px ±4",
        tab.height()
    );

    // Status bar: SURFACE band across the very bottom (spec §4.2: ~24px).
    let status = expect_region(&harness, "status bar", |r, c| {
        c == Palette::SURFACE && r.width() >= 900.0 && r.top() >= rail.bottom() - 2.0
    });
    assert!(
        (status.height() - 24.0).abs() <= 4.0,
        "status bar height {} deviates from 24px ±4",
        status.height()
    );
}

#[test]
fn commit_is_the_only_primary_toolbar_button() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    let toolbar = toolbar_rect(&harness);
    let brand_in_toolbar: Vec<Rect> = filled_rects(&harness)
        .into_iter()
        .filter(|(r, c)| *c == Palette::BRAND && toolbar.intersects(*r))
        .map(|(r, _)| r)
        .collect();
    assert_eq!(
        brand_in_toolbar.len(),
        1,
        "exactly one primary-styled button may exist in the toolbar"
    );
    // …and it is the Commit button (its label sits inside the brand fill).
    let commit_pos = galley_origin(&harness, "Commit").expect("Commit label painted");
    assert!(
        brand_in_toolbar[0].contains(commit_pos),
        "the single primary button must be Commit"
    );
}

// --- Cycle 3: rail switches tool windows; tab strip reflects + controls ------

#[test]
fn rail_click_switches_tool_window_and_tab_strip_reflects_it() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);
    assert_eq!(harness.state().ui.tab, Tab::Commit);

    harness.get_by_label("Git Log").click(); // sidebar rail button
    settle(&mut harness);

    assert_eq!(
        harness.state().ui.tab,
        Tab::Log,
        "clicking the Git Log rail button must switch tool windows"
    );
    // Reflection: the active-tab surface rect wraps the Log label…
    let active = active_tab_rect(&harness);
    let log_pos = galley_origin(&harness, "Log").expect("Log tab painted");
    assert!(active.contains(log_pos), "Log tab must render as active");
    // …and the deleted History tab is nowhere in the strip (issue #19).
    assert_not_painted(&harness, "History");
}

#[test]
fn tab_strip_controls_the_active_tool_window() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    harness.get_by_label("Settings").click();
    settle(&mut harness);
    assert_eq!(harness.state().ui.tab, Tab::Settings);

    harness.get_by_label("Log").click();
    settle(&mut harness);
    assert_eq!(harness.state().ui.tab, Tab::Log);
}

// --- Cycle 4: inert chrome ----------------------------------------------------

#[test]
fn inert_run_debug_search_clicks_change_nothing() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);
    let before = modal_state(&harness);

    harness.get_by_label("Run").click();
    settle(&mut harness);
    harness.get_by_label("Debug").click();
    settle(&mut harness);
    // "Search" exists twice (toolbar + rail); both are inert chrome.
    {
        let mut searches = harness.get_all_by_label("Search");
        searches
            .next()
            .expect("Search chrome must be queryable")
            .click();
    }
    settle(&mut harness);

    assert_eq!(
        modal_state(&harness),
        before,
        "Run/Debug/Search are inert in v1 and must not change any state"
    );
}

// --- Cycle 5: the five frozen shortcuts (ADR-0009) ----------------------------

#[test]
fn ctrl_k_returns_to_the_commit_tool_window() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    harness.get_by_label("Settings").click();
    settle(&mut harness);
    assert_eq!(harness.state().ui.tab, Tab::Settings);

    harness.key_press_modifiers(Modifiers::CTRL, Key::K);
    settle(&mut harness);

    assert_eq!(
        harness.state().ui.tab,
        Tab::Commit,
        "Ctrl+K must switch to the Commit tool window"
    );
}

#[test]
fn ctrl_shift_k_opens_the_push_dialog() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness); // shell without any dialog

    assert_eq!(harness.state().ui.dialog, None);

    harness.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::K);
    settle(&mut harness);

    assert_eq!(
        harness.state().ui.dialog,
        Some(Dialog::Push),
        "Ctrl+Shift+K must open the Push dialog"
    );
    // The dialog chrome is really painted, not just state.
    assert_painted(&harness, "Remote:");
    assert_painted(&harness, "Branch:");
    assert_painted(&harness, "Force push (--force-with-lease)");
}

#[test]
fn ctrl_t_rescans_without_disturbing_shell_state() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    harness.key_press_modifiers(Modifiers::CTRL, Key::T);
    settle(&mut harness);

    let after = modal_state(&harness);
    assert_eq!(after.tab, Tab::Commit);
    assert_eq!(after.dialog, None, "Ctrl+T must not open dialogs");
    assert!(!after.command_palette && !after.vcs_popup && !after.branches_popup);
}

#[test]
fn ctrl_shift_a_opens_the_command_palette() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    harness.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::A);
    settle(&mut harness);

    assert!(
        harness.state().ui.command_palette,
        "Ctrl+Shift+A must open the command palette"
    );
    assert_painted(&harness, "Find Action");
}

#[test]
fn alt_backtick_opens_the_vcs_operations_popup() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    harness.key_press_modifiers(Modifiers::ALT, Key::Backtick);
    settle(&mut harness);

    assert!(
        harness.state().ui.vcs_popup,
        "Alt+` must open the VCS operations popup"
    );
    assert_painted(&harness, "VCS Operations");
}
