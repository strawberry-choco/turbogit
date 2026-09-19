//! Shared widget vocabulary (issue #6, spec §7).
//!
//! One place for buttons, badges/chips, tree/list rows, inputs, and dialog /
//! tool-window chrome. Every visual decision is a pure function over the
//! central [`crate::theme::Palette`] tokens (spec §2): idle/hover/active/
//! disabled states map onto the token ladder exactly as §2.5 prescribes, so
//! later tickets can adopt these widgets surface-by-surface without any
//! ad-hoc colors.
//!
//! Behavioral rules implemented here (spec §7.2):
//! - Hover fills are `SURFACE_2` unless the widget is already solid-filled
//!   (primary buttons brighten instead).
//! - Selected tree/list rows get a solid `BRAND` fill with brand ink.
//! - Disabled controls keep their idle fill and drop to `INK_3` ink.
//! - Focused inputs/buttons get a 1px `BRAND` focus ring.
//!
//! ## Who owns which title band
//!
//! Three names cover "the label that heads a group" and each owns a different
//! job (conformance issue 07); a screen picking the wrong one is a visible
//! layout change, not a style preference:
//!
//! - [`group_title`] — a plain uppercase section *title* in a dialog, panel or
//!   list: text only, no band, no chevron. The 11 px `INK_3` micro-header. This
//!   is the one to reach for unless a bullet below applies.
//! - [`components::section_header`] — a *toggleable group band*: the
//!   `SECTION_BG` strip a `LOCAL` / `REMOTE` / smart-group header sits on, with
//!   a chevron, a live count pill and a click target that collapses the group.
//!   Its uppercase text comes through [`components::section_label`], the shared
//!   transform, which is not itself a widget.
//! - A region header that is neither — the `SURFACE` strip over the diff panes,
//!   a repo block in the branch tree — is not a title widget at all. It is a
//!   painted band whose fill, radius and height belong to that screen
//!   (theme.rs's G1 boundary); only its literals get tokenised.
//!
//! Row fills likewise have one decision function, [`components::row_fill`] over
//! [`components::RowState`], and [`paint_row`] is its painter-level form for the
//! hand-painted tables.

use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Frame, InnerResponse, Layout, Margin, Pos2,
    Rect, Response, RichText, Sense, Stroke, StrokeKind, TextEdit, TextStyle, Ui, UiBuilder, Vec2,
    WidgetInfo, WidgetType,
};

use super::components::{RowState, row_fill};
use super::icons::{self, Icon};
use crate::theme::{CHIP_RADIUS, CONTROL_RADIUS, FILE_ROW_HEIGHT, Palette, TYPE_BODY, chrome_font};

// --- Metrics (spec §4.2 fixed heights) -------------------------------------

/// Alpha used when tinting an accent over [`Palette::BG`] for badge fills.
pub const BADGE_TINT: f32 = 0.18;
/// Badge / ref-label chip height (pill). This is THE chip height: every chip
/// and pill in the crate takes it from here rather than declaring its own, and
/// the corner radius is always derived from it (half the height) so a chip
/// reads as a pill at any size the role changes to.
pub const CHIP_HEIGHT: f32 = 18.0;

const BUTTON_HEIGHT: f32 = 32.0; // .tg-btn
const COMPACT_BUTTON_HEIGHT: f32 = 28.0; // h-7 compact variants
const ICON_BUTTON_SIZE: f32 = 28.0; // square ghost (dialog close X)
const BUTTON_ICON_SIZE: f32 = 16.0; // §5.3: 16×16 in buttons
/// Horizontal padding of the shared chip ([`chip`], [`badge`], [`ref_label`]) and
/// of every pill that mirrors it.
///
/// Three chip paddings exist in the app and each is a distinct existing role,
/// so none of them collapses into another (conformance issue 05): 6 px here is
/// the non-interactive chip, `theme::DENSITY_COMPACT_BUTTON.x` (8 px) is the
/// commit window's quiet label roundel, and `theme::BUTTON_PADDING.x` (10 px)
/// is the interactive selectable chip — padded like every other button and like
/// [`segmented_control`]'s segments.
pub const CHIP_PAD_X: f32 = 6.0;
const TOOLWINDOW_HEADER_HEIGHT: f32 = 28.0;
const MICRO_TEXT: f32 = crate::theme::TYPE_CONTROL; // uppercase micro-headers (§3.3)
const INPUT_ICON_SIZE: f32 = 14.0; // §5.3: 14×14 in inputs/badges

