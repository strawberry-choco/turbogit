//! Welcome page (issue #10; redesign spec §3).
//!
//! Shown instead of the active tool window whenever no project is open or the
//! user returned to it via the command palette's Open Welcome (`AppState::
//! show_welcome`, ADR-0004). Full-window, scrollable, left-aligned content
//! (max-width 980px), one dominant region down to a two-column footer:
//!
//! 1. Hero: brand tile + wordmark + tagline, with the "What's new" trigger.
//! 2. Clone panel: the one clone door (URL input + shallow checkbox + Clone).
//! 3. Quick actions: three cards — Open / Initialize / Attach workspace.
//! 4. Lower (two columns, left wider): the recents card (ADR-0005) and the
//!    getting-started card, side by side.
//!
//! Branch indicators on recent rows are computed live at render time through
//! the engine seam and cached in memory only (never persisted).

use crate::theme::Palette;
use egui::{
    Align, Align2, Color32, CornerRadius, Id, Layout, Order, Pos2, Rect, RichText, Sense, Stroke,
    StrokeKind, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType,
};
use std::time::{Duration, Instant};
use turbogit_app::state::{AppState, Toast};

use super::components;
use super::icons::{self, Icon};
use super::widgets;

/// Content column width (spec §8.1: max-width 980px).
const CONTENT_WIDTH: f32 = 980.0;
/// Hero brand-tile edge (spec §5.1: a 64px rounded tile).
const HERO_TILE: f32 = 64.0;
/// Uniform horizontal gutter: the gap between the three quick-action cards and
/// between the two `lower` columns, matching the 20px vertical section rhythm.
const GUTTER: f32 = 20.0;
/// Quick-action card icon tile edge (spec §5.3: a 30px `SURFACE_3` tile).
const ICON_TILE: f32 = 30.0;
/// Getting-started step pill edge (spec §5.5: a 20px `SURFACE_3` numeral).
const STEP_PILL: f32 = 20.0;
/// A recent-project row: name, path and last-opened on three lines, so the row
/// is three text lines plus leading rather than a step on the 24/26 px single-line
/// ramp — which is why it stays here rather than joining `theme`'s row heights.
const RECENT_ROW_HEIGHT: f32 = 64.0;

/// Paint alpha of the hero's soft `BRAND` gradient overlay (spec §5.1). A mix
/// fraction, not a design token — egui has no linear-gradient fill, so the halo
/// is one `BRAND`-tinted rect blended over [`Palette::BG`].
const GRADIENT_ALPHA: f32 = 0.22;

/// Branch indicators recompute at most this often (ADR-0005: computed live
/// at render with in-memory caching — never stored).
const BRANCH_TTL: Duration = Duration::from_secs(5);

/// Render the Welcome page inside the shell's central panel.
pub fn show(ui: &mut Ui, state: &mut AppState) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        let avail = ui.available_width();
        let margin = ((avail - CONTENT_WIDTH) / 2.0).max(0.0);
        ui.add_space(20.0);
        ui.horizontal(|ui| {
            ui.add_space(margin);
            ui.vertical(|ui| {
                ui.set_max_width(CONTENT_WIDTH.min(avail));
                // Left-aligned vertical rhythm (spec §5.6): hero → clone panel →
                // quick actions → lower, uniform 20px between sections.
                hero(ui, state);
                ui.add_space(20.0);
                clone_box(ui, state);
                ui.add_space(20.0);
                quick_actions(ui, state);
                ui.add_space(20.0);
                lower(ui, state);
            });
        });
    });
    changelog_overlay(ui, state);
}

// --- Hero --------------------------------------------------------------------

