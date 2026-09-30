//! The painted-ink seam: a label reports the colour it was **painted** with.
//!
//! Every remaining design-system migration ticket wants to say things like
//! "column headers paint in `INK_3`, not `INK_4`" or "the state word paints
//! with no background". None of those is assertable unless the test can read
//! the ink a string was actually drawn with — and the naive way to read it is
//! wrong. A galley keeps the colour it was *laid out* with, and this codebase
//! lays shared-button text out in `Color32::WHITE`, applying the real ink at
//! paint time by overriding the text colour
//! (`Painter::galley_with_override_text_color`). Reading
//! `galley.job.sections[0].format.color` alone therefore answers `WHITE` for
//! every shared button, kit button, sidebar row, menu item and segmented
//! option.
//!
//! So this file pins the seam itself, over both paint paths:
//!
//! - **override-painted** — a real shared widget whose label goes through
//!   `widgets::controls`' measure-in-`WHITE`-then-override dance. Its reported
//!   ink must be the widget's state-table ink.
//! - **laid-out only** — a plain `Ui::label`, which paints through
//!   `Painter::galley` and so carries no override. Its reported ink must still
//!   be the colour it was laid out with, i.e. the fallback branch is load
//!   bearing and not a coincidence.
//!
//! The coherence assertion — `PaintedGalley::color` and `painted_ink` agreeing
//! — is the one that was impossible before: two accessors, one rule, no
//! second implementation to drift.

use egui::epaint::TextShape;
use egui::{Color32, RichText, Shape};
use egui_kittest::Harness;
use test_support::harness::{painted_galleys, painted_ink, settle, widget_harness};
use turbogit_ui::theme::Palette;
use turbogit_ui::ui::widgets::{ButtonVariant, WidgetState, ghost_button, primary_button};

/// A control painted through the override path, whose state-table ink is
/// `INK_2` and therefore provably not the `WHITE` it was laid out in.
const GHOST_LABEL: &str = "Ghost action";
/// A control painted through the override path whose ink happens to *be*
/// `WHITE` (`BRAND_INK`), kept for the coherence assertion and for the
/// structural proof that the override — not a laid-out white — is what paints
/// it.
const PRIMARY_LABEL: &str = "Primary action";
/// A label painted with no override at all, so the fallback branch answers.
const LAID_OUT_LABEL: &str = "laid out label";
/// The ink `LAID_OUT_LABEL` is asked to carry, so the assertion is about the
/// rule rather than about whatever egui's default text colour happens to be.
const LAID_OUT_INK: Color32 = Palette::INK_3;

/// A harness rendering two override-painted shared controls beside one label
/// painted with no override.
///
/// The 480×220 box the pixel assertions below were measured in, stated here
/// rather than inherited from kittest's 800×600 default.
fn ink_harness() -> Harness<'static, ()> {
    widget_harness(egui::vec2(480.0, 220.0), |ui| {
        // Override-painted: `widgets::controls` measures the label in
        // `Color32::WHITE`, then repaints it through the override helper.
        primary_button(ui, None, PRIMARY_LABEL);
        ghost_button(ui, None, GHOST_LABEL);
        // Not override-painted: `Ui::label` paints through `Painter::galley`,
        // whose colour argument is only a fallback for `PLACEHOLDER` spans.
        ui.label(RichText::new(LAID_OUT_LABEL).color(LAID_OUT_INK));
    })
}

/// The painted text shape whose string is exactly `needle`, so a test can
/// inspect the raw paint-time fields the accessors resolve.
fn text_shape<'h>(harness: &'h Harness<'_, ()>, needle: &str) -> Option<&'h TextShape> {
    harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(text) if text.galley.text() == needle => Some(text),
            _ => None,
        })
}

/// The colour `PaintedGalley::color` reports for `needle`, which is the
/// accessor the whole migration will be asserted through.
fn galley_ink(harness: &Harness<'_, ()>, needle: &str) -> Option<Color32> {
    painted_galleys(harness)
        .into_iter()
        .find(|g| g.text == needle)
        .map(|g| g.color)
}

