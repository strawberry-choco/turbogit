use egui::{
    Align, Color32, CornerRadius, Layout, Pos2, Rect, Response, Sense, Stroke, StrokeKind,
    TextStyle, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType,
};

use crate::theme::{CHIP_RADIUS, CONTROL_RADIUS, Palette, TYPE_BODY, chrome_font};
use crate::ui::icons::{self, Icon};

const BUTTON_HEIGHT: f32 = 32.0;
const COMPACT_BUTTON_HEIGHT: f32 = 28.0;
pub const ICON_BUTTON_SIZE: f32 = 28.0;
const BUTTON_ICON_SIZE: f32 = 16.0;
const ICON_LABEL_GAP: f32 = 6.0;

const _: () = {
    assert!(
        COMPACT_BUTTON_HEIGHT == super::containers::PANE_HEADER_HEIGHT,
        "the compact primary is sized for the pane-header band, so the two \
         numbers are one decision"
    )
};

/// Linear blend of two opaque colors; `t` is the amount of `b` mixed into `a`.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let ch = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(ch(a.r(), b.r()), ch(a.g(), b.g()), ch(a.b(), b.b()))
}

pub fn tint_over_bg(accent: Color32, t: f32) -> Color32 {
    mix(Palette::BG, accent, t)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WidgetState {
    Idle,
    Hovered,
    Active,
    Disabled,
}

impl WidgetState {
    pub fn from_response(enabled: bool, response: &Response) -> Self {
        if !enabled {
            Self::Disabled
        } else if response.is_pointer_button_down_on() {
            Self::Active
        } else if response.hovered() {
            Self::Hovered
        } else {
            Self::Idle
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ButtonVariant {
    Ghost,
    Primary,
    Compact,
    Icon,
    CompactPrimary,
}

impl ButtonVariant {
    pub fn fill(self, state: WidgetState) -> Color32 {
        use WidgetState::{Active, Disabled, Hovered, Idle};
        match (self, state) {
            (Self::Primary | Self::CompactPrimary, Idle | Disabled) => Palette::BRAND,
            (Self::Primary | Self::CompactPrimary, Hovered) => {
                mix(Palette::BRAND, Color32::WHITE, 0.10)
            }
            (Self::Primary | Self::CompactPrimary, Active) => {
                mix(Palette::BRAND, Color32::WHITE, 0.20)
            }
            (_, Idle | Disabled) => Color32::TRANSPARENT,
            (_, Hovered) => Palette::SURFACE_2,
            (_, Active) => Palette::SURFACE_3,
        }
    }

    pub fn text(self, state: WidgetState) -> Color32 {
        use WidgetState::{Active, Disabled, Hovered, Idle};
        match (self, state) {
            (_, Disabled) => Palette::INK_3,
            (Self::Primary | Self::CompactPrimary, _) => Palette::BRAND_INK,
            (_, Idle) => Palette::INK_2,
            (_, Hovered | Active) => Palette::INK,
        }
    }
}

pub fn focus_ring(ui: &Ui, response: &Response) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect.expand(1.0),
            CornerRadius::same(CONTROL_RADIUS),
            Stroke::new(1.0, Palette::BRAND),
            StrokeKind::Outside,
        );
    }
}

/// Run `contents` in a child scope, disabling that child when `enabled` is
/// false, then advance the caller's cursor past whatever the child consumed.
///
/// The point of the child scope is containment: a disabled control dims itself
/// and turns its clicks into no-ops without leaking `disabled` styling into the
/// widgets beside it. Layout, styling, and the caller's cursor are otherwise
/// untouched, and the closure runs identically whether the control is enabled
/// or disabled.
///
/// Whether a given control *delegates at all* is the caller's decision, and the
/// enabled-gated buttons make different ones: [`action_button`] always builds
/// and paints the control, while [`compact_button_enabled`] and the commit
/// window's enabled-gated buttons short-circuit on the enabled path and
/// delegate only the disabled one. Both shapes share this scope.
pub fn disabled_child_scope<R>(
    ui: &mut Ui,
    enabled: bool,
    contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(ui.available_rect_before_wrap())
            .layout(*ui.layout()),
    );
    if !enabled {
        child.disable();
    }
    let out = contents(&mut child);
    ui.advance_cursor_after_rect(child.min_rect());
    out
}