/// Named bold family registered by `theme::install_fonts` (ADR-0002).
const BOLD_FAMILY: &str = "jetbrains-mono-bold";

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

// --- Chip decisions ----------------------------------------------------------

/// Background/foreground pair painted by badges and ref labels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChipColors {
    pub bg: Color32,
    pub fg: Color32,
}

/// File-status badge kinds (`.tg-badge`, spec §7.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadgeKind {
    Neutral,
    Added,
    Modified,
    Deleted,
}

impl BadgeKind {
    /// The status token this badge kind is decided by (R1.4 mapping).
    pub fn accent(self) -> Color32 {
        match self {
            Self::Neutral => Palette::INK_2,
            Self::Added => Palette::STATE_SUCCESS,
            Self::Modified => Palette::STATE_WARNING,
            Self::Deleted => Palette::STATE_ERROR,
        }
    }

    /// Colors: neutral sits on the input/badge surface token; status badges
    /// tint their accent over BG with accent-colored ink (§2.1/§2.2 usage).
    pub fn colors(self) -> ChipColors {
        let fg = self.accent();
        let bg = match self {
            Self::Neutral => Palette::SURFACE_3,
            _ => tint_over_bg(fg, BADGE_TINT),
        };
        ChipColors { bg, fg }
    }
}

/// Git ref chip kinds (`.tg-label`, spec §7.1/§8.3).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RefKind {
    Branch,
    Remote,
    Tag,
}

impl RefKind {
    /// The token this ref kind is decided by: branch=brand, remote=success,
    /// tag=warning.
    pub fn accent(self) -> Color32 {
        match self {
            Self::Branch => Palette::BRAND,
            Self::Remote => Palette::STATE_SUCCESS,
            Self::Tag => Palette::STATE_WARNING,
        }
    }

    /// Colors: solid pills. Ink picks the palette token with real contrast
    /// against the fill — white brand ink on BRAND, dark background ink on
    /// the lighter success/warning fills.
    pub fn colors(self) -> ChipColors {
        let fg = match self {
            Self::Branch => Palette::BRAND_INK,
            Self::Remote | Self::Tag => Palette::BG,
        };
        ChipColors {
            bg: self.accent(),
            fg,
        }
    }
}

/// Direction of a branch ahead/behind count chip (issue #01).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CountDirection {
    /// Ahead of upstream — outgoing commits waiting to push.
    Ahead,
    /// Behind upstream — incoming commits waiting to pull/fetch.
    Behind,
}

/// Status badges from the screens-gap vocabulary (issue #01): direction-tagged
/// ahead/behind counts, protected-branch lock, stale-age, FOCUSED modal
/// marker, and CASCADE operation indicator. Each variant picks a token from
/// the central palette so a chip carrying any of them has a real color and
/// never invents a hue.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StatusBadge {
    /// Ahead/behind count chip, direction decides the hue.
    Count(CountDirection),
    /// Lock indicator on a protected branch.
    Lock,
    /// Stale-age chip ("3d ago"); mirrors the `STATUS_STALE` token.
    Stale,
    /// FOCUSED marker — a modal-active accent.
    Focused,
    /// CASCADE operation indicator.
    Cascade,
}

impl StatusBadge {
    /// The palette token this kind is decided by.
    ///
    /// Ahead counts share the success accent; behind counts share the warning
    /// accent; lock reads as a caution (mirroring the tag-ref decision);
    /// stale mirrors STATUS_STALE/INFO; focused mirrors BRAND/selection;
    /// cascade has its own accent so cascade chips never collide with focus
    /// or selection color.
    pub fn accent(self) -> Color32 {
        match self {
            Self::Count(CountDirection::Ahead) => Palette::STATE_SUCCESS,
            Self::Count(CountDirection::Behind) => Palette::STATE_WARNING,
            Self::Lock => Palette::STATE_WARNING,
            Self::Stale => Palette::STATE_INFO,
            Self::Focused => Palette::BRAND,
            Self::Cascade => CASCADE_ACCENT,
        }
    }
}

