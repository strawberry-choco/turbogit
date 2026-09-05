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
    Align, Color32, CornerRadius, FontFamily, FontId, Layout, Pos2, RichText, ScrollArea, Sense,
    Stroke, Ui, Vec2, WidgetInfo, WidgetType,
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
        render_collapse_toggle(ui, state);
        ui.label(
            RichText::new("ACTIVITY")
                .strong()
                .font(FontId::new(HEADER_TEXT, FontFamily::Proportional))
                .color(Palette::INK_3),
        );
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

/// The chevron toggle. Its accessibility label names the action it performs
/// ("Collapse activity" / "Expand activity") so it is queryable headlessly.
fn render_collapse_toggle(ui: &mut Ui, state: &mut AppState) {
    let expanded = state.ui.activity.expanded;
    let (label, icon) = if expanded {
        ("Collapse activity", Icon::CHEVRON_DOWN)
    } else {
        ("Expand activity", Icon::CHEVRON_RIGHT)
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    if response.clicked() {
        state.ui.activity.expanded = !expanded;
    }
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(3), Palette::SURFACE_2);
    }
    widgets::focus_ring(ui, &response);
    icons::icon(ui, icon, 12.0, Palette::INK_2);
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
