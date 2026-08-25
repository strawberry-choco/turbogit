//! IDE shell frame (issue #9, spec §6): topbar, toolbar, sidebar rail,
//! tab strip, status bar.
//!
//! The shell is the always-present frame of the main window (CONTEXT.md:
//! "Shell"); every page renders inside it. Region metrics come from spec
//! §4.2 and are exposed as constants so tests can assert them.
//!
//! Frozen keyboard shortcuts (ADR-0009) are dispatched here unchanged:
//! Ctrl+K commit · Ctrl+Shift+K push · Ctrl+T refresh · Ctrl+Shift+A find ·
//! Alt+` VCS operations.

use std::sync::Arc;

use egui::Galley;
use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Frame, Key, Layout, Margin, Panel, Pos2,
    Rect, RichText, Sense, Stroke, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType,
};

use super::icons::Icon;
use super::popups::{self, Action};
use super::widgets;
use crate::root_caches::Affected;
use crate::state::{AppState, Dialog, Tab};
use crate::theme::Palette;

// --- Spec metrics (§4.2 fixed heights) --------------------------------------

/// Top menubar height (`.tg-topbar`).
pub const TOPBAR_HEIGHT: f32 = 38.0;
/// Toolbar height (`.tg-toolbar`).
pub const TOOLBAR_HEIGHT: f32 = 34.0;
/// Sidebar rail width (`.tg-sidebar`).
pub const RAIL_WIDTH: f32 = 48.0;
/// Tab strip height (`.tg-tabs`).
pub const TAB_STRIP_HEIGHT: f32 = 32.0;
/// Single tab item height.
pub const TAB_ITEM_HEIGHT: f32 = 31.0;
/// Status bar height.
pub const STATUS_BAR_HEIGHT: f32 = 24.0;

const MENU_TEXT: f32 = 12.0; // .tg-menubar: 12px
const RAIL_BUTTON_SIZE: f32 = 36.0; // §4.2 sidebar buttons
const RAIL_ICON_SIZE: f32 = 18.0; // §5.3 rail icons
const TAB_ICON_SIZE: f32 = 14.0; // §6.2 tab icons
const TAB_TEXT: f32 = 12.0;

// --- Composition -------------------------------------------------------------

/// Compose the whole shell: frozen shortcuts, the five frame regions, then
/// the central body (Welcome placeholder or active tool window).
pub fn render(ui: &mut Ui, state: &mut AppState) {
    handle_shortcuts(ui, state);

    // Panel order fixes the geometry: top strips claim full width first, the
    // status bar claims the bottom before the rail narrows the remainder —
    // so the status bar spans edge to edge under the rail (spec §6.1).
    render_topbar(ui, state);
    if state.ui.show_toolbar {
        render_toolbar(ui, state);
    }
    if state.ui.show_status_bar {
        render_status_bar(ui, state);
    }
    render_rail(ui, state);

    egui::CentralPanel::default().show(ui, |ui| {
        render_tab_strip(ui, state);
        if state.show_welcome() {
            super::welcome::show(ui, state);
        } else {
            show_tool_window(ui, state);
        }
    });

    // While an async op is in flight, keep frames coming so its completion
    // (OpCompleted → refresh → preview reload) lands without waiting for
    // unrelated input — the headless harness relies on the same signal
    // `app.rs` gets from drain_events in production (spec R2 story 8).
    if state.ui.busy || state.ui.diff_loading {
        ui.ctx().request_repaint();
    }
}

