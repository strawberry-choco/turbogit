//! IDE shell frame (issue #9, spec §6): workspace sidebar, tool tab strip,
//! status bar.
//!
//! The shell is the always-present frame of the main window (CONTEXT.md:
//! "Shell"); every page renders inside it. Region metrics come from spec
//! §4.2 and are exposed as constants so tests can assert them.
//!
//! Frozen keyboard shortcuts (ADR-0009) are dispatched here unchanged:
//! Ctrl+K commit · Ctrl+Shift+K push · Ctrl+T refresh · Ctrl+Shift+A find ·
//! Alt+` VCS operations. Spec R7 adds F7/Shift+F7 hunk navigation and the
//! `/` file-filter focus behind a three-tier input gate (dialogs → popups →
//! focused text fields) — see [`handle_shortcuts`].

use std::sync::Arc;

use egui::Galley;
use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Frame, Key, Layout, Margin, Panel, Pos2,
    Rect, RichText, Sense, Stroke, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType,
};

use super::icons::{self, Icon};
use super::widgets;
use crate::theme::Palette;
use turbogit_app::root_caches::Affected;
use turbogit_app::state::{AppState, Dialog, Granularity, Tab};
// --- Spec metrics (§4.2 fixed heights) --------------------------------------
/// Tab strip height (`.tg-tabs`). The strip is the content area's topmost
/// chrome since the repo header was deleted, so this band now sits flush
/// against the top window edge.
pub const TAB_STRIP_HEIGHT: f32 = 32.0;
/// Single tab item height.
pub const TAB_ITEM_HEIGHT: f32 = 31.0;
/// Status bar height.
pub const STATUS_BAR_HEIGHT: f32 = 24.0;
/// Minimum work-area width at which the workspace sidebar renders
/// (issue #05): below it the log's minimum pane sizes cannot hold.
pub const MIN_SIDEBAR_WINDOW_WIDTH: f32 = 1000.0;

const TAB_ICON_SIZE: f32 = 14.0; // §6.2 tab icons
const TAB_TEXT: f32 = crate::theme::TYPE_CONTROL;
/// The active tab's accent underline, in px.
///
/// **The rail width, read from the token layer** — the same 2 px
/// [`crate::theme::RAIL_WIDTH`] a selected row's leading rail is, and the second
/// of R1's two non-button jobs for `BRAND`. The two numbers are one number, so
/// they are read from one declaration: a local `2.0` here would be a second
/// definition of the accent's thickness that could drift from the row's, and the
/// underline would quietly stop matching the rail it is the horizontal twin of.
///
/// `tests/branch_component_kit.rs::the_rail_width_is_defined_in_the_token_layer_and_consumed_by_one_painter`
/// pins the set of files that may **name** `RAIL_WIDTH` to exactly three — the
/// token layer that defines it, `components.rs` (the one rail painter), and this
/// module — so the reading is recorded rather than invisible. Note what that
/// ratchet does *not* claim: it does not make this a rail site. A rail is a 2 px
/// vertical bar at a row's **leading edge**, and this is a 2 px horizontal rule
/// at a tab's **bottom edge**: same width dimension, same colour, different
/// geometry, and routing it through `components::paint_rail` would paint a
/// vertical bar. What the two share is the *width*, which is why they share the
/// token; what they do not share is the painter.
const TAB_SELECTION_RULE: f32 = crate::theme::RAIL_WIDTH;

