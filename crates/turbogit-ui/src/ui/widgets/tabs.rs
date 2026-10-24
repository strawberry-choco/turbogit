//! One tab bar and its padded, switched content region: [`tab_view`].
//!
//! The bar is a **top band, not a hanging tab row**. The strip is the content
//! area's topmost chrome, flush against the window edge, so it keeps bare
//! [`Palette::BG`] and gains **no** top hairline — a rule there would frame the
//! window inside itself — and the single bottom `LINE` edge carries all of the
//! chrome-from-content information.
//!
//! The active tab is a full-strip-height band anchored to that top edge, not a
//! floating pill: a pill needs surrounding air to read as a pill, and at a
//! window edge there is no air above it. It is marked by exactly two marks and
//! no others — the `BRAND` underline of `TAB_SELECTION_RULE` along
//! the band's bottom edge, overwriting the strip's `LINE` across the tab's own
//! width so the selection mark and the divider read as one edge, and a label
//! ink one ramp step brighter than every inactive tab's. A raised fill behind
//! the label is deliberately absent: it was a *second* selection vocabulary
//! beside the sidebar's, and the strip sits on `BG`, where raised is `SURFACE`,
//! not `SURFACE_2`.
//!
//! The inks are two ramp steps, and both are legal where they land: the bar's
//! own fill is [`Palette::BG`], on which `INK` measures 12.59:1 and `INK_3`
//! measures 5.02:1. The active tab is the *brightest* ink in the strip and the
//! inactive tabs the muted one: "which pane am I in" is a reading-order
//! question, and the ramp is the answer to that.

use std::sync::Arc;

use egui::{
    Align, Color32, CornerRadius, Galley, Layout, Pos2, Rect, Sense, Ui, UiBuilder, Vec2,
    WidgetInfo, WidgetType,
};

use crate::theme::{Palette, TYPE_CONTROL, chrome_font};
use crate::ui::icons::{self, Icon};

use super::{Edge, edge_rule, focus_ring, tint_over_bg};

/// Tab strip height, 32.0. The widget owns the band's geometry.
pub const TAB_STRIP_HEIGHT: f32 = 32.0;
/// Single tab item height, 31.0 — the click target; the active tab's underline
/// paints over the FULL strip below it, so the rule reaches the divider with no
/// unclaimed sliver.
pub const TAB_ITEM_HEIGHT: f32 = 31.0;

/// Widths and gaps of the tab row. Widget-internal geometry, mirroring the
/// `CHIP_GEOMETRY` precedent in `chips.rs`: these are this widget's body, not
/// design tokens (the token layer owns colours and the type ramp, not a tab's
/// leading inset).
const LEAD_INSET: f32 = 4.0;
/// First tab's left inset is [`LEAD_INSET`]; the rect leads its content by
/// [`ICON_LEAD`] and trails it by the same, which is [`ITEM_LEAD`] with an icon
/// and the trailing half without one.
const ITEM_LEAD: f32 = 24.0;
const ICON_LEAD: f32 = 12.0;
const ICON_LABEL_GAP: f32 = 6.0;
const TAB_ADVANCE: f32 = 2.0;
/// The end section's right inset from the strip edge.
const END_INSET: f32 = 8.0;
const TAB_ICON_SIZE: f32 = 14.0;

/// The active tab's accent underline, in px — **the rail width, read from the
/// token layer**, the same 2 px [`crate::theme::RAIL_WIDTH`] a selected row's
/// leading rail is. The two numbers are one number, so they are read from one
/// declaration: a local `2.0` here would be a second definition of the accent's
/// thickness that could drift from the row's, and the underline would quietly
/// stop matching the rail it is the horizontal twin of.
const TAB_SELECTION_RULE: f32 = crate::theme::RAIL_WIDTH;

const ACTIVE_TAB_INK: Color32 = Palette::INK;
const INACTIVE_TAB_INK: Color32 = Palette::INK_3;

/// One tab, as data. `T` is the caller's id enum; the bar compares ids against
/// `active` and hands nothing else back to the caller.
///
/// Titles are const vocabulary, so the lifetime lets callers declare const
/// tables (`TabItem` is `Copy` when `T` is). The dynamic part — the badge — is
/// data, rendered by the bar.
#[derive(Clone, Copy)]
pub struct TabItem<'a, T> {
    /// The caller's selection identity (`Tab`, `CommitSubTab`, …).
    pub id: T,
    /// The tab title. With a badge the bar paints `"{title} {n}"` — one
    /// spelling of how a badge reads, owned here.
    pub title: &'a str,
    /// Optional start icon, painted before the label in the tab's ink.
    pub icon: Option<Icon>,
    /// Optional end icon, painted after the label in the same ink.
    pub end_icon: Option<Icon>,
    /// Optional count badge, appended to the label. Data, not a consumer-side
    /// label hack.
    pub badge: Option<usize>,
}

/// Render the bar AND the padded, switched content region, in one call.
///
/// Claims the whole region of `ui` (`ui.max_rect()`) — call it at the top of a
/// region, as both production call sites do. The bar is the top
/// [`TAB_STRIP_HEIGHT`] band of it, the content is the rest inset by
/// [`crate::theme::PANEL_PADDING`] on all sides; every tab content region pads,
/// so a nested `tab_view` sits 12 px inside its outer one by design. Advances
/// `ui`'s cursor past the bar plus the content's **used** height, not the
/// reserved rect: sections rendered after the body (a conflict editor, an error
/// line) must flow beneath the content, not beneath the region, or they clip
/// past the window's bottom edge.
///
/// The three `&mut C` users run strictly in order in one frame — per-click
/// `on_select` inside the tab loop, the end section after it, content last — so
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

/// The bar: the `LINE` bottom edge rule, the tab loop, the end section.
///
/// The end region is measured from the strip's **right edge inward** and lays
/// out right-to-left: the first widget added lands at its right edge, so it
/// holds fixed-size controls — a wrapping label there re-runs the status-bar
/// bug a right-anchored cluster cannot survive (a wrapping label claims the
/// whole remaining width and anchors its galley at the rect's right edge, off
/// the window).
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
        // The badge rides the painted label AND the accessible label, so both
        // readings agree; the widget id stays on the bare title so ids are
        // stable as counts change.
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

/// One tab's paint + hit test. Pure over its inputs — the selection mutation
/// stays with the caller via the returned bool.
///
/// The band is the tab's column over the *whole* strip (`rect.top()` is the
/// strip's top edge — the caller places it there), anchored to the window frame
/// rather than floating inside the band. Hover is a pointer state, not a
/// selection: a quiet 8% wash on `BG`, so a hovered-but-inactive tab never
/// reads as the selected one — it has no underline and its label stays at the
/// muted step.
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
