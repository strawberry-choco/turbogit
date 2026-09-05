//! IDE shell frame (issue #9, spec §6): topbar, toolbar, sidebar rail,
//! tab strip, status bar.
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
use super::popups::{self, Action};
use super::widgets;
use crate::theme::Palette;
use turbogit_app::root_caches::Affected;
use turbogit_app::state::{AppState, Dialog, Granularity, Tab};
// --- Spec metrics (§4.2 fixed heights) --------------------------------------
/// Top menubar height (`.tg-topbar`).
pub const TOPBAR_HEIGHT: f32 = 38.0;
/// Repo header bar height (issue #03, screen 01).
pub const REPO_HEADER_HEIGHT: f32 = 48.0;
/// Tab strip height (`.tg-tabs`).
pub const TAB_STRIP_HEIGHT: f32 = 32.0;
/// Single tab item height.
pub const TAB_ITEM_HEIGHT: f32 = 31.0;
/// Status bar height.
pub const STATUS_BAR_HEIGHT: f32 = 24.0;
/// Metadata rail width (issue #03, screen 01).
pub const METADATA_RAIL_WIDTH: f32 = 260.0;
/// Minimum work-area width at which the workspace sidebar renders
/// (issue #05): below it the log's minimum pane sizes cannot hold.
pub const MIN_SIDEBAR_WINDOW_WIDTH: f32 = 1000.0;

const TAB_ICON_SIZE: f32 = 14.0; // §6.2 tab icons
const TAB_TEXT: f32 = 12.0;

