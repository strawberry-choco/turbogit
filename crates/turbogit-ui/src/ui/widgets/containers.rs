//! Dialog, card, tool-window, feedback-container, and commit-detail composition.

use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Frame, InnerResponse, Layout, Margin,
    Painter, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2,
};

use super::chips::BADGE_TINT;
use super::chips::count_chip;
use super::controls::tint_over_bg;
use super::inputs::INPUT_ICON_SIZE;
use crate::theme::{CONTROL_RADIUS, Palette, TYPE_CONTROL, TYPE_SECTION};
use crate::ui::icons::{self, Icon};
use turbogit_services::history_editor::RebaseCaution;

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

/// Which edge of a rect a hairline rule runs along. The set is closed: a division
/// *inside* a region is a placed rule, not an edge.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edge {
    Bottom,
    Top,
    Right,
}

/// The shared hairline-edge rule: a 1px line along one edge of `rect`, in `ink`,
/// with **no layout effect**.
///
/// The `±0.5` offset is the whole correctness argument, and why this is one function
/// rather than a shape a caller copies: a 1px stroke centred on an integer coordinate
/// straddles two device pixels and paints both at half coverage, so a rule that looks
/// right in the geometry is blurred. The offset lands the centre on a half coordinate,
/// which at any integer `pixels_per_point` falls inside exactly one pixel row.
///
/// `ink` is the caller's: the named hairline roles in `theme` decide which tone an edge
/// wears. `docs/design-system-roles.md` records a structural edge wearing
/// [`Palette::LINE`] as a known mismatch — do not quietly correct it here.
pub fn edge_rule(painter: &Painter, rect: Rect, edge: Edge, ink: Color32) {
    let (a, b) = match edge {
        Edge::Bottom => (
            Pos2::new(rect.left(), rect.bottom() - 0.5),
            Pos2::new(rect.right(), rect.bottom() - 0.5),
        ),
        Edge::Top => (
            Pos2::new(rect.left(), rect.top() + 0.5),
            Pos2::new(rect.right(), rect.top() + 0.5),
        ),
        Edge::Right => (
            Pos2::new(rect.right() - 0.5, rect.top()),
            Pos2::new(rect.right() - 0.5, rect.bottom()),
        ),
    };
    painter.line_segment([a, b], Stroke::new(1.0, ink));
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

/// Width of the one hairline a card is allowed to paint around itself, and
/// only under [`CardFrame::bordered`]. A named constant because egui folds the
/// stroke width into the frame's inner margin, so this number is also the
/// number of points of padding a bordered card gains on every side.
/// Crate-internal: `commit_window.rs` spells its own divider width on purpose.
const BORDER_HAIRLINE_WIDTH: f32 = 1.0;

/// The two tones a card frame may wear: the fill it paints with, which a
/// bordered and an unbordered card choose from the same two rungs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CardSurface {
    /// The content tone — a card sitting on a `BG` panel.
    Content,
    /// The raised tone — a card sitting on a `CONTENT_BG` surface, where a
    /// `CONTENT_BG` fill would be invisible.
    Raised,
}

/// How a card claims its space inside its parent. `Eq` is not derivable here
/// because the pinned width is an `f32`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CardSizing {
    /// Span the parent's full available width, so a card reads as a region
    /// rather than hugging its content.
    Stretch,
    /// Span the parent's full available width **and** height, for a card that *is* a
    /// pane. The height must be reserved **before** the contents run: a min-height
    /// claim measured afterwards comes from the content, so a tall body pushes the
    /// surface past the region it was meant to fill.
    StretchHeight,
    /// Pin a minimum width and let the content grow past it — for a card that
    /// floats above its parent (an overlay) rather than filling a pane.
    MinWidth(f32),
}

/// Card geometry: the fill and corner radius that make a panel read as a card,
/// the edge it wears (see [`CardFrame::bordered`]), plus the two things a call
/// site genuinely varies — inner padding and how the card claims its width. One
/// owner for the shape.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CardFrame {
    /// Which of the two card tones this frame wears.
    pub surface: CardSurface,
    /// Inner margin on every side, in points.
    pub pad: i8,
    /// How the card claims its width.
    pub sizing: CardSizing,
    /// Whether a 1px `LINE` hairline is drawn around the card's own edge.
    /// `false` by default, and `true` only for a surface that floats.
    pub bordered: bool,
}

