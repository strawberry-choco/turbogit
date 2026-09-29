//! Shared chip geometry and the semantic chip/badge vocabulary built on it.
//!
//! # The chip set is closed: three chips
//!
//! A chip is a compact fact marker, and there are exactly three of them, so a
//! chip anywhere in the app means the same thing:
//!
//! | Chip | Produced by | Fill / ink | Carries |
//! | --- | --- | --- | --- |
//! | **Ref chip** | [`ref_chip`] | [`Palette::RAISED_ON_CARD`] / [`Palette::INK_2`] | a ref name |
//! | **Current chip** | [`current_chip`] | [`Palette::ROW_SELECTED`] / [`Palette::ACCENT_TEXT`] | "this is the current ref" |
//! | **Count chip** | [`count_chip`] | [`Palette::RAISED`] / [`Palette::INK_2`] | an ad-hoc number |
//!
//! One function produces each, so there is nowhere for a second opinion to come
//! from: the three colour pairs below are the only place in the module a chip
//! names a fill, and the three constructors are the only functions that paint a
//! chip at all. A fourth chip is a new pair in that table and fails the
//! closed-set ratchet in `tests/widget_library.rs` before it reaches a screen.
//!
//! Two neighbours are deliberately **not** in that set:
//!
//! - **The status badge** ([`badge`] + [`BadgeKind`]) keeps the **pill** radius
//!   ([`PILL_RADIUS`], 9) and its own slot. It states a file's status, it is not
//!   a ref and not a number, and it is not reclassified as a chip to tidy the
//!   vocabulary. The two radii stay distinct and both stay pinned; they are
//!   never merged into one "chip radius".
//! - **`hash_chip`** is a *label*, not a control and not a chip (ADR-0024): the
//!   copy verb for a hash lives in the commit's context menu, so the chip must
//!   not swallow presses or answer `Click` in the accessibility tree.
//!
//! # Semantic state is not a chip
//!
//! `dirty`, `↓ 2 behind`, `Up to date` and `Needs update` render as **coloured
//! text or a leading dot**, coloured from the one repository-state map,
//! [`crate::theme::RepoState::color`]. A state is not a ref and not a number, so
//! a filled chip is the wrong shape for it: a state behind a fill stops reading
//! as state and starts reading as a category. This is the rule the branches
//! screen violated twice, and it is why no colour in that map appears in the
//! three tables above.
//!
//! # Why the current chip is allowed near the brand
//!
//! The current chip is the **only** chip permitted to carry an accent, and only
//! because "this is the current ref" is a *fact about a ref* rather than a call
//! to action. It paints the selected-row fill with [`Palette::ACCENT_TEXT`], not
//! a solid brand fill, precisely so the permission cannot be spent on a button:
//! the moment this chip is reused for a primary button, a menu item, or any
//! other invitation to act, the rule is broken, the accent stops meaning "the
//! current ref", and the blue soup this vocabulary exists to remove comes back.

use egui::{
    Color32, CornerRadius, FontFamily, FontId, Pos2, Rect, Response, Sense, Ui, Vec2, WidgetInfo,
    WidgetType,
};

use super::controls::tint_over_bg;
use crate::theme::{
    CHIP_RADIUS, CONTROL_RADIUS, PILL_RADIUS, Palette, TYPE_CHIP, TYPE_CONTROL, data_font,
};

/// Alpha used when tinting an accent over [`Palette::BG`] for badge fills.
pub const BADGE_TINT: f32 = 0.18;
/// Shared chip height (pill).
pub const CHIP_HEIGHT: f32 = 18.0;
/// Horizontal text inset on each side of the shared non-interactive chip.
pub const CHIP_PAD_X: f32 = 6.0;
const MICRO_TEXT: f32 = TYPE_CONTROL;

// --- Chip decisions ----------------------------------------------------------

