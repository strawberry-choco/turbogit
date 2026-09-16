//! Activity log panel (issue #04, screen 01): a collapsible strip at the
//! bottom of the shell recording every dispatched git operation and its
//! outcome. Entries are session-durable — the feed outlives toasts; each
//! row shows timestamp · repo · human-readable outcome, colored by severity.
//!
//! Filtering is by repo and by time window (header controls); Clear empties
//! the feed. Collapse/expand only flips [`turbogit_app::activity::ActivityLog::expanded`]
//! — the entries stay.

use chrono::Local;
use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Layout, Pos2, Rect, RichText, ScrollArea,
    Sense, Shape, Stroke, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType,
};

use super::icons::{self, Icon};
use super::widgets;
use crate::theme::Palette;
use turbogit_app::activity::{ActivityKind, TimeWindow};
use turbogit_app::state::AppState;

/// Expanded panel height (header strip + feed).
pub const ACTIVITY_HEIGHT: f32 = 200.0;
/// Collapsed height: the header strip only.
pub const ACTIVITY_COLLAPSED_HEIGHT: f32 = 26.0;

const HEADER_TEXT: f32 = 11.0;
const ROW_TEXT: f32 = 12.0;

/// The STATE_* token an entry's severity paints with (issue #04).
pub fn kind_color(kind: ActivityKind) -> Color32 {
    match kind {
        ActivityKind::Success => Palette::STATE_SUCCESS,
        ActivityKind::Warning => Palette::STATE_WARNING,
        ActivityKind::Error => Palette::STATE_ERROR,
    }
}

/// Render the panel into `ui`, whose max rect is the panel's reserved
/// bottom strip of the central body (see `shell::render`).
pub fn show(ui: &mut Ui, state: &mut AppState) {
    let rect = ui.max_rect();
    ui.painter()
        .rect_filled(rect, CornerRadius::same(0), Palette::SURFACE);
    ui.painter().line_segment(
        [
            Pos2::new(rect.left(), rect.top() + 0.5),
            Pos2::new(rect.right(), rect.top() + 0.5),
        ],
        Stroke::new(1.0, Palette::LINE),
    );

    // Snapshot the visible feed so the rows borrow nothing while the
    // header controls mutate the log.
    let rows: Vec<_> = state
        .ui
        .activity
        .visible(Local::now())
        .into_iter()
        .cloned()
        .collect();

    ui.add_space(2.0);
    ui.horizontal(|ui| {
        render_title_chip(ui, state);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if state.ui.activity.expanded {
                if ui.button(header_button("Clear feed")).clicked() {
                    state.ui.activity.clear();
                }
                if ui
                    .button(header_button(window_label(state.ui.activity.window)))
                    .clicked()
                {
                    state.ui.activity.window = match state.ui.activity.window {
                        TimeWindow::All => TimeWindow::Last30Min,
                        TimeWindow::Last30Min => TimeWindow::All,
                    };
                }
                if ui.button(header_button(&repo_label(state))).clicked() {
                    cycle_repo_filter(state);
                }
            }
        });
    });

    if !state.ui.activity.expanded {
        return;
    }

    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        if rows.is_empty() {
            ui.add_space(16.0);
            ui.label(
                RichText::new("No activity yet — dispatched operations land here.")
                    .font(FontId::new(ROW_TEXT, FontFamily::Proportional))
                    .color(Palette::INK_3),
            );
            return;
        }
        // Newest first: the feed is a record, but what the user wants to
        // see is what just happened.
        for e in rows.iter().rev() {
            ui.horizontal(|ui| {
                ui.style_mut().spacing.item_spacing.x = 8.0;
                let time = e.at.format("%H:%M").to_string();
                ui.label(
                    RichText::new(time)
                        .font(FontId::new(HEADER_TEXT, FontFamily::Monospace))
                        .color(Palette::INK_3),
                );
                ui.label(
                    RichText::new(e.repo.clone().unwrap_or_else(|| "all repos".to_owned()))
                        .font(FontId::new(HEADER_TEXT, FontFamily::Proportional))
                        .color(Palette::INK_2),
                );
                ui.label(
                    RichText::new(&e.message)
                        .font(FontId::new(ROW_TEXT, FontFamily::Proportional))
                        .color(kind_color(e.kind)),
                );
            });
        }
    });
}