/// Compose the whole shell: frozen shortcuts, the shell frame regions
/// (center tabs / status bar) around the workspace sidebar, then the
/// central body (Welcome placeholder or active tool window). The tool
/// window spans the full content width — the third metadata column was
/// removed in the local-changes redesign (its information moved to the
/// status bar).
pub fn render(ui: &mut Ui, state: &mut AppState) {
    handle_shortcuts(ui, state);

    // Issue 14: the tab-strip badges and both tool tabs read the focused
    // root's worktree & submodule caches — keep them filled (fetch on
    // miss, i.e. first frame and after every refresh invalidation).
    // Ticket 03: the cheap list is always kept for the badge, but the
    // per-worktree dirty probes run only while the Worktrees window is
    // open — they are never part of the badge / eager-fill path.
    if !state.show_welcome() {
        ensure_worktree_data(state);
        if state.ui.tab == Tab::Worktrees {
            state.ensure_worktree_probes();
        }
    }

    // Panel order fixes the geometry: the status bar claims the bottom
    // and the central body takes the rest, with the tab strip laid out
    // inside it — the tool window spans the whole content width (the
    // metadata rail that once split it was removed, issue 03). Nothing
    // claims the top window edge: the CentralPanel starts there, and the
    // tab strip is the first band inside it.
    if state.ui.show_status_bar {
        render_status_bar(ui, state);
    }

    egui::CentralPanel::default().show(ui, |ui| {
        if state.show_welcome() {
            // Welcome page: no project is open, so the tool-window tab strip
            // (Changes / Log / Branches / Worktrees / Submodules) is dead
            // chrome — skip it and show only the page itself.
            super::welcome::show(ui, state);
        } else {
            // The tool window, the activity log panel (issue #04), and the
            // workspace sidebar (issue #05) share the central body with the
            // tab strip. Reserve explicit rects: a `ui.horizontal` would size
            // its children to one interact row, collapsing every ScrollArea
            // inside the tool window. The sidebar claims the left edge of the
            // work area and runs the full height of it; the tab strip starts
            // at the sidebar's right edge and is the content column's
            // topmost band, since the repo header above it was deleted; the
            // activity log keeps its full-width strip at the bottom (screen
            // 01).
            let body = ui.available_rect_before_wrap();
            let activity_h = if state.ui.activity.expanded {
                super::activity_panel::ACTIVITY_HEIGHT
            } else {
                super::activity_panel::ACTIVITY_COLLAPSED_HEIGHT
            };
            let activity_rect =
                Rect::from_min_max(Pos2::new(body.min.x, body.max.y - activity_h), body.max);
            let work_rect =
                Rect::from_min_max(body.min, Pos2::new(body.max.x, activity_rect.min.y));
            // The sidebar is a wide-window region: below the threshold the
            // remaining tool area can no longer hold the log's four panes at
            // their minimum sizes (issue #23), so the rail hides and the
            // pre-sidebar geometry holds (issue #05).
            let sidebar_visible = work_rect.width() >= MIN_SIDEBAR_WINDOW_WIDTH;
            let sidebar_w = if sidebar_visible {
                super::sidebar::SIDEBAR_WIDTH
            } else {
                0.0
            };
            let sidebar_rect = Rect::from_min_max(
                work_rect.min,
                Pos2::new(work_rect.min.x + sidebar_w, work_rect.max.y),
            );
            let right_rect = Rect::from_min_max(
                Pos2::new(sidebar_rect.max.x, work_rect.min.y),
                work_rect.max,
            );
            // The tab strip took the deleted header's place at the top of
            // the content column: it starts at the sidebar's right edge and
            // is flush with the window's top edge.
            let tabs_rect = Rect::from_min_max(
                right_rect.min,
                Pos2::new(right_rect.max.x, right_rect.min.y + TAB_STRIP_HEIGHT),
            );
            let content_rect =
                Rect::from_min_max(Pos2::new(right_rect.min.x, tabs_rect.max.y), right_rect.max);
            // The tool window takes the full content width — there is no
            // third metadata column beside it (local-changes redesign 03).
            let tool_rect = content_rect;

            let mut sidebar_ui = ui.new_child(
                UiBuilder::new()
                    .max_rect(sidebar_rect)
                    .layout(Layout::top_down(Align::Min)),
            );
            if sidebar_visible {
                super::sidebar::show(&mut sidebar_ui, state);
            }
            ui.advance_cursor_after_rect(sidebar_rect);
            let mut tabs_ui = ui.new_child(
                UiBuilder::new()
                    .max_rect(tabs_rect)
                    .layout(Layout::top_down(Align::Min)),
            );
            render_tab_strip(&mut tabs_ui, state);
            ui.advance_cursor_after_rect(tabs_rect);
            let mut tool_ui = ui.new_child(
                UiBuilder::new()
                    .max_rect(tool_rect)
                    .layout(Layout::top_down(Align::Min)),
            );
            show_tool_window(&mut tool_ui, state);
            ui.advance_cursor_after_rect(tool_rect);
            let mut activity_ui = ui.new_child(
                UiBuilder::new()
                    .max_rect(activity_rect)
                    .layout(Layout::top_down(Align::Min)),
            );
            super::activity_panel::show(&mut activity_ui, state);
            ui.advance_cursor_after_rect(activity_rect);
        }
    });

    // While an async op or a keyed read is in flight, keep frames coming so its
    // completion (OpCompleted → refresh → reload, or a settled cache value)
    // lands without waiting for unrelated input — the headless harness relies
    // on the same signal `app.rs` gets from drain_events in production
    // (spec R2 story 8). `busy` is dispatch's; `read_pending` is the reads',
    // and it says only that *a* surface is working, not which.
    if state.ui.busy || state.read_pending() {
        ui.ctx().request_repaint();
    }
}

// --- Composition (helpers) ---------------------------------------------------

