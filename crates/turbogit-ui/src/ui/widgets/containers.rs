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
use turbogit_services::history_editor::RebaseCaution;

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

/// The shared modal footer rule: the 1px line that separates a modal body from
/// its action slot, painted in [`Palette::RULE_FOOTER`].
///
/// Deliberately a stronger tone than the content divider
/// ([`Palette::RULE_CONTENT`]) — a modal's action slot is a *boundary*, not a
/// division, and the strength difference is intent rather than drift.
///
/// The allocation is deliberately the one `dialog_footer` has always made: a
/// full-available-width 1px band registered with [`Sense::hover`], painted as a
/// fill rather than stroked. Two reasons, both load-bearing:
///
/// - a fill is what the rest of the container vocabulary paints, and it is the
///   only primitive the painted-output harness can see, so a stroked rule
///   cannot be asserted on at all;
/// - egui's default `Ui::separator()` reserves a 6px band with the line at its
///   *centre* and paints a stroke in a tone no design token owns. A caller
///   migrating onto this rule that wants the old vertical rhythm back adds
///   `ui.add_space(5.0)` first — see `ui::interactive_rebase` — because the
///   reserved band, not just the painted row, is what preserves the layout.
pub(crate) fn footer_rule(ui: &mut Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 1.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, Palette::RULE_FOOTER);
}

/// Dialog footer: top [`Palette::RULE_FOOTER`] rule with right-aligned action
/// buttons (§7.1). The rule itself is [`footer_rule`]'s, shared with the other
/// modal bodies that rule themselves off from an action slot without owning a
/// footer; the gap below the rule and the action alignment stay this function's.
pub fn dialog_footer<R>(ui: &mut Ui, buttons: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    footer_rule(ui);
    ui.add_space(6.0);
    ui.with_layout(Layout::right_to_left(Align::Center), buttons)
}

// --- Section chrome ----------------------------------------------------------

/// The two tones a bordered card frame may wear.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CardSurface {
    /// The content tone — a card sitting on a `BG` panel.
    Content,
    /// The raised tone — a card sitting on a `CONTENT_BG` surface, where a
    /// `CONTENT_BG` fill would be invisible.
    Raised,
}

/// How a card claims its width inside its parent. `Eq` is not derivable here
/// because the pinned width is an `f32`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CardSizing {
    /// Span the parent's full available width, so a card reads as a region
    /// rather than hugging its content.
    Stretch,
    /// Pin a minimum width and let the content grow past it — for a card that
    /// floats above its parent (an overlay) rather than filling a pane.
    MinWidth(f32),
}

/// Card geometry: the fill, corner radius, and hairline stroke that make a
/// panel read as a card, plus the two things a call site genuinely varies —
/// inner padding and how the card claims its width. One owner for the shape.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CardFrame {
    /// Which of the two card tones this frame wears.
    pub surface: CardSurface,
    /// Inner margin on every side, in points.
    pub pad: i8,
    /// How the card claims its width.
    pub sizing: CardSizing,
}

impl Default for CardFrame {
    fn default() -> Self {
        Self {
            surface: CardSurface::Content,
            pad: crate::theme::PANEL_PADDING as i8,
            sizing: CardSizing::Stretch,
        }
    }
}

impl CardFrame {
    /// Wear the raised `SURFACE` tone instead of `CONTENT_BG`.
    pub fn raised(self) -> Self {
        Self {
            surface: CardSurface::Raised,
            ..self
        }
    }

    /// Override the inner margin.
    pub fn padded(self, pad: i8) -> Self {
        Self { pad, ..self }
    }

    /// Pin a minimum width instead of stretching to the parent's full width.
    pub fn min_width(self, width: f32) -> Self {
        Self {
            sizing: CardSizing::MinWidth(width),
            ..self
        }
    }
}

/// Bordered card surface (Local Changes redesign): the containment the
/// mockup gives every region, so neighbouring controls read as one group
/// instead of as a flat stack of headings and separators.
///
/// The default fill is [`Palette::CONTENT_BG`], deliberately *not* `BG` — the
/// panel behind a card is `BG`, so a `BG` card would be invisible (risk R2). A
/// card that sits on a `CONTENT_BG` surface asks for
/// [`CardFrame::raised`] instead. The body lays out inside the frame's margin
/// and, under the default [`CardSizing::Stretch`], is stretched to the
/// caller's full available width, so a card spans its pane rather than hugging
/// its content. The returned rect is the card's outer edge, which is what a
/// caller capping a scroll area against its own container needs (risk R1).
pub fn card<R>(
    ui: &mut Ui,
    frame: CardFrame,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<R> {
    let fill = match frame.surface {
        CardSurface::Content => Palette::CONTENT_BG,
        CardSurface::Raised => Palette::SURFACE,
    };
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, Palette::LINE))
        .corner_radius(CornerRadius::same(crate::theme::CARD_RADIUS))
        .inner_margin(Margin::same(frame.pad))
        .show(ui, |ui| {
            match frame.sizing {
                CardSizing::Stretch => ui.set_width(ui.available_width()),
                CardSizing::MinWidth(w) => ui.set_min_width(w),
            }
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

/// The CAUTIONS rail a history rewrite shows before it runs: a titled count and
/// one warning line per caution, in warning ink, and nothing at all when the
/// plan is clean.
///
/// SHARED by every surface that rewrites history — the interactive rebase
/// editor's right rail and the drop preflight — because "what is likely to go
/// wrong" is one set of facts with one wording, and two copies of this paint
/// path would drift the moment a caution is added. A clean plan paints nothing:
/// an empty titled section is a section the developer has to read past.
pub fn cautions_rail(ui: &mut Ui, cautions: &[RebaseCaution]) {
    if cautions.is_empty() {
        return;
    }
    group_title(ui, &format!("CAUTIONS · {}", cautions.len()));
    for caution in cautions {
        let text = match caution {
            RebaseCaution::ConflictRisk { files } => format!(
                "Conflicts likely on {files} {}",
                if *files == 1 { "file" } else { "files" }
            ),
            RebaseCaution::MixedIdentities { authors } => format!(
                "Mixed committer identities ({authors} {}) — verify signatures",
                if *authors == 1 { "author" } else { "authors" }
            ),
        };
        ui.colored_label(Palette::STATE_WARNING, format!("⚠ {text}"));
    }
    ui.add_space(6.0);
}

/// The RECOVERY note a history rewrite shows beside its cautions: what the
/// backup ref is for, and which ref carries it.
///
/// SHARED with [`cautions_rail`] for the same reason — the promise that a
/// rewrite can be undone, and the name of the thing that undoes it, are one
/// wording. The restore ACTION is not here: only the editor runs a replay
/// long enough for "Abort & restore" to mean anything, so that button stays in
/// the editor.
pub fn recovery_note(ui: &mut Ui, backup_ref: &str) {
    group_title(ui, "RECOVERY");
    ui.label("A backup ref is written before the first commit is replayed, and the recovery path restores the pre-rewrite state from it.");
    ui.monospace(backup_ref.to_owned());
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
