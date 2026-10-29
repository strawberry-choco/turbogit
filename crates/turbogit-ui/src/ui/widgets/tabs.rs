use std::sync::Arc;

use egui::{
    Align, Color32, CornerRadius, Galley, Layout, Pos2, Rect, Sense, Ui, UiBuilder, Vec2,
    WidgetInfo, WidgetType,
};

use crate::theme::{Palette, TYPE_CONTROL, chrome_font};
use crate::ui::icons::{self, Icon};

use super::{Edge, edge_rule, focus_ring, tint_over_bg};

pub const TAB_STRIP_HEIGHT: f32 = 32.0;
pub const TAB_ITEM_HEIGHT: f32 = 31.0;

const LEAD_INSET: f32 = 4.0;
const ITEM_LEAD: f32 = 24.0;
const ICON_LEAD: f32 = 12.0;
const ICON_LABEL_GAP: f32 = 6.0;
const TAB_ADVANCE: f32 = 2.0;
const END_INSET: f32 = 8.0;
const TAB_ICON_SIZE: f32 = 14.0;

const TAB_SELECTION_RULE: f32 = crate::theme::RAIL_WIDTH;

const ACTIVE_TAB_INK: Color32 = Palette::INK;
const INACTIVE_TAB_INK: Color32 = Palette::INK_3;

#[derive(Clone, Copy)]
pub struct TabItem<'a, T> {
    pub id: T,
    pub title: &'a str,
    pub icon: Option<Icon>,
    pub end_icon: Option<Icon>,
    pub badge: Option<usize>,
}

/// Render the bar AND the padded, switched content region, in one call.
///
/// Claims the whole region of `ui` (`ui.max_rect()`), call it at the top of a
/// region, as both production call sites do. The bar is the top
/// [`TAB_STRIP_HEIGHT`] band of it, the content is the rest inset by
/// [`crate::theme::PANEL_PADDING`] on all sides; every tab content region pads,
/// so a nested `tab_view` sits 12 px inside its outer one by design. Advances
/// `ui`'s cursor past the bar plus the content's **used** height, not the
/// reserved rect: sections rendered after the body (a conflict editor, an error
/// line) must flow beneath the content, not beneath the region, or they clip
/// past the window's bottom edge.
///
/// The three `&mut C` users run strictly in order in one frame, per-click
/// `on_select` inside the tab loop, the end section after it, content last, so
/// one reborrow threads through all of them with no interior mutability. The
/// generic `C` is dependency honesty rather than a purity precedent (the widget
/// library already touches `turbogit_app` elsewhere); it keeps this widget's
/// own dependencies minimal.
///
/// `content` receives **no id** on purpose: `on_select` may have run earlier
/// this very frame, and the caller's fresh read of its own state is what makes
/// a click switch the body same-frame. A content parameter the caller must
/// ignore to avoid a one-frame lag is a trap, not a convenience.
pub fn tab_view<'a, T, C>(
    ui: &mut Ui,
    ctx: &mut C,
    tabs: &[TabItem<'a, T>],
    active: T,
    on_select: impl FnMut(&mut C, T),
    end_section: impl FnOnce(&mut Ui, &mut C),
    content: impl FnOnce(&mut Ui, &mut C),
) where
    T: Copy + PartialEq,
{
    let column = ui.max_rect();
    let strip = Rect::from_min_max(
        column.min,
        Pos2::new(column.max.x, column.min.y + TAB_STRIP_HEIGHT),
    );
    {
        let mut strip_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(strip)
                .layout(Layout::top_down(Align::Min)),
        );
        paint_strip(&mut strip_ui, ctx, tabs, active, on_select, end_section);
    }
    let inner = Rect::from_min_max(
        Pos2::new(
            column.min.x + crate::theme::PANEL_PADDING,
            strip.max.y + crate::theme::PANEL_PADDING,
        ),
        Pos2::new(
            column.max.x - crate::theme::PANEL_PADDING,
            column.max.y - crate::theme::PANEL_PADDING,
        ),
    );
    let mut content_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::top_down(Align::Min)),
    );
    content(&mut content_ui, ctx);
    let used = content_ui.min_size().y;
    ui.advance_cursor_after_rect(Rect::from_min_size(
        column.min,
        Vec2::new(
            column.width(),
            TAB_STRIP_HEIGHT + 2.0 * crate::theme::PANEL_PADDING + used,
        ),
    ));
}

