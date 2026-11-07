use egui::{
    Color32, CornerRadius, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo,
    WidgetType,
};

use crate::theme::{
    CONTROL_RADIUS, GROUP_ROW_HEIGHT, Palette, TYPE_CHIP, TYPE_CONTROL, chrome_font, data_font,
    icon_color,
};
use crate::ui::components::middle_truncate_to_width;
use crate::ui::icons::{self, Icon};

use super::controls::{ButtonVariant, WidgetState, disabled_child_scope, focus_ring};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuItemKind {
    Default,
    Primary,
    Danger,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuItemState {
    pub enabled: bool,
    pub reason: Option<&'static str>,
}

impl MenuItemState {
    pub const fn enabled() -> Self {
        Self {
            enabled: true,
            reason: None,
        }
    }

    pub const fn disabled(reason: &'static str) -> Self {
        Self {
            enabled: false,
            reason: Some(reason),
        }
    }
}

pub struct MenuItemProps<'a> {
    pub icon: Icon,
    pub label: &'a str,
    pub data: Option<&'a str>,
    pub shortcut: Option<&'a str>,
    pub state: MenuItemState,
    pub kind: MenuItemKind,
}

pub fn menu_item(ui: &mut Ui, props: MenuItemProps<'_>) -> Response {
    if props.state.enabled {
        return menu_item_row(ui, &props);
    }
    disabled_child_scope(ui, false, |child| menu_item_row(child, &props))
}

fn menu_item_row(ui: &mut Ui, props: &MenuItemProps<'_>) -> Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(width, GROUP_ROW_HEIGHT),
        if props.state.enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );

    let state = WidgetState::from_response(props.state.enabled, &response);
    let fill = match props.kind {
        MenuItemKind::Primary => ButtonVariant::Primary.fill(state),
        _ => match state {
            WidgetState::Active => Palette::SURFACE_3,
            WidgetState::Hovered => Palette::SURFACE_2,
            _ => Color32::TRANSPARENT,
        },
    };
    if fill != Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(CONTROL_RADIUS), fill);
    }

    let (mut label_ink, mut glyph_ink) = match props.kind {
        MenuItemKind::Primary => (Palette::BRAND_INK, Palette::BRAND_INK),
        MenuItemKind::Danger => (Palette::DANGER, Palette::DANGER),
        MenuItemKind::Default => (Palette::INK, icon_color()),
    };
    if !props.state.enabled {
        label_ink = label_ink.gamma_multiply(0.45);
        glyph_ink = glyph_ink.gamma_multiply(0.45);
    }

    let slot = Rect::from_min_size(
        Pos2::new(rect.left() + 8.0, rect.center().y - 7.0),
        Vec2::splat(14.0),
    );
    icons::paint_icon(ui.painter(), slot.min, 14.0, props.icon, glyph_ink);

    let mut content_right = rect.right() - 8.0;
    if let Some(shortcut) = props.shortcut {
        let shortcut_ink = if props.state.enabled {
            Palette::T_MUTED
        } else {
            Palette::T_MUTED.gamma_multiply(0.45)
        };
        let galley =
            ui.painter()
                .layout_no_wrap(shortcut.to_owned(), data_font(TYPE_CHIP), shortcut_ink);
        let width = galley.size().x;
        ui.painter().galley(
            Pos2::new(
                content_right - width,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            Color32::WHITE,
        );
        content_right -= 8.0 + width;
    }

    let content_x = slot.right() + 8.0;
    let budget = (content_right - content_x).max(0.0);
    let label_font = chrome_font(TYPE_CONTROL);
    let data_font_id = data_font(TYPE_CONTROL);
    let (label_galley, data_galley) = match props.data {
        Some(data) => {
            let label =
                ui.painter()
                    .layout_no_wrap(props.label.to_owned(), label_font.clone(), label_ink);
            let data_budget = (budget - label.size().x - 6.0).max(0.0);
            let fitted = middle_truncate_to_width(ui, data, &data_font_id, data_budget);
            let data = ui.painter().layout_no_wrap(fitted, data_font_id, label_ink);
            (label, Some(data))
        }
        None => {
            let fitted = middle_truncate_to_width(ui, props.label, &label_font, budget);
            (
                ui.painter().layout_no_wrap(fitted, label_font, label_ink),
                None,
            )
        }
    };
    ui.painter().galley(
        Pos2::new(content_x, rect.center().y - label_galley.size().y / 2.0),
        label_galley.clone(),
        Color32::WHITE,
    );
    if let Some(data) = data_galley {
        ui.painter().galley(
            Pos2::new(
                content_x + label_galley.size().x + 6.0,
                rect.center().y - data.size().y / 2.0,
            ),
            data,
            Color32::WHITE,
        );
    }

    // A disabled row states ONE reason, in two places: the row's own hover text,
    // and the shell's status bar, which is where the reference documents it
    // ("hover over the action in the context menu and look for the message in the
    // status bar"), and the reason this primitive raises the text into a slot the
    // shell reads rather than keeping it inside the menu. See
    // [`take_status_hint`] for the channel and its one-frame latency.
    //
    // The pointer test is the ROW'S RECT, not `response.hovered()`: a blocked row
    // is painted inside a `disabled_child_scope`, and a `Ui` that is disabled
    // reports no hover for its widgets, which is exactly why egui needs
    // `on_disabled_hover_text` for the tooltip at all. Asking the rect is the one
    // question that has the same answer for a live row and a blocked one.
    let response = match props.state.reason {
        Some(reason) => {
            if ui.rect_contains_pointer(rect) {
                ui.ctx().data_mut(|d| {
                    d.insert_temp(status_hint_id(), reason.to_owned());
                });
                // The reason is painted by the NEXT frame, the shell lays the
                // bottom panel out before the tool window that hosts this menu,
                // so the frame has to be had.
                ui.ctx().request_repaint();
            }
            response.on_disabled_hover_text(reason)
        }
        None => response,
    };
    response
        .widget_info(|| WidgetInfo::labeled(WidgetType::Button, props.state.enabled, props.label));
    focus_ring(ui, &response);
    response
}

