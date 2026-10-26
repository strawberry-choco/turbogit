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
    Align, Color32, CornerRadius, FontId, Layout, Pos2, Response, RichText, Sense, Stroke,
    StrokeKind, Ui, Vec2, WidgetInfo, WidgetType,
};

use super::icons::{self, Icon};
use super::widgets::{
    CHIP_PAD_X, COMPACT_CHIP_GEOMETRY, COUNT_CHIP_COLORS, REF_CHIP_COLORS, WidgetState, count_chip,
    mix, tint_over_bg,
};
use crate::theme::{Palette, TYPE_BODY, TYPE_CHIP, TYPE_SECTION, chrome_font, data_font};

// --- §12 geometry ------------------------------------------------------------

/// Branch toolbar height: one centered row of controls (search + actions).
pub const TOOLBAR_H: f32 = 36.0;
/// Section header height (Local / Remote / Tags).
pub const SECTION_H: f32 = 26.0;
/// Branch row height — dense IDE list row.
pub const BRANCH_ROW_H: f32 = 30.0;
/// Left repo-tree sidebar / right metadata panel width.
pub const SIDE_PANEL_W: f32 = 220.0;
/// Horizontal padding inside list rows and headers.
pub const PAD_LIST: f32 = 16.0;
/// Padding inside toolbars and strips.
pub const PAD_STRIP: f32 = 12.0;

/// Icon drawing size for the branch screen — 12px (13px for the detail title
/// rows). Always paired with a ≥[`CLICK_TARGET_MIN`] hitbox.
pub const KIT_ICON: f32 = 12.0;
/// Slightly larger icon size for prominent rows.
pub const KIT_ICON_LARGE: f32 = 13.0;
/// Every clickable target is at least this tall (§14), even when the visible
/// control is smaller.
pub const CLICK_TARGET_MIN: f32 = 24.0;
/// Kit button height (on the dense scale; still ≥ the 24px target floor). Also
/// the height of one toolbar row.
pub const KIT_BUTTON_H: f32 = 28.0;

// --- §14.1 branch row states ------------------------------------------------

/// The fill states a row can be in while idle (design doc §13 Selection /
/// §7.2 hover). Stale and mid-operation are orthogonal markers rendered inside
/// the row (see [`row_ink`], [`mid_op_label`]); *current* is not — it owns its
/// own fill, through [`current_row_fill`].
///
/// Three of these are a selection, and they are three genuinely different jobs —
/// this is a set of three, not four, because one of the four was a duplicate:
///
/// - [`RowState::RowSelected`] — **the list row**: the opaque
///   [`Palette::ROW_SELECTED`] fill a chosen row in a tree or list takes, with
///   a [`crate::theme::RAIL_WIDTH`] accent rail at its leading edge, painted by
///   the one [`paint_rail`]. The row keeps its own ink on it — selecting a row
///   never inverts its text, which is the whole reason this state exists rather
///   than the solid brand band it replaces.
/// - **the current ref** — which branch HEAD points at is a fact about the
///   repository, not a row state, so it is not a variant here at all:
///   [`current_row_fill`] owns it, and the band it paints for a selected row is
///   [`Palette::SELECTION`]. The enum used to carry a *fourth* selection
///   variant that painted that same band through [`row_fill`], duplicating what
///   [`current_row_fill`] already answered for every selection state. It is
///   gone, which is what makes "no list row resolves to the current-ref token" a
///   property of this enum rather than a convention.
/// - [`RowState::FocusSelected`] — the translucent [`Palette::selection_bg()`]
///   focus band the log table, the sidebar tree and blame paint. Conformance
///   issue 07 names it because `selection_bg()` was being re-derived inline at
///   three different corner radii with no role to point at. It is also the band
///   a rail rides on: the sidebar's active band and the log's own rows pair it
///   with [`paint_rail`] through [`row_shell`], which is why the rail's
///   condition is [`RowState::is_selected`] rather than one variant.
///
/// The two-state constructor every hand-painted row uses,
/// [`RowState::from_flags`], resolves `selected` to the list row — so the log,
/// the settings rail, the rebase todo, the welcome recents and the
/// multi-selection panel all take the *same* selection, which is the point:
/// three screens that each invented a selection used to produce three
/// selections.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowState {
    Default,
    Hover,
    RowSelected,
    FocusSelected,
}