/// The screen's one dominant region (spec §5.1): a soft `BRAND` gradient band
/// carrying the 64px brand tile + white `FOLDER_GIT`, the wordmark and tagline
/// beside it, and the "What's new" ghost trigger on the right (SPACE_BETWEEN).
/// It replaces the old centered `brand_header` + `what_new_link`. No top bar is
/// painted here — the shell already renders one over this panel (plan decision
/// Q1). The gradient is one subtle overlay rect; if it ever reads as a blob at
/// some width, drop it — the hero stands without it.
fn hero(ui: &mut Ui, state: &mut AppState) {
    let pad = 16.0;
    let gap = 10.0;
    let avail = ui.available_width();

    // Size the band to its content, not to the 64px tile: the 42px wordmark
    // stacked over a 14px tagline is taller than the tile, so a tile-height band
    // let the text overflow the gradient vertically and touch its top edge.
    // Measure both lines, then pad every side so the headline keeps clearance.
    let painter = ui.painter().clone();
    let wm_font = crate::theme::chrome_font(crate::theme::TYPE_WORDMARK);
    let wm = painter.layout_no_wrap("TurboGit".to_owned(), wm_font.clone(), Palette::INK);
    let inner_w = (avail - 2.0 * pad).max(HERO_TILE + 2.0 * gap + 80.0);
    let text_w = (inner_w - HERO_TILE - gap).max(80.0);
    let tg = painter.layout(
        "A fast, keyboard-friendly Git client for your desktop.".to_owned(),
        crate::theme::chrome_font(crate::theme::TYPE_TAGLINE),
        Palette::INK_3,
        text_w,
    );
    let text_h = wm.size().y + ui.spacing().item_spacing.y + tg.size().y;
    let band_h = HERO_TILE.max(text_h) + 2.0 * pad;
    let (band, _) = ui.allocate_exact_size(Vec2::new(avail, band_h), Sense::hover());

    let radius = CornerRadius::same(crate::theme::CARD_RADIUS);
    painter.rect_filled(
        band,
        radius,
        widgets::mix(Palette::BG, Palette::BRAND, GRADIENT_ALPHA),
    );

    // Content inset from every band edge so nothing sits flush on the gradient.
    let inner = band.shrink(pad);

    // Left cluster: brand tile, then the wordmark/tagline stack.
    let mut left = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::left_to_right(Align::Center)),
    );
    let (tile, _) = left.allocate_exact_size(Vec2::splat(HERO_TILE), Sense::hover());
    left.painter().rect_filled(tile, radius, Palette::BRAND);
    let icon_size = HERO_TILE / 2.0;
    paint_icon_at(
        &mut left,
        Icon::FOLDER_GIT,
        Pos2::new(
            tile.center().x - icon_size / 2.0,
            tile.center().y - icon_size / 2.0,
        ),
        icon_size,
        Palette::BRAND_INK,
    );
    left.add_space(gap);
    left.vertical(|ui| {
        ui.label(
            RichText::new("TurboGit")
                .strong()
                .font(wm_font)
                .color(Palette::INK),
        );
        ui.label(
            RichText::new("A fast, keyboard-friendly Git client for your desktop.")
                .size(crate::theme::TYPE_TAGLINE)
                .color(Palette::INK_3),
        );
    });

    // Right: the "What's new" ghost button (SPACE_BETWEEN), opening the same
    // changelog overlay the retired centered link did.
    let mut right = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::right_to_left(Align::Center)),
    );
    if widgets::ghost_button(&mut right, Some(Icon::STAR), "What's new").clicked() {
        state.ui.show_changelog = true;
    }
}

// --- Lower band: recents + getting started -----------------------------------

fn lower(ui: &mut Ui, state: &mut AppState) {
    // Two columns on the shared card grid (spec §5.6): the getting-started card
    // is exactly one card module (sitting directly under the third quick-action
    // card), recents fills the remaining two modules, and the gap between them
    // stays one `GUTTER`. `item_spacing.x` is pinned to 0 so `add_space(GUTTER)`
    // is exact — otherwise egui's auto spacing overhangs the content column.
    let avail = ui.available_width();
    let getting_w = card_module(avail).min((avail - GUTTER) * 0.5);
    let recents_w = (avail - GUTTER) - getting_w;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.vertical(|ui| {
            ui.set_min_width(recents_w);
            ui.set_max_width(recents_w);
            recents_column(ui, state);
        });
        ui.add_space(GUTTER);
        ui.vertical(|ui| {
            ui.set_min_width(getting_w);
            ui.set_max_width(getting_w);
            getting_started(ui);
        });
    });
}

// --- Action cards -----------------------------------------------------------------

#[derive(Clone, Copy)]
enum CardAction {
    /// Pick a folder and open it as a project (end-to-end).
    OpenProject,
    /// Pick a folder, `git init` it, and enter it (end-to-end).
    InitRepo,
    /// Pick a directory tree and register every repo in it as a workspace
    /// (issue #34).
    AttachWorkspace,
}

/// One column of the shared 3-up card grid: the content width split into three
/// equal modules with `GUTTER` gaps. `lower` reuses this so the getting-started
/// card is exactly one module wide — aligned under the third quick-action card.
fn card_module(width: f32) -> f32 {
    (width - 2.0 * GUTTER) / 3.0
}