/// CASCADE accent — distinct from BRAND/STATE_* so cascade operations never
/// read as focus, success, warning, info, or error. Reused by chips and any
/// other cascade surface.
///
/// Kept, with no `src` consumer (conformance issue 18): `status_badge` below is
/// the only thing that reads it, the cascade run screens render their status
/// through `RepoState` instead, and `widget_library.rs` pins the whole
/// `StatusBadge` vocabulary by painting it. Deleting the enum means deleting
/// that contract in the same change, which is its own review.
pub const CASCADE_ACCENT: Color32 = Color32::from_rgb(0xa7, 0x8b, 0xfa);

/// Status badge from the screens-gap vocabulary (issue #01): count chips,
/// lock, stale-age, FOCUSED, and CASCADE markers. Paints the same 18px pill
/// shape as [`badge`] so any chip carrying these states matches the rest of
/// the badge vocabulary at every callsite.
pub fn status_badge(ui: &mut Ui, text: &str, kind: StatusBadge) -> Response {
    let fg = kind.accent();
    let bg = tint_over_bg(fg, BADGE_TINT);
    chip(ui, text, ChipColors { bg, fg })
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

// --- Chips -------------------------------------------------------------------

/// Status badge (`.tg-badge`): 18px pill, tinted background + accent ink.
pub fn badge(ui: &mut Ui, text: &str, kind: BadgeKind) -> Response {
    chip(ui, text, kind.colors())
}

/// Git ref chip (`.tg-label`): 18px solid pill (branch=brand, remote=success,
/// tag=warning).
pub fn ref_label(ui: &mut Ui, text: &str, kind: RefKind) -> Response {
    chip(ui, text, kind.colors())
}

/// The chip's corner radius, derived from [`CHIP_HEIGHT`] rather than fixed.
/// Half the height is what makes these read as pills — which is also where
/// `theme`'s old 9 px `PILL_RADIUS` token got its number from, and why that
/// token was redundant once every pill derived its own (conformance issue 18).
pub fn chip_radius() -> CornerRadius {
    CornerRadius::same((CHIP_HEIGHT / 2.0) as u8)
}

/// The rect a [`paint_chip`] of `galley` occupies when its right edge is at
/// `right_edge` and it is vertically centred on `cy`.
///
/// A surface that stacks two chips in one row asks this for the first so it
/// knows where the second may sit without colliding (welcome's recent-project
/// row does exactly that).
pub fn chip_rect_right(right_edge: f32, cy: f32, galley: &egui::Galley) -> Rect {
    let width = galley.size().x + CHIP_PAD_X * 2.0;
    Rect::from_min_size(
        Pos2::new(right_edge - width, cy - CHIP_HEIGHT / 2.0),
        Vec2::new(width, CHIP_HEIGHT),
    )
}

/// Paint one chip of the shared geometry into a rect the caller already placed:
/// `bg` at [`chip_radius`], then `galley` centred in `ink`.
///
/// [`chip`] is the layout-level form and the one to reach for; this exists for
/// surfaces that position a chip by coordinate, which no layout child can do —
/// the welcome screen's recent-project row paints a branch chip and a repo-count
/// chip at the right edge of a hand-painted row (conformance issue 09). Both
/// forms go through here, so height, pad and radius are defined once.
pub fn paint_chip(
    painter: &egui::Painter,
    rect: Rect,
    galley: std::sync::Arc<egui::Galley>,
    bg: Color32,
    ink: Color32,
) {
    painter.rect_filled(rect, chip_radius(), bg);
    painter.galley(
        Pos2::new(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        ink,
    );
}

fn chip(ui: &mut Ui, text: &str, colors: ChipColors) -> Response {
    let font_id = FontId::new(MICRO_TEXT, FontFamily::Proportional);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font_id, colors.fg);
    let size = Vec2::new(galley.size().x + CHIP_PAD_X * 2.0, CHIP_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());

    paint_chip(ui.painter(), rect, galley, colors.bg, colors.fg);

    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, text));
    response
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

// --- Trees & lists -----------------------------------------------------------

/// Fixed-height tree row: hover SURFACE_2, selected = BRAND fill with
/// brand-ink content (§7.1/§7.2). The height is the shared
/// [`FILE_ROW_HEIGHT`], so a vocabulary row and a changes-tree row are one row.
pub fn tree_row(ui: &mut Ui, selected: bool, contents: impl FnOnce(&mut Ui)) -> Response {
    row_impl(ui, selected, contents)
}