impl RowState {
    /// The state of a row that tracks selection and hover as two booleans —
    /// every hand-painted row in this crate. `selected` resolves to
    /// [`RowState::RowSelected`], the one list-row selection fill; a row that
    /// wants the focus band instead builds [`RowState::FocusSelected`] itself.
    pub fn from_flags(selected: bool, hovered: bool) -> Self {
        match (selected, hovered) {
            (true, _) => Self::RowSelected,
            (false, true) => Self::Hover,
            (false, false) => Self::Default,
        }
    }

    /// Whether this state is one of the two **selection** roles — the rows a
    /// rail marks as chosen.
    ///
    /// The rule is *chosen*, not *this particular fill*: `RowSelected` (the
    /// opaque list-row fill) and `FocusSelected` (the translucent focus band)
    /// are two bands for the same fact, and the sidebar has always painted them
    /// as a pair — its [`paint_rail`] rides on the focus band's left edge. So
    /// the rail's condition is asked of the state rather than of one variant,
    /// which is what lets the log's own rows carry the band *and* the rail
    /// through the one shell ([`row_shell`]) instead of every surface's
    /// selection site deciding for itself.
    ///
    /// Enumerated rather than inferred from a fill comparison, so a fourth state
    /// that is a selection has to answer this question instead of inheriting an
    /// answer by accident.
    pub fn is_selected(self) -> bool {
        matches!(self, Self::RowSelected | Self::FocusSelected)
    }
}

/// What a row shell lays down in a row rect: the fill alone, or the fill plus
/// the leading-edge accent rail.
///
/// **Both variants take an already-allocated rect and paint into it.** There is
/// no variant that measures, allocates or lays out: the row's height, its width
/// and its position are the caller's, decided before the shell is called, and a
/// shell that re-measured would move a row's own geometry the moment it
/// selected.
///
/// That is what makes the log possible. The commit table is virtualised — it
/// hands `show_rows` a measured pitch and builds only the visible window — so a
/// shell that re-measured per row would rewrite the hottest path in the app to
/// adopt a row vocabulary. [`RowShell::Railed`] is the escape hatch: the log
/// allocates its rows exactly as it always has and shares the fills, the rail and
/// the ink ramp. `tests/branch_component_kit.rs` pins the contract from the
/// signature and the body, since a shell that started measuring would be
/// invisible from a painted seam.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowShell {
    /// The one row-state fill at the control radius, and nothing else — the row
    /// vocabulary's base form, for the hand-painted tables that want the fill and
    /// bring their own selection marker.
    Fill,
    /// The fill **plus** [`paint_rail`] at the row's leading edge, for a chosen
    /// row. The rail is paint over the fill, never padding beside it, so a
    /// selected row's text origin *is* an unselected row's — which is the
    /// property the log's rows are held to.
    Railed,
}

/// Paint one row's shell: the fill at the control radius, and — for
/// [`RowShell::Railed`] on a state [`RowState::is_selected`] — the one accent
/// rail at the row's leading edge.
///
/// **The rect is the caller's, already allocated.** Nothing here measures,
/// allocates, or lays out: the body names no `allocate*`, no `available_*`, no
/// `spacing()`, and no font. A row that needs its own geometry lays it out first
/// (which is also the only order in which a row's own buttons keep their clicks)
/// and hands the rect in.
///
/// `tests/branch_component_kit.rs` asserts that from the source, because the
/// failure it prevents is not observable in a frame: a shell that re-measured
/// would still paint a correct-looking row, one pitch out of step with the
/// virtualised list that asked for it.
///
/// The `Fill` variant is the row fill's one construction site. It is the
/// former `fill`, with its decision, its control radius and its "no text
/// geometry" rule absorbed here; the two spellings were the same
/// `(ui, rect, state)` call, and a screen that reached the fill directly rather
/// than through the shell is the two-opinion case the row vocabulary exists to
/// prevent.
pub fn row_shell(ui: &Ui, rect: egui::Rect, state: RowState, variant: RowShell) {
    let fill = row_fill(state);
    if fill != Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(crate::theme::CONTROL_RADIUS), fill);
    }
    if variant == RowShell::Railed && state.is_selected() {
        paint_rail(ui.painter(), rect);
    }
}