/// Compose the whole shell: frozen shortcuts, the new shell frame
/// regions (topbar / repo header / center tabs / metadata rail / status
/// bar), then the central body (Welcome placeholder or active tool
/// window).
pub fn render(ui: &mut Ui, state: &mut AppState) {
    handle_shortcuts(ui, state);

    // Issue 14: the tab-strip badges and both tool tabs read the focused
    // root's worktree & submodule caches — keep them filled (fetch on
    // miss, i.e. first frame and after every refresh invalidation).
    if !state.show_welcome() {
        ensure_worktree_data(state);
    }

    // Panel order fixes the geometry: top strips claim full width first,
    // the status bar claims the bottom, and the central body takes
    // what remains — the metadata rail sits inside the central body
    // (issue #03, screen 01) alongside the active tool window.
    render_topbar(ui, state);
    if state.ui.show_status_bar {
        render_status_bar(ui, state);
    }

    egui::CentralPanel::default().show(ui, |ui| {
        if state.show_welcome() {
            render_tab_strip(ui, state);
            super::welcome::show(ui, state);
        } else {
            // The tool window, the metadata rail, the activity log panel
            // (issue #04), and the workspace sidebar (issue #05) share the
            // central body with the repo header and tab strip. Reserve
            // explicit rects: a `ui.horizontal` would size its children to
            // one interact row, collapsing every ScrollArea inside the tool
            // window, and an unsized tool window would consume the rail's
            // width. The sidebar claims the left edge of the work area; the
            // repo header and tab strip start to its right; the activity
            // log keeps its full-width strip at the bottom (screen 01).
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
            let header_rect = Rect::from_min_max(
                right_rect.min,
                Pos2::new(right_rect.max.x, right_rect.min.y + REPO_HEADER_HEIGHT),
            );
            let tabs_rect = Rect::from_min_max(
                Pos2::new(right_rect.min.x, header_rect.max.y),
                Pos2::new(right_rect.max.x, header_rect.max.y + TAB_STRIP_HEIGHT),
            );
            let content_rect =
                Rect::from_min_max(Pos2::new(right_rect.min.x, tabs_rect.max.y), right_rect.max);
            let tool_rect = Rect::from_min_max(
                content_rect.min,
                Pos2::new(content_rect.max.x - METADATA_RAIL_WIDTH, content_rect.max.y),
            );
            let rail_rect = Rect::from_min_max(
                Pos2::new(tool_rect.max.x, content_rect.min.y),
                content_rect.max,
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
            let mut header_ui = ui.new_child(
                UiBuilder::new()
                    .max_rect(header_rect)
                    .layout(Layout::top_down(Align::Min)),
            );
            render_repo_header(&mut header_ui, header_rect, state);
            ui.advance_cursor_after_rect(header_rect);
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
            let mut rail_ui = ui.new_child(
                UiBuilder::new()
                    .max_rect(rail_rect)
                    .layout(Layout::top_down(Align::Min)),
            );
            render_metadata_rail(&mut rail_ui, state);
            ui.advance_cursor_after_rect(rail_rect);
            let mut activity_ui = ui.new_child(
                UiBuilder::new()
                    .max_rect(activity_rect)
                    .layout(Layout::top_down(Align::Min)),
            );
            super::activity_panel::show(&mut activity_ui, state);
            ui.advance_cursor_after_rect(activity_rect);
        }
    });

    // While an async op is in flight, keep frames coming so its completion
    // (OpCompleted → refresh → preview reload) lands without waiting for
    // unrelated input — the headless harness relies on the same signal
    // `app.rs` gets from drain_events in production (spec R2 story 8).
    if state.ui.busy || state.ui.diff_loading || state.ui.blame_loading {
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

    let popup_open = state.ui.vcs_popup || state.ui.command_palette || state.ui.branches_popup;
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
/// so `ensure_diff` loads the new diff and lands on its first hunk.
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
                // Landing hunk: ensure_diff resets to the first hunk on the
                // fresh load.
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
    state.persist_ui();
}

// --- Shared painting helpers ---------------------------------------------------

enum Edge {
    Top,
    Bottom,
}

/// 1px LINE border along one edge of a rect, without affecting layout
/// (spec §6.2: bottom strokes on topbar/toolbar/tab strip, top stroke on
/// the status bar, right stroke on the rail).
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

/// Paint one icon primitive centered at `origin` without disturbing layout.
fn paint_icon_centered(ui: &mut Ui, icon: Icon, center: Pos2, size: f32, color: Color32) {
    let mut child =
        ui.new_child(UiBuilder::new().max_rect(Rect::from_center_size(center, Vec2::splat(size))));
    super::icons::icon(&mut child, icon, size, color);
}

// --- Topbar ------------------------------------------------------------------

const TOPBAR_BRANDSIZE: f32 = 16.0; // brand-icon size (spec §4.2)
const TOPBAR_ICON_SIZE: f32 = 14.0; // topbar action icons (spec §4.2)
const TOPBAR_TEXT: f32 = 13.0; // topbar text scale (spec §4.2)
const TOPBAR_ACTIONS_TEXT: f32 = 12.0; // right-cluster button text

/// Topbar (issue #03, screen 01): TurboGit brand on the left, then a
/// workspace selector and a breadcrumb (project / focused repo); the
/// right-aligned action cluster ([`render_topbar_actions`]) is rendered
/// as a sibling top panel so it shares the same horizontal row.
///
/// Replaces the IDE menubar from the previous design — every shortcut
/// previously reachable through the File / Git / View menus is now
/// reachable through this topbar or the palette (Ctrl+Shift+A).
fn render_topbar(ui: &mut Ui, state: &mut AppState) {
    Panel::top("topbar")
        .exact_size(TOPBAR_HEIGHT)
        .frame(
            Frame::new()
                .fill(Palette::SURFACE)
                .inner_margin(Margin::symmetric(12, 0)),
        )
        .show(ui, |ui| {
            ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::TRANSPARENT;
            ui.style_mut().spacing.button_padding = Vec2::new(8.0, 4.0);
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                // Brand: icon + wordmark.
                icons::icon(ui, Icon::FOLDER_GIT, TOPBAR_BRANDSIZE, Palette::BRAND);
                ui.add_space(8.0);
                ui.label(
                    RichText::new("TurboGit")
                        .strong()
                        .font(FontId::new(TOPBAR_TEXT + 1.0, FontFamily::Proportional))
                        .color(Palette::INK),
                );
                ui.add_space(16.0);

                // Workspace selector: project_dir basename + chevron.
                if let Some(workspace) = workspace_label(state) {
                    let selector = ui
                        .button(
                            RichText::new(workspace)
                                .font(FontId::new(TOPBAR_TEXT, FontFamily::Proportional))
                                .color(Palette::INK_2),
                        )
                        .on_hover_text("Switch workspace");
                    widgets::focus_ring(ui, &selector);
                    // v1 placeholder — issue #34 owns the picker.
                    let _ = selector.clicked();
                    icons::icon(ui, Icon::CHEVRON_DOWN, TOPBAR_ICON_SIZE, Palette::INK_3);
                    ui.add_space(16.0);
                }

                // Breadcrumb: project / focused repo.
                if let Some(crumbs) = breadcrumb_labels(state) {
                    for (i, crumb) in crumbs.iter().enumerate() {
                        if i > 0 {
                            ui.label(
                                RichText::new("/")
                                    .font(FontId::new(TOPBAR_TEXT, FontFamily::Proportional))
                                    .color(Palette::INK_3),
                            );
                        }
                        ui.label(
                            RichText::new(crumb.as_str())
                                .font(FontId::new(TOPBAR_TEXT, FontFamily::Proportional))
                                .color(Palette::INK_2),
                        );
                    }
                }

                // Version/git-status line (issue #34): app version, resolved
                // git version, and the total indexed repo count. Appears in
                // the header on the Welcome screen and the shell alike.
                let version_line = format!(
                    "v{} · git {} · {} repos indexed",
                    env!("CARGO_PKG_VERSION"),
                    state.git_version,
                    state.multi.roots.len(),
                );
                ui.label(
                    RichText::new(version_line)
                        .font(FontId::new(TOPBAR_ACTIONS_TEXT, FontFamily::Proportional))
                        .color(Palette::INK_3),
                );

                // Right-aligned action cluster: Fetch / Pull / Push /
                // Branch / More (screen 01). Sharing the topbar row keeps
                // the chrome to its spec height — a sibling top panel
                // would stack a second full-height row instead.
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let more = ui.button(topbar_action_text("More"));
                    widgets::focus_ring(ui, &more);
                    if more.clicked() {
                        state.ui.command_palette = true;
                        state.ui.command_query.clear();
                    }
                    let branch = ui.button(topbar_action_text("Branch"));
                    widgets::focus_ring(ui, &branch);
                    if branch.clicked() {
                        state.ui.branches_popup = true;
                        state.ui.branch_filter.clear();
                    }
                    let push = ui.button(topbar_action_text("Push"));
                    widgets::focus_ring(ui, &push);
                    if push.clicked() {
                        state.ui.dialog = Some(Dialog::Push);
                    }
                    let pull = ui.button(topbar_action_text("Pull"));
                    widgets::focus_ring(ui, &pull);
                    if pull.clicked() {
                        popups::run_action(state, Action::Pull);
                    }
                    let fetch = ui.button(topbar_action_text("Fetch"));
                    widgets::focus_ring(ui, &fetch);
                    if fetch.clicked() {
                        popups::run_action(state, Action::Fetch);
                    }
                });
            });
            paint_edge_line(ui, Edge::Bottom);
        });
}