fn quick_actions(ui: &mut Ui, state: &mut AppState) {
    // Exactly three equal-width cards in one row (spec §5.3). `item_spacing.x`
    // is pinned to 0 so `add_space(GUTTER)` is the exact gap and the three cards
    // span precisely the content column — their right edge lines up with the
    // hero and clone panel instead of overhanging it.
    let card_w = card_module(ui.available_width());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        action_card_title(ui, state, CardAction::OpenProject, card_w);
        ui.add_space(GUTTER);
        action_card_title(ui, state, CardAction::InitRepo, card_w);
        ui.add_space(GUTTER);
        action_card_title(ui, state, CardAction::AttachWorkspace, card_w);
    });
}

/// Dispatch-layer wrapper: yields the card's copy so the shared painter can
/// stay single-purpose. Descriptions are the spec-Q3 tightened wording.
fn action_card_title(ui: &mut Ui, state: &mut AppState, action: CardAction, width: f32) {
    let (title, body, icon) = match action {
        CardAction::OpenProject => (
            "Open Project",
            "Open a folder as a TurboGit project.",
            Icon::FOLDER_OPEN,
        ),
        CardAction::InitRepo => (
            "Initialize Repository",
            "Create a fresh Git repository in a chosen folder.",
            Icon::FOLDER_GIT,
        ),
        CardAction::AttachWorkspace => (
            "Attach Workspace Root",
            "Index every repo in a folder tree.",
            Icon::FOLDER,
        ),
    };
    action_card(ui, state, icon, title, body, width, action);
}