/// Row fill decision: transparent at rest, the raised-on-card hover fill, then
/// whichever of the two row-level selection roles the row is in.
///
/// **The negative rule, stated at the construction site: no list row resolves to
/// [`Palette::SELECTION`].** That token is the current-ref treatment and
/// belongs to [`current_row_fill`]; a row that is merely *chosen* takes
/// [`Palette::ROW_SELECTED`]. Nothing in this function may answer
/// `Palette::SELECTION`, and `tests/branch_component_kit.rs` pins that by
/// enumerating every variant rather than by scanning source — a render seam can
/// only prove the rule at the sites a test enumerates, and this is the site
/// where the fills are chosen.
pub fn row_fill(state: RowState) -> Color32 {
    match state {
        RowState::Default => Color32::TRANSPARENT,
        // Hover is "raised relative to the surface it actually sits on", and a
        // list row sits on the content surface — so it takes the content rung of
        // the ladder, not the app/panel one. Same value as before this role was
        // named; the name is the point.
        RowState::Hover => Palette::RAISED_ON_CARD,
        RowState::RowSelected => Palette::ROW_SELECTED,
        RowState::FocusSelected => Palette::selection_bg(),
    }
}

/// The one accent rail: [`crate::theme::RAIL_WIDTH`] of [`Palette::BRAND`] at
/// the row's **leading edge**, for the row's full height, in a rect the caller
/// has already allocated.
///
/// The accent's only row-shaped use (R1/R4), and it is **paint, not layout**:
/// it is drawn *over* the row's own fill rather than beside it, so a selected
/// row's text origin *is* an unselected row's text origin. A rail reserved as
/// padding shifts every row's content the moment it is selected, which is the
/// drift `tests/branch_component_kit.rs` asserts against painted output.
///
/// **Every rail in the tree paints through here**, which is the
/// whole point of having one painter: the sidebar's `paint_active_band` used to
/// *stroke* a 2 px segment inset 1 px from the leading edge and now calls this
/// (so the rail sits flush at the leading edge, where a painted rail belongs).
/// One painter means a row that grows a rail places it
/// identically wherever it appears, and the width is read from the token layer
/// rather than restated at either site.
///
/// Deliberately not [`crate::theme::MARK_RADIUS`]: that is a 2 px **radius**,
/// this is a 2 px **width**.
///
/// **The log's stripe collision, resolved (conformance issue 14).** The log
/// painted a 3 px full-height per-root stripe at this same leading edge, and
/// the answer is that the two **share the gutter, rail first**: the rail leads
/// flush at the row's leading edge — where this painter puts it and where every
/// other rail in the app is — and the stripe sits immediately inside it, so
/// both are on screen at once and neither displaces the other. The cost the
/// ticket expected (5 px out of every row) does not arise, because the log's
/// gutter was *already* 5 px wide: 3 px of stripe plus 2 px of air. The rail
/// takes that air, the row's cells do not move by a point, and the stripe's own
/// 3 px survives on the one row the user is looking at. Conformance issue 15
/// gives the stripe a header and a name as the ROOTS column, measured from that
/// same gutter.
pub fn paint_rail(painter: &egui::Painter, rect: egui::Rect) {
    painter.rect_filled(
        egui::Rect::from_min_max(
            egui::Pos2::new(rect.left(), rect.top()),
            egui::Pos2::new(rect.left() + crate::theme::RAIL_WIDTH, rect.bottom()),
        ),
        CornerRadius::ZERO,
        Palette::BRAND,
    );
}

