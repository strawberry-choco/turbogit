//! Branches-screen component kit (design doc §12–§14).
//!
//! The screen is mostly repetition of a small set of components. This module
//! owns that vocabulary: geometry constants (§12), decision functions
//! (row fills, sync badges, middle truncation, button states — §14), and the
//! rendering primitives (branch row, section header, kit buttons, overflow
//! cluster). Every visual decision maps onto the central
//! [`crate::theme::Palette`] tokens; the pure helpers are unit-testable and
//! the painted widgets follow the same token rules.
//!
//! No shadows anywhere (separation comes from surface color, §13). Icons are
//! inline SVG at 12–13px; every clickable target is ≥24px tall even when the
//! visible control is smaller (§14).

use egui::{
    Align, Color32, CornerRadius, Layout, Response, RichText, Sense, Stroke, StrokeKind, Ui, Vec2,
    WidgetInfo, WidgetType,
};

use super::icons::{self, Icon};
use super::widgets::{WidgetState, mix, tint_over_bg};
use crate::theme::{Palette, TYPE_BODY, TYPE_SECTION, chrome_font, data_font};

// --- §12 geometry ------------------------------------------------------------

/// Branch toolbar height (search + actions).
pub const TOOLBAR_H: f32 = 44.0;
/// Section header height (Local / Remote / Tags).
pub const SECTION_H: f32 = 26.0;
/// Branch row height — dense IDE list row.
pub const BRANCH_ROW_H: f32 = 30.0;
/// Branch detail panel width (inside the content area, right side).
pub const DETAIL_W: f32 = 280.0;
/// Left repo-tree sidebar / right metadata panel width.
pub const SIDE_PANEL_W: f32 = 220.0;
/// Horizontal padding inside list rows and headers.
pub const PAD_LIST: f32 = 16.0;
/// Padding inside toolbars and strips.
pub const PAD_STRIP: f32 = 12.0;
/// Padding inside side panels.
pub const PAD_PANEL: f32 = 16.0;

/// Icon drawing size for the branch screen — 12px (13px for the detail title
/// rows). Always paired with a ≥[`CLICK_TARGET_MIN`] hitbox.
pub const KIT_ICON: f32 = 12.0;
/// Slightly larger icon size for prominent rows.
pub const KIT_ICON_LARGE: f32 = 13.0;
/// Every clickable target is at least this tall (§14), even when the visible
/// control is smaller.
pub const CLICK_TARGET_MIN: f32 = 24.0;
/// Kit button height (on the dense scale; still ≥ the 24px target floor).
const KIT_BUTTON_H: f32 = 28.0;

// --- §14.1 branch row states ------------------------------------------------

/// The three fill states a branch row can be in while idle (design doc §13
/// Selection / §7.2 hover). Current, stale and mid-operation are orthogonal
/// markers rendered inside the row (see [`row_ink`], [`mid_op_label`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowState {
    Default,
    Hover,
    Selected,
}

/// Row fill decision: transparent at rest, the app-wide SURFACE_2 hover fill,
/// then the §13 Selection token for the active row.
pub fn row_fill(state: RowState) -> Color32 {
    match state {
        RowState::Default => Color32::TRANSPARENT,
        RowState::Hover => Palette::SURFACE_2,
        RowState::Selected => Palette::SELECTION,
    }
}

/// Branch-name ink: stale rows dim to muted — never hidden (§4, §14.1) —
/// everything else reads at primary.
pub fn row_ink(stale: bool) -> Color32 {
    if stale {
        Palette::T_MUTED
    } else {
        Palette::T_PRIMARY
    }
}

/// Mid-operation label, rendered in place of the row's sync marker: the state
/// is a first-class citizen, never an edge case (design doc §10).
pub fn mid_op_label(op: &str) -> String {
    format!("{op}…")
}

// --- §14.2 section header ------------------------------------------------------

/// Uppercase section label with the live count ("LOCAL 3"). The transform and
/// the count are mandatory (§3.3); the label is rendered in chrome type.
pub fn section_label(title: &str, count: usize) -> String {
    format!("{} {count}", title.to_uppercase())
}

