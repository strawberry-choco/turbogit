//! The shared menu-item primitive: the row of a menu.
//!
//! One glyph slot, one label, an optional data-font segment, an optional
//! right-aligned shortcut column, and the rule that separates item groups.
//! It takes plain data plus `&mut Ui` and returns a `Response` — no
//! application state, no git, no policy — so any surface can build a menu
//! without acquiring one. Every value it paints comes from `crate::theme`.

use egui::{Color32, CornerRadius, Pos2, Rect, Response, Sense, Ui, Vec2, WidgetInfo, WidgetType};

use crate::theme::{
    CONTROL_RADIUS, GROUP_ROW_HEIGHT, Palette, TYPE_CHIP, TYPE_CONTROL, chrome_font, data_font,
    icon_color,
};
use crate::ui::components::middle_truncate_to_width;
use crate::ui::icons::{self, Icon};

use super::controls::{ButtonVariant, WidgetState, disabled_child_scope, focus_ring};

/// How strongly the item reads. `Primary` takes the brand fill the
/// `KitButton::Primary` treatment already gives the default action of a
/// menu; `Danger` keeps the row flat and inks it with the severity token.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuItemKind {
    Default,
    Primary,
    Danger,
}

/// One menu item's gate: whether it acts, and what it says when it cannot.
///
/// A widget-layer concept, not a branch one: every menu in the app answers one
/// of these per item, and [`menu_item`] paints it — the row stays rendered,
/// swallows its own click, and hands `reason` to the disabled hover text.
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

/// Plain data for one [`menu_item`] row — no application state.
pub struct MenuItemProps<'a> {
    /// The leading glyph, painted in the 14 px slot after the 8 px gutter.
    pub icon: Icon,
    /// The item's prose label, in the chrome face.
    pub label: &'a str,
    /// An optional second segment rendered in the data face (a branch name,
    /// a path) so a name never reads as a label.
    pub data: Option<&'a str>,
    /// An optional right-aligned accelerator column.
    pub shortcut: Option<&'a str>,
    /// A disabled item stays rendered and swallows its own click.
    pub enabled: bool,
    /// Why a disabled item is blocked, bound as the disabled hover text.
    pub disabled_reason: Option<&'a str>,
    pub kind: MenuItemKind,
}

/// Paint one menu row and report its `Response`.
///
/// A disabled row is not a different widget: the same painter runs inside a
/// [`disabled_child_scope`], so the row stays laid out and rendered, dims its
/// own ink to 45 %, swallows its click, and hands the reason to
/// `on_disabled_hover_text` — the gating convention of `branch_widget.rs`.
pub fn menu_item(ui: &mut Ui, props: MenuItemProps<'_>) -> Response {
    if props.enabled {
        return menu_item_row(ui, &props);
    }
    disabled_child_scope(ui, false, |child| menu_item_row(child, &props))
}

fn menu_item_row(ui: &mut Ui, props: &MenuItemProps<'_>) -> Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(width, GROUP_ROW_HEIGHT),
        if props.enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );

    // The row-state ladder: transparent at rest, SURFACE_2 on hover,
    // SURFACE_3 while held; the primary item instead keeps the shared
    // brand-button fill decision. A disabled row never changes fill.
    let state = if !props.enabled {
        WidgetState::Disabled
    } else if response.is_pointer_button_down_on() {
        WidgetState::Active
    } else if response.hovered() {
        WidgetState::Hovered
    } else {
        WidgetState::Idle
    };
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
    if !props.enabled {
        label_ink = label_ink.gamma_multiply(0.45);
        glyph_ink = glyph_ink.gamma_multiply(0.45);
    }

    // Glyph: 8 px gutter, then a 14 px slot, painted without registering a
    // widget so the row keeps its own clicks.
    let slot = Rect::from_min_size(
        Pos2::new(rect.left() + 8.0, rect.center().y - 7.0),
        Vec2::splat(14.0),
    );
    icons::paint_icon(ui.painter(), slot.min, 14.0, props.icon, glyph_ink);

    // The shortcut column is measured and painted first: it is right-aligned
    // at the 8 px inset and never moves to make content fit.
    let mut content_right = rect.right() - 8.0;
    if let Some(shortcut) = props.shortcut {
        let shortcut_ink = if props.enabled {
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

    // Label (chrome) plus an optional data segment (the data face at the same
    // size). When the row is too narrow the content middle-truncates — the
    // data segment first, the label when it is alone — before the shortcut
    // would move.
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

    let response = if let Some(reason) = props.disabled_reason {
        response.on_disabled_hover_text(reason)
    } else {
        response
    };
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, props.enabled, props.label));
    focus_ring(ui, &response);
    response
}

/// Paint the 1 px rule that separates one group of menu items from the
/// next, with 4 px of air either side.
///
/// This is deliberately not `ui.separator()`: that paints egui's stock
/// `noninteractive.bg_stroke` (`#3C3C3C`), which `configure_style` never
/// assigns. And the content-divider role is no help either — `RULE_CONTENT`
/// aliases the raised surface (`#2B2D30`), exactly the menu's own fill, so a
/// rule using it is invisible inside a menu. The structural
/// hairline (`LINE_SUBTLE`) is the only token that reads on the menu
/// surface; a dedicated `MENU_RULE` role is follow-up work.
///
/// The 4 px of air is stated inside the row flow: a menu composes its items
/// flush (the caller's `item_spacing.y` is 0), so the painted gap is exactly
/// 4 px on either side.
pub fn menu_rule(ui: &mut Ui) {
    ui.add_space(4.0);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 1.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, Palette::RULE_STRUCTURAL);
    ui.add_space(4.0);
}

/// The menu's surface frame: the popup chrome `configure_style` already maps
/// the tokens onto — `SURFACE` fill, `LINE` border, `MENU_RADIUS` corners —
/// owned in one place so no call site restates them.
pub fn menu_surface(ui: &Ui) -> egui::Frame {
    egui::Frame::popup(ui.style())
}