fn status_hint_id() -> egui::Id {
    egui::Id::new("tg_status_bar_hint")
}

/// Take the reason a blocked menu row raised, leaving the slot empty.
///
/// **Take, not peek**, and that is the whole behaviour: the status bar reports
/// what the pointer is on NOW, so moving off a blocked row goes silent one frame
/// later instead of stranding the last reason on the line. Called once per frame
/// by [`crate::ui::shell::render`] before the bottom panel is laid out, whether
/// or not the status bar is showing, with the region hidden the reason is the
/// row's hover text alone, exactly as the reference documents for a user who has
/// not enabled a status text region.
pub fn take_status_hint(ctx: &egui::Context) -> Option<String> {
    ctx.data_mut(|d| d.remove_temp::<String>(status_hint_id()))
}

pub fn menu_rule(ui: &mut Ui) {
    ui.add_space(4.0);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 1.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, Palette::RULE_STRUCTURAL);
    ui.add_space(4.0);
}

const CHECK_BOX_SIZE: f32 = 14.0;

/// A menu-surface checkbox row. One hazard this shape is built around: composing
/// `menu_row` with `ui.checkbox` gives TWO nodes ONE label, and kittest's
/// `get_by_label` panics on two hits rather than arbitrating. This paints one node
/// and registers it `WidgetInfo::selected(Checkbox, ..)`, which is how a headless
/// test reads the tick state and the label from the same hit.
pub fn check_row(ui: &mut Ui, label: &str, checked: bool, swatch: Option<Color32>) -> Response {
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, GROUP_ROW_HEIGHT), Sense::click());

    // A ticked row is not a selected row. `menu_row` fills on `active` because active IS
    // its whole state; here the box and its tick already carry the state, and borrowing
    // `Palette::SELECTION` would make every checked repository look like the pane's one
    // selected row at the same time.
    let fill = if response.hovered() {
        Palette::SURFACE_2
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(CONTROL_RADIUS), fill);
    }

    let box_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 8.0, rect.center().y - CHECK_BOX_SIZE / 2.0),
        Vec2::splat(CHECK_BOX_SIZE),
    );
    let radius = CornerRadius::same(crate::theme::CHIP_RADIUS);
    ui.painter()
        .rect_filled(box_rect, radius, Palette::RAISED_ON_CARD);
    let border = if checked || response.hovered() {
        Palette::INK
    } else {
        Palette::INK_2
    };
    ui.painter().rect_stroke(
        box_rect,
        radius,
        Stroke::new(1.0, border),
        StrokeKind::Inside,
    );
    if checked {
        let check = Stroke::new(1.6, Palette::BRAND_INK);
        let p1 = Pos2::new(box_rect.left() + 3.5, box_rect.center().y - 1.0);
        let p2 = Pos2::new(box_rect.left() + 6.0, box_rect.center().y + 2.0);
        let p3 = Pos2::new(box_rect.left() + 11.5, box_rect.center().y - 3.5);
        ui.painter().line_segment([p1, p2], check);
        ui.painter().line_segment([p2, p3], check);
    }

    // The tick box and the row tint carry the state; the label keeps the shared
    // row grammar's selection-independent identity ink.
    let ink = Palette::INK;
    let mut text_x = box_rect.right() + 8.0;
    if let Some(color) = swatch {
        ui.painter()
            .circle_filled(Pos2::new(text_x + 4.0, rect.center().y), 3.5, color);
        text_x += 14.0;
    }
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), chrome_font(TYPE_CONTROL), ink);
    ui.painter().galley(
        Pos2::new(text_x, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );

    response.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, checked, label));
    focus_ring(ui, &response);
    response
}

pub fn menu_surface(ui: &Ui) -> egui::Frame {
    egui::Frame::popup(ui.style())
}