impl Default for CardFrame {
    /// The default card: a *surface*, not a floating panel — no stroke.
    ///
    /// The rule this default exists to install is stated here, on the default
    /// itself, so a later reader asking "did we drop the border?" gets the
    /// answer in the code rather than in a test name or a design doc:
    ///
    /// > **A card is a surface, and a stroke means the surface floats.**
    ///
    /// A 1px hairline is therefore reserved for the four surfaces that really
    /// do sit above their surroundings — popovers, dialogs, menus and toasts —
    /// and for nothing else. A content region separates itself with the air it
    /// already carries: [`crate::theme::PANEL_PADDING`] on every side, which is
    /// exactly what the hairline was redundant with. That is why dropping the
    /// line does not collapse the region, and why there is no padding change to
    /// pair with it — a padding change would move every carded region's
    /// content.
    ///
    /// A region that genuinely wants a border is asking for a different
    /// surface, and asks for it with [`CardFrame::bordered`] rather than by
    /// painting its own hairline.
    fn default() -> Self {
        Self {
            surface: CardSurface::Content,
            pad: crate::theme::PANEL_PADDING as i8,
            sizing: CardSizing::Stretch,
            bordered: false,
        }
    }
}

impl CardFrame {
    /// Draw a 1px `LINE` hairline around the card's own edge — the one case in
    /// which a stroke is allowed to mean something, because the surface
    /// carrying it floats above its surroundings (a popover, a dialog, a menu,
    /// a toast).
    ///
    /// Reserved, not offered: a *content* region that calls this is asking for
    /// a different surface, and a view-phase pass is not a licence to hand the
    /// border back to a region that only wanted a line around its text. The
    /// app's one in-tree caller is the Welcome changelog, which lives in an
    /// `egui::Area` above the page.
    ///
    /// Two consequences worth knowing before picking it, both from egui
    /// folding `Frame::stroke.width` into the frame's inner margin
    /// (`Frame::total_margin`):
    ///
    /// - the body sits [`Self::pad`] + 1 pt from the card's edge, where the
    ///   unbordered card of the same padding sits [`Self::pad`] pt from it, and
    /// - the card is 2 pt taller than the unbordered one of the same content.
    pub fn bordered(self) -> Self {
        Self {
            bordered: true,
            ..self
        }
    }

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

    pub fn stretch_height(self) -> Self {
        Self {
            sizing: CardSizing::StretchHeight,
            ..self
        }
    }
}