/// One quick-action card (spec §5.3): a `CONTENT_BG` fill, `LINE` border and
/// `CARD_RADIUS`, content-hugging — its height derives from its own galleys
/// rather than a fixed box. A 30px `SURFACE_3` tile (`CONTROL_RADIUS`) carries a
/// 15px `ACCENT_TEXT` icon; title `TYPE_DETAIL_TITLE` `INK`, description
/// `TYPE_BODY` `INK_2`. Hover brightens the fill and border.
fn action_card(
    ui: &mut Ui,
    state: &mut AppState,
    icon: Icon,
    title: &str,
    body: &str,
    width: f32,
    action: CardAction,
) {
    let pad = 16.0;
    let gap = 8.0;
    let icon_size = 15.0;
    let painter = ui.painter().clone();
    let title_galley = painter.layout_no_wrap(
        title.to_owned(),
        crate::theme::chrome_font(crate::theme::TYPE_DETAIL_TITLE),
        Palette::INK,
    );
    let body_galley = painter.layout(
        body.to_owned(),
        crate::theme::chrome_font(crate::theme::TYPE_BODY),
        Palette::INK_2,
        (width - 2.0 * pad).max(40.0),
    );
    let height = 2.0 * pad + ICON_TILE + gap + title_galley.size().y + gap + body_galley.size().y;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let hovered = response.hovered();
    let radius = CornerRadius::same(crate::theme::CARD_RADIUS);
    painter.rect_filled(
        rect,
        radius,
        if hovered {
            Palette::SURFACE_2
        } else {
            Palette::CONTENT_BG
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

    let tile = Rect::from_min_size(
        Pos2::new(rect.left() + pad, rect.top() + pad),
        Vec2::splat(ICON_TILE),
    );
    painter.rect_filled(
        tile,
        CornerRadius::same(crate::theme::CONTROL_RADIUS),
        Palette::SURFACE_3,
    );
    paint_icon_at(
        ui,
        icon,
        Pos2::new(
            tile.center().x - icon_size / 2.0,
            tile.center().y - icon_size / 2.0,
        ),
        icon_size,
        Palette::ACCENT_TEXT,
    );

    let x = rect.left() + pad;
    let title_y = tile.bottom() + gap;
    let title_h = title_galley.size().y;
    painter.galley(Pos2::new(x, title_y), title_galley, Palette::INK);
    painter.galley(
        Pos2::new(x, title_y + title_h + gap),
        body_galley,
        Palette::INK_2,
    );

    // Accessibility / headless-test queryability.
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, title));
    widgets::focus_ring(ui, &response);
    if !response.clicked() {
        return;
    }
    match action {
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
        CardAction::AttachWorkspace => {
            if let Some(dir) = pick_dir(state, "Attach Workspace Root") {
                state.attach_workspace(&dir);
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

/// Folder-picker entry for the workspace picker (issue #34) and the Welcome
/// Open card — one shared seam for both folder-picker flows.
pub fn pick_dir_public(state: &mut AppState, purpose: &str) -> Option<std::path::PathBuf> {
    pick_dir(state, purpose)
}

// --- Clone box ----------------------------------------------------------------------

/// The one clone door (spec §5.2): a full-width `CONTENT_BG` card merging the
/// old "Clone from URL" card and the inline clone box. A header row carries the
/// `DOWNLOAD` accent icon + "Clone a repository" with the shallow checkbox
/// right-aligned; the body row is the URL input (filling) plus a primary Clone
/// button. Input and checkbox keep their exact kittest labels ("Repository URL",
/// "Shallow clone (--depth 1)"), so the clone flow's headless coverage is
/// untouched. The card is the shared [`widgets::card`] at 16 px of padding
/// (wider than the panel padding) and a full-width stretch.
fn clone_box(ui: &mut Ui, state: &mut AppState) {
    widgets::card(ui, widgets::CardFrame::default().padded(16), |ui| {
        ui.horizontal(|ui| {
            icons::icon(ui, Icon::DOWNLOAD, 16.0, Palette::ACCENT_TEXT);
            ui.add_space(6.0);
            ui.label(
                RichText::new("Clone a repository")
                    .font(crate::theme::chrome_font(crate::theme::TYPE_DETAIL_TITLE))
                    .color(Palette::INK),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.checkbox(
                    &mut state.ui.welcome_shallow,
                    RichText::new("Shallow clone (--depth 1)")
                        .size(crate::theme::TYPE_CONTROL)
                        .color(Palette::INK_2),
                );
            });
        });
        ui.add_space(10.0);

        // Body: URL input fills the row; the Clone button is pinned right.
        // Reserve the button's width first so `text_input` doesn't swallow
        // the whole line (spec §5.2: input + button on one row).
        let label_w = ui
            .painter()
            .layout_no_wrap(
                "Clone".to_owned(),
                crate::theme::chrome_font(crate::theme::TYPE_CONTROL),
                Color32::WHITE,
            )
            .size()
            .x;
        let btn_w = 2.0 * ui.spacing().button_padding.x + (16.0 + 6.0) + label_w;
        let input_w = (ui.available_width() - btn_w - ui.spacing().item_spacing.x).max(120.0);
        let mut input_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(Rect::from_min_size(
                    ui.cursor().min,
                    Vec2::new(input_w, 32.0),
                ))
                .layout(*ui.layout()),
        );
        let response = widgets::text_input(
            &mut input_ui,
            "Repository URL",
            &mut state.ui.welcome_clone_url,
        );
        ui.advance_cursor_after_rect(input_ui.min_rect());
        if std::mem::take(&mut state.ui.welcome_focus_clone) {
            response.request_focus();
        }
        if widgets::primary_button(ui, Some(Icon::DOWNLOAD), "Clone").clicked() {
            clone_from_url(state);
        }
    });
}

/// Clone the entered URL into a picked parent folder and enter the result.
/// The clone itself is the app layer's (["AppState::clone_into"]); this only
/// gathers the URL, the folder and the folder name the URL implies.
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
    state.clone_into(&url, &dest, depth);
}

// --- Recent projects column ------------------------------------------------------------

/// Recent projects as a real card (spec §5.4): a `CONTENT_BG` frame holding the
/// "RECENT PROJECTS" group title + a right-aligned count badge, one clickable
/// row per entry, and a "Show all projects" footer. `recent_row`'s internals are
/// unchanged apart from the branch chip fill. Clicking a row reopens that project.
/// The card is the shared [`widgets::card`] at 16 px of padding and a
/// full-width stretch.
fn recents_column(ui: &mut Ui, state: &mut AppState) {
    widgets::card(ui, widgets::CardFrame::default().padded(16), |ui| {
        let count = state.ui.recent_projects.len();
        ui.horizontal(|ui| {
            widgets::group_title(ui, "Recent Projects");
            if count > 0 {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    widgets::badge(ui, &count.to_string(), widgets::BadgeKind::Neutral);
                });
            }
        });
        ui.add_space(4.0);

        let recents = state.ui.recent_projects.clone();
        if recents.is_empty() {
            ui.label(
                RichText::new("No recent projects yet.")
                    .size(crate::theme::TYPE_BODY)
                    .color(Palette::INK_3),
            );
            return;
        }
        for r in &recents {
            recent_row(ui, state, r);
            ui.add_space(4.0);
        }

        // Footer: a visually-honest v1 no-op — there is no recents browser
        // to route to yet (recorded as a follow-up in plan §4).
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Show all projects")
                    .size(crate::theme::TYPE_CONTROL)
                    .color(Palette::ACCENT_TEXT),
            );
            icons::icon(ui, Icon::CHEVRON_RIGHT, 14.0, Palette::ACCENT_TEXT);
        });
    });
}