pub fn ghost_button(ui: &mut Ui, icon: Option<Icon>, label: &str) -> Response {
    button_response(ui, ButtonVariant::Ghost, icon, Some(label))
}

pub fn primary_button(ui: &mut Ui, icon: Option<Icon>, label: &str) -> Response {
    button_response(ui, ButtonVariant::Primary, icon, Some(label))
}

pub fn compact_primary_button(ui: &mut Ui, label: &str) -> Response {
    button_response(ui, ButtonVariant::CompactPrimary, None, Some(label))
}

pub fn compact_button(ui: &mut Ui, label: &str) -> Response {
    button_response(ui, ButtonVariant::Compact, None, Some(label))
}

pub fn button_enabled(ui: &mut Ui, enabled: bool, build: impl Fn(&mut Ui) -> Response) -> Response {
    if enabled {
        return build(ui);
    }
    disabled_child_scope(ui, enabled, |child| build(child))
}

pub fn compact_button_enabled(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    button_enabled(ui, enabled, |ui| compact_button(ui, label))
}

pub fn icon_button(ui: &mut Ui, icon: Icon) -> Response {
    button_response(ui, ButtonVariant::Icon, Some(icon), None)
}

pub fn icon_button_enabled(ui: &mut Ui, icon: Icon, enabled: bool) -> Response {
    button_enabled(ui, enabled, |ui| icon_button(ui, icon))
}

pub fn ghost_icon_button(
    ui: &mut Ui,
    rect: Rect,
    id: egui::Id,
    label: &str,
    enabled: bool,
    paint_content: impl FnOnce(&mut Ui, Rect, WidgetState),
) -> Response {
    let response = ui.interact(
        rect,
        id,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );

    let state = WidgetState::from_response(enabled, &response);

    let painter = ui.painter().clone();
    let fill = ButtonVariant::Ghost.fill(state);
    if fill != Color32::TRANSPARENT {
        painter.rect_filled(rect, CornerRadius::same(CONTROL_RADIUS), fill);
    }
    paint_content(ui, rect, state);

    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    focus_ring(ui, &response);
    response
}

pub fn action_button(ui: &mut Ui, label: &str, primary: bool, enabled: bool) -> Response {
    let variant = if primary {
        ButtonVariant::Primary
    } else {
        ButtonVariant::Ghost
    };
    disabled_child_scope(ui, enabled, |child| {
        let width = child.available_width();
        button_response_sized(
            child,
            variant,
            None,
            Some(label),
            None,
            None,
            None,
            Some(width),
        )
    })
}

fn button_response(
    ui: &mut Ui,
    variant: ButtonVariant,
    icon: Option<Icon>,
    label: Option<&str>,
) -> Response {
    button_response_sized(ui, variant, icon, label, None, None, None, None)
}