/// Shortcut dispatch (ADR-0009 frozen five + spec R7's F7/Shift+F7 and `/`),
/// behind the three-tier input gate:
///
/// - **Tier 1** — a dimmed modal dialog (dialog, Settings, confirm prompt,
///   conflict editor) is open: every shell shortcut is suppressed.
/// - **Tier 2** — a floating popup (VCS operations, command palette,
///   Branches) is open: the popup owns plain typing through its own widgets,
///   and the R7 keys pause. The frozen five keep their dispatch-first
///   contract — they read raw input before any widget by design
///   (`redesign_polish.rs` pins it), so they still fire here; Alt+` stays
///   live as the VCS popup's owning key and toggles it closed.
/// - **Tier 3** — a text field holds keyboard focus: `/` goes to that input
///   instead of arming the file filter, while F7/Shift+F7 stay live (they
///   are not text keys).
fn handle_shortcuts(ui: &mut Ui, state: &mut AppState) {
    let dialog_open = state.ui.dialog.is_some()
        || state.ui.settings_open
        || state.ui.confirm.is_some()
        || state.ui.conflict_open.is_some()
        || state.ui.bulk_op.is_some()
        || state.ui.bulk_run.is_some();

    // Alt+` opens the VCS operations popup — and closes it again while it is
    // itself open (its owning key), unless a modal dialog swallowed input.
    if !dialog_open && ui.input(|i| i.key_pressed(Key::Backtick) && i.modifiers.alt) {
        state.ui.vcs_popup = !state.ui.vcs_popup;
    }

    // The branch context menu joins the popup gate: while it is open the
    // R7 keys pause (the frozen five keep their dispatch-first contract).
    let popup_open = state.ui.vcs_popup
        || state.ui.command_palette
        || state.ui.branches_popup
        || state.ui.branches_tree.context_menu.is_some();
    if dialog_open {
        return;
    }

    let ks = ui.input(|i| Shortcut {
        commit: i.modifiers.ctrl && !i.modifiers.shift && i.key_pressed(Key::K),
        push: i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(Key::K),
        refresh: i.modifiers.ctrl && i.key_pressed(Key::T),
        find: i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(Key::A),
    });
    if ks.commit {
        switch_tab(state, Tab::Commit);
    }
    if ks.push {
        state.ui.dialog = Some(Dialog::Push);
    }
    if ks.refresh {
        // Manual refresh (decision 8): the full scoped refresh — drops every
        // cache entry (decorations and path history included) and rescans.
        state.refresh(Affected::All);
    }
    if ks.find {
        state.ui.command_palette = true;
        state.ui.command_query.clear();
    }

    if popup_open {
        return;
    }

    // `/` arms the Commit tool window's file filter (spec R7) — only over
    // the Commit window itself, and never while any text field (including
    // the filter input) holds the keyboard: then `/` types into it.
    let slash = ui.input(|i| i.key_pressed(Key::Slash) && !i.modifiers.any());
    if slash
        && state.ui.tab == Tab::Commit
        && !state.show_welcome()
        && ui.ctx().memory(|m| m.focused().is_none())
    {
        state.ui.focus_file_filter = true;
    }

    // F7 / Shift+F7 hunk navigation (spec R7).
    let (f7_next, f7_prev) = ui.input(|i| {
        (
            i.key_pressed(Key::F7) && !i.modifiers.shift,
            i.key_pressed(Key::F7) && i.modifiers.shift,
        )
    });
    if f7_next {
        apply_hunk_nav(state, super::hunk_nav::Dir::Next);
    } else if f7_prev {
        apply_hunk_nav(state, super::hunk_nav::Dir::Prev);
    }

    // Char-range staging (issue 19): Enter stages the armed selection, Esc
    // clears it. Plain keys only, and never while a text field (commit
    // message, filter) owns the keyboard — then they type into it.
    if state.ui.char_selection.is_some() {
        let text_focus = ui.ctx().memory(|m| m.focused().is_some());
        let enter = ui.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.any());
        let esc = ui.input(|i| i.key_pressed(Key::Escape));
        if !text_focus {
            if enter {
                turbogit_app::granular::stage_char_selection(state);
            } else if esc {
                turbogit_app::granular::clear_char_selection(state);
            }
        }
    }
}

/// Apply one F7/Shift+F7 press (spec R7): decide via the pure
/// [`hunk_nav::advance_hunk`] over the active Commit sub-tab's changed-file
/// list, then move the current hunk, show the transient edge hint, or cross
/// files by retargeting the preview — the same path a file-list click uses,
/// so the diff read loads the new diff and lands on its first hunk.
fn apply_hunk_nav(state: &mut AppState, dir: super::hunk_nav::Dir) {
    // Only meaningful over a loaded diff preview in the Commit tool window.
    if state.ui.tab != Tab::Commit || state.show_welcome() || state.ui.preview_change.is_none() {
        return;
    }
    let total = super::diff::preview_hunk_count(state);
    if total == 0 {
        return;
    }

    // Cross-file traversal list: the active sub-tab's changed files in
    // display order. Hunk counts are known only for the previewed file's
    // loaded diff; every other listed entry is presumed navigable (it is
    // listed because it changed).
    let files = super::commit_window::active_subtab_files(state);
    let current_file = files
        .iter()
        .position(|p| Some(p) == state.ui.preview_change.as_ref());
    let counts: Vec<usize> = files
        .iter()
        .map(|p| {
            if Some(p) == state.ui.preview_change.as_ref() {
                total
            } else {
                1
            }
        })
        .collect();
    let has_adjacent_file = current_file.is_some_and(|from| {
        super::hunk_nav::adjacent_file_with_hunks(&counts, from, dir).is_some()
    });

    let now = std::time::Instant::now();
    match super::hunk_nav::advance_hunk(
        state.ui.diff_current_hunk,
        total,
        dir,
        now,
        state.ui.hunk_nav_armed_edge,
        super::hunk_nav::EDGE_WINDOW,
        has_adjacent_file,
    ) {
        super::hunk_nav::Outcome::Moved(idx) => {
            state.ui.diff_current_hunk = idx;
            state.ui.hunk_nav_armed_edge = None;
        }
        super::hunk_nav::Outcome::Nudge => {
            let hint = match dir {
                super::hunk_nav::Dir::Next => "Press again for next file",
                super::hunk_nav::Dir::Prev => "Press again for previous file",
            };
            state.ui.toast = Some(turbogit_app::state::Toast::info(hint));
            state.ui.hunk_nav_armed_edge = Some((dir, now));
        }
        super::hunk_nav::Outcome::CrossFile => {
            state.ui.hunk_nav_armed_edge = None;
            if let Some(from) = current_file
                && let Some(target) = super::hunk_nav::adjacent_file_with_hunks(&counts, from, dir)
                && let Some(path) = files.get(target)
            {
                state.ui.preview_change = Some(path.clone());
                // Landing hunk: the diff read resets to the first hunk on
                // its fresh load.
            }
        }
    }
}

