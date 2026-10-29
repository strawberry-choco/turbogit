use crate::theme::Palette;
use egui::{
    Align, Align2, Color32, CornerRadius, Id, Layout, Order, Pos2, Rect, RichText, Sense, Ui,
    UiBuilder, Vec2, WidgetInfo, WidgetType,
};
use std::time::{Duration, Instant};
use turbogit_app::state::{AppState, Toast};

use super::components;
use super::icons::{self, Icon};
use super::widgets;

const CONTENT_WIDTH: f32 = 980.0;
const HERO_TILE: f32 = 64.0;
const GUTTER: f32 = 20.0;
const ICON_TILE: f32 = 30.0;
const STEP_PILL: f32 = 20.0;
const RECENT_ROW_HEIGHT: f32 = 64.0;

const RECENT_ACTIONS_ZONE: f32 = 44.0;

/// Paint alpha of the hero's soft `BRAND` gradient overlay (spec §5.1). A mix
/// fraction, not a design token, egui has no linear-gradient fill, so the halo
/// is one `BRAND`-tinted rect blended over [`Palette::BG`].
const GRADIENT_ALPHA: f32 = 0.22;

const BRANCH_TTL: Duration = Duration::from_secs(5);

fn pane_title_font() -> egui::FontId {
    crate::theme::chrome_font(crate::theme::TYPE_PANE_TITLE)
}

fn wordmark_font() -> egui::FontId {
    crate::theme::chrome_font(crate::theme::TYPE_WORDMARK)
}