fn row_impl(ui: &mut Ui, selected: bool, contents: impl FnOnce(&mut Ui)) -> Response {
    let width = ui.available_width();
    // Reserve the exact row space up-front so the centered cross-layout can
    // never swallow the parent's remaining height.
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, FILE_ROW_HEIGHT), Sense::hover());

    // Contents live strictly inside the reserved rect.
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    if selected {
        // Selected rows flip content ink to brand ink (§7.2).
        child.visuals_mut().override_text_color = Some(Palette::BRAND_INK);
    }
    contents(&mut child);

    // Interact *after* the content is registered so the row — not the labels
    // inside it — owns hover and click events across its full rect.
    let id = ui.auto_id_with("tree_row");
    let response = ui.interact(rect, id, Sense::click());

    let fill = row_fill(RowState::from_flags(selected, response.hovered()));
    if fill != Color32::TRANSPARENT {
        // Paint behind the already-emitted content shapes.
        let mut bg = ui.painter().clone();
        bg.set_layer_id(egui::LayerId::new(egui::Order::Background, response.id));
        bg.rect_filled(rect, CornerRadius::same(CONTROL_RADIUS), fill);
    }
    focus_ring(ui, &response);

    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), ""));
    response
}

/// Paint one row's fill for a [`RowState`] in a rect the caller already
/// allocated.
///
/// [`tree_row`] lays its own row out and needs nothing here; this is for the
/// hand-painted tables — the settings category rail, the rebase todo, the commit
/// window's file rows — that allocate a rect, paint behind their content and
/// still want the one row-state decision. The corner is [`CONTROL_RADIUS`],
/// which is what every such row has always used; a surface that rounds its rows
/// differently (the sidebar's full-bleed band, blame's dense rows) is a real
/// geometry difference and stays local rather than being flattened by this call.
pub fn paint_row(ui: &Ui, rect: Rect, state: RowState) {
    let fill = row_fill(state);
    if fill != Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(CONTROL_RADIUS), fill);
    }
}

// --- Inputs ------------------------------------------------------------------

/// Single-line text input: SURFACE_3 fill, LINE border, BRAND focus ring.
pub fn text_input(ui: &mut Ui, placeholder: &str, buf: &mut String) -> Response {
    input_frame(ui, placeholder, buf, false)
}

/// Search input: like [`text_input`] plus a leading magnifier icon.
pub fn search_input(ui: &mut Ui, placeholder: &str, buf: &mut String) -> Response {
    input_frame(ui, placeholder, buf, true)
}

fn input_frame(ui: &mut Ui, placeholder: &str, buf: &mut String, search_icon: bool) -> Response {
    let avail_w = ui.available_width();
    let icon_area = if search_icon {
        INPUT_ICON_SIZE + 4.0
    } else {
        0.0
    };
    // Frame margins (8×2) + stroke (1×2) leave the rest for the edit.
    let edit_w = (avail_w - 16.0 - 2.0 - icon_area).max(40.0);

    let frame = Frame::new()
        .fill(Palette::SURFACE_3)
        .stroke(Stroke::new(1.0, Palette::LINE))
        .corner_radius(CornerRadius::same(CONTROL_RADIUS))
        .inner_margin(Margin::symmetric(8, 4));

    let outer = frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            if search_icon {
                icons::icon(ui, Icon::SEARCH, INPUT_ICON_SIZE, Palette::INK_3);
                ui.add_space(4.0);
            }
            let resp = ui.add(
                TextEdit::singleline(buf)
                    .hint_text(placeholder)
                    .desired_width(edit_w)
                    .frame(egui::Frame::new()),
            );
            resp.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, placeholder));
            resp
        })
        .inner
    });

    let edit_response = outer.inner;
    if edit_response.has_focus() {
        ui.painter().rect_stroke(
            outer.response.rect.expand(1.0),
            CornerRadius::same(CONTROL_RADIUS),
            Stroke::new(1.0, Palette::BRAND),
            StrokeKind::Outside,
        );
    }
    edit_response
}

// --- Dialog chrome -----------------------------------------------------------