/// Fill for the row carrying the current branch, given its interaction state.
///
/// Which branch HEAD points at is a fact about the repository, not about the
/// pointer, so it answers at rest rather than only on hover — and it stays
/// *under* hover and selection instead of replacing them, so a current row that
/// is also selected reads as both facts. The resting band is a brand tint well
/// below [`Palette::SELECTION`]'s strength, which is what keeps the two apart:
/// the tint is a *background* for running text, and [`Palette::SELECTION`] is
/// the heavier step a current ref gets on top of it.
///
/// This is the one place [`Palette::SELECTION`] is reached. A branch row is only
/// ever in [`RowState::RowSelected`] when it is selected; the other selection
/// role belongs to surfaces with no current-branch concept, and it resolves the
/// same way here so no state can fall through.
pub fn current_row_fill(state: RowState) -> Color32 {
    match state {
        RowState::Default => tint_over_bg(Palette::BRAND, 0.16),
        RowState::Hover => tint_over_bg(Palette::BRAND, 0.24),
        RowState::RowSelected | RowState::FocusSelected => Palette::SELECTION,
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

// --- R6: the mark pair — a state dot and coloured state text --------------------

/// Radius of the mark pair's leading dot. A **dot**, deliberately not a chip:
/// a mark has no fill rect, no radius and no geometry of its own, so a state
/// can never acquire a chip's shape by accident.
///
/// 3px, so the dot reads as punctuation before the words rather than as a
/// bullet of its own.
pub const STATE_DOT_R: f32 = 3.0;

/// The mark pair's geometry, measured in the face its text is painted in. The
/// dot, its offset and its gap are decided in exactly one place.
///
/// A mark with no words is a dot alone, so it reserves no gap for text it is
/// not painting.
fn mark_width(ui: &Ui, text: &str, font: &FontId) -> f32 {
    if text.is_empty() {
        return STATE_DOT_R * 2.0;
    }
    STATE_DOT_R * 2.0
        + 6.0
        + ui.painter()
            .layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE)
            .size()
            .x
}

/// **The mark pair**: a leading dot and the state in words, in one allocation.
///
/// R6's replacement for a state that used to arrive as a filled pill. A
/// repository state is a fact about the repository — not a ref and not a number
/// — so it is a coloured word from [`crate::theme::RepoState::color`], with no
/// fill behind the text and no chip geometry anywhere in it. A state that stands
/// alone keeps this dot; the branches band and tree paint [`state_words`] and
/// [`badge_words`] instead.
///
/// The dot is painted here, by this function, because this is the only place
/// the mark's geometry is decided: a circle at the leading edge's
/// [`STATE_DOT_R`] offset and the row's vertical centre, taking its colour as
/// an argument and holding no state vocabulary of its own — so a second colour
/// map cannot start here. It is **not** a fill rect either, which is the whole
/// of "a mark has no chip body": a state behind a rect stops reading as state
/// and starts reading as a category.
///
/// The text is interface, not data, so it reads in the proportional face at the
/// chip's type size: a state is a sentence rather than a ref name. A row's
/// direction badge is a count and takes the data face through [`badge_words`]
/// instead; the band's summary is the one place counts and words share this
/// chrome galley, its arrows riding the fallback chain like the sidebar's rows.
pub fn state_mark(ui: &mut Ui, text: &str, color: Color32) -> Response {
    mark(ui, text, color, &chrome_font(TYPE_CHIP))
}

/// The words-only form's measured width: the text as it will paint, with no dot
/// and no gap. One implementation beside the paint it measures, so no caller's
/// reserved slot can disagree with what reaches the screen.
fn words_width(ui: &Ui, text: &str, font: &FontId) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    ui.painter()
        .layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE)
        .size()
        .x
}

/// The words-only form: one state or one badge in coloured text, in a
/// caller-chosen face. [`state_words`] and [`badge_words`] are its two faces.
fn words(ui: &mut Ui, text: &str, color: Color32, font: &FontId) -> Response {
    let w = words_width(ui, text, font);
    let h = super::widgets::CHIP_HEIGHT;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
    if ui.is_rect_visible(rect) {
        let galley = ui
            .painter()
            .layout_no_wrap(text.to_owned(), font.clone(), color);
        ui.painter().galley(
            Pos2::new(rect.left(), rect.center().y - galley.size().y / 2.0),
            galley,
            color,
        );
    }
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, text));
    response
}