/// The load-bearing case: a control that overrides its colour at paint time
/// reports that ink, not the `WHITE` its galley was measured in.
///
/// Before the seam existed, `PaintedGalley::color` read the layout colour and
/// answered `WHITE` here — a test asserting `INK_2` would have failed for a
/// reason that had nothing to do with the widget.
#[test]
fn an_override_painted_control_reports_its_ink_not_the_white_layout_placeholder() {
    let mut harness = ink_harness();
    harness.remove_cursor();
    settle(&mut harness);

    // Read the expectation from the widget's own state table rather than from a
    // literal invented here: a ghost at rest (§7.2) inks `INK_2`.
    let want = ButtonVariant::Ghost.text(WidgetState::Idle);
    assert_eq!(want, Palette::INK_2, "§7.2: ghost text at rest is INK_2");
    assert_ne!(
        want,
        Color32::WHITE,
        "the witness is only sharp if its expected ink differs from the layout \
         colour the shared control measures its label in"
    );

    let shape = text_shape(&harness, GHOST_LABEL).expect("the ghost label paints");
    assert_eq!(
        shape.galley.job.sections[0].format.color,
        Color32::WHITE,
        "precondition: the shared control really does lay the label out in WHITE"
    );
    assert_eq!(
        shape.override_text_color,
        Some(want),
        "and supplies the ink at paint time"
    );

    assert_eq!(
        painted_ink(&harness, GHOST_LABEL),
        Some(want),
        "`painted_ink` must report the paint-time ink"
    );
    assert_eq!(
        galley_ink(&harness, GHOST_LABEL),
        Some(want),
        "`PaintedGalley::color` must agree, or the two accessors disagree about \
         the same string"
    );
}

/// The fallback branch: a label with no override still reports the colour it
/// was laid out with. Without this the override could simply be absent
/// everywhere and the case above would prove nothing.
#[test]
fn a_label_without_a_paint_time_override_still_reports_its_laid_out_colour() {
    let mut harness = ink_harness();
    settle(&mut harness);

    let shape = text_shape(&harness, LAID_OUT_LABEL).expect("the plain label paints");
    assert_eq!(
        shape.override_text_color, None,
        "precondition: `Ui::label` paints through `Painter::galley`, which sets \
         only a fallback colour, so the ink is the laid-out one"
    );
    assert_eq!(
        shape.galley.job.sections[0].format.color, LAID_OUT_INK,
        "precondition: the galley was laid out carrying the requested ink"
    );

    assert_eq!(
        painted_ink(&harness, LAID_OUT_LABEL),
        Some(LAID_OUT_INK),
        "with no override, the laid-out colour is the answer"
    );
    assert_eq!(
        galley_ink(&harness, LAID_OUT_LABEL),
        Some(LAID_OUT_INK),
        "and the two accessors still agree on it"
    );
}

/// The coherence assertion for the widget the migration tickets name by
/// example. The primary button's idle ink is `BRAND_INK`, which *is* `WHITE`,
/// so its value alone cannot separate the two paint paths — the raw shape
/// proves the override is what supplies it, and the two accessors must report
/// the same colour for the same string.
#[test]
fn the_primary_button_inks_from_its_override_and_both_accessors_agree() {
    let mut harness = ink_harness();
    harness.remove_cursor();
    settle(&mut harness);

    let want = ButtonVariant::Primary.text(WidgetState::Idle);
    assert_eq!(want, Palette::BRAND_INK);
    assert_eq!(
        text_shape(&harness, PRIMARY_LABEL)
            .expect("the primary label paints")
            .override_text_color,
        Some(want),
        "the ink came from the paint-time override, not a white laid-out galley"
    );

    assert_eq!(painted_ink(&harness, PRIMARY_LABEL), Some(want));
    assert_eq!(
        galley_ink(&harness, PRIMARY_LABEL),
        painted_ink(&harness, PRIMARY_LABEL),
        "`PaintedGalley::color` and `painted_ink` are one rule, not two"
    );
}

/// `painted_ink` says "no such text was painted" honestly. A `TRANSPARENT`
/// sentinel for an unpainted string would let a mistyped needle assert itself
/// against a colour nothing drew.
#[test]
fn painted_ink_is_none_for_a_string_the_frame_never_painted() {
    let mut harness = ink_harness();
    settle(&mut harness);

    assert_eq!(
        painted_ink(&harness, "never painted"),
        None,
        "a string the frame does not paint has no ink to report"
    );
    // Exact matching, the same discipline `galley_origin` uses: a substring of
    // a painted label is not itself a painted label.
    assert_eq!(
        painted_ink(&harness, "Ghost"),
        None,
        "matching is exact, so a prefix of a painted label does not answer"
    );
    assert_eq!(
        painted_ink(&harness, GHOST_LABEL),
        Some(ButtonVariant::Ghost.text(WidgetState::Idle))
    );
}