/// Topbar action button text (right cluster).
fn topbar_action_text(label: &str) -> RichText {
    RichText::new(label)
        .font(FontId::new(TOPBAR_ACTIONS_TEXT, FontFamily::Proportional))
        .color(Palette::INK_2)
}

/// Workspace selector label: project_dir basename, `None` on the
/// Welcome page.
fn workspace_label(state: &AppState) -> Option<String> {
    if state.show_welcome() {
        None
    } else {
        Some(
            state
                .project_dir
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("<workspace>")
                .to_string(),
        )
    }
}

/// Breadcrumb crumbs: project_dir basename, then the focused root's
/// path relative to project_dir (joined with `/`). When the focused
/// root equals project_dir, the crumbs collapse to a single entry.
fn breadcrumb_labels(state: &AppState) -> Option<Vec<String>> {
    if state.show_welcome() {
        return None;
    }
    let project = state
        .project_dir
        .file_name()
        .and_then(|s| s.to_str())
        .map(str::to_string);
    let focused = state
        .selected_root
        .as_ref()
        .and_then(|id| state.multi.by_id(id))
        .map(|r| breadcrumb_root_name(state, r));
    let mut crumbs = Vec::new();
    if let Some(p) = project {
        crumbs.push(p);
    }
    if let Some(r) = focused {
        let same = crumbs.last() == Some(&r);
        if !same {
            crumbs.push(r);
        }
    }
    if crumbs.is_empty() {
        None
    } else {
        Some(crumbs)
    }
}