fn recent_row(ui: &mut Ui, state: &mut AppState, project: &turbogit_app::recents::RecentProject) {
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, RECENT_ROW_HEIGHT), Sense::click());
    let hovered = response.hovered();
    let painter = ui.painter().clone();
    widgets::paint_row(ui, rect, components::RowState::from_flags(false, hovered));

    let pad_x = 10.0;

    let name_galley = painter.layout_no_wrap(
        truncate(&project.name, 24),
        crate::theme::chrome_font(crate::theme::TYPE_DETAIL_TITLE),
        Palette::INK,
    );
    let path_galley = painter.layout_no_wrap(
        truncate(&project.path.display().to_string(), 38),
        crate::theme::chrome_font(crate::theme::TYPE_CONTROL),
        Palette::INK_3,
    );
    let meta_galley = painter.layout_no_wrap(
        turbogit_app::recents::format_last_opened(project.last_opened),
        crate::theme::chrome_font(crate::theme::TYPE_CONTROL),
        Palette::INK_3,
    );
    let x = rect.left() + pad_x;
    painter.galley(Pos2::new(x, rect.top() + 7.0), name_galley, Palette::INK);
    painter.galley(Pos2::new(x, rect.top() + 25.0), path_galley, Palette::INK_3);
    painter.galley(Pos2::new(x, rect.top() + 42.0), meta_galley, Palette::INK_3);

    // Live branch indicator (ADR-0005): computed at render time, cached in
    // memory, never stored. Small text on the chip uses the readable accent
    // ink (ACCENT_TEXT), not the action-fill BRAND — BRAND-on-SELECTION only
    // clears ~2.5:1, while ACCENT_TEXT reaches 4.5:1 on the SELECTION chip
    // (see the design_tokens audit). The SELECTION fill is the deliberate
    // brand-warm contrast change from spec §4.
    if let Some(branch) = cached_branch(state, &project.path) {
        let branch_galley = painter.layout_no_wrap(
            truncate(&branch, 18),
            crate::theme::chrome_font(crate::theme::TYPE_CONTROL),
            Palette::ACCENT_TEXT,
        );
        let chip_rect =
            widgets::chip_rect_right(rect.right() - pad_x, rect.center().y, &branch_galley);
        widgets::paint_chip(
            &painter,
            chip_rect,
            branch_galley,
            Palette::SELECTION,
            Palette::ACCENT_TEXT,
        );
    }

    // Workspace rows (issue #34) additionally show their indexed repo count
    // as a chip opposite the branch indicator, then restore via the attach
    // flow instead of a single-project open.
    if project.kind == turbogit_app::recents::RecentKind::Workspace
        && let Some(count) = project.repo_count
    {
        let count_text = if count == 1 {
            "1 repo".to_string()
        } else {
            format!("{count} repos")
        };
        let count_galley = painter.layout_no_wrap(
            count_text,
            crate::theme::chrome_font(crate::theme::TYPE_CONTROL),
            Palette::ACCENT_TEXT,
        );
        let right_edge = rect.right() - pad_x;
        // A branch chip would already own the right side; stack the count
        // above it, or sit at the row's vertical middle when absent.
        let cy = if rect.contains(Pos2::new(right_edge, rect.center().y)) {
            rect.top() + 10.0
        } else {
            rect.center().y
        };
        let count_rect = widgets::chip_rect_right(right_edge, cy, &count_galley);
        widgets::paint_chip(
            &painter,
            count_rect,
            count_galley,
            Palette::SURFACE_3,
            Palette::ACCENT_TEXT,
        );
    }

    // Accessibility / headless-test queryability: rows are labelled by the
    // project name.
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, project.name.as_str()));
    widgets::focus_ring(ui, &response);
    if response.clicked() {
        // Shared dispatch (issue #34): the topbar workspace picker routes
        // through the same helper, so the kind semantics — a Workspace row
        // deep-scans, a Project row bounded-scans — live in one place.
        state.open_recent(project);
    }
}