struct Shortcut {
    commit: bool,
    push: bool,
    refresh: bool,
    find: bool,
}

/// The live badge count for a shell tab (issue 14): the focused root's
/// cached worktree / submodule list length when loaded and non-zero,
/// `None` otherwise (no badge for tabs without a count).
fn tab_badge_count(state: &AppState, tab: Tab) -> Option<usize> {
    let id = state.selected_root.as_ref()?;
    let n = match tab {
        Tab::Worktrees => state.caches.worktrees(id)?.len(),
        Tab::Submodules => state.caches.submodules(id)?.len(),
        _ => return None,
    };
    (n > 0).then_some(n)
}

/// Keep the focused root's worktree & submodule caches filled (issue 14).
/// Fetches are async; the caches fill through the event pump.
fn ensure_worktree_data(state: &mut AppState) {
    if let Some(id) = state.selected_root.clone() {
        if state.caches.worktrees(&id).is_none() {
            state.fetch_worktrees(id.clone());
        }
        if state.caches.submodules(&id).is_none() {
            state.fetch_submodules(id);
        }
    }
}

fn switch_tab(state: &mut AppState, tab: Tab) {
    state.ui.tab = tab;
    if tab == Tab::Branches {
        // Focus lands in the Branches search box when the tab opens (issue 06).
        state.ui.branches_focus_search = true;
    }
    state.persist_ui();
}

// --- Shared painting helpers ---------------------------------------------------

enum Edge {
    Top,
    Bottom,
}

/// 1px LINE border along one edge of a rect, without affecting layout
/// (spec §6.2: bottom strokes on the repo header and tab strip, top
/// stroke on the status bar, right stroke on the rail).
fn paint_edge_line_at(ui: &Ui, rect: Rect, edge: Edge) {
    let stroke = Stroke::new(1.0, Palette::LINE);
    let painter = ui.painter();
    match edge {
        Edge::Bottom => painter.line_segment(
            [
                Pos2::new(rect.left(), rect.bottom() - 0.5),
                Pos2::new(rect.right(), rect.bottom() - 0.5),
            ],
            stroke,
        ),
        Edge::Top => painter.line_segment(
            [
                Pos2::new(rect.left(), rect.top() + 0.5),
                Pos2::new(rect.right(), rect.top() + 0.5),
            ],
            stroke,
        ),
    };
}

fn paint_edge_line(ui: &Ui, edge: Edge) {
    paint_edge_line_at(ui, ui.max_rect(), edge);
}

// --- Tab strip ---------------------------------------------------------------------

/// Shell tabs in strip order (issue #03). The legacy History tab was
/// deleted in issue #19 (file history lives in Git Log's path-scoped
/// view), and Settings left the strip in issue #16: it is a gear-only
/// modal now (spec §9.1 correction). Branches is unimplemented in v1 and
/// renders a labeled placeholder pane (ADR-0008); Worktrees / Submodules
/// are real browsers since issue 14, with live count badges.
const SHELL_TABS: [(Tab, Icon, &str); 5] = [
    (Tab::Commit, Icon::GIT_COMMIT, "Changes"),
    (Tab::Log, Icon::GIT_BRANCH, "Log"),
    (Tab::Branches, Icon::GIT_BRANCH, "Branches"),
    (Tab::Worktrees, Icon::FOLDER, "Worktrees"),
    (Tab::Submodules, Icon::FOLDER_GIT, "Submodules"),
];