/// How wide [`state_words`] will paint `text` — the measured words alone, with
/// no mark in front.
pub fn state_words_width(ui: &Ui, text: &str) -> f32 {
    words_width(ui, text, &chrome_font(TYPE_CHIP))
}

/// The state in coloured words alone — no [`state_mark`] dot.
///
/// The branches band's summary and the branch tree's rows both read state this
/// way: the head-ref chip and the Fetch already fix what the band is, and a
/// row's own ref chip does the same, so the words are a sentence rather than a
/// mark needing a shape. The words keep the mark pair's other half exactly: one
/// colour from [`crate::theme::RepoState::color`], the chrome face at the chip
/// size, no fill, no chip geometry. A state with nothing to report paints
/// nothing at all, so the band skips the call on an empty summary.
pub fn state_words(ui: &mut Ui, text: &str, color: Color32) -> Response {
    words(ui, text, color, &chrome_font(TYPE_CHIP))
}

/// The mark pair painted in a caller-chosen face: see [`mark_width`] for why
/// there is only one copy of the geometry.
fn mark(ui: &mut Ui, text: &str, color: Color32, font: &FontId) -> Response {
    let w = mark_width(ui, text, font);
    let h = super::widgets::CHIP_HEIGHT;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter().circle_filled(
            Pos2::new(rect.left() + STATE_DOT_R, rect.center().y),
            STATE_DOT_R,
            color,
        );
        let galley = ui
            .painter()
            .layout_no_wrap(text.to_owned(), font.clone(), color);
        ui.painter().galley(
            Pos2::new(
                rect.left() + STATE_DOT_R * 2.0 + 6.0,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            color,
        );
    }
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, text));
    response
}

/// How wide [`badge_words`] will paint one sync badge.
pub fn badge_words_width(ui: &Ui, kind: SyncKind, text: &str) -> f32 {
    words_width(ui, text, &badge_face(kind))
}

/// The sync badge as coloured glyphs alone: the direction arrow and its count,
/// without the mark pair's leading dot.
pub fn badge_words(ui: &mut Ui, kind: SyncKind, text: &str, color: Color32) -> Response {
    words(ui, text, color, &badge_face(kind))
}

/// The face one sync badge's label is painted in, and why it is not the chrome
/// face. Ahead and behind lead with a direction arrow, and the chrome chain owns
/// U+2191/U+2193 only through a fallback face — so a chrome `↑2` lays its arrow
/// out in one cut and its digit in another, which is two typefaces inside a
/// three-glyph mark. JetBrains Mono carries both arrow and digit, so the badge
/// reads as one thing, and a count with its direction glyph on it is data by §19
/// regardless. `gone` is still a word, so it stays in chrome like every other
/// state.
///
/// The coverage fact is contingent (an added fallback can supply the arrows) and
/// so is not the claim any test makes: what is pinned is that the data face owns
/// both glyphs, and that a row's badge paints in the monospace family.
fn badge_face(kind: SyncKind) -> FontId {
    match kind {
        SyncKind::Ahead | SyncKind::Behind => data_font(TYPE_CHIP),
        SyncKind::Diverged | SyncKind::InSync | SyncKind::Gone => chrome_font(TYPE_CHIP),
    }
}

// --- R6: the ref chip, in the row's own ink ------------------------------------

