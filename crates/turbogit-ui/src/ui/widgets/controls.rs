//! Interactive control states, shared buttons, focus, and segmented choices.

use egui::{
    Align, Color32, CornerRadius, Layout, Pos2, Rect, Response, Sense, Stroke, StrokeKind,
    TextStyle, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType,
};

use crate::theme::{CHIP_RADIUS, CONTROL_RADIUS, Palette, TYPE_BODY, chrome_font};
use crate::ui::icons::{self, Icon};

const BUTTON_HEIGHT: f32 = 32.0; // .tg-btn
const COMPACT_BUTTON_HEIGHT: f32 = 28.0; // h-7 compact variants
const ICON_BUTTON_SIZE: f32 = 28.0; // square ghost (dialog close X)
const BUTTON_ICON_SIZE: f32 = 16.0; // §5.3: 16×16 in buttons

// --- Token-derived color math ----------------------------------------------

/// Linear blend of two opaque colors; `t` is the amount of `b` mixed into `a`.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let ch = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(ch(a.r(), b.r()), ch(a.g(), b.g()), ch(a.b(), b.b()))
}

/// Opaque tint of `accent` over the app background (spec §2.4 derived colors).
///
/// Blending over `BG` keeps chips deterministic and legible on panel fills.
pub fn tint_over_bg(accent: Color32, t: f32) -> Color32 {
    mix(Palette::BG, accent, t)
}

// --- Interactive-state decisions --------------------------------------------

/// The four interactive states every widget must style (task/spec §2.5).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WidgetState {
    Idle,
    Hovered,
    Active,
    Disabled,
}

/// Button families in the vocabulary (spec §7.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ButtonVariant {
    /// Transparent-at-rest toolbar/dialog button (`.tg-toolbar-btn` / `.tg-btn`).
    Ghost,
    /// Solid brand action (`.tg-btn-primary`); brightens on hover instead of
    /// taking a surface fill.
    Primary,
    /// Small ghost (`h-7 px-3 text-xs`); shares ghost color decisions.
    Compact,
    /// Square ghost holding only an icon (e.g. dialog close X).
    Icon,
}

impl ButtonVariant {
    /// Token-driven fill for one interactive state (§2.5 table + §7.2 rules).
    pub fn fill(self, state: WidgetState) -> Color32 {
        use WidgetState::{Active, Disabled, Hovered, Idle};
        match (self, state) {
            (Self::Primary, Idle | Disabled) => Palette::BRAND,
            (Self::Primary, Hovered) => mix(Palette::BRAND, Color32::WHITE, 0.10),
            (Self::Primary, Active) => mix(Palette::BRAND, Color32::WHITE, 0.20),
            (_, Idle | Disabled) => Color32::TRANSPARENT,
            (_, Hovered) => Palette::SURFACE_2,
            (_, Active) => Palette::SURFACE_3,
        }
    }

    /// Token-driven ink for one interactive state.
    ///
    /// Ghost-family text steps INK_2 → INK when engaged; primary keeps brand
    /// ink on its solid fill; everything drops to muted INK_3 when disabled.
    pub fn text(self, state: WidgetState) -> Color32 {
        use WidgetState::{Active, Disabled, Hovered, Idle};
        match (self, state) {
            (_, Disabled) => Palette::INK_3,
            (Self::Primary, _) => Palette::BRAND_INK,
            (_, Idle) => Palette::INK_2,
            (_, Hovered | Active) => Palette::INK,
        }
    }
}
// --- Focus -------------------------------------------------------------------

/// Paint the token-spec keyboard-focus ring (§7.2): a 1px `BRAND` stroke just
/// outside the widget rect, approximating the mockups' CSS box-shadow spread.
///
/// The vocabulary buttons ([`button_response_sized`]) and inputs
/// ([`input_frame`]) paint their own rings inline; this helper is for the
/// custom-drawn controls (rows, tabs, rail buttons, chips, cards) so keyboard
/// focus is never ambiguous anywhere in the shell (issue #23).
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

// --- Buttons -----------------------------------------------------------------

/// Ghost button (`.tg-toolbar-btn` / `.tg-btn`): transparent at rest,
/// SURFACE_2 hover, SURFACE_3 pressed.
pub fn ghost_button(ui: &mut Ui, icon: Option<Icon>, label: &str) -> Response {
    button_response(ui, ButtonVariant::Ghost, icon, Some(label))
}

/// Primary button (`.tg-btn-primary`): solid brand fill that brightens on
/// hover/press instead of taking surface fills.
pub fn primary_button(ui: &mut Ui, icon: Option<Icon>, label: &str) -> Response {
    button_response(ui, ButtonVariant::Primary, icon, Some(label))
}

/// Compact ghost button (`h-7 px-3 text-xs`) for dense toolbars and footers.
pub fn compact_button(ui: &mut Ui, label: &str) -> Response {
    button_response(ui, ButtonVariant::Compact, None, Some(label))
}

/// [`compact_button`] with an explicit `enabled` flag (issue 32 popup row
/// actions): disabled dims the button and turns clicks into no-ops, rendered
/// in a child scope so the disabled state never leaks into the caller's
/// remaining widgets. Pair it with `on_disabled_hover_text` so the gating
/// reason stays discoverable.
pub fn compact_button_enabled(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    if enabled {
        return compact_button(ui, label);
    }
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(ui.available_rect_before_wrap())
            .layout(*ui.layout()),
    );
    child.disable();
    let response = button_response(&mut child, ButtonVariant::Compact, None, Some(label));
    ui.advance_cursor_after_rect(child.min_rect());
    response
}

/// Square ghost button holding only an icon (e.g. dialog close X).
pub fn icon_button(ui: &mut Ui, icon: Icon) -> Response {
    button_response(ui, ButtonVariant::Icon, Some(icon), None)
}