/// Tab strip (32px, bare BG, bottom border LINE): icon + label entries.
/// Since the repo header above it was deleted the strip is the content
/// area's **topmost** chrome — it sits flush against the top window edge
/// — so it is treated as a top band rather than as a hanging tab row:
///
/// - The strip keeps bare `Palette::BG` and gains **no** top hairline. Its
///   top edge *is* the window edge, and a rule there would frame the
///   window inside itself. The content column therefore reads as one
///   continuous surface from the tab strip down through the tool window,
///   with the single bottom `LINE` edge carrying all of the
///   chrome-from-content information (spec §6.2).
/// - The active tab is no longer a floating pill inset from the strip's
///   top/bottom. A pill needs surrounding air to read as a pill; with the
///   header gone there is no air above it, so it would read as a detached
///   chip jammed against the window frame. It is now a full-strip-height
///   band anchored to the top edge — a 2px `BRAND` underline along its own
///   bottom edge, with the label and its icon one step brighter than every
///   inactive tab's.
///
/// **Those two marks and no others.** The band used to also carry a raised
/// `SURFACE_2` fill, which made the active tab a *second* selection vocabulary
/// beside the sidebar's — a filled slab here, a fill-plus-rail there, for the
/// same fact. R1 gives `BRAND` three jobs in the whole app and the underline is
/// one of them; a raised fill behind a tab is a fourth way of saying "this one"
/// that no other surface in the app uses, and it is also the wrong rung of the
/// raised ladder (the strip sits on `BG`, where raised is `SURFACE`, not
/// `SURFACE_2` — R5). The underline overwrites the strip's `LINE` across the
/// active tab's own width, so the selection underline and the divider read as
/// one edge, which is why the two together need no second rule.
fn render_tab_strip(ui: &mut Ui, state: &mut AppState) {
    let width = ui.available_width();
    let (strip, _) = ui.allocate_exact_size(Vec2::new(width, TAB_STRIP_HEIGHT), Sense::hover());
    paint_edge_line_at(ui, strip, Edge::Bottom);

    let font = FontId::new(TAB_TEXT, FontFamily::Proportional);
    let mut x = strip.left() + 4.0;
    for (tab, icon, label) in SHELL_TABS {
        // Live count badge (issue 14, screen 01 "Worktrees 3"): appended to
        // the label galley when the focused root's cached list carries a
        // non-zero count — data the fetch-on-miss trigger keeps filled.
        let label = match tab_badge_count(state, tab) {
            Some(n) => format!("{label} {n}"),
            None => label.to_string(),
        };
        let galley = ui
            .painter()
            .layout_no_wrap(label.clone(), font.clone(), Color32::WHITE);
        let content_w = TAB_ICON_SIZE + 6.0 + galley.size().x;
        // The row is the spec's 31px tab height and is the click target;
        // `tab_item` paints its underline over the full 32px strip below, so
        // the rule reaches the divider with no unclaimed sliver.
        let rect = Rect::from_min_size(
            Pos2::new(x, strip.top()),
            Vec2::new(24.0 + content_w, TAB_ITEM_HEIGHT),
        );
        tab_item(ui, state, rect, tab, icon, &label, galley);
        x += rect.width() + 2.0;
    }
}

/// The ink an active tab's label and icon paint, and an inactive tab's.
///
/// Two ramp steps, and both are legal where they land: the strip's own fill is
/// `Palette::BG`, on which `INK` measures 12.59:1 and `INK_3` measures 5.02:1.
/// With the raised band gone there is no raised surface under either label, so
/// the muted step is legal again on the active tab too — which is the point of
/// removing the band rather than keeping it and merely adding the underline.
/// The active tab is the *brightest* ink in the strip and the inactive tabs are
/// the muted one: "which tool window am I in" is a reading-order question, and
/// the ramp is the answer to that.
const ACTIVE_TAB_INK: Color32 = Palette::INK;
const INACTIVE_TAB_INK: Color32 = Palette::INK_3;

fn tab_item(
    ui: &mut Ui,
    state: &mut AppState,
    rect: Rect,
    tab: Tab,
    icon: Icon,
    label: &str,
    galley: Arc<Galley>,
) {
    let id = ui.auto_id_with(("shell_tab", label));
    let response = ui.interact(rect, id, Sense::click());
    let active = state.ui.tab == tab;
    let painter = ui.painter().clone();
    // The band is the row's column over the *whole* strip (`rect.top()` is the
    // strip's top edge — the caller places it there), anchored to the window
    // frame rather than floating inside the band.
    let band = Rect::from_min_max(
        Pos2::new(rect.left(), rect.top()),
        Pos2::new(rect.right(), rect.top() + TAB_STRIP_HEIGHT),
    );
    if active {
        // The accent underline, the shared width, on the band's bottom edge —
        // the same measure and weight the sidebar's active-row rail uses, so
        // "selected" is one idea across the shell rather than two. It overwrites
        // the strip's `LINE` divider across the tab's own width, so the
        // selection mark and the divider read as one edge.
        let rule = Rect::from_min_max(
            Pos2::new(band.left(), band.bottom() - TAB_SELECTION_RULE),
            Pos2::new(band.right(), band.bottom()),
        );
        painter.rect_filled(rule, CornerRadius::ZERO, Palette::BRAND);
    } else if response.hovered() {
        // Hover is a pointer state, not a selection: a quiet wash on `BG`, so
        // a hovered-but-inactive tab never reads as the selected one — it has
        // no underline and its label stays at the muted step.
        painter.rect_filled(
            band,
            CornerRadius::ZERO,
            widgets::tint_over_bg(Palette::INK, 0.08),
        );
    }

    let ink = if active {
        ACTIVE_TAB_INK
    } else {
        INACTIVE_TAB_INK
    };
    let cy = rect.center().y;
    let mut cx = rect.left() + 12.0;
    icons::centered_icon(
        ui,
        icon,
        Pos2::new(cx + TAB_ICON_SIZE / 2.0, cy),
        TAB_ICON_SIZE,
        ink,
    );
    cx += TAB_ICON_SIZE + 6.0;
    painter.galley_with_override_text_color(Pos2::new(cx, cy - galley.size().y / 2.0), galley, ink);

    widgets::focus_ring(ui, &response);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    if response.clicked() {
        switch_tab(state, tab);
    }
}

// --- Status bar -----------------------------------------------------------------------