/// The ref chip's **shape**, with a caller-chosen ink: the shared compact chip
/// geometry and [`REF_CHIP_COLORS`]'s raised-on-card fill, so a ref name is the
/// same object everywhere it appears, and only the ink differs.
///
/// Why the ink is a parameter. The shared [`super::widgets::ref_chip`] is right
/// for a ref name that stands alone in chrome, where [`Palette::INK_2`] is the
/// legal step on a raised surface. A **branch row's** name is the row's identity
/// text, and the row owns its identity ink — [`row_ink`], which dims a stale row
/// and reads at primary otherwise, and which `tests/branch_component_kit.rs`
/// pins against `Palette::T_PRIMARY` for both branches. Re-inking the chip to
/// the row's own step is what keeps "a branch name is a ref chip" true without
/// also dimming every name in the list by a step the token layer does not ask
/// for. The fill, the radius, the type size and the face are the shared ones, so
/// this is the ref chip and not a fourth chip: nothing here names a new pair.
pub fn ref_chip_with_ink(ui: &mut Ui, text: &str, ink: Color32) -> Response {
    let font = data_font(TYPE_CHIP);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, ink);
    let size = COMPACT_CHIP_GEOMETRY.size(&galley);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        COMPACT_CHIP_GEOMETRY.paint(ui.painter(), rect, galley, REF_CHIP_COLORS.bg, ink);
    }
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, text));
    response
}

/// The fill [`ref_chip_with_ink`] paints, read through the one ref-chip role
/// rather than restated.
///
/// A caller-chosen **ink** is what that variant is for; a caller-chosen *fill*
/// would be a fourth chip, and the difference is only visible from outside this
/// module — so it is a function rather than a private constant, and
/// `tests/widget_library.rs`'s whole-tree ref-chip ratchet asserts that it is
/// the ref chip's own fill.
pub fn ref_chip_with_ink_fill() -> Color32 {
    REF_CHIP_COLORS.bg
}

/// How wide [`ref_chip_with_ink`] paints `text` — the shared geometry's own
/// measure, so a row reserving space for the name reserves exactly what the
/// chip takes.
pub fn ref_chip_with_ink_width(ui: &Ui, text: &str) -> f32 {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), data_font(TYPE_CHIP), Color32::WHITE);
    COMPACT_CHIP_GEOMETRY.size(&galley).x
}

// --- R7: the pane's scope control ----------------------------------------------

/// The branches pane's **scope chip**: the count chip's treatment — the raised
/// fill for the chrome row it sits on, with secondary ink — carrying a chevron
/// because it is a **control**, and the one chip in the app that is one.
///
/// The earlier comment under this function said a chip would "newly register an
/// accessibility node for a piece of status text". ADR-0027 reverses that and
/// answers it: once the scope indicator is something you press to change what
/// the list below is filtered to, the node it registers is the node for a
/// control with a state and an action, and a screen-reader user gains the filter
/// rather than losing status text. So the accessible label is the **state** —
/// "all 2 repos", "filtered to beta" — and never a bare word like "Scope": the
/// node has to describe what the pane is showing before it says it can be
/// changed.
///
/// It is the only place in the pane with a click plane among the chips, and it
/// paints no brand: the one blue object in the branches toolbar is New Branch.
pub fn scope_chip(ui: &mut Ui, label: &str) -> Response {
    let font = chrome_font(TYPE_CHIP);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, COUNT_CHIP_COLORS.fg);
    let chevron = super::widgets::CHIP_HEIGHT * 0.5;
    let w = galley.size().x + CHIP_PAD_X * 2.0 + chevron + 4.0;
    let h = super::widgets::CHIP_HEIGHT;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
    if ui.is_rect_visible(rect) {
        ui.painter().rect_filled(
            rect,
            super::widgets::compact_chip_radius(),
            COUNT_CHIP_COLORS.bg,
        );
        // Painted at explicit positions inside the allocated rect, so the chip
        // is one object: the words lead, the chevron says "press me".
        ui.painter().galley(
            Pos2::new(
                rect.left() + CHIP_PAD_X,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            COUNT_CHIP_COLORS.fg,
        );
        icons::paint_icon(
            ui.painter(),
            Pos2::new(
                rect.right() - CHIP_PAD_X - chevron,
                rect.center().y - chevron / 2.0,
            ),
            chevron,
            Icon::CHEVRON_DOWN,
            COUNT_CHIP_COLORS.fg,
        );
    }
    // The state, not the word "scope": a node that announced a bare noun would
    // tell a screen-reader user that a control exists and not what it is set to.
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    response
}

