//! Welcome page (issue #10, spec §8.1).
//!
//! Shown instead of the active tool window whenever no project is open or the
//! user returned to it via File → Welcome (`AppState::show_welcome`,
//! ADR-0004). Full-window, scrollable, centered content (max-width 980px):
//!
//! 1. Brand header: logo icon + "TurboGit" + tagline.
//! 2. Two-column grid: three action cards (Clone / Open / Initialize) and the
//!    inline clone form on the left; recent projects on the right (ADR-0005).
//! 3. Getting-started hints.
//!
//! Branch indicators on recent rows are computed live at render time through
//! the engine seam and cached in memory only (never persisted).

use crate::theme::Palette;
use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Layout, Pos2, Rect, RichText, Sense, Stroke,
    StrokeKind, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType,
};
use std::time::{Duration, Instant};
use turbogit_app::state::{AppState, Toast};

use super::icons::{self, Icon};
use super::widgets;

/// Content column width (spec §8.1: max-width 980px).
const CONTENT_WIDTH: f32 = 980.0;
/// Right column (recent projects) fixed width (spec §8.1).
const RECENTS_WIDTH: f32 = 260.0;
const COLUMN_GAP: f32 = 16.0;
const CARD_GAP: f32 = 12.0;
const CARD_HEIGHT: f32 = 120.0;
const CARD_PADDING: f32 = 18.0;
const RECENT_ROW_HEIGHT: f32 = 64.0;
const RADIUS_MD: u8 = 6;

/// Branch indicators recompute at most this often (ADR-0005: computed live
/// at render with in-memory caching — never stored).
const BRANCH_TTL: Duration = Duration::from_secs(5);

/// Render the Welcome page inside the shell's central panel.
pub fn show(ui: &mut Ui, state: &mut AppState) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        let avail = ui.available_width();
        let margin = ((avail - CONTENT_WIDTH) / 2.0).max(0.0);
        ui.add_space(28.0);
        ui.horizontal(|ui| {
            ui.add_space(margin);
            ui.vertical(|ui| {
                ui.set_max_width(CONTENT_WIDTH.min(avail));
                brand_header(ui);
                ui.add_space(28.0);
                columns(ui, state);
                ui.add_space(28.0);
                getting_started(ui);
                ui.add_space(24.0);
            });
        });
    });
}

// --- Brand header ------------------------------------------------------------

fn brand_header(ui: &mut Ui) {
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.horizontal(|ui| {
            icons::icon(ui, Icon::FOLDER_GIT, 38.0, Palette::BRAND);
            ui.add_space(10.0);
            ui.label(
                RichText::new("TurboGit")
                    .strong()
                    .font(FontId::new(42.0, FontFamily::Proportional))
                    .color(Palette::INK),
            );
        });
        ui.add_space(6.0);
        ui.label(
            RichText::new("A fast, keyboard-friendly Git client for your desktop.")
                .size(14.0)
                .color(Palette::INK_3),
        );
    });
}

// --- Two-column grid -----------------------------------------------------------

fn columns(ui: &mut Ui, state: &mut AppState) {
    // Spacing-aware split guaranteed to fit: auto item spacing around the
    // explicit gap plus both columns never exceeds the viewport, so neither
    // column is ever clipped out of reach (issue #23).
    let avail = ui.available_width();
    let gaps = COLUMN_GAP + 2.0 * ui.style().spacing.item_spacing.x;
    let usable = (avail - gaps).max(160.0);
    let recents_w = RECENTS_WIDTH.min(usable * 0.4);
    let left_w = (usable - recents_w).max(usable * 0.5);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            // Pin exactly: inputs size themselves from available width, so
            // an uncapped column would grow frame over frame (issue #23).
            ui.set_min_width(left_w);
            ui.set_max_width(left_w);
            action_cards(ui, state, left_w);
            ui.add_space(20.0);
            clone_box(ui, state);
        });
        ui.add_space(COLUMN_GAP);
        ui.vertical(|ui| {
            ui.set_min_width(recents_w);
            ui.set_max_width(recents_w);
            recents_column(ui, state);
        });
    });
}