/// Status bar (issue #03, screen 01): the version / git-status line, then
/// aggregated workspace state across every registered root — diverged,
/// conflicts, unpulled, archived, dirty totals on the left; total root
/// count on the right; busy spinner at the far right. The version line
/// came here with the topbar's deletion so it paints on Welcome and in
/// the shell alike.
fn render_status_bar(ui: &mut Ui, state: &mut AppState) {
    let agg = AggregatedStatus::compute(&state.multi.roots, &|id| state.caches.ahead_behind(id));

    Panel::bottom("status_bar")
        .exact_size(STATUS_BAR_HEIGHT)
        .frame(
            Frame::new()
                .fill(Palette::SURFACE)
                .inner_margin(Margin::symmetric(8, 0)),
        )
        .show(ui, |ui| {
            ui.style_mut().spacing.button_padding = crate::theme::DENSITY_DENSE_BUTTON;
            // Two clusters on one row, each given its **own** rect rather than
            // a nested `right_to_left` inside a `left_to_right`.
            //
            // The nested form does not hold its right edge: the left cluster is
            // measured first, the nested layout takes whatever remains, and the
            // total lands *past* the panel's right inset — at the default
            // 1024 headless width it painted at x 1008..1040 in a panel whose
            // content ends at 1008, so 32 points of the "1 total" label were
            // off the window. Two explicit halves cannot drift that way: the
            // right one is measured from the panel's own right edge, so the
            // total's right edge is the inset no matter what the left cluster
            // grows to.
            let band = ui.max_rect();
            // The right cluster gets a rect **measured from the panel's own
            // right edge** and sized to its own content, then lays out left to
            // right inside it. A `Layout::right_to_left` cannot do this job: a
            // `ui.label` is a *wrapping* label, so it claims the whole
            // remaining width and then anchors its galley at that rect's
            // **right** edge — which puts the text's left edge at the panel's
            // right inset and runs it off the window. At the default 1024
            // headless width "1 total" painted at x 1008..1040 inside a panel
            // whose content ends at 1008, so 32 points of the label were
            // outside the frame. This bug predates every change in this ticket;
            // it is what the "no clipping at the default size" criterion finds.
            //
            // Measuring from the right edge cannot drift: however far the left
            // cluster grows, the total's right edge stays at the inset.
            let total_text = status_total_text(&agg);
            let total_w = ui
                .painter()
                .layout_no_wrap(
                    total_text.clone(),
                    crate::theme::chrome_font(crate::theme::TYPE_CONTROL),
                    STATUS_BAR_INK,
                )
                .size()
                .x;
            let spinner_w = if state.ui.busy {
                ui.spacing().interact_size.y + ui.spacing().item_spacing.x
            } else {
                0.0
            };
            let split = (band.max.x - total_w - spinner_w).max(band.min.x);
            let left_rect = Rect::from_min_max(band.min, Pos2::new(split, band.max.y));
            let right_rect = Rect::from_min_max(Pos2::new(split, band.min.y), band.max);
            ui.scope_builder(UiBuilder::new().max_rect(left_rect), |ui| {
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    version_line(ui, state);
                    aggregated_status_chips(ui, &agg);
                    granular_status_chips(ui, state);
                });
            });
            ui.scope_builder(UiBuilder::new().max_rect(right_rect), |ui| {
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    if state.ui.busy {
                        ui.spinner();
                    }
                    ui.label(
                        RichText::new(total_text)
                            .font(crate::theme::chrome_font(crate::theme::TYPE_CONTROL))
                            .color(STATUS_BAR_INK),
                    );
                });
            });
            paint_edge_line(ui, Edge::Top);
        });
}

/// Version / git-status line (issue #34): app version, resolved git
/// version, and the total indexed repo count — leftmost in the cluster,
/// ahead of the aggregated counters, at the same quiet ink every other status
/// line wears. It paints on every screen: the status bar is the one chrome
/// band Welcome keeps.
///
/// **The ink is `INK_2`, not `INK_3`, and that is a legality answer rather than
/// a preference.** The band this line paints on is `Palette::SURFACE` — a
/// *raised* surface — and ticket 02 narrowed `INK_3`'s legal set to the app
/// background, the content surface and the sidebar surface. On `SURFACE` it
/// measures 4.20:1, under AA, and the token layer's own instruction for a
/// caller in that position is to step *up*. `INK_2` measures 6.58:1 there. The
/// step-up is why every status-bar line shares one ink: the bar is one raised
/// band, so it gets one step.
fn version_line(ui: &mut Ui, state: &AppState) {
    ui.label(
        RichText::new(format!(
            "v{} · git {} · {} repos indexed",
            env!("CARGO_PKG_VERSION"),
            state.git_version,
            state.multi.roots.len(),
        ))
        .font(crate::theme::chrome_font(crate::theme::TYPE_CONTROL))
        .color(Palette::INK_2),
    );
}

/// The status bar's one separator ink.
///
/// `INK_2` for the same reason [`version_line`] uses it: the bar's band is
/// `Palette::SURFACE`, a raised surface, and `INK_3` is not legal there
/// (4.20:1). One named value for the bar's quiet text means a fourth status
/// line added later inherits the step-up instead of re-deciding it.
const STATUS_BAR_INK: Color32 = Palette::INK_2;

