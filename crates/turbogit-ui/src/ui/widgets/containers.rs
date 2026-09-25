//! Dialog, card, tool-window, feedback-container, and commit-detail composition.

use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Frame, InnerResponse, Layout, Margin, Pos2,
    Rect, RichText, Sense, Stroke, Ui, Vec2,
};

use super::chips::BADGE_TINT;
use super::controls::tint_over_bg;
use super::inputs::INPUT_ICON_SIZE;
use crate::theme::{CONTROL_RADIUS, Palette, TYPE_CONTROL};
use crate::ui::icons::{self, Icon};

const TOOLWINDOW_HEADER_HEIGHT: f32 = 28.0;
const CHURN_BAR_HEIGHT: f32 = 4.0;
const ALERT_ICON_SIZE: f32 = INPUT_ICON_SIZE;
const MICRO_TEXT: f32 = TYPE_CONTROL;
const BOLD_FAMILY: &str = "jetbrains-mono-bold";

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
/// padding, the containment a dialog hands a preview, a summary, or an
/// explanatory note.
///
/// This is the *unbordered* sibling of [`card`], which is a bordered content
/// region on `CONTENT_BG`. Pass a colour in `severity` for a note that is also a
/// warning — the merge-cascade and rebase summaries, which must read as
/// attention without becoming the app's contained alert ([`alert_box`] is
/// filled rather than stroked, and is the other one).
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
/// Author avatar diameter (mockup: a 28px initials circle).
pub const AVATAR_SIZE: f32 = 28.0;

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