// --- Action cards -----------------------------------------------------------------

#[derive(Clone, Copy)]
enum CardAction {
    /// Focus the inline clone URL input.
    FocusClone,
    /// Pick a folder and open it as a project (end-to-end).
    OpenProject,
    /// Pick a folder, `git init` it, and enter it (end-to-end).
    InitRepo,
}

fn action_cards(ui: &mut Ui, state: &mut AppState, left_w: f32) {
    // No hard floor: cards shrink with their column so three always fit
    // (issue #23). At spec widths this equals the mockup's ~226px card.
    let card_w = (left_w - 2.0 * CARD_GAP) / 3.0;
    ui.horizontal(|ui| {
        action_card(
            ui,
            state,
            Icon::BOOK_OPEN,
            "Clone from URL",
            "Fetch an existing repository from a remote provider.",
            card_w,
            CardAction::FocusClone,
        );
        ui.add_space(CARD_GAP);
        action_card(
            ui,
            state,
            Icon::FOLDER_OPEN,
            "Open Project",
            "Browse for a folder and open it as a TurboGit project.",
            card_w,
            CardAction::OpenProject,
        );
        ui.add_space(CARD_GAP);
        action_card(
            ui,
            state,
            Icon::FOLDER_GIT,
            "Initialize Repository",
            "Create a fresh Git repository in a chosen folder.",
            card_w,
            CardAction::InitRepo,
        );
    });
}

/// One action card (spec §8.1): SURFACE bg, LINE border, radius-md, 18px
/// padding, icon 22px BRAND, title 13px, body 12px INK_3. Hover: SURFACE_2
/// bg + BRAND border.
fn action_card(
    ui: &mut Ui,
    state: &mut AppState,
    icon: Icon,
    title: &str,
    body: &str,
    width: f32,
    action: CardAction,
) {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, CARD_HEIGHT), Sense::click());
    let hovered = response.hovered();
    let painter = ui.painter().clone();
    let radius = CornerRadius::same(RADIUS_MD);
    painter.rect_filled(
        rect,
        radius,
        if hovered {
            Palette::SURFACE_2
        } else {
            Palette::SURFACE
        },
    );
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(
            1.0,
            if hovered {
                Palette::BRAND
            } else {
                Palette::LINE
            },
        ),
        StrokeKind::Outside,
    );

    let pad = CARD_PADDING;
    paint_icon_at(
        ui,
        icon,
        Pos2::new(rect.left() + pad, rect.top() + pad),
        22.0,
        Palette::BRAND,
    );

    let title_galley = painter.layout_no_wrap(
        title.to_owned(),
        FontId::new(13.0, FontFamily::Proportional),
        Palette::INK,
    );
    let body_galley = painter.layout(
        body.to_owned(),
        FontId::new(12.0, FontFamily::Proportional),
        Palette::INK_3,
        width - 2.0 * pad,
    );
    let x = rect.left() + pad;
    let title_y = rect.top() + pad + 22.0 + 10.0;
    painter.galley(Pos2::new(x, title_y), title_galley, Palette::INK);
    painter.galley(
        Pos2::new(x, title_y + 19.0 + 6.0),
        body_galley,
        Palette::INK_3,
    );

    // Accessibility / headless-test queryability.
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, title));
    widgets::focus_ring(ui, &response);
    if !response.clicked() {
        return;
    }
    match action {
        CardAction::FocusClone => state.ui.welcome_focus_clone = true,
        CardAction::OpenProject => {
            if let Some(dir) = pick_dir(state, "Open Project") {
                state.open_project(&dir);
            }
        }
        CardAction::InitRepo => {
            if let Some(dir) = pick_dir(state, "Initialize Repository") {
                state.initialize_and_enter(&dir);
            }
        }
    }
}

/// Ask the injected/native folder picker for a directory. Missing or
/// cancelled picks surface a toast instead of failing silently.
fn pick_dir(state: &mut AppState, purpose: &str) -> Option<std::path::PathBuf> {
    let Some(pick) = state.dir_picker.as_ref() else {
        state.ui.toast = Some(Toast::error(format!(
            "{purpose}: no folder picker available"
        )));
        return None;
    };
    let picked = pick();
    if picked.is_none() {
        state.ui.toast = Some(Toast::error(format!("{purpose}: no folder selected")));
    }
    picked
}