/// Staging-granularity readout (issue 19, screen 06): the armed line
/// selection, the active granularity, and how many repos granular ops
/// reach. Only over an open project — the welcome page has nothing in
/// scope. An empty multi-repo selection means the focused single root.
fn granular_status_chips(ui: &mut Ui, state: &AppState) {
    if state.show_welcome() {
        return;
    }
    let chip = |ui: &mut Ui, color: Color32, text: String| {
        ui.label(
            RichText::new("·")
                .font(crate::theme::chrome_font(crate::theme::TYPE_CONTROL))
                .color(STATUS_BAR_INK),
        );
        ui.colored_label(color, text);
    };
    if state.ui.char_selection.is_some() {
        chip(
            ui,
            Palette::ACCENT_TEXT,
            "1 line-selection active".to_owned(),
        );
    }
    let granularity = match state.ui.diff_granularity {
        Granularity::File => "file",
        Granularity::Hunk => "hunk",
        Granularity::Line => "line",
    };
    chip(ui, STATUS_BAR_INK, format!("granularity: {granularity}"));
    let scope = if state.ui.repo_selection.is_empty() {
        1
    } else {
        state.ui.repo_selection.len()
    };
    chip(
        ui,
        STATUS_BAR_INK,
        format!("{scope} repo{} in scope", if scope == 1 { "" } else { "s" }),
    );
}

/// The three aggregated counters the status bar surfaces (issue #03).
///
/// **Not a colour map.** It used to be: a local enum whose `color()` spelled
/// `STATUS_DIVERGED` / `COUNTER` / `COUNTER` out as bare tokens, sitting beside
/// `theme::RepoState::color()` — two answers to "what colour is a diverged
/// root" in the same file, one of them three lines from the other. This type
/// now carries no colour at all; it names *which* aggregate a chip reports, and
/// [`CounterChip::state`] answers through the one repository-state map. The
/// consequence is the property worth having: a fourth aggregate added here
/// cannot pick a colour, because there is nowhere to pick one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CounterChip {
    Diverged,
    Unpulled,
    Dirty,
}

impl CounterChip {
    /// This chip's repository state, and through it the one colour map.
    fn state(self) -> crate::theme::RepoState {
        use crate::theme::RepoState as S;
        match self {
            CounterChip::Diverged => S::Diverged,
            CounterChip::Unpulled => S::Unpulled,
            CounterChip::Dirty => S::Dirty,
        }
    }

    fn color(self) -> Color32 {
        self.state().color()
    }
}

/// Workspace aggregates used by the status bar (issue #03).
#[derive(Default)]
struct AggregatedStatus {
    diverged: usize,
    conflicts: usize,
    unpulled: usize,
    archived: usize,
    dirty: usize,
    total: usize,
}

impl AggregatedStatus {
    /// Aggregate across the registered roots; `ahead_behind` reads the root
    /// caches (absent entries → `(0, 0)`). Pure over its inputs so the
    /// counting semantics are unit-testable (issue 02).
    fn compute(
        roots: &[turbogit_domain::model::Root],
        ahead_behind: &dyn Fn(&turbogit_domain::model::RootId) -> Option<(usize, usize)>,
    ) -> Self {
        let mut agg = AggregatedStatus {
            total: roots.len(),
            ..Default::default()
        };
        for root in roots {
            if !root.status.conflicted.is_empty() {
                agg.conflicts += 1;
                agg.dirty += 1;
            }
            if root.status.modified() > 0 || root.status.unversioned() > 0 {
                agg.dirty += 1;
            }
            // Archived roots: spec definition is "frozen / excluded from
            // cascades" — v1 has no archived concept yet, so the count
            // stays at zero (the chip appears once a real signal lands).
            if let Some((ahead, behind)) = ahead_behind(&root.id) {
                // Loose "diverged" in v1: any root whose branch has moved
                // relative to its upstream counts.
                if ahead > 0 || behind > 0 {
                    agg.diverged += 1;
                }
                if behind > 0 {
                    agg.unpulled += 1;
                }
            }
        }
        agg
    }
}

/// Left-cluster chips — each only paints when its count > 0 so a quiet
/// project doesn't drown in zeros. Order matches screen 01:
/// diverged · conflicts · unpulled · archived.
fn aggregated_status_chips(ui: &mut Ui, agg: &AggregatedStatus) {
    let mut first = true;
    let mut chip = |ui: &mut Ui, color: Color32, text: String| {
        if !first {
            ui.label(
                RichText::new("·")
                    .font(crate::theme::chrome_font(crate::theme::TYPE_CONTROL))
                    .color(STATUS_BAR_INK),
            );
        }
        first = false;
        ui.colored_label(color, text);
    };
    if agg.diverged > 0 {
        chip(
            ui,
            CounterChip::Diverged.color(),
            format!("{} diverged", agg.diverged),
        );
    }
    if agg.conflicts > 0 {
        // A conflicted root is a *conflict*, so its chip reads the conflict
        // state out of the one map too — the same error red as diverged, by
        // design, with the word beside it saying which of the two it is.
        chip(
            ui,
            crate::theme::RepoState::Conflict.color(),
            format!("{} conflict", agg.conflicts),
        );
    }
    if agg.unpulled > 0 {
        chip(
            ui,
            CounterChip::Unpulled.color(),
            format!("{} unpulled", agg.unpulled),
        );
    }
    if agg.archived > 0 {
        chip(
            ui,
            Palette::STATE_INFO,
            format!("{} archived", agg.archived),
        );
    }
    if agg.dirty > 0 {
        chip(
            ui,
            CounterChip::Dirty.color(),
            format!("{} dirty", agg.dirty),
        );
    }
}