/// Mid-operation label, rendered in place of the row's sync marker: the state
/// is a first-class citizen, never an edge case (design doc §10).
pub fn mid_op_label(op: &str) -> String {
    format!("{op}…")
}

// --- §14.2 section header ------------------------------------------------------

/// The uppercase section label ("LOCAL"). The transform is mandatory (§3.3); the
/// live count is a chip beside it, right-aligned, never characters inside it.
pub fn section_label(title: &str) -> String {
    title.to_uppercase()
}

/// One 26px section header on its own band: chevron, uppercase label, count
/// chip right-aligned, optional trailing action. Returns the strip's response —
/// clicking the strip toggles expanded/collapsed. The trailing action is
/// rendered *after* the strip's own interact target, so egui hit-testing gives it
/// precedence and a Fetch button never toggles collapse.
pub fn section_header<R>(
    ui: &mut Ui,
    title: &str,
    count: usize,
    expanded: bool,
    actions: impl FnOnce(&mut Ui) -> R,
) -> Response {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, SECTION_H), Sense::hover());
    // Scaffolding, so it sits on a band — one that is neither the repo header's
    // SURFACE nor any row state's fill.
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, Palette::SECTION_BG);

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
    child.spacing_mut().item_spacing.x = 0.0;
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
        RichText::new(section_label(title))
            .font(chrome_font(TYPE_SECTION))
            .color(Palette::T_SECONDARY),
    ));

    // The count is right-aligned on the band's trailing edge, and it is the
    // shared **count chip**: a number is a number in every pane, and a second
    // counter rendering — the v1 `PillKind::Count`, a 9px proportional badge on
    // `RAISED` with secondary ink — is the same role in the wrong face at the
    // wrong size. It sits beside the label, never inside the label string.
    child.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.add_space(PAD_LIST);
        ui.scope(actions);
        ui.add_space(4.0);
        count_chip(ui, &count.to_string());
    });

    response
}

// --- §14.3 sync badge -----------------------------------------------------------

/// The sync relationship of one branch row — icon+count pairs, quiet in-sync
/// confirmation, or a deleted-upstream marker.
///
/// `Diverged` and `InSync` are **unreachable from `sync_badge`**, which emits
/// ahead, behind and gone only; they are consumed by `sync_ink` and pinned by
/// the token suite. That is noted and untouched — wiring them is a product
/// question (should a sync badge ever say "diverged"?) and deleting one is a
/// change to a pinned assertion, neither of which this vocabulary's owner
/// decided.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncKind {
    Ahead,
    Behind,
    Diverged,
    InSync,
    Gone,
}

/// The row's sync relationship as a list of `(kind, label)` badges — the one
/// place the counts are turned into what a branch row paints. Each direction
/// gets its own badge, which is what lets a diverged row show both arrows
/// instead of one combined marker; an empty list means there is nothing to say
/// (no upstream, or in sync).
///
/// A direction is stated as `glyph + count` (`↑2`, `↓1`), the spelling the
/// sidebar's repository rows, the branches popup and the repository band's
/// summary all use, so one meaning does not arrive in two formats across the
/// panes. [`badge_words`] paints the glyph and count on the branch tree's rows.
pub fn sync_badge(ahead: usize, behind: usize, gone: bool) -> Vec<(SyncKind, String)> {
    if gone {
        return vec![(SyncKind::Gone, "gone".to_string())];
    }
    let mut badges = Vec::new();
    if ahead > 0 {
        badges.push((SyncKind::Ahead, format!("↑{ahead}")));
    }
    if behind > 0 {
        badges.push((SyncKind::Behind, format!("↓{behind}")));
    }
    badges
}

/// §13 meaning token for a sync badge's foreground.
pub fn sync_ink(kind: SyncKind) -> Color32 {
    match kind {
        SyncKind::Ahead | SyncKind::InSync => crate::theme::RepoState::Unpushed.color(),
        SyncKind::Behind => crate::theme::RepoState::Unpulled.color(),
        SyncKind::Diverged => crate::theme::RepoState::Diverged.color(),
        SyncKind::Gone => Palette::DANGER,
    }
}