/// Card surface: the containment every region gets, so neighbouring controls
/// read as one group instead of as a flat stack of headings and separators.
///
/// The default fill is [`Palette::CONTENT_BG`], deliberately *not* `BG` — the
/// panel behind a card is `BG`, so a `BG` card would be invisible (risk R2). A
/// card that sits on a `CONTENT_BG` surface asks for
/// [`CardFrame::raised`] instead. The card paints **no stroke**: the
/// [`CardFrame`] default states the rule, and [`CardFrame::bordered`] is the
/// only way to get a hairline. The body lays out inside the frame's margin
/// and, under the default [`CardSizing::Stretch`], is stretched to the
/// caller's full available width, so a card spans its pane rather than hugging
/// its content; a card that *is* a pane asks for [`CardSizing::StretchHeight`],
/// which fills the region in both axes. The returned rect is the card's outer edge,
/// which is what a caller capping a scroll area against its own container needs.
pub fn card<R>(
    ui: &mut Ui,
    frame: CardFrame,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<R> {
    let fill = match frame.surface {
        CardSurface::Content => Palette::CONTENT_BG,
        CardSurface::Raised => Palette::SURFACE,
    };
    let mut card = Frame::new()
        .fill(fill)
        .corner_radius(CornerRadius::same(crate::theme::CARD_RADIUS))
        .inner_margin(Margin::same(frame.pad));
    // The hairline is the *only* stroke a card may paint, it is off by default,
    // and it exists for a surface that floats — see `CardFrame::bordered`.
    if frame.bordered {
        card = card.stroke(Stroke::new(BORDER_HAIRLINE_WIDTH, Palette::LINE));
    }
    card.show(ui, |ui| {
        // Width is set **before** the contents run: `set_min_width` grows the Ui's
        // min rect, so afterwards it is a no-op. `StretchHeight`'s height is read
        // first and applied last.
        match frame.sizing {
            CardSizing::Stretch => {
                ui.set_width(ui.available_width());
                add_contents(ui)
            }
            CardSizing::StretchHeight => {
                let height = ui.available_height();
                ui.set_width(ui.available_width());
                let out = add_contents(ui);
                // `.max(0.0)`: contents taller than the region are the inner
                // scroller's overflow to handle, not the card's to shrink.
                ui.add_space((height - ui.min_rect().height()).max(0.0));
                out
            }
            CardSizing::MinWidth(w) => {
                ui.set_min_width(w);
                add_contents(ui)
            }
        }
    })
}

/// Header strip inside a [`card`]: the caller's own header rows, ruled off
/// from the body below by a hairline. Shared so every carded region gets
/// identical containment while each states what it holds.
///
/// This is a *rule inside* a card, not the card's own edge, so the "a stroke
/// means it floats" rule does not reach it: there is at most one of these per
/// carded region, and it divides a region rather than boxing one.
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
/// This is a *different primitive* from [`card`], not a bordered card, and it
/// strokes for a different reason: a `severity` ink, not a floating edge. Pass
/// a colour in `severity` for a note that is also a warning — the
/// merge-cascade and rebase summaries, which must read as attention without
/// becoming the app's contained alert ([`alert_box`] is filled rather than
/// stroked, and is the other one). Never fold this into
/// [`CardFrame::bordered`]: a note is an inset, and a bordered card is a
/// surface.
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

// --- Pane chrome (R7) ---------------------------------------------------------
//
// **One pane header, one column-header row.** A new pane makes no decision
// about its chrome because there is nothing to decide: the two functions below
// are the whole vocabulary, and the retired `toolwindow_header` is gone from
// this module rather than kept as a second way to say the same thing. The
// conflict grammar's module-private header was absorbed into [`pane_header`]
// for the same reason — a second implementation is not a harmless second
// spelling, it is a header the user can tell apart from the first one.

/// Height of the one pane header's band, in points.
///
/// A named constant because the claim "every tool pane has the same header" is
/// only checkable if the bands are comparable: `tests/widget_library.rs`
/// renders the worktrees, submodules and log panes and compares their painted
/// header geometry to each other, and that comparison means something because
/// all three read this one number.
pub const PANE_HEADER_HEIGHT: f32 = 28.0;

/// Height of the one column-header row's band. Sized to the 9px label it holds
/// plus its air, and the band the log's commit table already reserved for its
/// micro headers, so adopting the shared row moved the table by exactly the
/// underline it gained.
pub const COLUMN_HEADER_HEIGHT: f32 = 16.0;

/// Width of the one hairline a pane header and a column-header row each paint
/// under themselves. One pixel, and *one line*: two rules between a header and
/// its content is the nested-boxes problem this migration exists to remove, and
/// it comes back quietly — a caller wanting more air adds `ui.add_space`.
const HEADER_RULE_HEIGHT: f32 = 1.0;

/// Air between a pane header's title and its optional count chip.
const PANE_HEADER_CHIP_GAP: f32 = 6.0;

/// Allocate a header band of exactly `height` with **no** inter-item gap above
/// or below it, and run `add` in it.
///
/// egui's own allocation advances the caller's cursor past the rect *plus*
/// `spacing.item_spacing.y`, so a band followed by its own hairline would float
/// that hairline six points below the band it belongs to — a rule that is
/// decoration rather than structure. A header is band-then-rule with nothing in
/// between, so the gap is suppressed here and handed back to the caller
/// afterwards, which keeps the rhythm of everything *after* the header exactly
/// as it was.
fn header_band<R>(
    ui: &mut Ui,
    width: f32,
    height: f32,
    add: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<R> {
    let gap = ui.spacing().item_spacing.y;
    ui.spacing_mut().item_spacing.y = 0.0;
    let band = ui.allocate_ui_with_layout(
        Vec2::new(width, height),
        Layout::left_to_right(Align::Center),
        add,
    );
    ui.spacing_mut().item_spacing.y = gap;
    band
}

/// The one pane header (R7): a 9px tracked title in the muted ink, an optional
/// [count chip](count_chip), a right-aligned action slot, **one**
/// [`Palette::RULE_STRUCTURAL`] hairline, then the pane's content.
///
/// The single implementation every tool pane uses — worktrees, submodules, the
/// log's branches strip, the blame surface, and the changes card that joins it
/// in the changes-screen ticket. `count` is `None` for a pane with nothing to
/// count, and that is the common case: a count chip is a fact the pane already
/// knows, never a fourth kind of badge invented here.
///
/// The returned [`InnerResponse`] is the *band's* rect, not the header's whole
/// extent — a caller that wants to mark the header (the conflict grammar's
/// focus rail) measures the rule from its own cursor.
///
/// **The title is rendered exactly as written.** The case is the caller's word
/// to get right, because it is the pane's own title and the app renders every
/// one of them upper (the `BRANCHES` strip beside the log is the case that had
/// drifted, and it drifted *here*). `tests/widget_library.rs` pins that across
/// the panes it renders rather than hiding the decision inside a `to_uppercase`
/// nobody can see fail.
pub fn pane_header<R>(
    ui: &mut Ui,
    title: &str,
    count: Option<&str>,
    actions: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<R> {
    // The width is read ONCE, before the band is allocated. A band takes the
    // full available width, so re-reading it afterwards asks a horizontal
    // layout for the air its own first child already used — which is zero, and
    // which silently produced a hairline one pixel wide.
    let width = ui.available_width();
    let band = header_band(ui, width, PANE_HEADER_HEIGHT, |ui| {
        // The title and its count chip are one mark, not two: the gap between
        // them is named below rather than left to item spacing.
        ui.spacing_mut().item_spacing.x = 0.0;
        let (font, ink) = pane_title();
        ui.label(RichText::new(title).font(font).color(ink));
        if let Some(count) = count {
            ui.add_space(PANE_HEADER_CHIP_GAP);
            count_chip(ui, count);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), actions)
            .inner
    });
    pane_header_rule(ui, width);
    band
}

/// The one hairline a pane header paints under itself, and the shared body of
/// the header rule — [`Palette::RULE_STRUCTURAL`], the chrome that gives a
/// surface its structure, across `width`.
///
/// Split out because "the band" and "the rule" are two different measurements
/// and a caller that has to re-derive the rule's geometry to mark the header
/// will eventually derive it differently.
///
/// Returns the rule it painted, so a caller that needs the rule's rect
/// measures the same one instead of allocating a second strip and hoping it
/// lands in the same place.
fn pane_header_rule(ui: &mut Ui, width: f32) -> Rect {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, HEADER_RULE_HEIGHT), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, Palette::RULE_STRUCTURAL);
    rect
}