fn paint_strip<'a, T, C>(
    ui: &mut Ui,
    ctx: &mut C,
    tabs: &[TabItem<'a, T>],
    active: T,
    mut on_select: impl FnMut(&mut C, T),
    end_section: impl FnOnce(&mut Ui, &mut C),
) where
    T: Copy + PartialEq,
{
    let width = ui.available_width();
    let (strip, _) = ui.allocate_exact_size(Vec2::new(width, TAB_STRIP_HEIGHT), Sense::hover());
    edge_rule(ui.painter(), strip, Edge::Bottom, Palette::LINE);

    let font = chrome_font(TYPE_CONTROL);
    let mut x = strip.left() + LEAD_INSET;
    for item in tabs {
        let label = match item.badge {
            Some(n) => format!("{} {n}", item.title),
            None => item.title.to_owned(),
        };
        let galley = ui
            .painter()
            .layout_no_wrap(label.clone(), font.clone(), Color32::WHITE);
        let icon_w = match item.icon {
            Some(_) => TAB_ICON_SIZE + ICON_LABEL_GAP,
            None => 0.0,
        };
        let end_w = match item.end_icon {
            Some(_) => ICON_LABEL_GAP + TAB_ICON_SIZE,
            None => 0.0,
        };
        let rect = Rect::from_min_size(
            Pos2::new(x, strip.top()),
            Vec2::new(
                ITEM_LEAD + icon_w + galley.size().x + end_w,
                TAB_ITEM_HEIGHT,
            ),
        );
        if tab_item(ui, rect, item, active, &label, galley) {
            on_select(ctx, item.id);
        }
        x += rect.width() + TAB_ADVANCE;
    }

    let end_right = strip.right() - END_INSET;
    let end_rect = Rect::from_min_max(
        Pos2::new(x.min(end_right), strip.top()),
        Pos2::new(end_right, strip.bottom()),
    );
    let mut end_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(end_rect)
            .layout(Layout::right_to_left(Align::Center)),
    );
    end_section(&mut end_ui, ctx);
}

fn tab_item<T: Copy + PartialEq>(
    ui: &mut Ui,
    rect: Rect,
    item: &TabItem<'_, T>,
    active: T,
    label: &str,
    galley: Arc<Galley>,
) -> bool {
    let id = ui.auto_id_with(("tab_view", item.title));
    let response = ui.interact(rect, id, Sense::click());
    let active = active == item.id;
    let painter = ui.painter().clone();
    let band = Rect::from_min_max(
        Pos2::new(rect.left(), rect.top()),
        Pos2::new(rect.right(), rect.top() + TAB_STRIP_HEIGHT),
    );
    if active {
        let rule = Rect::from_min_max(
            Pos2::new(band.left(), band.bottom() - TAB_SELECTION_RULE),
            Pos2::new(band.right(), band.bottom()),
        );
        painter.rect_filled(rule, CornerRadius::ZERO, Palette::BRAND);
    } else if response.hovered() {
        painter.rect_filled(band, CornerRadius::ZERO, tint_over_bg(Palette::INK, 0.08));
    }

    let ink = if active {
        ACTIVE_TAB_INK
    } else {
        INACTIVE_TAB_INK
    };
    let cy = rect.center().y;
    let mut cx = rect.left() + ICON_LEAD;
    if let Some(icon) = item.icon {
        icons::centered_icon(
            ui,
            icon,
            Pos2::new(cx + TAB_ICON_SIZE / 2.0, cy),
            TAB_ICON_SIZE,
            ink,
        );
        cx += TAB_ICON_SIZE + ICON_LABEL_GAP;
    }
    let galley_w = galley.size().x;
    painter.galley_with_override_text_color(Pos2::new(cx, cy - galley.size().y / 2.0), galley, ink);
    if let Some(end_icon) = item.end_icon {
        icons::centered_icon(
            ui,
            end_icon,
            Pos2::new(cx + galley_w + ICON_LABEL_GAP + TAB_ICON_SIZE / 2.0, cy),
            TAB_ICON_SIZE,
            ink,
        );
    }

    focus_ring(ui, &response);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    response.clicked()
}