/// Background/foreground pair painted by the badge family.
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
    ///
    /// This is the *colour vocabulary* half of the ref-chip role, and it
    /// outlived the ref-label render function: `ui::log_window` maps its own
    /// reference kind onto [`RefKind::accent`] for colouring and never renders
    /// a ref label. The retired `RefKind::colors` — the solid-pill colour pair
    /// only that render function needed — went with it. Keep `accent()`.
    ///
    /// Note this is **not** the ref chip's fill: R6 makes the ref chip itself
    /// neutral, so nothing in [`ref_chip`] consults this map.
    ///
    /// **The log's ref markers were the open question, and the log's own ticket
    /// (14) settled them: they are not ref chips.** The collapsed marker in the
    /// graph carries **no ref name** — the names are in the hover tooltip — so
    /// painting it as a ref chip would spend the compact chip radius (and the
    /// `REF_CHIP_COLORS` pair) on something that is not a ref, and that radius
    /// would come to mean two things: "a ref name" and "a collapsed decoration".
    /// It therefore stays the neutral badge in the **pill** slot, which is a
    /// different shape for a different job, and R6's one-function-per-chip
    /// property survives: [`ref_chip`] is the only thing that produces a ref
    /// chip anywhere in the app. `tests/git_log.rs` pins the log's half from
    /// paint (the marker wears `PILL_RADIUS`, a real ref chip in the same frame
    /// wears `CHIP_RADIUS`) and `tests/widget_library.rs` pins the whole-tree
    /// half — no screen module names the ref chip's fill or its geometry.
    pub fn accent(self) -> Color32 {
        match self {
            Self::Branch => Palette::BRAND,
            Self::Remote => Palette::STATE_SUCCESS,
            Self::Tag => Palette::STATE_WARNING,
        }
    }
}

// --- Chips -------------------------------------------------------------------

/// Status badge (`.tg-badge`): 18px pill, tinted background + accent ink.
///
/// Not one of the three chips: a file status keeps the pill radius and its own
/// slot, and the ref chip's compact radius is a different shape for a
/// different job. See the module docs on the two radii staying distinct.
pub fn badge(ui: &mut Ui, text: &str, kind: BadgeKind) -> Response {
    chip_with(
        ui,
        text,
        CHIP_GEOMETRY,
        FontId::new(MICRO_TEXT, FontFamily::Proportional),
        kind.colors(),
    )
}

/// Geometry shared by the non-interactive chip family.
///
/// This is deliberately a value, not a semantic chip type: branch, status,
/// reference, welcome, and quiet wrappers keep their own meaning and may
/// intentionally choose different metrics. This contract only centralizes
/// the low-level decisions that are actually common.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChipGeometry {
    /// Outer chip height.
    pub height: f32,
    /// Horizontal text inset on each side.
    pub pad_x: f32,
    /// Corner radius in logical pixels.
    pub radius: f32,
}

impl ChipGeometry {
    /// Measure the full chip for a laid-out label.
    pub fn size(self, galley: &egui::Galley) -> Vec2 {
        Vec2::new(galley.size().x + self.pad_x * 2.0, self.height)
    }

    /// Place a chip with its right edge at `right_edge` and centre on `cy`.
    pub fn rect_right(self, right_edge: f32, cy: f32, galley: &egui::Galley) -> Rect {
        let size = self.size(galley);
        Rect::from_min_size(Pos2::new(right_edge - size.x, cy - self.height / 2.0), size)
    }

    /// Place a label centred in a chip rect.
    pub fn text_origin(self, rect: Rect, galley: &egui::Galley) -> Pos2 {
        Pos2::new(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        )
    }

    /// Paint this geometry's body and its centred label.
    pub fn paint(
        self,
        painter: &egui::Painter,
        rect: Rect,
        galley: std::sync::Arc<egui::Galley>,
        bg: Color32,
        ink: Color32,
    ) {
        painter.rect_filled(rect, CornerRadius::same(self.radius as u8), bg);
        painter.galley(self.text_origin(rect, &galley), galley, ink);
    }
}

/// The **pill** slot: 18px high, 6px inset, [`PILL_RADIUS`] rounded. The status
/// badge's geometry, and the one the log's label pill and the preview-header
/// roundel already borrow. It is *not* the chips' geometry — see
/// [`COMPACT_CHIP_GEOMETRY`].
///
/// The two slots are never merged. The pill is the full-height wrap for a
/// status or a tag icon; the compact radius is the chip role's, and unifying
/// them would change the shape of a file-status badge the moment a ref chip
/// landed. Both stay pinned, by value and by test.
pub const CHIP_GEOMETRY: ChipGeometry = ChipGeometry {
    height: CHIP_HEIGHT,
    pad_x: CHIP_PAD_X,
    radius: PILL_RADIUS as f32,
};

/// The **chip** slot: the one geometry all three chips share. 18px high, 6px
/// inset, and the theme's compact [`CHIP_RADIUS`] (3) — the radius the token
/// layer documents as the chip radius and that the shared family, until this
/// vocabulary landed, did not use.
pub const COMPACT_CHIP_GEOMETRY: ChipGeometry = ChipGeometry {
    height: CHIP_HEIGHT,
    pad_x: CHIP_PAD_X,
    radius: CHIP_RADIUS as f32,
};

