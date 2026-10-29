use egui::{Color32, CornerRadius, Pos2, Rect, Response, Sense, Ui, Vec2, WidgetInfo, WidgetType};

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

pub fn menu_surface(ui: &Ui) -> egui::Frame {
    egui::Frame::popup(ui.style())
}