/// Full-width stacked action button (issue 15 details-pane Actions section):
/// ghost at rest, or the solid-brand primary when `primary`; `enabled =
/// false` dims it and turns clicks into no-ops. Rendered in a child scope so
/// the disabled state never leaks into the caller's remaining widgets.
pub fn action_button(ui: &mut Ui, label: &str, primary: bool, enabled: bool) -> Response {
    let variant = if primary {
        ButtonVariant::Primary
    } else {
        ButtonVariant::Ghost
    };
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(ui.available_rect_before_wrap())
            .layout(*ui.layout()),
    );
    if !enabled {
        child.disable();
    }
    let width = child.available_width();
    let response = button_response_sized(
        &mut child,
        variant,
        None,
        Some(label),
        None,
        None,
        None,
        Some(width),
    );
    ui.advance_cursor_after_rect(child.min_rect());
    response
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
    let compact = matches!(variant, ButtonVariant::Compact);
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
        ButtonVariant::Compact => 12.0, // px-3
        ButtonVariant::Icon => 6.0,
        _ => ui.style().spacing.button_padding.x,
    });
    let height = height_override.unwrap_or(match variant {
        ButtonVariant::Icon => ICON_BUTTON_SIZE,
        ButtonVariant::Compact => COMPACT_BUTTON_HEIGHT,
        _ => BUTTON_HEIGHT,
    });
    let icon_size = icon_size_override.unwrap_or(BUTTON_ICON_SIZE);

    // Measure once (color is overridden at paint time), allocate, decide.
    let galley = label.map(|l| {
        ui.painter()
            .layout_no_wrap(l.to_owned(), font_id.clone(), Color32::WHITE)
    });
    let label_w = galley.as_ref().map_or(0.0, |g| g.size().x);
    let icon_w = if icon.is_some() && !icon_only {
        icon_size + 6.0
    } else {
        0.0
    };
    let content_w = icon_w + label_w;
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

    let state = if !enabled {
        WidgetState::Disabled
    } else if response.is_pointer_button_down_on() {
        WidgetState::Active
    } else if response.hovered() {
        WidgetState::Hovered
    } else {
        WidgetState::Idle
    };

    let painter = ui.painter().clone();
    let radius = CornerRadius::same(CONTROL_RADIUS);
    let fill = variant.fill(state);
    if fill != Color32::TRANSPARENT {
        painter.rect_filled(rect, radius, fill);
    }
    if response.has_focus() {
        // Focus ring: BRAND 1px stroke approximating the CSS box-shadow (§7.2).
        painter.rect_stroke(
            rect.expand(1.0),
            radius,
            Stroke::new(1.0, Palette::BRAND),
            StrokeKind::Outside,
        );
    }

    // Center the [icon][gap][label] group inside the button.
    let ink = variant.text(state);
    let cy = rect.center().y;
    let mut x = rect.left() + (rect.width() - content_w) / 2.0;
    if let Some(ic) = icon {
        paint_icon_at(ui, ic, Pos2::new(x, cy - icon_size / 2.0), icon_size, ink);
        if !icon_only {
            x += icon_size + 6.0;
        }
    }
    if let Some(g) = galley {
        painter.galley_with_override_text_color(Pos2::new(x, cy - g.size().y / 2.0), g, ink);
    }

    // Accessibility: labeled buttons are queryable/clickable via kittest and
    // screen readers alike. The label is materialized only when the info is
    // actually requested (interaction events) — never eagerly per frame.
    response.widget_info(|| {
        WidgetInfo::labeled(
            WidgetType::Button,
            enabled,
            label.or_else(|| icon.map(|i| i.name())).unwrap_or_default(),
        )
    });
    response
}

/// Paint one icon primitive centered at `origin` without disturbing layout.
fn paint_icon_at(ui: &mut Ui, icon: Icon, origin: Pos2, size: f32, color: Color32) {
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_size(origin, Vec2::splat(size)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    icons::icon(&mut child, icon, size, color);
}
// --- Segmented control ------------------------------------------------------

/// Compact segmented control (spec §8.4, issue #01): SURFACE_2 track, the
/// selected segment sits on SURFACE_3 with INK ink. Returns the clicked
/// option index (the caller applies the change to its own state).
///
/// The widget is the shared picker used by every surface that needs a
/// token-exact two- or three-way choice — diff side-by-side/unified, file/
/// hunk/line granularity, CLI/libgit2/Auto backend, strategy pickers — so
/// these controls render identically everywhere and a new surface can adopt
/// the widget instead of inventing a second variant.
pub fn segmented_control(ui: &mut Ui, options: &[&str], selected: usize) -> Option<usize> {
    const SEGMENT_H: f32 = 24.0;
    // Segments are controls, so they take the shared button padding.
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
            // The inset step inside a CONTROL_RADIUS track is the compact chip
            // radius — the same 3 px a badge rounds at, not a new role.
            ui.painter()
                .rect_filled(seg, CornerRadius::same(CHIP_RADIUS), Palette::SURFACE_3);
        }
        let ink = if is_selected || resp.hovered() {
            Palette::INK
        } else {
            Palette::INK_2
        };
        let galley = ui
            .painter()
            .layout_no_wrap((*option).to_owned(), font_id.clone(), ink);
        ui.painter().galley(
            Pos2::new(
                seg.center().x - galley.size().x / 2.0,
                seg.center().y - galley.size().y / 2.0,
            ),
            galley,
            ink,
        );
        resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, *option));
        focus_ring(ui, &resp);
        if resp.clicked() {
            clicked = Some(i);
        }
        x += widths[i];
    }
    clicked
}
