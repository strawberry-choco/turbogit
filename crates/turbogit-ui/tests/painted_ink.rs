use egui::epaint::TextShape;
use egui::{Color32, RichText, Shape};
use egui_kittest::Harness;
use test_support::harness::{painted_galleys, painted_ink, settle, widget_harness};
use turbogit_ui::theme::Palette;
use turbogit_ui::ui::widgets::{ButtonVariant, WidgetState, ghost_button, primary_button};

const GHOST_LABEL: &str = "Ghost action";
const PRIMARY_LABEL: &str = "Primary action";
const LAID_OUT_LABEL: &str = "laid out label";
const LAID_OUT_INK: Color32 = Palette::INK_3;

fn ink_harness() -> Harness<'static, ()> {
    widget_harness(egui::vec2(480.0, 220.0), |ui| {
        primary_button(ui, None, PRIMARY_LABEL);
        ghost_button(ui, None, GHOST_LABEL);
        ui.label(RichText::new(LAID_OUT_LABEL).color(LAID_OUT_INK));
    })
}

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

fn galley_ink(harness: &Harness<'_, ()>, needle: &str) -> Option<Color32> {
    painted_galleys(harness)
        .into_iter()
        .find(|g| g.text == needle)
        .map(|g| g.color)
}

#[test]
fn an_override_painted_control_reports_its_ink_not_the_white_layout_placeholder() {
    let mut harness = ink_harness();
    harness.remove_cursor();
    settle(&mut harness);

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

#[test]
fn painted_ink_is_none_for_a_string_the_frame_never_painted() {
    let mut harness = ink_harness();
    settle(&mut harness);

    assert_eq!(
        painted_ink(&harness, "never painted"),
        None,
        "a string the frame does not paint has no ink to report"
    );
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