/// Fit identifying ends to actual font metrics, including wide Unicode glyphs.
pub fn middle_truncate_to_width(ui: &Ui, text: &str, font: &egui::FontId, width: f32) -> String {
    let fits = |s: &str| {
        ui.painter()
            .layout_no_wrap(s.to_owned(), font.clone(), Color32::WHITE)
            .size()
            .x
            <= width.max(0.0)
    };
    if fits(text) {
        return text.to_owned();
    }
    let mut budget = text.chars().count().saturating_sub(1);
    while budget >= 4 {
        let candidate = middle_truncate(text, budget);
        if fits(&candidate) {
            return candidate;
        }
        budget -= 1;
    }
    if fits("…") {
        "…".to_owned()
    } else {
        String::new()
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

// --- count pluralisation -----------------------------------------------------

/// One count-pluralised phrase (`2 conflicts`, `1 file`). The rebased-parity
/// spec mandates a SINGLE shared helper for pluralisation — per-call-site
/// conditionals drift, and the reference's progress strings distinguish
/// `1#Conflict|2#Conflicts` deliberately.
///
/// The noun pair is the interface: a caller names the singular and the plural
/// it already means, so the "one reads singular, every other count reads
/// plural" rule stops being restated per call site. Every pluraliser in the
/// tree but [`crate::ui::branch_divergence::repositories`] — which wants a bare
/// noun with no count — now answers from here.
pub fn counted(n: usize, singular: &str, plural: &str) -> String {
    if n == 1 {
        format!("{n} {singular}")
    } else {
        format!("{n} {plural}")
    }
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
            (Self::Danger, Idle) => tint_over_bg(Palette::DANGER, 0.12),
            (Self::Danger, Disabled) => Color32::TRANSPARENT,
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

/// Paint one kit button, sized to its label (28px tall — above the 24px target
/// floor). Renders the four interactive states through [`KitButton::fill`]/
/// [`ink`] with the §13 control radius and a brand focus ring.
pub fn kit_button(ui: &mut Ui, kind: KitButton, label: &str) -> Response {
    kit_button_at(ui, kind, label, kit_button_width(ui, label))
}

/// The width [`kit_button`] gives `label` — its 12 px padding on either side
/// plus the measured text. A caller laying out a row of controls measures here
/// rather than duplicating the formula.
pub fn kit_button_width(ui: &Ui, label: &str) -> f32 {
    12.0 * 2.0
        + ui.painter()
            .layout_no_wrap(label.to_owned(), chrome_font(TYPE_BODY), Color32::WHITE)
            .size()
            .x
}

/// [`kit_button`] at an explicit width. A caller laying out a column of actions
/// gives every button the same width, so the column reads as one shape instead
/// of a stack of differently-wide buttons.
pub fn kit_button_at(ui: &mut Ui, kind: KitButton, label: &str, width: f32) -> Response {
    let font_id = chrome_font(TYPE_BODY);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font_id, Color32::WHITE);
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(width, KIT_BUTTON_H),
        if ui.is_enabled() {
            Sense::click()
        } else {
            Sense::hover()
        },
    );

    let state = WidgetState::from_response(ui.is_enabled(), &response);

    let painter = ui.painter().clone();
    let radius = CornerRadius::same(crate::theme::CONTROL_RADIUS);
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
    // NOT `widgets::paint_centered_text`, deliberately — this is the same
    // two-axis arithmetic, but the galley above is laid out in `WHITE` (so the
    // width is measured once and reused for the natural-width measure) and the
    // ink is applied at paint time by *overriding* the text color. The shared
    // helper paints with `Painter::galley`, whose color argument is only a
    // fallback for `PLACEHOLDER` spans: a galley laid out in white keeps
    // painting white, so folding this call in would put every label on
    // `WHITE` instead of the state's ink. The two paths are not
    // interchangeable, so this site keeps the override call.
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