/// The chip's face, at the chip type size ([`TYPE_CHIP`], 10px). A chip holds a
/// ref name or a number, both of which are data, so the chip type is the data
/// face — the same rule `chrome_font`/`data_font` splits the rest of the app by,
/// and the same one `hash_chip` follows.
fn chip_font() -> FontId {
    data_font(TYPE_CHIP)
}

/// The pill slot's corner radius, derived from [`CHIP_GEOMETRY`].
pub fn chip_radius() -> CornerRadius {
    CornerRadius::same(CHIP_GEOMETRY.radius as u8)
}

/// The chip slot's corner radius, derived from [`COMPACT_CHIP_GEOMETRY`].
pub fn compact_chip_radius() -> CornerRadius {
    CornerRadius::same(COMPACT_CHIP_GEOMETRY.radius as u8)
}

/// The rect a pill-slot chip occupies when its right edge is at `right_edge`.
pub fn chip_rect_right(right_edge: f32, cy: f32, galley: &egui::Galley) -> Rect {
    CHIP_GEOMETRY.rect_right(right_edge, cy, galley)
}

/// Place a pill-slot chip's label centred in its rect.
pub fn chip_text_origin(rect: Rect, galley: &egui::Galley) -> Pos2 {
    CHIP_GEOMETRY.text_origin(rect, galley)
}

/// Paint one pill-slot chip into a caller-placed rect.
pub fn paint_chip(
    painter: &egui::Painter,
    rect: Rect,
    galley: std::sync::Arc<egui::Galley>,
    bg: Color32,
    ink: Color32,
) {
    CHIP_GEOMETRY.paint(painter, rect, galley, bg, ink);
}

// --- The three chips ---------------------------------------------------------

/// **Ref chip** colours: the raised-on-card fill with secondary ink.
///
/// The raised role is [`Palette::RAISED_ON_CARD`] and not the plain raised
/// surface because a ref name mostly sits on a `CONTENT_BG` card, and a control
/// that does not step up from the surface it is on is invisible (R5). The ink
/// is [`Palette::INK_2`] and *not* the muted step: the muted step is not legal on
/// a raised surface, and the constructor would be the place that rule broke.
pub const REF_CHIP_COLORS: ChipColors = ChipColors {
    bg: Palette::RAISED_ON_CARD,
    fg: Palette::INK_2,
};

/// **Current chip** colours: the selected-row fill with the readable accent ink.
///
/// The fill is the same opaque value a chosen list row takes, so "the current
/// ref" and "the selected row" cannot be two different blues; the ink is
/// [`Palette::ACCENT_TEXT`], which clears AA on it.
pub const CURRENT_CHIP_COLORS: ChipColors = ChipColors {
    bg: Palette::ROW_SELECTED,
    fg: Palette::ACCENT_TEXT,
};

/// **Count chip** colours: the plain raised surface with secondary ink.
///
/// The raised role here is [`Palette::RAISED`] rather than the raised-on-card
/// one because a count usually sits in an already-raised chrome row (a pane
/// header, a status bar), where stepping up twice would be the wrong lift. The
/// ink is monospaced so a column of counts aligns digit-for-digit.
pub const COUNT_CHIP_COLORS: ChipColors = ChipColors {
    bg: Palette::RAISED,
    fg: Palette::INK_2,
};

/// The one function that produces a **ref chip**: a ref name on the
/// raised-on-card fill, in the chip radius, at the chip type size, in the data
/// face.
///
/// Used by branch names, remote refs, worktree branches and the log's ref pills.
/// It is a **label, not a control**, and it inherits that from
/// [`hash_chip`] for the same reason: every press a ref chip could take is
/// already a verb somewhere else (checkout, fetch, delete the branch), and a
/// chip that swallowed the press and answered `Click` in the accessibility
/// tree would be an unnamed button where there is no button. So the sense is
/// hover-only and there is no focus ring to offer — the branches *scope* chip,
/// which genuinely is a control, carries its own node instead.
pub fn ref_chip(ui: &mut Ui, text: &str) -> Response {
    chip_with(
        ui,
        text,
        COMPACT_CHIP_GEOMETRY,
        chip_font(),
        REF_CHIP_COLORS,
    )
}