pub fn show(ui: &mut Ui, state: &mut AppState) {
    egui::ScrollArea::vertical()
        .id_salt("welcome_page")
        .show(ui, |ui| {
            let avail = ui.available_width();
            let margin = ((avail - CONTENT_WIDTH) / 2.0).max(0.0);
            ui.add_space(20.0);
            ui.horizontal(|ui| {
                ui.add_space(margin);
                ui.vertical(|ui| {
                    ui.set_max_width(CONTENT_WIDTH.min(avail));
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

fn hero(ui: &mut Ui, state: &mut AppState) {
    let pad = 16.0;
    let gap = 10.0;
    let avail = ui.available_width();

    let painter = ui.painter().clone();
    let wm_font = wordmark_font();
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

    let inner = band.shrink(pad);

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

    let mut right = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::right_to_left(Align::Center)),
    );
    if widgets::ghost_button(&mut right, Some(Icon::STAR), "What's new").clicked() {
        state.ui.show_changelog = true;
    }
}

fn lower(ui: &mut Ui, state: &mut AppState) {
    // Two columns on the shared card grid (spec §5.6): the getting-started card
    // is exactly one card module (sitting directly under the third quick-action
    // card), recents fills the remaining two modules, and the gap between them
    // stays one `GUTTER`. `item_spacing.x` is pinned to 0 so `add_space(GUTTER)`
    // is exact, otherwise egui's auto spacing overhangs the content column.
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

#[derive(Clone, Copy)]
enum CardAction {
    OpenProject,
    InitRepo,
    AttachWorkspace,
}

fn card_module(width: f32) -> f32 {
    (width - 2.0 * GUTTER) / 3.0
}

fn quick_actions(ui: &mut Ui, state: &mut AppState) {
    // Exactly three equal-width cards in one row (spec §5.3). `item_spacing.x`
    // is pinned to 0 so `add_space(GUTTER)` is the exact gap and the three cards
    // span precisely the content column, their right edge lines up with the
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
            Palette::RAISED_ON_CARD
        } else {
            Palette::CONTENT_BG
        },
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

pub fn pick_dir_public(state: &mut AppState, purpose: &str) -> Option<std::path::PathBuf> {
    pick_dir(state, purpose)
}

fn clone_box(ui: &mut Ui, state: &mut AppState) {
    let git = state.git_env_check();
    let url_present = !state.ui.welcome_clone_url.trim().is_empty();
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

        if let Err(reason) = &git {
            ui.label(
                RichText::new(format!(
                    "Git was not found ({reason}). Install Git, or set its location in \
                     Settings \u{203a} Git."
                ))
                .size(crate::theme::TYPE_BODY)
                .color(Palette::STATE_WARNING),
            );
            ui.add_space(8.0);
        }

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
        let can_clone = git.is_ok() && url_present;
        let clicked = widgets::disabled_child_scope(ui, can_clone, |ui| {
            widgets::primary_button(ui, Some(Icon::DOWNLOAD), "Clone").clicked()
        });
        if clicked {
            clone_from_url(state);
        }
    });
}

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
            widgets::empty_state(ui, "No recent projects yet.");
            return;
        }
        for r in &recents {
            recent_row(ui, state, r);
            ui.add_space(4.0);
        }

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
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, RECENT_ROW_HEIGHT), Sense::hover());
    let clickable = Rect::from_min_max(
        rect.min,
        Pos2::new(
            (rect.right() - RECENT_ACTIONS_ZONE).max(rect.left()),
            rect.bottom(),
        ),
    );
    let response = ui.interact(
        clickable,
        ui.auto_id_with(("recent_row", &project.path)),
        Sense::click(),
    );
    let hovered = ui.rect_contains_pointer(rect);
    let painter = ui.painter().clone();
    widgets::paint_row(ui, rect, components::RowState::from_flags(false, hovered));

    let pad_x = 10.0;
    let chips_right = rect.right() - RECENT_ACTIONS_ZONE;

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

    let branch = cached_branch(state, &project.path);
    if let Some(branch) = &branch {
        let branch_galley = painter.layout_no_wrap(
            truncate(branch, 18),
            crate::theme::chrome_font(crate::theme::TYPE_CONTROL),
            Palette::ACCENT_TEXT,
        );
        let chip_rect = widgets::chip_rect_right(chips_right, rect.center().y, &branch_galley);
        widgets::paint_chip(
            &painter,
            chip_rect,
            branch_galley,
            Palette::SELECTION,
            Palette::ACCENT_TEXT,
        );
    }

    if project.kind == turbogit_app::recents::RecentKind::Workspace
        && let Some(count) = project.repo_count
    {
        let count_text = components::counted(count, "repo", "repos");
        let count_galley = painter.layout_no_wrap(
            count_text,
            crate::theme::chrome_font(crate::theme::TYPE_CONTROL),
            Palette::ACCENT_TEXT,
        );
        let cy = if branch.is_some() {
            rect.top() + 10.0
        } else {
            rect.center().y
        };
        let count_rect = widgets::chip_rect_right(chips_right, cy, &count_galley);
        widgets::paint_chip(
            &painter,
            count_rect,
            count_galley,
            Palette::SURFACE_3,
            Palette::ACCENT_TEXT,
        );
    }

    let remove_zone = Rect::from_min_max(
        Pos2::new(chips_right, rect.top()),
        Pos2::new(rect.right(), rect.bottom()),
    );
    let mut remove_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(remove_zone)
            .layout(Layout::right_to_left(Align::Center)),
    );
    remove_ui.add_space(pad_x);
    let remove = widgets::icon_button(&mut remove_ui, Icon::TRASH_2);
    remove.widget_info(|| {
        WidgetInfo::labeled(
            WidgetType::Button,
            true,
            format!("Remove recent project {}", project.name),
        )
    });
    let remove = remove.on_hover_text(format!("Remove '{}' from recent projects", project.name));
    if remove.clicked() {
        state.remove_recent(&project.path);
        return;
    }

    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, project.name.as_str()));
    widgets::focus_ring(ui, &response);
    if response.clicked() {
        state.open_recent(project);
    }
}

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

const HINTS: [&str; 5] = [
    "Stage files in the Commit tool window.",
    "Write a message and commit your changelist.",
    "Push branches to share your work.",
    "Pull to bring in teammates' changes.",
    "Browse history in the Git Log tool window.",
];

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
                    .bordered()
                    .padded(20)
                    .min_width(420.0),
                |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("What's New")
                                .strong()
                                .font(pane_title_font())
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
                        .id_salt("welcome_changelog")
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

fn paint_icon_at(ui: &mut Ui, icon: Icon, origin: Pos2, size: f32, color: Color32) {
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_size(origin, Vec2::splat(size)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    icons::icon(&mut child, icon, size, color);
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let keep = max.saturating_sub(1) / 2;
    let head: String = s.chars().take(keep).collect();
    let tail: String = s.chars().skip(s.chars().count() - keep).collect();
    format!("{head}…{tail}")
}
