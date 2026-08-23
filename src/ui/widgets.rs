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

use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Frame, InnerResponse, Layout, Margin, Pos2,
    Rect, Response, RichText, Sense, Stroke, StrokeKind, TextEdit, TextStyle, Ui, UiBuilder, Vec2,
    WidgetInfo, WidgetType,
};

use super::icons::{self, Icon};
use crate::theme::Palette;

// --- Metrics (spec §4.2 fixed heights) -------------------------------------

/// Alpha used when tinting an accent over [`Palette::BG`] for badge fills.
pub const BADGE_TINT: f32 = 0.18;
/// Tree row height (`.tg-tree-row`).
pub const ROW_HEIGHT: f32 = 24.0;
/// Badge / ref-label chip height (pill).
pub const CHIP_HEIGHT: f32 = 18.0;

const RADIUS_SM: u8 = 4; // --tg-radius-sm
const BUTTON_HEIGHT: f32 = 32.0; // .tg-btn
const COMPACT_BUTTON_HEIGHT: f32 = 28.0; // h-7 compact variants
const ICON_BUTTON_SIZE: f32 = 28.0; // square ghost (dialog close X)
const BUTTON_ICON_SIZE: f32 = 16.0; // §5.3: 16×16 in buttons
pub const TOOLBAR_BUTTON_HEIGHT: f32 = 26.0; // §4.2: toolbar buttons
const TOOLBAR_ICON_SIZE: f32 = 14.0; // §5.3/§6.2: 14×14 in the toolbar
const CHIP_PAD_X: f32 = 6.0;
const DIALOG_HEADER_HEIGHT: f32 = 40.0;
const TOOLWINDOW_HEADER_HEIGHT: f32 = 28.0;
const MICRO_TEXT: f32 = 11.0; // uppercase micro-headers (§3.3)
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
    /// Whether this variant is solid-filled with the brand token.
    fn is_primary(self) -> bool {
        matches!(self, Self::Primary)
    }

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
            CornerRadius::same(RADIUS_SM),
            Stroke::new(1.0, Palette::BRAND),
            StrokeKind::Outside,
        );
    }
}

// --- Row decisions -----------------------------------------------------------