/// Breadcrumb leaf: focused root's path components relative to
/// project_dir joined by `/`, or the root's basename when the root lives
/// outside the project tree.
fn breadcrumb_root_name(state: &AppState, root: &turbogit_domain::model::Root) -> String {
    match root.path.strip_prefix(&state.project_dir) {
        Ok(r) if !r.as_os_str().is_empty() => r
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"),
        _ => root
            .path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("<repo>")
            .to_string(),
    }
}
// --- Repo header -------------------------------------------------------------

/// Repo header bar (issue #03, screen 01): focused root folder + name +
/// chevron, branch pill, and the combined ahead/behind/conflict badge,
/// plus a Refresh button. Sits on BG with a LINE bottom stroke; renders
/// into the shell's reserved header strip to the right of the workspace
/// sidebar (issue #05).
fn render_repo_header(ui: &mut Ui, rect: Rect, state: &mut AppState) {
    let Some(root) = state
        .selected_root
        .as_ref()
        .and_then(|id| state.multi.by_id(id))
    else {
        return;
    };
    // Snapshot the data the header paints so the closure borrows `state`
    // mutably only via `state.refresh` (Refresh button click).
    let root_name = root
        .path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("<repo>")
        .to_owned();
    let branch = root
        .current_branch
        .clone()
        .unwrap_or_else(|| "<detached>".to_owned());
    let ahead_behind = state
        .selected_root
        .as_ref()
        .and_then(|id| state.caches.ahead_behind(id))
        .unwrap_or((0, 0));
    let conflicts = root.status.conflicted.len();

    ui.painter()
        .rect_filled(rect, CornerRadius::same(0), Palette::BG);
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::top_down(Align::Min)),
    );
    Frame::new()
        .inner_margin(Margin::symmetric(12, 0))
        .show(&mut child, |ui| {
            ui.style_mut().spacing.button_padding = Vec2::new(8.0, 4.0);
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                // Focused root folder icon + name + chevron.
                icons::icon(ui, Icon::FOLDER_GIT, 16.0, Palette::INK_2);
                ui.add_space(8.0);
                ui.label(
                    RichText::new(&root_name)
                        .strong()
                        .font(FontId::new(14.0, FontFamily::Proportional))
                        .color(Palette::INK),
                );
                icons::icon(ui, Icon::CHEVRON_DOWN, 12.0, Palette::INK_3);

                ui.add_space(16.0);

                // Branch pill (issue #03).
                branch_pill(ui, &branch);

                ui.add_space(12.0);

                // Combined ahead/behind/conflict badge.
                combined_branch_badge(ui, ahead_behind, conflicts);

                // Right-aligned Refresh (screen 01). Sharing the header
                // row keeps the chrome to its spec height — a sibling top
                // panel would stack a second full-height row instead.
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let refresh = ui.button(
                        RichText::new("Refresh")
                            .font(FontId::new(12.0, FontFamily::Proportional))
                            .color(Palette::INK_2),
                    );
                    widgets::focus_ring(ui, &refresh);
                    if refresh.clicked() {
                        state.refresh(Affected::All);
                    }
                });
            });
        });
    paint_edge_line_at(ui, rect, Edge::Bottom);
}

