use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike};
use egui::{Color32, CornerRadius, Id, Pos2, Rect, Sense, Ui, Vec2, WidgetInfo, WidgetType};
use egui_extras::DatePickerButton;
use jiff::civil::Date;

use crate::theme::{CONTROL_RADIUS, GROUP_ROW_HEIGHT, Palette, TYPE_CONTROL, chrome_font};
use crate::ui::widgets::focus_ring;

// Calendar-local geometry. The stepper width is screen-local.
const STEPPER_WIDTH: f32 = 34.0;
const CELL_GAP: f32 = 2.0;

/// A date button and time steppers for one local wall-clock instant.
/// Returns the picked value, or `None` when the popup is only browsed or dismissed.
pub fn instant_picker(ui: &mut Ui, id: Id, current: NaiveDateTime) -> Option<NaiveDateTime> {
    let mut selection = civil_date(current.date());
    let mut picked = None;
    ui.push_id(id, |ui| {
        let response = ui.add(
            DatePickerButton::new(&mut selection)
                .show_icon(false)
                .format("%Y-%m-%d")
                .highlight_weekends(false)
                .combo_boxes(false)
                .calendar_week(false),
        );
        if response.changed() {
            picked = naive_date(selection).map(|date| date.and_time(current.time()));
        }
        if let Some(time) = time_row(ui, current) {
            picked = Some(time);
        }
    });
    picked
}

fn civil_date(date: NaiveDate) -> Date {
    Date::new(date.year() as i16, date.month() as i8, date.day() as i8)
        .unwrap_or_else(|_| Date::new(1970, 1, 1).expect("the epoch is a valid date"))
}

fn naive_date(date: Date) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(
        i32::from(date.year()),
        date.month() as u32,
        date.day() as u32,
    )
}

fn time_row(ui: &mut Ui, current: NaiveDateTime) -> Option<NaiveDateTime> {
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = CELL_GAP;
        let label_w = (ui.available_width() - 4.0 * STEPPER_WIDTH - 4.0 * CELL_GAP).max(0.0);
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(label_w, GROUP_ROW_HEIGHT), Sense::hover());
        center_text(
            ui,
            rect,
            &format!("{:02}:{:02}", current.hour(), current.minute()),
            Palette::INK,
        );
        for (label, text, h, m) in [
            ("Hour minus", "-1h", -1i32, 0i32),
            ("Hour plus", "+1h", 1, 0),
            ("Minute minus", "-1m", 0, -1),
            ("Minute plus", "+1m", 0, 1),
        ] {
            if stepper(ui, label, text, STEPPER_WIDTH).clicked() {
                picked = Some(
                    current
                        .with_hour(((current.hour() as i32 + 24 + h) % 24) as u32)
                        .unwrap_or(current)
                        .with_minute(((current.minute() as i32 + 60 + m) % 60) as u32)
                        .unwrap_or(current),
                );
            }
        }
    });
    picked
}

fn stepper(ui: &mut Ui, label: &str, text: &str, width: f32) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, GROUP_ROW_HEIGHT), Sense::click());
    let fill = if response.hovered() {
        Palette::SURFACE_2
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(CONTROL_RADIUS), fill);
    }
    center_text(ui, rect, text, Palette::INK);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    focus_ring(ui, &response);
    response
}

fn center_text(ui: &Ui, rect: Rect, text: &str, ink: Color32) {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), chrome_font(TYPE_CONTROL), ink);
    ui.painter().galley(
        Pos2::new(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        ink,
    );
}