/// Bold body font via the named family registered by `install_fonts`,
/// falling back to the regular proportional face when fonts are not
/// installed yet (epaint panics on unbound families).
pub(crate) fn bold_font_if_available(ui: &Ui) -> FontId {
    let has_bold = ui.ctx().fonts(|f| {
        f.definitions()
            .families
            .contains_key(&FontFamily::Name(BOLD_FAMILY.into()))
    });
    if has_bold {
        FontId::new(
            crate::theme::TYPE_DETAIL_TITLE,
            FontFamily::Name(BOLD_FAMILY.into()),
        )
    } else {
        crate::theme::chrome_font(crate::theme::TYPE_DETAIL_TITLE)
    }
}

/// Dialog footer: top LINE border with right-aligned action buttons (§7.1).
pub fn dialog_footer<R>(ui: &mut Ui, buttons: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 1.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, Palette::LINE);
    ui.add_space(6.0);
    ui.with_layout(Layout::right_to_left(Align::Center), buttons)
}

// --- Section chrome ----------------------------------------------------------

/// Bordered card surface (Local Changes redesign): the containment the
/// mockup gives every region, so neighbouring controls read as one group
/// instead of as a flat stack of headings and separators.
///
/// The fill is [`Palette::CONTENT_BG`], deliberately *not* `BG` — the panel
/// behind a card is `BG`, so a `BG` card would be invisible (risk R2). The
/// body lays out inside the frame's margin and is stretched to the caller's
/// full available width, so a card spans its pane rather than hugging its
/// content. The returned rect is the card's outer edge, which is what a
/// caller capping a scroll area against its own container needs (risk R1).
pub fn card<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    Frame::new()
        .fill(Palette::CONTENT_BG)
        .stroke(Stroke::new(1.0, Palette::LINE))
        .corner_radius(CornerRadius::same(crate::theme::CARD_RADIUS))
        .inner_margin(Margin::same(crate::theme::PANEL_PADDING as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add_contents(ui)
        })
}

/// Header strip inside a [`card`]: the caller's own header rows, ruled off
/// from the body below by a hairline. Shared so every carded region gets
/// identical containment while each states what it holds.
///
/// The row layout is the caller's, not this function's, because a header is
/// not always one line — the changes card puts its title and icon cluster on
/// one row and its filter on the next, since the commit panel is too narrow to
/// hold both on a single line.
pub fn card_header(ui: &mut Ui, contents: impl FnOnce(&mut Ui)) {
    contents(ui);
    ui.separator();
}