/// Branch pill: 22px-tall SURFACE_2-rounded chip carrying the branch name
/// in BRAND ink.
fn branch_pill(ui: &mut Ui, branch: &str) {
    let galley = ui.painter().layout_no_wrap(
        branch.to_owned(),
        FontId::new(12.0, FontFamily::Proportional),
        Palette::BRAND,
    );
    let pad = 6.0;
    let h = 22.0;
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(galley.size().x + pad * 2.0, h), Sense::hover());
    let radius = CornerRadius {
        nw: 4,
        ne: 4,
        sw: 4,
        se: 4,
    };
    ui.painter().rect_filled(rect, radius, Palette::SURFACE_2);
    ui.painter().rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, Palette::LINE_SUBTLE),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley_with_override_text_color(
        Pos2::new(rect.left() + pad, rect.center().y - galley.size().y / 2.0),
        galley,
        Palette::BRAND,
    );
}

/// Combined ahead/behind/conflict badge (issue #03): a single chip
/// carrying the union of the focused root's outgoing count, incoming
/// count, and merge-conflict count. Hidden when every count is zero.
fn combined_branch_badge(ui: &mut Ui, ahead_behind: (usize, usize), conflicts: usize) {
    let ahead = ahead_behind.0;
    let behind = ahead_behind.1;
    if ahead == 0 && behind == 0 && conflicts == 0 {
        return;
    }
    let mut pieces = Vec::new();
    if ahead > 0 {
        pieces.push(format!("↑{ahead}"));
    }
    if behind > 0 {
        pieces.push(format!("↓{behind}"));
    }
    if conflicts > 0 {
        pieces.push(format!("⊗{conflicts}"));
    }
    let text = pieces.join(" ");
    let fg = if conflicts > 0 {
        Palette::STATE_ERROR
    } else if behind > 0 {
        Palette::STATE_WARNING
    } else {
        Palette::STATE_SUCCESS
    };
    let bg = widgets::tint_over_bg(fg, 0.18);
    let galley = ui.painter().layout_no_wrap(
        text.clone(),
        FontId::new(11.0, FontFamily::Proportional),
        fg,
    );
    let pad = 6.0;
    let h = 18.0;
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(galley.size().x + pad * 2.0, h), Sense::hover());
    let radius = CornerRadius {
        nw: 4,
        ne: 4,
        sw: 4,
        se: 4,
    };
    ui.painter().rect_filled(rect, radius, bg);
    ui.painter().galley_with_override_text_color(
        Pos2::new(rect.left() + pad, rect.center().y - galley.size().y / 2.0),
        galley,
        fg,
    );
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

