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

// The compact primary's whole reason to exist is the pane-header band, so the
// compact height **is** the band height. Asserted here rather than restated at
// the call site: a band that grew would otherwise leave a button that no longer
// fits it, and the failure would show up as a pane whose header is taller than
// every other pane's rather than as a number that disagrees with a number.
const _: () = {
    assert!(
        COMPACT_BUTTON_HEIGHT == super::containers::PANE_HEADER_HEIGHT,
        "the compact primary is sized for the pane-header band, so the two \
         numbers are one decision"
    )
};

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
    /// The primary at the **band height** (28 px): the brand primary sized to sit
    /// in a pane header's fixed-height band.
    ///
    /// This is [`Self::Primary`] and nothing else. It exists because the shared
    /// vocabulary had a 32 px primary and a 28 px *ghost* and no brand button at
    /// the height a pane header is, so the one place that needed a primary
    /// inside a header band reached for a page-local button family instead — which
    /// is a second vocabulary, and the R1 primary is precisely the thing that must
    /// not have two spellings. It is a **height**, not a colour: the fill and ink
    /// ladders are shared with `Primary` arm for arm (and
    /// `tests/widget_library.rs` asserts the two ladders are equal), because a
    /// brand button that is 4 px shorter must not also be a slightly different
    /// blue.
    ///
    /// A host that needs a *third* height, a different radius, or a different
    /// fill is not this variant: R5 says the host knows its own surface, and this
    /// vocabulary deliberately has no flags to be configured with.
    CompactPrimary,
}

impl ButtonVariant {
    /// Token-driven fill for one interactive state (§2.5 table + §7.2 rules).
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

    /// Token-driven ink for one interactive state.
    ///
    /// Ghost-family text steps INK_2 → INK when engaged; primary keeps brand
    /// ink on its solid fill; everything drops to muted INK_3 when disabled.
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

// --- Disabled control scope -------------------------------------------------

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

/// **The brand primary at the band height** (28 px): [`ButtonVariant::CompactPrimary`]
/// as a control, for a pane header's right-aligned action slot.
///
/// It exists because [`PANE_HEADER_HEIGHT`](super::containers::PANE_HEADER_HEIGHT)
/// is a fixed 28 px band that grows to fit its tallest child, so a 32 px
/// [`primary_button`] in that slot makes one pane's header taller than every
/// other pane's and pushes its own hairline down with it — and the cross-pane
/// header-geometry ratchet in `tests/widget_library.rs` is right to catch that.
/// Before this, the one call site that needed it reached for a page-local button
/// family, which is a second spelling of the R1 primary. Same brand ladder, same
/// ink, same radius: only the height differs, and the height is what the band
/// names.
pub fn compact_primary_button(ui: &mut Ui, label: &str) -> Response {
    button_response(ui, ButtonVariant::CompactPrimary, None, Some(label))
}

/// Compact ghost button (`h-7 px-3 text-xs`) for dense toolbars and footers.
pub fn compact_button(ui: &mut Ui, label: &str) -> Response {
    button_response(ui, ButtonVariant::Compact, None, Some(label))
}

/// [`compact_button`] with an explicit `enabled` flag (issue 32 popup row
/// actions): disabled dims the button and turns clicks into no-ops, painted
/// through [`disabled_child_scope`] so the disabled state never leaks into the
/// caller's remaining widgets. An enabled flag short-circuits straight to
/// [`compact_button`], so only the disabled path is built in a child scope.
/// Pair it with `on_disabled_hover_text` so the gating reason stays
/// discoverable.
pub fn compact_button_enabled(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    if enabled {
        return compact_button(ui, label);
    }
    disabled_child_scope(ui, enabled, |child| {
        button_response(child, ButtonVariant::Compact, None, Some(label))
    })
}

/// Square ghost button holding only an icon (e.g. dialog close X).
pub fn icon_button(ui: &mut Ui, icon: Icon) -> Response {
    button_response(ui, ButtonVariant::Icon, Some(icon), None)
}

/// The ghost icon-button state ladder — one definition of how a borderless
/// icon button fills and inks as it is hovered, held, and disabled, built over
/// [`ButtonVariant::Ghost`] and [`WidgetState`] so every ghost icon button in
/// the app fills, inks, and rounds identically.
///
/// The rect is the size parameter: a host either allocates a standard square
/// and passes it in, or allocates a dense, deliberately-shaped hit rect of its
/// own (a diff gutter cell) and passes that. The vocabulary owns the fill, the
/// ink, the corner radius, the accessibility node, and the single brand focus
/// ring; the host owns its id, its tooltip, and its glyph — `paint_content`
/// receives the resolved [`WidgetState`] so it can take its own ink from
/// `ButtonVariant::Ghost.text(state)` rather than re-deriving a second ladder.
///
/// This is deliberately not a configurable universal button: a host that
/// needs a different fill, ink, or radius has a different role and keeps its
/// own paint rather than growing a flag here.
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
    let fill = ButtonVariant::Ghost.fill(state);
    if fill != Color32::TRANSPARENT {
        painter.rect_filled(rect, CornerRadius::same(CONTROL_RADIUS), fill);
    }
    paint_content(ui, rect, state);

    // Accessibility: a labeled Button, so kittest and screen readers find it.
    // The info is materialized only when actually requested, never eagerly.
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    focus_ring(ui, &response);
    response
}

/// Full-width stacked action button (issue 15 details-pane Actions section):
/// ghost at rest, or the solid-brand primary when `primary`; `enabled =
/// false` dims it and turns clicks into no-ops, painted through
/// [`disabled_child_scope`] so the disabled state never leaks into the
/// caller's remaining widgets.
///
/// Unlike [`compact_button_enabled`], this button always builds and paints the
/// control — the full-width measure and the widget itself are the same either
/// way, so only the `disable()` call is conditional. The two shapes are kept
/// deliberately distinct rather than flattened into one.
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
        ButtonVariant::Compact | ButtonVariant::CompactPrimary => 12.0, // px-3
        ButtonVariant::Icon => 6.0,
        _ => ui.style().spacing.button_padding.x,
    });
    let height = height_override.unwrap_or(match variant {
        ButtonVariant::Icon => ICON_BUTTON_SIZE,
        ButtonVariant::Compact | ButtonVariant::CompactPrimary => COMPACT_BUTTON_HEIGHT,
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
        // The shared two-axis centring, measured and painted in one call.
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