/// Branch of `path` for the welcome indicator: recomputed when missing or
/// older than [`BRANCH_TTL`], otherwise served from the in-memory cache.
/// Detached HEAD / non-repos cache as `None`.
fn cached_branch(state: &mut AppState, path: &std::path::Path) -> Option<String> {
    if let Some((branch, at)) = state.ui.welcome_branch_cache.get(path)
        && at.elapsed() < BRANCH_TTL
    {
        return branch.clone();
    }
    let branch = state.current_branch_of(path);
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

/// Numbered getting-started tips as a `SURFACE` card (spec §5.5): the group
/// title plus five steps, each a `STEP_PILL` numeral pill before its body text.
/// It sits beside the recents card, so it wears the shared
/// [`widgets::CardFrame::raised`] tone at 16 px of padding — a `CONTENT_BG`
/// fill there would be the same colour as the pane behind it.
fn getting_started(ui: &mut Ui) {
    widgets::card(
        ui,
        widgets::CardFrame::default().raised().padded(16),
        |ui| {
            widgets::group_title(ui, "Getting Started");
            ui.add_space(6.0);
            for (i, hint) in HINTS.iter().enumerate() {
                ui.horizontal(|ui| {
                    step_pill(ui, i + 1);
                    ui.label(
                        RichText::new(*hint)
                            .size(crate::theme::TYPE_BODY)
                            .color(Palette::INK_2),
                    );
                });
                ui.add_space(4.0);
            }
        },
    );
}

/// A getting-started step numeral: a `STEP_PILL` `SURFACE_3` pill
/// (`PILL_RADIUS`) carrying a mono `ACCENT_TEXT` digit (spec §5.5).
fn step_pill(ui: &mut Ui, n: usize) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(STEP_PILL), Sense::hover());
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(crate::theme::PILL_RADIUS),
        Palette::SURFACE_3,
    );
    widgets::paint_centered_text(
        ui.painter(),
        rect,
        &n.to_string(),
        crate::theme::data_font(crate::theme::TYPE_CONTROL),
        Palette::ACCENT_TEXT,
    );
}

// --- What's new / changelog ------------------------------------------------------

/// In-app changelog entries (issue #34), rendered by the "What's new" overlay.
const CHANGELOG: &[(&str, &str)] = &[
    (
        "v0.9.0",
        "• Attach a workspace root and index every repository in a folder tree.",
    ),
    (
        "v0.9.0",
        "• Recents now show workspace repo counts and restore a workspace on click.",
    ),
    (
        "v0.9.0",
        "• The header reports the app version, git version, and indexed repo count.",
    ),
    ("v0.9.0", "• A “What's new” link opens this changelog."),
];

/// Center-anchored changelog overlay (issue #34): a framed panel listing
/// [`CHANGELOG`] entries with a Close button. Painted above the Welcome page
/// content while [`UiState::show_changelog`] is set.
///
/// It floats rather than filling a pane, so it is the shared [`widgets::card`]
/// in its pinned form — [`widgets::CardFrame::raised`] tone, 20 px of padding,
/// and a 420 px minimum width — inside the centering [`egui::Area`].
fn changelog_overlay(ui: &mut Ui, state: &mut AppState) {
    if !state.ui.show_changelog {
        return;
    }
    egui::Area::new(Id::new("welcome_changelog_overlay"))
        .order(Order::Tooltip)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .show(ui.ctx(), |ui| {
            widgets::card(
                ui,
                widgets::CardFrame::default()
                    .raised()
                    .padded(20)
                    .min_width(420.0),
                |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("What's New")
                                .strong()
                                .size(crate::theme::TYPE_PANE_TITLE)
                                .color(Palette::INK),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button("Close").clicked() {
                                state.ui.show_changelog = false;
                            }
                        });
                    });
                    ui.add_space(10.0);
                    egui::ScrollArea::vertical()
                        .max_height(400.0)
                        .show(ui, |ui| {
                            for (version, note) in CHANGELOG {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(*version)
                                            .strong()
                                            .size(crate::theme::TYPE_BODY)
                                            .color(Palette::BRAND),
                                    );
                                    ui.label(
                                        RichText::new(*note)
                                            .size(crate::theme::TYPE_BODY)
                                            .color(Palette::INK_2),
                                    );
                                });
                                ui.add_space(6.0);
                            }
                        });
                },
            );
        });
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