/// Tab strip (32px, BG, bottom border LINE): icon + label entries; the
/// active tab renders INK-on-SURFACE with a LINE border on its top/left/right
/// edges only, top-rounded (spec §6.2).
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
        let rect = Rect::from_min_size(
            Pos2::new(x, strip.top()),
            Vec2::new(24.0 + content_w, TAB_ITEM_HEIGHT),
        );
        tab_item(ui, state, rect, tab, icon, &label, galley);
        x += rect.width() + 2.0;
    }
}

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
    let radius = CornerRadius {
        nw: 4,
        ne: 4,
        sw: 0,
        se: 0,
    };
    if active {
        painter.rect_filled(rect, radius, Palette::SURFACE);
        let s = Stroke::new(1.0, Palette::LINE);
        // Border on top/left/right only (bottom edge stays open into body).
        painter.line_segment(
            [
                Pos2::new(rect.left() + 3.0, rect.top() + 0.5),
                Pos2::new(rect.right() - 3.0, rect.top() + 0.5),
            ],
            s,
        );
        painter.line_segment(
            [
                Pos2::new(rect.left() + 0.5, rect.top() + 3.0),
                Pos2::new(rect.left() + 0.5, rect.bottom()),
            ],
            s,
        );
        painter.line_segment(
            [
                Pos2::new(rect.right() - 0.5, rect.top() + 3.0),
                Pos2::new(rect.right() - 0.5, rect.bottom()),
            ],
            s,
        );
    } else if response.hovered() {
        painter.rect_filled(rect, radius, Palette::SURFACE_2);
    }

    let ink = if active { Palette::INK } else { Palette::INK_3 };
    let cy = rect.center().y;
    let mut cx = rect.left() + 12.0;
    paint_icon_centered(
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

/// Status bar (issue #03, screen 01): aggregated workspace state across
/// every registered root — diverged, conflicts, unpulled, archived,
/// dirty totals on the left; total root count on the right; busy
/// spinner at the far right.
fn render_status_bar(ui: &mut Ui, state: &mut AppState) {
    let agg = AggregatedStatus::compute(state);

    Panel::bottom("status_bar")
        .exact_size(STATUS_BAR_HEIGHT)
        .frame(
            Frame::new()
                .fill(Palette::SURFACE)
                .inner_margin(Margin::symmetric(8, 0)),
        )
        .show(ui, |ui| {
            ui.style_mut().spacing.button_padding = Vec2::new(6.0, 2.0);
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                aggregated_status_chips(ui, &agg);
                granular_status_chips(ui, state);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    status_total(&agg, ui);
                    if state.ui.busy {
                        ui.spinner();
                    }
                });
            });
            paint_edge_line(ui, Edge::Top);
        });
}

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
                .font(FontId::new(11.0, FontFamily::Proportional))
                .color(Palette::INK_3),
        );
        ui.colored_label(color, text);
    };
    if state.ui.char_selection.is_some() {
        chip(ui, Palette::BRAND, "1 line-selection active".to_owned());
    }
    let granularity = match state.ui.diff_granularity {
        Granularity::File => "file",
        Granularity::Hunk => "hunk",
        Granularity::Line => "line",
    };
    chip(ui, Palette::INK_2, format!("granularity: {granularity}"));
    let scope = if state.ui.repo_selection.is_empty() {
        1
    } else {
        state.ui.repo_selection.len()
    };
    chip(
        ui,
        Palette::INK_2,
        format!("{scope} repo{} in scope", if scope == 1 { "" } else { "s" }),
    );
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
    fn compute(state: &AppState) -> Self {
        let mut agg = AggregatedStatus {
            total: state.multi.roots.len(),
            ..Default::default()
        };
        for root in &state.multi.roots {
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
            if let Some((ahead, behind)) = state.caches.ahead_behind(&root.id) {
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
                    .font(FontId::new(11.0, FontFamily::Proportional))
                    .color(Palette::INK_3),
            );
        }
        first = false;
        ui.colored_label(color, text);
    };
    if agg.diverged > 0 {
        chip(
            ui,
            Palette::STATE_ERROR,
            format!("{} diverged", agg.diverged),
        );
    }
    if agg.conflicts > 0 {
        chip(
            ui,
            Palette::STATE_ERROR,
            format!("{} conflict", agg.conflicts),
        );
    }
    if agg.unpulled > 0 {
        chip(
            ui,
            Palette::STATE_WARNING,
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
        chip(ui, Palette::STATE_WARNING, format!("{} dirty", agg.dirty));
    }
}

/// Right-cluster total count: "<N> total".
fn status_total(agg: &AggregatedStatus, ui: &mut Ui) {
    ui.label(
        RichText::new(format!("{} total", agg.total))
            .font(FontId::new(11.0, FontFamily::Proportional))
            .color(Palette::INK_2),
    );
}

// --- Metadata rail ---------------------------------------------------------------