/// Tree/list row fill decision (§7.2): a selected row paints solid BRAND no
/// matter what; unselected rows only pick up the SURFACE_2 hover fill.
pub fn row_fill(selected: bool, hovered: bool) -> Color32 {
    if selected {
        Palette::BRAND
    } else if hovered {
        Palette::SURFACE_2
    } else {
        Color32::TRANSPARENT
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

/// Square ghost button holding only an icon (e.g. dialog close X).
pub fn icon_button(ui: &mut Ui, icon: Icon) -> Response {
    button_response(ui, ButtonVariant::Icon, Some(icon), None)
}

/// Toolbar button (spec §4.2/§6.2): 26px tall, 0×8 padding, 14×14 icon +
/// label. `primary = true` renders the solid-brand variant — the toolbar's
/// single primary action (Commit).
pub fn toolbar_button(ui: &mut Ui, icon: Icon, label: &str, primary: bool) -> Response {
    let variant = if primary {
        ButtonVariant::Primary
    } else {
        ButtonVariant::Ghost
    };
    button_response_sized(
        ui,
        variant,
        Some(icon),
        Some(label),
        Some(TOOLBAR_BUTTON_HEIGHT),
        Some(8.0),
        Some(TOOLBAR_ICON_SIZE),
    )
}

fn button_response(
    ui: &mut Ui,
    variant: ButtonVariant,
    icon: Option<Icon>,
    label: Option<&str>,
) -> Response {
    button_response_sized(ui, variant, icon, label, None, None, None)
}

fn button_response_sized(
    ui: &mut Ui,
    variant: ButtonVariant,
    icon: Option<Icon>,
    label: Option<&str>,
    height_override: Option<f32>,
    pad_x_override: Option<f32>,
    icon_size_override: Option<f32>,
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
        .unwrap_or_else(|| {
            FontId::new(if compact { 12.0 } else { 14.0 }, FontFamily::Proportional)
        });

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
    let width = if icon_only {
        height
    } else {
        pad_x * 2.0 + content_w
    };

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
    let radius = CornerRadius::same(RADIUS_SM);
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
    // screen readers alike.
    let info_label = label
        .map(str::to_owned)
        .or_else(|| icon.map(|i| i.name().to_owned()))
        .unwrap_or_default();
    let closure_label = info_label.clone();
    response.widget_info(move || {
        WidgetInfo::labeled(WidgetType::Button, enabled, closure_label.clone())
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

fn chip(ui: &mut Ui, text: &str, colors: ChipColors) -> Response {
    let font_id = FontId::new(MICRO_TEXT, FontFamily::Proportional);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font_id, colors.fg);
    let size = Vec2::new(galley.size().x + CHIP_PAD_X * 2.0, CHIP_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());

    let painter = ui.painter();
    painter.rect_filled(
        rect,
        CornerRadius::same((CHIP_HEIGHT / 2.0) as u8),
        colors.bg,
    );
    painter.galley(
        Pos2::new(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        colors.fg,
    );

    response.widget_info(move || WidgetInfo::labeled(WidgetType::Label, true, text.to_owned()));
    response
}

// --- Trees & lists -----------------------------------------------------------

/// Fixed-height tree row (24px): hover SURFACE_2, selected = BRAND fill with
/// brand-ink content (§7.1/§7.2).
pub fn tree_row(ui: &mut Ui, selected: bool, contents: impl FnOnce(&mut Ui)) -> Response {
    row_impl(ui, selected, contents)
}

/// Generic list row with hover feedback only — no persistent selection.
pub fn selectable_row(ui: &mut Ui, contents: impl FnOnce(&mut Ui)) -> Response {
    row_impl(ui, false, contents)
}

fn row_impl(ui: &mut Ui, selected: bool, contents: impl FnOnce(&mut Ui)) -> Response {
    let width = ui.available_width();
    // Reserve the exact row space up-front so the centered cross-layout can
    // never swallow the parent's remaining height.
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, ROW_HEIGHT), Sense::hover());

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

    let fill = row_fill(selected, response.hovered());
    if fill != Color32::TRANSPARENT {
        // Paint behind the already-emitted content shapes.
        let mut bg = ui.painter().clone();
        bg.set_layer_id(egui::LayerId::new(egui::Order::Background, response.id));
        bg.rect_filled(rect, CornerRadius::same(RADIUS_SM), fill);
    }
    focus_ring(ui, &response);

    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), ""));
    response
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
        .corner_radius(CornerRadius::same(RADIUS_SM))
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
            let closure_label = placeholder.to_owned();
            resp.widget_info(move || {
                WidgetInfo::labeled(WidgetType::TextEdit, true, closure_label.clone())
            });
            resp
        })
        .inner
    });

    let edit_response = outer.inner;
    if edit_response.has_focus() {
        ui.painter().rect_stroke(
            outer.response.rect.expand(1.0),
            CornerRadius::same(RADIUS_SM),
            Stroke::new(1.0, Palette::BRAND),
            StrokeKind::Outside,
        );
    }
    edit_response
}

// --- Dialog chrome -----------------------------------------------------------

/// Result of [`dialog_header`]: the whole header strip plus the close (X)
/// button response.
pub struct DialogHeader {
    /// Response covering the full header strip.
    pub response: Response,
    /// The trailing X button — `.clicked()` means "close me".
    pub close: Response,
}

/// Dialog header strip (40px): title left, close X right (§7.1).
pub fn dialog_header(ui: &mut Ui, title: &str) -> DialogHeader {
    let width = ui.available_width();
    let title_font = bold_font_if_available(ui);
    let inner = ui.allocate_ui_with_layout(
        Vec2::new(width, DIALOG_HEADER_HEIGHT),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.label(RichText::new(title).font(title_font).color(Palette::INK));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                icon_button(ui, Icon::X)
            })
            .inner
        },
    );
    DialogHeader {
        response: inner.response,
        close: inner.inner,
    }
}

/// Bold body font via the named family registered by `install_fonts`,
/// falling back to the regular proportional face when fonts are not
/// installed yet (epaint panics on unbound families).
fn bold_font_if_available(ui: &Ui) -> FontId {
    let has_bold = ui.ctx().fonts(|f| {
        f.definitions()
            .families
            .contains_key(&FontFamily::Name(BOLD_FAMILY.into()))
    });
    if has_bold {
        FontId::new(14.0, FontFamily::Name(BOLD_FAMILY.into()))
    } else {
        FontId::new(14.0, FontFamily::Proportional)
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