/// The five frozen shortcuts (ADR-0009), dispatched exactly as before the
/// redesign — no rebinding, no new combos.
fn handle_shortcuts(ui: &mut Ui, state: &mut AppState) {
    // VCS Operations popup hotkey: Alt+` (Backquote).
    let open_popup = ui.input(|i| i.key_pressed(Key::Backtick) && i.modifiers.alt);
    if open_popup {
        state.ui.vcs_popup = true;
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
}

struct Shortcut {
    commit: bool,
    push: bool,
    refresh: bool,
    find: bool,
}

fn switch_tab(state: &mut AppState, tab: Tab) {
    state.ui.tab = tab;
    state.persist_ui();
}

// --- Shared painting helpers ---------------------------------------------------

enum Edge {
    Top,
    Bottom,
    Right,
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
        Edge::Right => painter.line_segment(
            [
                Pos2::new(rect.right() - 0.5, rect.top()),
                Pos2::new(rect.right() - 0.5, rect.bottom()),
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

const INERT_MENUS: [&str; 5] = ["Edit", "Navigate", "Code", "Window", "Help"];

fn menu_text(text: impl Into<String>) -> RichText {
    RichText::new(text.into())
        .font(FontId::new(MENU_TEXT, FontFamily::Proportional))
        .color(Palette::INK_2)
}

/// Top menubar (38px, SURFACE, bottom border LINE): eight IDE menu labels.
/// File / Git / View carry functional basics (§12.1); the other five are
/// inert chrome — visible, enabled-looking, deliberately without behavior.
fn render_topbar(ui: &mut Ui, state: &mut AppState) {
    Panel::top("topbar")
        .exact_size(TOPBAR_HEIGHT)
        .frame(
            Frame::new()
                .fill(Palette::SURFACE)
                .inner_margin(Margin::symmetric(8, 0)),
        )
        .show(ui, |ui| {
            // Flat at rest, SURFACE_2 on hover/press (mockup `.tg-topbar`).
            ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::TRANSPARENT;
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.menu_button(menu_text("File"), |ui| {
                    if ui.button("Open Project…").clicked() {
                        ui.close();
                        // Same end-to-end flow as the Welcome Open card
                        // (issue #10): pick a folder, open it as a project.
                        if let Some(dir) = super::welcome::pick_dir_public(state, "Open Project") {
                            state.open_project(&dir);
                        }
                    }
                    if ui.button("Clone…").clicked() {
                        ui.close();
                        popups::run_action(state, Action::Clone);
                    }
                    if ui.button("Init Repository…").clicked() {
                        ui.close();
                        state.init_repo();
                    }
                    ui.separator();
                    if ui.button("Welcome Screen").clicked() {
                        ui.close();
                        // Return to the Welcome screen, closing every open
                        // project (issue #10, ADR-0004).
                        state.close_all_projects();
                    }
                });
                for name in INERT_MENUS {
                    // Inert chrome (CONTEXT.md): rendered per the mockup,
                    // clicking is a no-op by design in v1.
                    let _ = ui.button(menu_text(name));
                }
                ui.menu_button(menu_text("Git"), |ui| {
                    for action in [
                        Action::Fetch,
                        Action::Pull,
                        Action::Push,
                        Action::Branches,
                        Action::NewBranch,
                        Action::Merge,
                        Action::Rebase,
                    ] {
                        if ui.button(action.label()).clicked() {
                            ui.close();
                            popups::run_action(state, action);
                        }
                    }
                });
                ui.menu_button(menu_text("View"), |ui| {
                    if ui.checkbox(&mut state.ui.show_toolbar, "Toolbar").clicked() {
                        ui.close();
                    }
                    if ui
                        .checkbox(&mut state.ui.show_status_bar, "Status Bar")
                        .clicked()
                    {
                        ui.close();
                    }
                });
            });
            paint_edge_line(ui, Edge::Bottom);
        });
}

// --- Toolbar -------------------------------------------------------------------

/// Toolbar (34px, BG, bottom border LINE): inert Run/Debug/Search chrome,
/// functional VCS actions, Commit as the single primary-styled button, and
/// a right-aligned settings gear (spec §6.2).
fn render_toolbar(ui: &mut Ui, state: &mut AppState) {
    Panel::top("toolbar")
        .exact_size(TOOLBAR_HEIGHT)
        .frame(
            Frame::new()
                .fill(Palette::BG)
                .inner_margin(Margin::symmetric(8, 0)),
        )
        .show(ui, |ui| {
            ui.style_mut().spacing.item_spacing.x = 4.0;
            // One 34px row: the settings gear stays PINNED to the right edge
            // (spec §6.2 — first child of the right-to-left layout) while the
            // action cluster scrolls horizontally on narrow windows, so
            // neither ever leaves the viewport irrecoverably (issue #23).
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::icon_button(ui, Icon::SETTINGS).clicked() {
                    state.ui.settings_open = true;
                }
                egui::ScrollArea::horizontal()
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            // Inert IDE chrome (v1): visible, no behavior.
                            widgets::toolbar_button(ui, Icon::PLAY, "Run", false);
                            widgets::toolbar_button(ui, Icon::BUG, "Debug", false);
                            widgets::toolbar_button(ui, Icon::SEARCH, "Search", false);
                            ui.add_space(6.0);
                            // The sole primary action of the toolbar.
                            if widgets::toolbar_button(ui, Icon::GIT_COMMIT, "Commit", true)
                                .clicked()
                            {
                                switch_tab(state, Tab::Commit);
                            }
                            if widgets::toolbar_button(
                                ui,
                                Icon::REFRESH_CW,
                                "Update Project",
                                false,
                            )
                            .clicked()
                            {
                                state.refresh(Affected::All);
                            }
                            if widgets::toolbar_button(ui, Icon::ARROW_DOWN, "Pull", false)
                                .clicked()
                            {
                                popups::run_action(state, Action::Pull);
                            }
                            if widgets::toolbar_button(ui, Icon::DOWNLOAD, "Fetch", false).clicked()
                            {
                                popups::run_action(state, Action::Fetch);
                            }
                            if widgets::toolbar_button(ui, Icon::UPLOAD, "Push", false).clicked() {
                                popups::run_action(state, Action::Push);
                            }
                            if widgets::toolbar_button(ui, Icon::GIT_BRANCH, "Branches", false)
                                .clicked()
                            {
                                popups::run_action(state, Action::Branches);
                            }
                            if widgets::toolbar_button(ui, Icon::TAG, "Tags", false).clicked() {
                                popups::run_action(state, Action::Tag);
                            }
                        });
                    });
            });
            paint_edge_line(ui, Edge::Bottom);
        });
}

// --- Sidebar rail ----------------------------------------------------------------