/// How one column of a column-oriented pane places its cells against its
/// offset.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColumnAlign {
    /// `x` is the offset from the pane's own left edge: the cell starts there.
    Start,
    /// `x` is the inset from the pane's trailing edge: the cell *ends* there.
    /// The log's date column is the case — it has to track a row that resizes.
    End,
}

/// One column of a column-oriented pane: the label its header cell shows, and
/// the offset its cells sit at, measured from the pane's own row rect.
///
/// **The same `x` places the header's label and the data row's cell.** That is
/// the entire reason this type exists, and it is why a pane passes one table
/// to [`column_header`] *and* reads the same table back when it lays out its
/// rows: a column cannot be narrow in the header and wide in the rows, because
/// there is one number for both and no second table for a header to consult.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PaneColumn {
    /// The label the header cell shows, in the app's one case.
    pub label: &'static str,
    /// The offset, interpreted by `align`.
    pub x: f32,
    /// Which edge `x` measures from.
    pub align: ColumnAlign,
}

impl PaneColumn {
    /// A column measured from the pane's own left edge.
    pub const fn start(label: &'static str, x: f32) -> Self {
        Self {
            label,
            x,
            align: ColumnAlign::Start,
        }
    }

    /// A trailing column measured from the pane's own right edge.
    pub const fn end(label: &'static str, inset: f32) -> Self {
        Self {
            label,
            x: inset,
            align: ColumnAlign::End,
        }
    }

    /// Where this column's cell starts, given the row rect both the header and
    /// the data rows measure from.
    pub fn origin(&self, row: Rect) -> f32 {
        match self.align {
            ColumnAlign::Start => row.left() + self.x,
            ColumnAlign::End => row.right() - self.x,
        }
    }
}