/// The one function that produces the **current chip**: "this is the current
/// ref", and nothing else.
///
/// ## This is the only chip allowed to carry an accent
///
/// The permission is narrow and it is a statement about the *ref*, not an
/// invitation to act. "This is the branch you are on" is a fact, the way a
/// selected row is a fact; a button that fills the brand is a request.
///
/// **Reusing this chip for a primary button, a menu item, or any other call to
/// action breaks the rule and the vocabulary at once.** The accent would stop
/// meaning "the current ref" and start meaning "do this", every screen would
/// carry it, and the blue soup this chip vocabulary exists to remove comes back
/// — with a name and a test that both say it is fine here.
///
/// It paints [`Palette::ROW_SELECTED`] with [`Palette::ACCENT_TEXT`] rather
/// than a solid brand fill for the same reason: a solid brand band behind a
/// ref name is a button's shape, whatever the code calls it.
pub fn current_chip(ui: &mut Ui, text: &str) -> Response {
    chip_with(
        ui,
        text,
        COMPACT_CHIP_GEOMETRY,
        chip_font(),
        CURRENT_CHIP_COLORS,
    )
}

/// The one function that produces a **count chip**: an ad-hoc number that is not
/// a repository state.
///
/// This is the chip that replaces a counter borrowing the **reserved counter
/// orange** for something that was not a dirt or unpushed count. The orange
/// itself does not move and is not repainted here: [`Palette::COUNTER`] still
/// means dirt and unpushed, and this chip means "a number" — which is the
/// reason the two were conflated.
///
/// A count is a *mark*, not a category: it never takes a state colour, and it
/// never takes the accent.
pub fn count_chip(ui: &mut Ui, text: &str) -> Response {
    chip_with(
        ui,
        text,
        COMPACT_CHIP_GEOMETRY,
        chip_font(),
        COUNT_CHIP_COLORS,
    )
}

/// The shared non-interactive chip body: measure the label in the chip's own
/// face, allocate exactly the geometry's size, paint the fill and the label.
///
/// Deliberately decides no role of its own — geometry, face and colours all
/// arrive from the caller, so the badge's pill slot and the chips' compact slot
/// share the arithmetic without sharing a look. `Sense::hover()` is fixed here
/// for the whole family: **no chip in this module has a click plane** (see
/// [`hash_chip`] and [`ref_chip`]), and the closed-set ratchet in
/// `tests/widget_library.rs` fails if that ever changes.
fn chip_with(
    ui: &mut Ui,
    text: &str,
    geometry: ChipGeometry,
    font: FontId,
    colors: ChipColors,
) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, colors.fg);
    let size = geometry.size(&galley);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());

    geometry.paint(ui.painter(), rect, galley, colors.bg, colors.fg);

    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, text));
    response
}

/// Commit hash as a chip: mono hash on `SURFACE_3` (redesign issue 03).
///
/// The chip is a LABEL, not a control. Copying the hash lives in the commit's
/// context menu, whose Copy hash item states this same short reference, so the
/// pane two inches away must not hold a second, dead copy of the verb (ADR-0024).
/// That is why the sense is hover-only: a clickable chip keeps the click plane —
/// swallowing presses over the commit metadata — and, being focusable, gets an
/// accessibility node that answers Click, so a screen reader announces an
/// unnamed button where there is no button. It cannot take focus either, so no
/// focus ring is painted and none is offered: there is no keyboard affordance to
/// a hash nobody can act on here.
///
/// The ink is [`Palette::LINK`], and that is the token's one consumer in the
/// tree. It is the *hash* colour, not the action colour: a hash chip answers no
/// press and offers no focus, so painting it in `BRAND` spent the one accent on
/// something that is not an action — and the accent was unreadable here anyway,
/// measuring 2.48:1 against this chip's own `SURFACE_3` fill where `LINK`
/// measures 4.12:1. A hash is information the user reads to name a commit, so
/// it is the one thing on this chip that may not be dimmed past legibility.
///
/// `hint` is the optional hover caption, and an empty one paints no tooltip:
/// this chip carries no caption, because there is nothing to say about a press
/// that no longer exists.
pub fn hash_chip(ui: &mut Ui, hash: &str, hint: &str) -> Response {
    let font = FontId::new(crate::theme::TYPE_BODY, FontFamily::Monospace);
    let galley = ui
        .painter()
        .layout_no_wrap(hash.to_owned(), font, Palette::LINK);
    let size = Vec2::new(galley.size().x + CHIP_PAD_X * 2.0, CHIP_HEIGHT + 6.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(CONTROL_RADIUS), Palette::SURFACE_3);
        ui.painter().galley(
            Pos2::new(
                rect.center().x - galley.size().x / 2.0,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            Palette::LINK,
        );
    }
    if hint.is_empty() {
        response
    } else {
        response.on_hover_text(hint)
    }
}