/// An inset note well: [`Palette::SURFACE_2`] at the control radius with 8 px of
/// padding, the containment a dialog hands a preview, a summary or a banner.
///
/// This is the *unbordered* sibling of [`card`], which is a bordered content
/// region on `CONTENT_BG`. Pass a colour in `severity` for a note that is also a
/// warning — the merge-cascade and rebase banners, which must read as attention
/// without becoming the app's contained alert ([`alert_box`] is filled rather
/// than stroked, and is the other one).
pub fn note<R>(
    ui: &mut Ui,
    severity: Option<Color32>,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<R> {
    let mut frame = Frame::new()
        .fill(Palette::SURFACE_2)
        .corner_radius(CornerRadius::same(CONTROL_RADIUS))
        .inner_margin(Margin::same(8));
    if let Some(ink) = severity {
        frame = frame.stroke(Stroke::new(1.0, ink));
    }
    frame.show(ui, add_contents)
}

/// Tool-window header (28px): 11px uppercase muted title left, right-aligned
/// actions slot (§7.1, §3.3).
pub fn toolwindow_header<R>(
    ui: &mut Ui,
    title: &str,
    actions: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<R> {
    let width = ui.available_width();
    ui.allocate_ui_with_layout(
        Vec2::new(width, TOOLWINDOW_HEADER_HEIGHT),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.label(micro_header(title));
            ui.with_layout(Layout::right_to_left(Align::Center), actions)
                .inner
        },
    )
}

/// Group title: 11px uppercase INK_3 section label ("RECENT", …) (§7.1).
pub fn group_title(ui: &mut Ui, title: &str) {
    ui.label(micro_header(title));
}

/// Uppercase micro-header text — the transform itself is mandatory (§3.3).
fn micro_header(text: &str) -> RichText {
    RichText::new(text.to_uppercase())
        .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
        .color(Palette::INK_3)
}

// --- Commit-detail primitives (logs-panels redesign issue 03) ----------------

/// Author avatar diameter (mockup: a 28px initials circle).
pub const AVATAR_SIZE: f32 = 28.0;
/// Churn bar track height.
const CHURN_BAR_HEIGHT: f32 = 4.0;
/// Alert icon size — the §5.3 size for icons inside inputs and badges.
const ALERT_ICON_SIZE: f32 = INPUT_ICON_SIZE;

/// Commit hash as a clickable chip: mono hash on `SURFACE_3` (redesign
/// issue 03). The click is *reported*, not acted on — the caller defers the
/// copy like every other pane interaction (plan §1.3).
pub fn hash_chip(ui: &mut Ui, hash: &str, hint: &str) -> Response {
    let font = FontId::new(crate::theme::TYPE_BODY, FontFamily::Monospace);
    let galley = ui
        .painter()
        .layout_no_wrap(hash.to_owned(), font, Palette::BRAND);
    let size = Vec2::new(galley.size().x + CHIP_PAD_X * 2.0, CHIP_HEIGHT + 6.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(CONTROL_RADIUS), Palette::SURFACE_3);
        ui.painter().galley(
            Pos2::new(
                rect.center().x - galley.size().x / 2.0,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            Palette::BRAND,
        );
    }
    focus_ring(ui, &response);
    response.on_hover_text(hint)
}

/// Author avatar: a 28px circle of brand-tinted surface carrying up to two
/// initials (redesign issue 03).
pub fn avatar_initials(ui: &mut Ui, name: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(AVATAR_SIZE), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    painter.circle_filled(
        rect.center(),
        AVATAR_SIZE / 2.0,
        tint_over_bg(Palette::BRAND, BADGE_TINT),
    );
    let galley = painter.layout_no_wrap(
        initials_of(name),
        FontId::new(MICRO_TEXT, FontFamily::Proportional),
        Palette::INK,
    );
    painter.galley(
        Pos2::new(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        Palette::INK,
    );
}

/// The initials an avatar shows: the first letter of the first and last words
/// ("Kangrui Johann Ye" → `KY`), or of the one word there is.
fn initials_of(name: &str) -> String {
    let mut words = name.split_whitespace();
    let first = words.next().and_then(|w| w.chars().next());
    let last = words.next_back().and_then(|w| w.chars().next());
    match (first, last) {
        (Some(a), Some(b)) => format!("{}{}", a.to_uppercase(), b.to_uppercase()),
        (Some(a), None) => a.to_uppercase().to_string(),
        (None, _) => String::new(),
    }
}

/// Contained warning surface (redesign issue 03): the guardrail explanation
/// that used to trail the action buttons as a bare label.
pub fn alert_box(ui: &mut Ui, text: &str) {
    Frame::new()
        .fill(Palette::SURFACE_WARNING)
        .corner_radius(CornerRadius::same(CONTROL_RADIUS))
        .inner_margin(Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::splat(ALERT_ICON_SIZE), Sense::hover());
                icons::paint_icon(
                    ui.painter(),
                    Pos2::new(rect.left(), rect.center().y - ALERT_ICON_SIZE / 2.0),
                    ALERT_ICON_SIZE,
                    Icon::ALERT_TRIANGLE,
                    Palette::STATE_WARNING,
                );
                let width = ui.available_width();
                ui.add_sized(
                    Vec2::new(width.max(0.0), 0.0),
                    egui::Label::new(
                        RichText::new(text)
                            .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                            .color(Palette::STATE_WARNING),
                    )
                    .wrap(),
                );
            });
        });
}

/// Churn bar (redesign issue 03): a 4px `SURFACE_3` track split between
/// additions and deletions in the two status tokens, in the same reading order
/// as the numbers beside it. A commit that changed no lines paints the empty
/// track.
pub fn churn_bar(ui: &mut Ui, added: usize, removed: usize) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, CHURN_BAR_HEIGHT), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let radius = CornerRadius::same(CHURN_BAR_HEIGHT as u8);
    let painter = ui.painter();
    painter.rect_filled(rect, radius, Palette::SURFACE_3);
    let total = added + removed;
    if total == 0 {
        return;
    }
    let added_width = rect.width() * added as f32 / total as f32;
    if added_width > 0.0 {
        painter.rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(added_width, rect.height())),
            radius,
            Palette::STATE_SUCCESS,
        );
    }
    let removed_width = rect.width() - added_width;
    if removed_width > 0.0 {
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(rect.left() + added_width, rect.top()),
                Vec2::new(removed_width, rect.height()),
            ),
            radius,
            Palette::STATE_ERROR,
        );
    }
}