/// One 26px section header: chevron, uppercase label, live count, optional
/// right-aligned trailing action. Returns the strip's response — clicking the
/// strip toggles expanded/collapsed. The trailing action is rendered *after*
/// the strip's own interact target, so egui hit-testing gives it precedence
/// and a Fetch button never toggles collapse.
pub fn section_header<R>(
    ui: &mut Ui,
    title: &str,
    count: usize,
    expanded: bool,
    actions: impl FnOnce(&mut Ui) -> R,
) -> Response {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, SECTION_H), Sense::hover());

    // The strip's toggle target registers first; later widgets inside the
    // rect (the trailing action) hit-test on top and keep their own clicks.
    let id = ui.auto_id_with(("section_header", title));
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, title));

    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );

    child.add_space(PAD_LIST);
    // Chevron: down when expanded, right when collapsed.
    let chevron = if expanded {
        Icon::CHEVRON_DOWN
    } else {
        Icon::CHEVRON_RIGHT
    };
    icons::icon(&mut child, chevron, KIT_ICON, Palette::T_MUTED);
    child.add_space(6.0);
    child.add(egui::Label::new(
        RichText::new(section_label(title, count))
            .font(chrome_font(TYPE_SECTION))
            .color(Palette::T_SECONDARY),
    ));

    // Trailing action, right-aligned (own child scope so it stays clickable
    // inside the strip's hover band).
    child.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.add_space(PAD_LIST);
        ui.scope(actions);
    });

    response
}

// --- §14.3 sync badge -----------------------------------------------------------

/// The sync relationship of one branch row — icon+count pairs, quiet in-sync
/// confirmation, or a deleted-upstream marker.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncKind {
    Ahead,
    Behind,
    Diverged,
    InSync,
    Gone,
}

/// The row's sync badge as `(kind, label)`. `None` means no upstream exists,
/// so there is nothing to say. Called only when the row tracks a remote.
pub fn sync_badge(ahead: usize, behind: usize, gone: bool) -> Option<(SyncKind, String)> {
    if gone {
        return Some((SyncKind::Gone, "gone".to_string()));
    }
    if ahead > 0 && behind > 0 {
        return Some((SyncKind::Diverged, format!("↑{ahead} ↓{behind}")));
    }
    if ahead > 0 {
        return Some((SyncKind::Ahead, format!("↑{ahead}")));
    }
    if behind > 0 {
        return Some((SyncKind::Behind, format!("↓{behind}")));
    }
    Some((SyncKind::InSync, "in sync".to_string()))
}

/// §13 meaning token for a sync badge's foreground.
pub fn sync_ink(kind: SyncKind) -> Color32 {
    match kind {
        SyncKind::Ahead | SyncKind::InSync => Palette::AHEAD,
        SyncKind::Behind | SyncKind::Diverged => Palette::BEHIND,
        SyncKind::Gone => Palette::DANGER,
    }
}

/// §13 meaning token for a sync badge's tinted background.
pub fn sync_bg(kind: SyncKind) -> Color32 {
    match kind {
        SyncKind::Ahead | SyncKind::InSync => tint_over_bg(Palette::AHEAD, 0.18),
        SyncKind::Behind | SyncKind::Diverged => tint_over_bg(Palette::BEHIND, 0.18),
        SyncKind::Gone => tint_over_bg(Palette::DANGER, 0.18),
    }
}

// --- §14.7 middle truncation ------------------------------------------------

/// Truncate a branch name in the middle so the distinguishing prefix and
/// suffix both stay readable (`feature/multi…executor`), never a bare end
/// ellipsis. `max_chars` is the full budget including the ellipsis.
pub fn middle_truncate(name: &str, max_chars: usize) -> String {
    if max_chars < 4 {
        // Degenerate budgets cannot hold both ends; keep a head-only slice —
        // still no bare trailing ellipsis.
        return name.chars().take(max_chars).collect();
    }
    if name.chars().count() <= max_chars {
        return name.to_string();
    }
    let keep = max_chars - 1; // room for the ellipsis
    // ~62% head / 38% tail gives the `feature/multi…executor` silhouette.
    let head = ((keep as f32 * 0.62).round() as usize).clamp(1, keep - 1);
    let tail = keep - head;
    let head_s: String = name.chars().take(head).collect();
    let tail_s: String = name
        .chars()
        .rev()
        .take(tail)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head_s}…{tail_s}")
}

// --- §14.5 buttons: exactly four variants -----------------------------------

/// The four kit button variants — primary (blue), secondary (raised), quiet
/// (text-only), danger (red text). Each drives default/hover/pressed/disabled
/// through [`KitButton::fill`] / [`KitButton::ink`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KitButton {
    Primary,
    Secondary,
    Quiet,
    Danger,
}