/// Rail entries: `(icon, label, tool window target)`. Entries without a
/// target are inert in v1 (spec §6.2/§12.4).
const RAIL_BUTTONS: [(Icon, &str, Option<Tab>); 4] = [
    (Icon::FOLDER, "Project", None),
    (Icon::GIT_COMMIT, "Commit", Some(Tab::Commit)),
    (Icon::GIT_BRANCH, "Git Log", Some(Tab::Log)),
    (Icon::SEARCH, "Search", None),
];

/// Sidebar rail (48px wide, SURFACE, right border LINE): 36×36 icon buttons
/// that switch the active tool window; the active entry shows a BRAND icon
/// on SURFACE_2.
fn render_rail(ui: &mut Ui, state: &mut AppState) {
    Panel::left("rail")
        .exact_size(RAIL_WIDTH)
        .frame(Frame::new().fill(Palette::SURFACE))
        .show(ui, |ui| {
            ui.add_space(4.0);
            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                for (icon, label, target) in RAIL_BUTTONS {
                    rail_button(ui, state, icon, label, target);
                }
            });
            paint_edge_line(ui, Edge::Right);
        });
}

fn rail_button(ui: &mut Ui, state: &mut AppState, icon: Icon, label: &str, target: Option<Tab>) {
    let active = target == Some(state.ui.tab);
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(RAIL_BUTTON_SIZE), Sense::click());
    let engaged = active || response.hovered();
    if engaged {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(6), // radius-md
            Palette::SURFACE_2,
        );
    }
    let ink = if active {
        Palette::BRAND
    } else if response.hovered() {
        Palette::INK
    } else {
        Palette::INK_3
    };
    paint_icon_centered(ui, icon, rect.center(), RAIL_ICON_SIZE, ink);
    widgets::focus_ring(ui, &response);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    if let Some(tab) = target
        && response.clicked()
    {
        switch_tab(state, tab);
    }
}

// --- Tab strip ---------------------------------------------------------------------

/// Shell tabs in strip order. The legacy History tab was deleted in
/// issue #19 (file history lives in Git Log's path-scoped view), and
/// Settings left the strip in issue #16: it is a gear-only modal now
/// (spec §9.1 correction).
const SHELL_TABS: [(Tab, Icon, &str); 2] = [
    (Tab::Commit, Icon::GIT_COMMIT, "Commit"),
    (Tab::Log, Icon::GIT_BRANCH, "Log"),
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
        let galley = ui
            .painter()
            .layout_no_wrap(label.to_owned(), font.clone(), Color32::WHITE);
        let content_w = TAB_ICON_SIZE + 6.0 + galley.size().x;
        let rect = Rect::from_min_size(
            Pos2::new(x, strip.top()),
            Vec2::new(24.0 + content_w, TAB_ITEM_HEIGHT),
        );
        tab_item(ui, state, rect, tab, icon, label, galley);
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

/// Status bar (~24px, SURFACE, top border LINE): branch indicator, change
/// counts, ahead/behind, busy spinner (spec §4.2/§6).
fn render_status_bar(ui: &mut Ui, state: &mut AppState) {
    Panel::bottom("status_bar")
        .exact_size(STATUS_BAR_HEIGHT)
        .frame(
            Frame::new()
                .fill(Palette::SURFACE)
                .inner_margin(Margin::symmetric(8, 0)),
        )
        .show(ui, |ui| {
            // Compact controls so everything fits the 24px band.
            ui.style_mut().spacing.button_padding = Vec2::new(6.0, 2.0);
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                super::branch_widget::widget(ui, state);
                ui.separator();
                if let Some(id) = &state.selected_root {
                    if let Some(root) = state.multi.by_id(id) {
                        ui.label(menu_text(format!(
                            "modified: {}   unversioned: {}   conflicts: {}",
                            root.status.modified(),
                            root.status.unversioned(),
                            root.status.conflicted.len(),
                        )));
                        if let Some((ahead, behind)) = state.caches.ahead_behind(&root.id) {
                            if ahead > 0 {
                                ui.colored_label(Palette::STATE_SUCCESS, format!("↑{ahead}"));
                            }
                            if behind > 0 {
                                ui.colored_label(Palette::STATE_WARNING, format!("↓{behind}"));
                            }
                        }
                    }
                } else {
                    ui.label(menu_text("No repository selected"));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if state.ui.busy {
                        ui.spinner();
                    }
                });
            });
            paint_edge_line(ui, Edge::Top);
        });
}

// --- Tool window body --------------------------------------------------------------------

/// Dispatch the active tool window inside the central panel. Log data is
/// fetched lazily exactly as the pre-shell layout did.
fn show_tool_window(ui: &mut Ui, state: &mut AppState) {
    if state.ui.tab == Tab::Log && state.selected_root.is_some() {
        let id = state.selected_root.clone().unwrap();
        if state.caches.log(&id).is_none() {
            state.fetch_log(id);
        }
    }
    match state.ui.tab {
        Tab::Commit => super::commit_window::show(ui, state),
        Tab::Log => super::log_window::show_log(ui, state),
    }
}