/// The header's title chip — the chevron and the ACTIVITY label inside one
/// interactive rect, so the glyph a user aims at is the glyph that responds.
///
/// History worth keeping: the old toggle allocated the click target and then
/// let `icons::icon` allocate its own slot *beside* it, so the painted
/// chevron landed outside the hit rect and the visible control was dead —
/// only the invisible 16px box to its left answered. The headless suite
/// never noticed because kittest clicks the a11y rect's centre.
fn render_title_chip(ui: &mut Ui, state: &mut AppState) {
    const ICON_SLOT: f32 = 16.0;
    const ICON_SIZE: f32 = 12.0;

    let expanded = state.ui.activity.expanded;
    let (label, icon) = if expanded {
        ("Collapse activity", Icon::CHEVRON_DOWN)
    } else {
        ("Expand activity", Icon::CHEVRON_RIGHT)
    };

    // Hover-tint slot: filled after the chip rect is known but written back
    // at its original index, so the tint renders behind the glyph and label.
    let hover_tint = ui.painter().add(Shape::Noop);

    let (icon_rect, _) = ui.allocate_exact_size(Vec2::splat(ICON_SLOT), Sense::hover());
    paint_icon_centered(ui, icon, icon_rect.center(), ICON_SIZE, Palette::INK_2);

    let text = ui.label(
        RichText::new("ACTIVITY")
            .strong()
            .font(FontId::new(HEADER_TEXT, FontFamily::Proportional))
            .color(Palette::INK_3),
    );

    let chip = icon_rect.union(text.rect).expand2(Vec2::new(2.0, 0.0));
    let response = ui.interact(chip, ui.auto_id_with("activity_title_chip"), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    if response.clicked() {
        state.ui.activity.expanded = !expanded;
    }
    if response.hovered() {
        ui.painter().set(
            hover_tint,
            Shape::rect_filled(chip, CornerRadius::same(3), Palette::SURFACE_2),
        );
    }
    widgets::focus_ring(ui, &response);
}

/// Paint one icon primitive centered at `center` without disturbing layout
/// (mirrors `shell::paint_icon_centered`).
fn paint_icon_centered(ui: &mut Ui, icon: Icon, center: Pos2, size: f32, color: Color32) {
    let mut child =
        ui.new_child(UiBuilder::new().max_rect(Rect::from_center_size(center, Vec2::splat(size))));
    icons::icon(&mut child, icon, size, color);
}

fn header_button(label: &str) -> RichText {
    RichText::new(label)
        .font(FontId::new(HEADER_TEXT, FontFamily::Proportional))
        .color(Palette::INK_2)
}

fn window_label(window: TimeWindow) -> &'static str {
    match window {
        TimeWindow::All => "All time",
        TimeWindow::Last30Min => "Last 30 min",
    }
}

/// The repo filter button's label: the active filter, or "All repos".
fn repo_label(state: &AppState) -> String {
    state
        .ui
        .activity
        .repo_filter
        .clone()
        .unwrap_or_else(|| "All repos".to_owned())
}

/// Cycle the repo filter: All repos → each registered root in registration
/// order → back to All repos.
fn cycle_repo_filter(state: &mut AppState) {
    let labels: Vec<String> = state
        .multi
        .roots
        .iter()
        .map(|r| turbogit_app::activity::root_label(&r.path))
        .collect();
    let current = state.ui.activity.repo_filter.clone();
    state.ui.activity.repo_filter = match current {
        None => labels.into_iter().next(),
        Some(cur) => labels
            .iter()
            .skip_while(|l| *l != &cur)
            .nth(1)
            .cloned()
            // Past the last root: back to all repos.
            .or(None),
    };
}