/// Folder-picker entry for the shell's File menu (same seam as the Welcome
/// Open card).
pub fn pick_dir_public(state: &mut AppState, purpose: &str) -> Option<std::path::PathBuf> {
    pick_dir(state, purpose)
}

// --- Clone box ----------------------------------------------------------------------

/// Inline clone form below the cards (spec §8.1): URL input + Clone primary
/// button + shallow checkbox row.
fn clone_box(ui: &mut Ui, state: &mut AppState) {
    widgets::group_title(ui, "Clone Repository");
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let response = widgets::text_input(ui, "Repository URL", &mut state.ui.welcome_clone_url);
        if std::mem::take(&mut state.ui.welcome_focus_clone) {
            response.request_focus();
        }
        if widgets::primary_button(ui, Some(Icon::DOWNLOAD), "Clone").clicked() {
            clone_from_url(state);
        }
    });
    ui.checkbox(&mut state.ui.welcome_shallow, "Shallow clone (--depth 1)");
}

/// Clone the entered URL into a picked parent folder and enter the result.
/// Synchronous like `init_repo` (v1 simplicity); all mutation crosses the
/// engine seam.
fn clone_from_url(state: &mut AppState) {
    let url = state
        .ui
        .welcome_clone_url
        .trim()
        .trim_end_matches('/')
        .to_string();
    if url.is_empty() {
        state.ui.toast = Some(Toast::error("Clone: enter a repository URL first"));
        return;
    }
    let Some(parent) = pick_dir(state, "Clone") else {
        return;
    };
    // Derive the folder name from the URL's last path segment. Split on both
    // separators so pasted Windows paths ("C:\repos\origin") work like URLs.
    let name = url
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("repo")
        .trim_end_matches(".git")
        .to_string();
    if name.is_empty() {
        state.ui.toast = Some(Toast::error(
            "Clone: could not derive a folder name from the URL",
        ));
        return;
    }
    let dest = parent.join(name);
    let depth = state.ui.welcome_shallow.then_some(1);
    match turbogit_engine_api::GitExecutor::clone(&*state.executor, &url, &dest, depth) {
        Ok(()) => {
            state.ui.welcome_clone_url.clear();
            state.open_project(&dest);
            state.ui.toast = Some(Toast::success("Repository cloned"));
        }
        Err(e) => {
            state.last_error = Some(e.to_string());
            state.ui.toast = Some(Toast::error(format!("Clone failed: {e}")));
        }
    }
}

// --- Recent projects column ------------------------------------------------------------

/// Recent projects (spec §8.1 right column): group title + count badge, then
/// one clickable row per entry with name, path, last-opened meta, and a live
/// branch indicator. Clicking a row reopens that project.
fn recents_column(ui: &mut Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        widgets::group_title(ui, "Recent Projects");
        ui.add_space(4.0);
        let count = state.ui.recent_projects.len();
        if count > 0 {
            widgets::badge(ui, &count.to_string(), widgets::BadgeKind::Neutral);
        }
    });
    ui.add_space(4.0);

    let recents = state.ui.recent_projects.clone();
    if recents.is_empty() {
        ui.label(
            RichText::new("No recent projects yet.")
                .size(12.0)
                .color(Palette::INK_3),
        );
        return;
    }
    for r in &recents {
        recent_row(ui, state, r);
        ui.add_space(4.0);
    }
}

