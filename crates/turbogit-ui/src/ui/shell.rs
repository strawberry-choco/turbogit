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

use egui::{
    Align, Color32, Frame, Key, Layout, Margin, Panel, Pos2, Rect, RichText, Ui, UiBuilder,
    WidgetInfo, WidgetType,
};

use super::components;
use super::icons::Icon;
use super::widgets;
use crate::theme::Palette;
use turbogit_app::root_caches::{Affected, LogRead};
use turbogit_app::state::{AppState, Dialog, Tab};
// --- Spec metrics (§4.2 fixed heights) --------------------------------------
/// Status bar height.
pub const STATUS_BAR_HEIGHT: f32 = 24.0;
/// Minimum work-area width at which the workspace sidebar renders
/// (issue #05): below it the log's minimum pane sizes cannot hold.
pub const MIN_SIDEBAR_WINDOW_WIDTH: f32 = 1000.0;

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
    //
    // The blocked-reason line is TAKEN here, before the bottom panel is laid
    // out: whatever a menu row raised last frame is what this frame reports, and
    // raising nothing means the line goes back to the workspace counters. The
    // take runs whether the region is showing or not, so a hidden status bar
    // cannot accumulate reasons behind itself.
    let status_hint = widgets::take_status_hint(ui.ctx());
    if state.ui.show_status_bar {
        render_status_bar(ui, state, status_hint.as_deref());
    }

    // The central panel carries egui's stock central-panel frame minus the
    // default 8px inner margin: the shell's regions lay out their own edges
    // (tab strip flush with the top, sidebar on the left, status bar at the
    // bottom), so an inset here would read as a floating island of content
    // inside the window.
    let shell_frame = egui::Frame::central_panel(ui.style()).inner_margin(Margin::ZERO);
    egui::CentralPanel::default()
        .frame(shell_frame)
        .show(ui, |ui| {
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

                let mut sidebar_ui = ui.new_child(
                    UiBuilder::new()
                        .max_rect(sidebar_rect)
                        .layout(Layout::top_down(Align::Min)),
                );
                if sidebar_visible {
                    super::sidebar::show(&mut sidebar_ui, state);
                }
                ui.advance_cursor_after_rect(sidebar_rect);
                // The tab strip took the deleted header's place at the top of
                // the content column: it starts at the sidebar's right edge and
                // is flush with the window's top edge. The strip and the tool
                // window below it are one `tab_view` over the whole right
                // column — the component splits the band from the body, so the
                // shell never computes a strip internal.
                let mut column_ui = ui.new_child(
                    UiBuilder::new()
                        .max_rect(right_rect)
                        .layout(Layout::top_down(Align::Min)),
                );
                let tabs = shell_tabs(state);
                let active = state.ui.tab;
                widgets::tab_view(
                    &mut column_ui,
                    state,
                    &tabs,
                    active,
                    switch_tab,
                    |ui, state| {
                        // The end region lays out right-to-left: the first
                        // widget added lands at its right edge, so the visual
                        // order (shortcuts, then settings) is unchanged.
                        let settings = widgets::icon_button(ui, Icon::SETTINGS);
                        settings.widget_info(|| {
                            WidgetInfo::labeled(WidgetType::Button, true, "Open settings")
                        });
                        if settings.clicked() {
                            state.ui.settings_open = true;
                        }
                        let shortcuts = widgets::icon_button(ui, Icon::KEYBOARD);
                        shortcuts.widget_info(|| {
                            WidgetInfo::labeled(WidgetType::Button, true, "Show shortcuts")
                        });
                        if shortcuts.clicked() {
                            state.ui.shortcuts_open = true;
                        }
                    },
                    show_tool_window,
                );
                ui.advance_cursor_after_rect(right_rect);
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
/// - **Tier 1** — a dimmed modal dialog (dialog, Settings, shortcuts help,
///   confirm prompt, conflict editor) is open: every shell shortcut is
///   suppressed.
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
        || state.ui.shortcuts_open
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

    // The diff viewer's three verbs: `⌃⇧D` opens its settings menu,
    // `⌃⇧⇥` moves the caret to the opposite pane (`Diff.FocusOppositePane`), `F4`
    // jumps to the caret's line on the source surface with that line preserved.
    // All three are meaningless with no diff previewed, so that is their gate —
    // the same gate `apply_hunk_nav` reads, not a new one.
    let diff_keys = ui.input(|i| {
        (
            i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(Key::D),
            i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(Key::Tab),
            i.key_pressed(Key::F4) && !i.modifiers.any(),
        )
    });
    let diff_preview_open =
        state.ui.tab == Tab::Commit && !state.show_welcome() && state.ui.preview_change.is_some();
    if diff_preview_open {
        if diff_keys.0 {
            state.ui.diff_options_open = !state.ui.diff_options_open;
        }
        if diff_keys.1 {
            state.ui.diff_pane = state.ui.diff_pane.opposite();
            // The caret is painted from the model, so moving it is a repaint, not
            // a re-read: nothing about the comparison changed.
            ui.ctx().request_repaint();
        }
        if diff_keys.2 {
            jump_to_source(state);
        }
    }

    // Space toggles the staging checkbox of the file being previewed — the
    // keyboard route to the same whole-file stage/un-stage a checkbox click
    // performs. Gated exactly like `/` above, because both are keys that type
    // into whatever text field holds the keyboard.
    let space = ui.input(|i| i.key_pressed(Key::Space) && !i.modifiers.any());
    if space
        && state.ui.tab == Tab::Commit
        && !state.show_welcome()
        && ui.ctx().memory(|m| m.focused().is_none())
    {
        super::commit_window::toggle_previewed_row_checkbox(state);
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

/// `F4` — Jump to Source, with the caret preserved.
///
/// The line is **derived** from the display model on the pane that holds the caret
/// (`preview_caret_line`), which is what makes the jump land where the
/// reader was looking instead of at the top of the file. Blame is this app's
/// source surface — one row per line of the file, with the commit that introduced
/// it — and it lives in the log workspace, so the tab moves with the jump and the
/// line travels in `blame_caret_line` for the surface to aim at.
///
/// Working-tree preview only, and so blame at `HEAD`: the pair being read here is
/// the working tree's, and the committed side is the only side with an author per
/// line. A pane with no numbered row at the caret — an insertion read as the
/// `Before` side — has no line to jump to, and the key does nothing.
fn jump_to_source(state: &mut AppState) {
    let Some((change, line)) = super::diff::preview_caret_line(state) else {
        return;
    };
    state.ui.blame = Some(turbogit_app::state::BlameTarget {
        root: change.root,
        path: change.path,
        rev: "HEAD".to_owned(),
    });
    state.ui.blame_caret_line = Some(line);
    switch_tab(state, Tab::Log);
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
    state.save_durable_ui();
}

// --- Shared painting helpers ---------------------------------------------------

/// Shell tabs in strip order (issue #03). The legacy History tab was
/// deleted in issue #19 (file history lives in Git Log's path-scoped
/// view), and Settings left the strip in issue #16: it is a gear-only
/// modal now (spec §9.1 correction). Branches is unimplemented in v1 and
/// renders a labeled placeholder pane (ADR-0008); Worktrees / Submodules
/// are real browsers since issue 14, with live count badges.
const SHELL_TAB_ITEMS: [widgets::TabItem<'static, Tab>; 5] = [
    widgets::TabItem {
        id: Tab::Commit,
        title: "Changes",
        icon: Some(Icon::GIT_COMMIT),
        end_icon: None,
        badge: None,
    },
    widgets::TabItem {
        id: Tab::Log,
        title: "Log",
        icon: Some(Icon::GIT_BRANCH),
        end_icon: None,
        badge: None,
    },
    widgets::TabItem {
        id: Tab::Branches,
        title: "Branches",
        icon: Some(Icon::GIT_BRANCH),
        end_icon: None,
        badge: None,
    },
    widgets::TabItem {
        id: Tab::Worktrees,
        title: "Worktrees",
        icon: Some(Icon::FOLDER),
        end_icon: None,
        badge: None,
    },
    widgets::TabItem {
        id: Tab::Submodules,
        title: "Submodules",
        icon: Some(Icon::FOLDER_GIT),
        end_icon: None,
        badge: None,
    },
];

/// The per-frame tab list: the const vocabulary with this frame's live badge
/// counts filled in (issue 14). `TabItem` is `Copy`, so this is a table copy
/// plus a badge fill, not a rebuild.
fn shell_tabs(state: &AppState) -> [widgets::TabItem<'static, Tab>; 5] {
    let mut tabs = SHELL_TAB_ITEMS;
    for item in tabs.iter_mut() {
        item.badge = tab_badge_count(state, item.id);
    }
    tabs
}

// --- Status bar -----------------------------------------------------------------------

/// Status bar (issue #03, screen 01): the version / git-status line, then
/// aggregated workspace state across every registered root — diverged,
/// conflicts, unpulled, archived, dirty totals on the left; total root
/// count on the right; busy spinner at the far right. The version line
/// came here with the topbar's deletion so it paints on Welcome and in
/// the shell alike.
///
/// `hint` is the one thing the pointer is blocked on right now, raised by a
/// disabled menu row on the previous frame (see
/// [`crate::ui::widgets::take_status_hint`]). It is the status text region the
/// reference documents as the home of "why can't I do this", and it paints as
/// its own line at the head of the left cluster, ahead of the version line and
/// the counters: a reason that arrives after three aggregate chips reads as
/// another chip, and the developer is looking for the sentence about the item
/// under the pointer.
fn render_status_bar(ui: &mut Ui, state: &mut AppState, hint: Option<&str>) {
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
                    if let Some(reason) = hint {
                        status_hint(ui, reason);
                    }
                    // The in-flight operation items, one per holding root. They
                    // are NOT this band's one-frame hint slot:
                    // the hint is a hover echo taken once per frame, and a
                    // persistent announcement routed through it would flicker and
                    // be dropped by the next hover. This channel has no slot and
                    // no producer — it reads git's own markers every frame — which
                    // is what keeps the hint's one-producer/one-consumer invariant
                    // provable.
                    super::status_inflight::render(ui, state);
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
            widgets::edge_rule(
                ui.painter(),
                ui.max_rect(),
                widgets::Edge::Top,
                Palette::LINE,
            );
        });
}

/// Paint the blocked-reason line: the sentence a disabled menu row states, in
/// the primary ink — the only text in this cluster that is about the thing under
/// the pointer rather than about the workspace — and followed by the same
/// hairline separator the aggregate chips are divided by, so it reads as the
/// line's head rather than as one more counter.
fn status_hint(ui: &mut Ui, reason: &str) {
    ui.label(
        RichText::new(reason)
            .font(crate::theme::chrome_font(crate::theme::TYPE_CONTROL))
            .color(Palette::INK),
    );
    ui.label(
        RichText::new("·")
            .font(crate::theme::chrome_font(crate::theme::TYPE_CONTROL))
            .color(STATUS_BAR_INK),
    );
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

/// Granular-staging status bar (issue 19, screen 06): how many repos granular
/// ops reach. Only over an open project — the welcome
/// page has nothing in scope. An empty multi-repo selection means the focused
/// single root.
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
    let scope = if state.ui.repo_selection.is_empty() {
        1
    } else {
        state.ui.repo_selection.len()
    };
    chip(
        ui,
        STATUS_BAR_INK,
        format!("{} in scope", components::counted(scope, "repo", "repos")),
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
                // A conflicted root counts as dirty as well as conflicted, so it
                // paints two chips where the owner's precedence ranks conflict
                // above dirty and could only ever produce one state. Preserved
                // on purpose (contradiction 2): dropping this increment is a
                // chip the user sees disappear, which is a human ruling, not a
                // refactor.
                agg.dirty += 1;
            }
            if crate::theme::RepoState::is_dirty(&root.status) {
                agg.dirty += 1;
            }
            // Archived roots: spec definition is "frozen / excluded from
            // cascades" — v1 has no archived concept yet, so the count
            // stays at zero (the chip appears once a real signal lands).
            if let Some((ahead, behind)) = ahead_behind(&root.id) {
                // The loose reading: any root whose branch has moved relative
                // to its upstream counts. It contradicts the repository dot one
                // screen away, which means *both* sides moved — and it is kept
                // that way deliberately, so the loose polarity is named
                // (`DivergenceMeaning::AnyDrift`) rather than an inline `||`
                // that reads as an accident.
                if crate::theme::DivergenceMeaning::AnyDrift.is_diverged(ahead, behind) {
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
        // A failed read is NOT re-asked every frame. The body asks while the
        // cache is cold, a failing read leaves it cold, and the cold cache is
        // then indistinguishable from a first paint — so without this gate the
        // log retries a command that just failed as fast as the pump can drain
        // it, and the state it paints alternates between the error and its own
        // loading. `Refresh` on the state the failure produced is the ask that
        // clears the gate, because that is when the developer says the reason
        // may have gone away.
        if state.caches.log(&id).is_none()
            && !state.caches.has_log_failure(&LogRead::Commits(id.clone()))
        {
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