impl KitButton {
    /// Token-driven fill for one interactive state (§2.5 table + §14.5).
    pub fn fill(self, state: WidgetState) -> Color32 {
        use WidgetState::{Active, Disabled, Hovered, Idle};
        match (self, state) {
            (Self::Primary, Idle | Disabled) => Palette::ACCENT,
            (Self::Primary, Hovered) => mix(Palette::ACCENT, Color32::WHITE, 0.10),
            (Self::Primary, Active) => mix(Palette::ACCENT, Color32::WHITE, 0.20),
            (Self::Secondary, Idle | Disabled) => Palette::RAISED,
            (Self::Secondary, Hovered) => mix(Palette::RAISED, Color32::WHITE, 0.10),
            (Self::Secondary, Active) => mix(Palette::RAISED, Color32::WHITE, 0.20),
            (Self::Quiet, Idle | Disabled) => Color32::TRANSPARENT,
            (Self::Quiet, Hovered) => Palette::SURFACE_2,
            (Self::Quiet, Active) => Palette::SURFACE_3,
            (Self::Danger, Idle | Disabled) => Color32::TRANSPARENT,
            (Self::Danger, Hovered) => tint_over_bg(Palette::DANGER, 0.18),
            (Self::Danger, Active) => tint_over_bg(Palette::DANGER, 0.30),
        }
    }

    /// Token-driven ink for one interactive state.
    pub fn ink(self, state: WidgetState) -> Color32 {
        use WidgetState::{Active, Disabled, Hovered, Idle};
        match (self, state) {
            (_, Disabled) => Palette::T_MUTED,
            (Self::Primary, _) => Palette::BRAND_INK,
            (Self::Danger, _) => Palette::DANGER,
            (_, Idle) => Palette::T_SECONDARY,
            (_, Hovered | Active) => Palette::T_PRIMARY,
        }
    }
}

/// Paint one kit button (28px tall — above the 24px target floor). Renders
/// the four interactive states through [`KitButton::fill`]/[`ink`] with the
/// §13 control radius and a brand focus ring.
pub fn kit_button(ui: &mut Ui, kind: KitButton, label: &str) -> Response {
    let font_id = chrome_font(TYPE_BODY);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font_id, Color32::WHITE);
    let pad_x = 12.0;
    let width = pad_x * 2.0 + galley.size().x;
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(width, KIT_BUTTON_H),
        if ui.is_enabled() {
            Sense::click()
        } else {
            Sense::hover()
        },
    );

    let state = if ui.is_enabled() {
        if response.is_pointer_button_down_on() {
            WidgetState::Active
        } else if response.hovered() {
            WidgetState::Hovered
        } else {
            WidgetState::Idle
        }
    } else {
        WidgetState::Disabled
    };

    let painter = ui.painter().clone();
    let radius = CornerRadius::same(Palette::RADIUS_CONTROL);
    let fill = kind.fill(state);
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
    let ink = kind.ink(state);
    painter.galley_with_override_text_color(
        egui::Pos2::new(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        ink,
    );

    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), label));
    response
}

// --- §14.8 row action cluster --------------------------------------------------

/// The ⋯ overflow trigger: an icon-only quiet button whose *hitbox* is the
/// 24px floor while the visible icon stays 12px (design doc §14: clickable
/// targets are always at least 24px tall).
pub fn overflow_button(ui: &mut Ui, label: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(CLICK_TARGET_MIN, CLICK_TARGET_MIN),
        Sense::click(),
    );
    let hovered = response.hovered() || response.is_pointer_button_down_on();
    let ink = if hovered {
        Palette::T_PRIMARY
    } else {
        Palette::T_MUTED
    };
    // Centered icon inside the allocated hitbox (12px visible, 24px target).
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(
        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
    ));
    icons::icon(&mut child, Icon::MORE_HORIZONTAL, KIT_ICON, ink);

    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    response
}

// --- Detail panel building blocks ------------------------------------------------

/// The detail panel header: 13px title in data type, with a 1px divider
/// underneath (§13 type detail title, §12 padding 16).
pub fn detail_panel_header(ui: &mut Ui, title: &str) {
    let width = ui.available_width();
    ui.allocate_ui_with_layout(
        Vec2::new(width, 36.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.add_space(PAD_PANEL);
            ui.add(egui::Label::new(
                RichText::new(title)
                    .font(data_font(crate::theme::TYPE_DETAIL_TITLE))
                    .color(Palette::T_PRIMARY),
            ));
        },
    );
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 1.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, Palette::DIVIDER);
}

/// A key/value row in the detail panel: muted 11px key, primary 12px value.
pub fn key_value_row(ui: &mut Ui, key: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.add_space(PAD_PANEL);
        ui.add(egui::Label::new(
            RichText::new(key)
                .font(chrome_font(crate::theme::TYPE_CONTROL))
                .color(Palette::T_MUTED),
        ));
        ui.add(egui::Label::new(
            RichText::new(value)
                .font(data_font(TYPE_BODY))
                .color(Palette::T_PRIMARY),
        ));
    });
    ui.add_space(2.0);
}