#[allow(clippy::too_many_arguments)]
fn button_response_sized(
    ui: &mut Ui,
    variant: ButtonVariant,
    icon: Option<Icon>,
    label: Option<&str>,
    height_override: Option<f32>,
    pad_x_override: Option<f32>,
    icon_size_override: Option<f32>,
    width_override: Option<f32>,
) -> Response {
    let enabled = ui.is_enabled();
    let compact = matches!(
        variant,
        ButtonVariant::Compact | ButtonVariant::CompactPrimary
    );
    let icon_only = icon.is_some() && label.is_none();

    let text_style = if compact {
        TextStyle::Small
    } else {
        TextStyle::Button
    };
    let font_id = ui
        .style()
        .text_styles
        .get(&text_style)
        .cloned()
        .unwrap_or_else(|| crate::theme::chrome_font(crate::theme::TYPE_CONTROL));

    let pad_x = pad_x_override.unwrap_or(match variant {
        ButtonVariant::Compact | ButtonVariant::CompactPrimary => 12.0,
        ButtonVariant::Icon => 6.0,
        _ => ui.style().spacing.button_padding.x,
    });
    let height = height_override.unwrap_or(match variant {
        ButtonVariant::Icon => ICON_BUTTON_SIZE,
        ButtonVariant::Compact | ButtonVariant::CompactPrimary => COMPACT_BUTTON_HEIGHT,
        _ => BUTTON_HEIGHT,
    });
    let icon_size = icon_size_override.unwrap_or(BUTTON_ICON_SIZE);

    let galley = label.map(|l| {
        ui.painter()
            .layout_no_wrap(l.to_owned(), font_id.clone(), Color32::WHITE)
    });
    let label_w = galley.as_ref().map_or(0.0, |g| g.size().x);
    let icon_w = if icon.is_some() { icon_size } else { 0.0 };
    let gap = if icon.is_some() && label.is_some() {
        ICON_LABEL_GAP
    } else {
        0.0
    };
    let content_w = icon_w + gap + label_w;
    let width = width_override.unwrap_or(if icon_only {
        height
    } else {
        pad_x * 2.0 + content_w
    });

    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(width, height),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );

    let state = WidgetState::from_response(enabled, &response);

    let painter = ui.painter().clone();
    let radius = CornerRadius::same(CONTROL_RADIUS);
    let fill = variant.fill(state);
    if fill != Color32::TRANSPARENT {
        painter.rect_filled(rect, radius, fill);
    }
    if response.has_focus() {
        painter.rect_stroke(
            rect.expand(1.0),
            radius,
            Stroke::new(1.0, Palette::BRAND),
            StrokeKind::Outside,
        );
    }

    let ink = variant.text(state);
    let cy = rect.center().y;
    let mut x = rect.left() + (rect.width() - content_w) / 2.0;
    if let Some(ic) = icon {
        paint_icon_at(ui, ic, Pos2::new(x, cy - icon_size / 2.0), icon_size, ink);
        if !icon_only {
            x += icon_size + ICON_LABEL_GAP;
        }
    }
    if let Some(g) = galley {
        painter.galley_with_override_text_color(Pos2::new(x, cy - g.size().y / 2.0), g, ink);
    }

    response.widget_info(|| {
        WidgetInfo::labeled(
            WidgetType::Button,
            enabled,
            label.or_else(|| icon.map(|i| i.name())).unwrap_or_default(),
        )
    });
    response
}

fn paint_icon_at(ui: &mut Ui, icon: Icon, origin: Pos2, size: f32, color: Color32) {
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_size(origin, Vec2::splat(size)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    icons::icon(&mut child, icon, size, color);
}

pub fn segmented_control(ui: &mut Ui, options: &[&str], selected: usize) -> Option<usize> {
    const SEGMENT_H: f32 = 24.0;
    let pad_x = crate::theme::BUTTON_PADDING.x;
    let font_id = chrome_font(TYPE_BODY);

    let widths: Vec<f32> = options
        .iter()
        .map(|o| {
            let g = ui
                .painter()
                .layout_no_wrap((*o).to_owned(), font_id.clone(), Color32::WHITE);
            g.size().x + pad_x * 2.0
        })
        .collect();
    let track_w: f32 = widths.iter().sum();

    let (track, _) = ui.allocate_exact_size(Vec2::new(track_w, SEGMENT_H), Sense::hover());
    ui.painter().rect_filled(
        track,
        CornerRadius::same(CONTROL_RADIUS),
        Palette::SURFACE_2,
    );

    let mut clicked = None;
    let mut x = track.left();
    for (i, option) in options.iter().enumerate() {
        let seg = Rect::from_min_size(Pos2::new(x, track.top()), Vec2::new(widths[i], SEGMENT_H));
        let id = ui.id().with(("segment", i));
        let resp = ui.interact(seg, id, Sense::click());
        let is_selected = i == selected;
        if is_selected {
            ui.painter()
                .rect_filled(seg, CornerRadius::same(CHIP_RADIUS), Palette::SURFACE_3);
        }
        let ink = if is_selected || resp.hovered() {
            Palette::INK
        } else {
            Palette::INK_2
        };
        super::paint_centered_text(ui.painter(), seg, option, font_id.clone(), ink);
        resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, *option));
        focus_ring(ui, &resp);
        if resp.clicked() {
            clicked = Some(i);
        }
        x += widths[i];
    }
    clicked
}