/// The one column-header row (R7): every column's label at 9px in the muted ink
/// over a single [`Palette::RULE_STRUCTURAL`] underline spanning the pane.
///
/// `row` is the rect the labels measure from, and it is the *same* rect the
/// pane's data rows measure from — which is what makes the labels sit over
/// their cells instead of near them. Returns the underline's rect.
///
/// **The ink is [`Palette::INK_3`], not [`Palette::INK_4`].** 9px is
/// normal-size text, `INK_4` is 3.2:1, and a column header is a label the user
/// reads to know what a bare number or timestamp means — never the dimmest
/// step. This is pinned in `tests/widget_library.rs` because the target frames
/// were drawn with the dim value before the contrast was checked, so the
/// temptation during implementation is to copy the frame.
pub fn column_header(ui: &mut Ui, row: Rect, columns: &[PaneColumn]) -> Rect {
    let (font, ink) = pane_title();
    // The width is read ONCE, before the band is allocated, for the same reason
    // as in `pane_header`: a band takes the full available width, so reading it
    // afterwards asks a horizontal layout for the air its own allocation used.
    let width = ui.available_width();
    let band = header_band(ui, width, COLUMN_HEADER_HEIGHT, |_| {});
    let rule = pane_header_rule(ui, width);
    // The labels are painted against the rule, not against the band callback's
    // idea of where it is. Two ways of getting this wrong look identical from
    // the band's own geometry: the rule is the band's *last* pixel, so it is
    // the one measurement in this function that is both known and checkable,
    // and every other position is derived from it rather than re-read from a
    // cursor that has already moved on.
    //
    // Painting them from inside the band callback instead — reading
    // `available_rect_before_wrap().top()`, which is where the band was *about
    // to be* — drew every label 8pt high and put the rule straight through the
    // lower third of them. `the_column_header_row_underlines_itself_once_in_the
    // _structural_tone` is the ratchet.
    let centre_y = rule.top() - HEADER_RULE_HEIGHT - COLUMN_HEADER_HEIGHT / 2.0;
    for column in columns {
        let galley = ui
            .painter()
            .layout_no_wrap(column.label.to_owned(), font.clone(), ink);
        let x = match column.align {
            ColumnAlign::Start => column.origin(row),
            // A trailing column's label ends where its cell ends.
            ColumnAlign::End => column.origin(row) - galley.size().x,
        };
        ui.painter()
            .galley(Pos2::new(x, centre_y - galley.size().y / 2.0), galley, ink);
    }
    let _ = band;
    rule
}

/// Run one data cell of a flow row at `columns[index]`'s own offset, reserving
/// whatever air is between the cursor and that offset first.
///
/// The column-oriented panes that lay their rows out as flows (worktrees,
/// submodules) use this rather than a grid, so a row's cells land on the same
/// offsets its header was painted from. A cell whose own content has already
/// passed its column's offset is **not** pushed backwards — the row runs on
/// instead, because a table that reversed to keep a column honest would be
/// worse than the overflow it is preventing. The log's commit table paints its
/// cells straight at [`PaneColumn::origin`] and needs nothing from here.
pub fn column_cell<R>(
    ui: &mut Ui,
    columns: &[PaneColumn],
    index: usize,
    cell: impl FnOnce(&mut Ui) -> R,
) -> R {
    let want = columns[index].origin(ui.max_rect());
    let have = ui.cursor().left();
    if want > have {
        ui.add_space(want - have);
    }
    cell(ui)
}

/// The one tracked micro type in the app: the section type size, in the chrome
/// face, in the muted ink — `(font, ink)`, so a title and a label cannot be
/// laid out with one and painted with the other.
///
/// Read by exactly two functions, [`pane_header`] and [`column_header`], which
/// is what "the tracking is uniform" means here: a pane title and a column
/// label are the same mark at two scales of importance, and the one thing that
/// must not differ between them is the type.
///
/// Note the ink: the muted step, which clears 4.5:1 on every surface a title is
/// painted on. The dim step is sub-AA and is reserved for placeholders, dim
/// path suffixes and hatches — a reader who reaches for it here because the
/// target frame is dimmer fails `a_column_header_is_ink_3_and_never_the_dim_ink`
/// on purpose.
fn pane_title() -> (FontId, Color32) {
    (
        FontId::new(TYPE_SECTION, FontFamily::Proportional),
        Palette::INK_3,
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