/// The right cluster's one line, as words: the total root count.
///
/// Split from the painting so the bar can **measure** it before laying out the
/// cluster that holds it. The split exists because of a real bug: a wrapping
/// `ui.label` inside a right-to-left layout anchors its galley at the rect's
/// right edge and runs the text off the window, so the bar has to know the
/// line's width *before* it decides where the right cluster starts. One
/// function builds the string, one paints it, and there is no second spelling
/// of the words.
fn status_total_text(agg: &AggregatedStatus) -> String {
    format!("{} total", agg.total)
}

// --- Tool window body --------------------------------------------------------------------

/// Dispatch the active tool window inside the central panel. Log data is
/// fetched lazily exactly as the pre-shell layout did. While a multi-repo
/// selection is live (issue #08) the summary surface takes the body —
/// clearing the selection returns the tool window.
fn show_tool_window(ui: &mut Ui, state: &mut AppState) {
    if !state.ui.repo_selection.is_empty() {
        super::multi_selection::show_summary(ui, state);
        return;
    }
    if state.ui.tab == Tab::Log && state.selected_root.is_some() {
        let id = state.selected_root.clone().unwrap();
        if state.caches.log(&id).is_none() {
            state.fetch_log(id);
        }
    }
    match state.ui.tab {
        Tab::Commit => super::commit_window::show(ui, state),
        Tab::Log => super::log_window::show_log(ui, state),
        // Branches (issues 03+): the grouped list + detail panel spine. No
        // longer a placeholder since issue 03.
        Tab::Branches => super::branches::show(ui, state),
        Tab::Worktrees => super::worktrees::show(ui, state),
        Tab::Submodules => super::submodules::show(ui, state),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use turbogit_domain::model::{Change, ChangeStatus, Root, RootId, RootStatus};

    fn root(path: &str, branch: Option<&str>) -> Root {
        Root {
            id: RootId(PathBuf::from(path).into()),
            path: PathBuf::from(path),
            remotes: vec![],
            branches: vec![],
            current_branch: branch.map(str::to_string),
            head: None,
            status: RootStatus::default(),
        }
    }

    fn change(path: &str, status: ChangeStatus) -> Change {
        Change {
            path: PathBuf::from(path),
            status,
            chunks: vec![],
            staged: false,
            unstaged: false,
            orig_path: None,
        }
    }

    #[test]
    fn counter_chip_reserves_orange_for_unpulled_and_dirty() {
        // Issue 02 (design doc §6-7): the three status-bar counters paint
        // diverged in the error red and unpulled/dirty in the reserved
        // counter orange — the orange is never any other counter or chip.
        assert_eq!(CounterChip::Diverged.color(), Palette::STATUS_DIVERGED);
        assert_eq!(CounterChip::Unpulled.color(), Palette::COUNTER);
        assert_eq!(CounterChip::Dirty.color(), Palette::COUNTER);
    }

    #[test]
    fn counter_orange_only_colors_the_unpulled_and_dirty_surfaces() {
        // Orange is reserved for dirty/unpulled counters in this surface and
        // no general accent usage — for every counter chip, COUNTER is used
        // exactly when the chip is Unpulled or Dirty.
        for kind in [
            CounterChip::Diverged,
            CounterChip::Unpulled,
            CounterChip::Dirty,
        ] {
            let is_counter = matches!(kind, CounterChip::Unpulled | CounterChip::Dirty);
            assert_eq!(
                kind.color() == Palette::COUNTER,
                is_counter,
                "{kind:?} must use COUNTER exactly when it is a dirty/unpulled counter"
            );
        }
    }

    #[test]
    fn computes_diverged_unpulled_and_dirty_counts_from_root_state() {
        // The three counter chips read the real status + ahead/behind data:
        // diverged = any upstream drift, unpulled = behind, dirty = conflicted
        // or modified/unversioned work. Archived stays zero in v1.
        let mut dirty = root("/w/dirty", Some("main"));
        dirty.status.changes = vec![change("a.txt", ChangeStatus::Modified)];
        let mut conflicted = root("/w/conflicted", Some("main"));
        conflicted.status.conflicted = vec![PathBuf::from("c.txt")];
        let diverged = root("/w/diverged", Some("main")); // ahead 1, behind 1
        let unpulled = root("/w/unpulled", Some("main")); // behind 2 only
        let clean = root("/w/clean", Some("main"));
        let roots = vec![dirty, conflicted, diverged, unpulled, clean];

        let agg = AggregatedStatus::compute(&roots, &|id| {
            let p = id.0.as_os_str();
            if p == "/w/diverged" {
                Some((1, 1))
            } else if p == "/w/unpulled" {
                Some((0, 2))
            } else {
                Some((0, 0))
            }
        });

        assert_eq!(agg.total, 5);
        assert_eq!(
            agg.dirty, 2,
            "modified and conflicted roots both count as dirty"
        );
        assert_eq!(agg.conflicts, 1);
        assert_eq!(
            agg.diverged, 2,
            "any ahead/behind drift is loosely diverged"
        );
        assert_eq!(
            agg.unpulled, 2,
            "behind roots count as unpulled whether or not also diverged"
        );
        assert_eq!(agg.archived, 0);
    }
}