fn recent_row(ui: &mut Ui, state: &mut AppState, project: &turbogit_app::recents::RecentProject) {
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, RECENT_ROW_HEIGHT), Sense::click());
    let hovered = response.hovered();
    let painter = ui.painter().clone();
    if hovered {
        painter.rect_filled(rect, CornerRadius::same(4), Palette::SURFACE_2);
    }

    let pad_x = 10.0;

    let name_galley = painter.layout_no_wrap(
        truncate(&project.name, 24),
        FontId::new(13.0, FontFamily::Proportional),
        Palette::INK,
    );
    let path_galley = painter.layout_no_wrap(
        truncate(&project.path.display().to_string(), 38),
        FontId::new(11.0, FontFamily::Proportional),
        Palette::INK_3,
    );
    let meta_galley = painter.layout_no_wrap(
        turbogit_app::recents::format_last_opened(project.last_opened),
        FontId::new(11.0, FontFamily::Proportional),
        Palette::INK_3,
    );
    let x = rect.left() + pad_x;
    painter.galley(Pos2::new(x, rect.top() + 7.0), name_galley, Palette::INK);
    painter.galley(Pos2::new(x, rect.top() + 25.0), path_galley, Palette::INK_3);
    painter.galley(Pos2::new(x, rect.top() + 42.0), meta_galley, Palette::INK_3);

    // Live branch indicator (ADR-0005): computed at render time, cached in
    // memory, never stored.
    if let Some(branch) = cached_branch(state, &project.path) {
        let branch_galley = painter.layout_no_wrap(
            truncate(&branch, 18),
            FontId::new(11.0, FontFamily::Proportional),
            Palette::BRAND,
        );
        let chip_w = branch_galley.size().x + 12.0;
        let chip_rect = Rect::from_min_size(
            Pos2::new(rect.right() - pad_x - chip_w, rect.center().y - 9.0),
            Vec2::new(chip_w, 18.0),
        );
        painter.rect_filled(chip_rect, CornerRadius::same(9), Palette::SURFACE_3);
        painter.galley(
            Pos2::new(
                chip_rect.left() + 6.0,
                chip_rect.center().y - branch_galley.size().y / 2.0,
            ),
            branch_galley,
            Palette::BRAND,
        );
    }

    // Accessibility / headless-test queryability: rows are labelled by the
    // project name.
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, project.name.as_str()));
    widgets::focus_ring(ui, &response);
    if response.clicked() {
        state.open_project(&project.path);
    }
}

/// Branch of `path` for the welcome indicator: recomputed through the engine
/// seam when missing or older than [`BRANCH_TTL`], otherwise served from the
/// in-memory cache. Detached HEAD / non-repos cache as `None`.
fn cached_branch(state: &mut AppState, path: &std::path::Path) -> Option<String> {
    if let Some((branch, at)) = state.ui.welcome_branch_cache.get(path)
        && at.elapsed() < BRANCH_TTL
    {
        return branch.clone();
    }
    let branch = state.executor.current_branch(path).ok().flatten();
    state
        .ui
        .welcome_branch_cache
        .insert(path.to_path_buf(), (branch.clone(), Instant::now()));
    branch
}

// --- Getting started ------------------------------------------------------------------

const HINTS: [&str; 5] = [
    "Stage files in the Commit tool window.",
    "Write a message and commit your changelist.",
    "Push branches to share your work.",
    "Pull to bring in teammates' changes.",
    "Browse history in the Git Log tool window.",
];

/// Numbered getting-started tips (spec §8.1 item 3).
fn getting_started(ui: &mut Ui) {
    widgets::group_title(ui, "Getting Started");
    ui.add_space(6.0);
    for (i, hint) in HINTS.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{}. ", i + 1))
                    .size(12.0)
                    .color(Palette::BRAND),
            );
            ui.label(RichText::new(*hint).size(12.0).color(Palette::INK_2));
        });
    }
}

// --- Small helpers ---------------------------------------------------------------------

/// Paint one icon primitive at `origin` without disturbing layout.
fn paint_icon_at(ui: &mut Ui, icon: Icon, origin: Pos2, size: f32, color: Color32) {
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_size(origin, Vec2::splat(size)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    icons::icon(&mut child, icon, size, color);
}

/// Middle-truncate `s` to roughly `max` characters so single-line galleys fit
/// their reserved width.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let keep = max.saturating_sub(1) / 2;
    let head: String = s.chars().take(keep).collect();
    let tail: String = s.chars().skip(s.chars().count() - keep).collect();
    format!("{head}…{tail}")
}