/// Metadata rail (issue #03, screen 01): a right-side vertical pane
/// showing the focused root's Path, Branch, and Upstream. Sits inside
/// the central body alongside the active tool window.
fn render_metadata_rail(ui: &mut Ui, state: &mut AppState) {
    let Some(root) = state
        .selected_root
        .as_ref()
        .and_then(|id| state.multi.by_id(id))
    else {
        return;
    };
    let width = METADATA_RAIL_WIDTH;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, ui.available_height()), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(0), Palette::SURFACE);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(0),
        Stroke::new(1.0, Palette::LINE),
        egui::StrokeKind::Inside,
    );

    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::top_down(Align::Min)),
    );
    child.add_space(12.0);

    // Header.
    child.label(
        RichText::new("METADATA")
            .strong()
            .font(FontId::new(11.0, FontFamily::Proportional))
            .color(Palette::INK_3),
    );
    child.add_space(12.0);

    // Path row.
    metadata_row(&mut child, "Path", &root.path.display().to_string());
    child.add_space(10.0);

    // Branch row.
    let branch = root
        .current_branch
        .clone()
        .unwrap_or_else(|| "<detached>".to_owned());
    metadata_row(&mut child, "Branch", &branch);
    child.add_space(10.0);

    // Upstream row: look up the focused branch's tracking field.
    let upstream = root
        .branches
        .iter()
        .find(|b| Some(&b.name) == root.current_branch.as_ref())
        .and_then(|b| b.tracking.clone())
        .unwrap_or_else(|| "—".to_owned());
    metadata_row(&mut child, "Upstream", &upstream);
}

/// One metadata rail row: small INK_3 label on the left, the value on
/// the right (top-down layout with absolute `x` offset for the value).
fn metadata_row(ui: &mut Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        let label_galley = ui.painter().layout_no_wrap(
            label.to_owned(),
            FontId::new(12.0, FontFamily::Proportional),
            Palette::INK_3,
        );
        let label_w = label_galley.size().x + 8.0;
        ui.allocate_exact_size(Vec2::new(label_w, label_galley.size().y), Sense::hover());
        ui.painter().galley_with_override_text_color(
            ui.cursor().left_top(),
            label_galley,
            Palette::INK_3,
        );
        ui.add_space(label_w + 4.0);
        let value_galley = ui.painter().layout_no_wrap(
            value.to_owned(),
            FontId::new(12.0, FontFamily::Proportional),
            Palette::INK,
        );
        ui.painter().galley_with_override_text_color(
            ui.cursor().left_top(),
            value_galley.clone(),
            Palette::INK,
        );
        ui.advance_cursor_after_rect(Rect::from_min_size(
            ui.cursor().min,
            Vec2::new(value_galley.size().x, value_galley.size().y),
        ));
    });
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
        // Phase-J placeholder (ADR-0008): a not-yet-implemented center tab
        // (Branches) renders a labeled placeholder pane rather than a
        // hidden or disabled tab. Worktrees / Submodules are real browsers
        // since issue 14.
        Tab::Branches => placeholder_tab_body(ui, state),
        Tab::Worktrees => super::worktrees::show(ui, state),
        Tab::Submodules => super::submodules::show(ui, state),
    }
}

/// Labeled placeholder for an unimplemented center tab (ADR-0008):
/// a centered explanatory pane rather than a hidden or disabled tab.
fn placeholder_tab_body(ui: &mut Ui, state: &AppState) {
    let label = match state.ui.tab {
        Tab::Branches => "Branches",
        Tab::Worktrees => "Worktrees",
        Tab::Submodules => "Submodules",
        _ => "Coming later",
    };
    ui.vertical_centered(|ui| {
        ui.add_space(120.0);
        ui.label(
            RichText::new(label)
                .strong()
                .font(FontId::new(18.0, FontFamily::Proportional))
                .color(Palette::INK),
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new("Arrives in a later release.")
                .font(FontId::new(13.0, FontFamily::Proportional))
                .color(Palette::INK_3),
        );
    });
}
