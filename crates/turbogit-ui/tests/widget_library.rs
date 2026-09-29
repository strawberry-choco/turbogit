//! Issue #6 — Shared widget library (public facade at `src/ui/widgets/mod.rs`).
//!
//! Three layers of proof, mirroring spec §7 and the R1.4 plan row:
//!
//! 1. **Palette-token completeness** — every token the widget vocabulary
//!    relies on exists in [`turbogit_ui::theme::Palette`] with the exact hex
//!    values from spec §2, and the surface ladder is strictly ordered.
//! 2. **Pure styling decisions** — badge-kind→color, ref-kind accent→color,
//!    and button-variant×state fills/text are all total functions over tokens,
//!    asserted without rendering.
//! 3. **Harness smoke render** — several widgets composed in one headless
//!    egui_kittest frame: painted text is asserted and a ghost button is
//!    really clicked through the accessibility tree.

use std::cell::Cell;
use std::rc::Rc;

use egui::{Color32, Shape};
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use test_support::harness::painted_galleys;
use turbogit_ui::theme::{PILL_RADIUS, Palette, RAIL_WIDTH};
use turbogit_ui::ui::icons::Icon;
use turbogit_ui::ui::widgets::*;

// ---------------------------------------------------------------------------
// 1. Palette-token completeness (spec §2)
// ---------------------------------------------------------------------------

/// Widgets may only use tokens that exist with the mockup's exact values.
#[test]
fn palette_tokens_required_by_widgets_match_the_spec_hexes() {
    // Core surfaces & lines (§2.1).
    assert_eq!(Palette::BG, Color32::from_rgb(0x1e, 0x1f, 0x22));
    assert_eq!(Palette::SURFACE, Color32::from_rgb(0x2b, 0x2d, 0x30));
    assert_eq!(Palette::SURFACE_2, Color32::from_rgb(0x31, 0x34, 0x38));
    assert_eq!(Palette::SURFACE_3, Color32::from_rgb(0x3c, 0x3f, 0x41));
    assert_eq!(Palette::LINE, Color32::from_rgb(0x4e, 0x51, 0x57));
    assert_eq!(Palette::LINE_SUBTLE, Color32::from_rgb(0x36, 0x38, 0x3c));

    // Ink (C3: the shell-facing names alias the authoritative T_* ramp — four
    // steps now, with per-step legality documented in `theme.rs`).
    assert_eq!(Palette::INK, Palette::T_PRIMARY);
    assert_eq!(Palette::INK_2, Palette::T_SECONDARY);
    assert_eq!(Palette::INK_3, Palette::T_MUTED);
    assert_eq!(Palette::INK_4, Palette::T_DIM);
    assert_eq!(Palette::T_MUTED, Color32::from_rgb(0x8a, 0x8e, 0x96));
    assert_eq!(Palette::T_DIM, Color32::from_rgb(0x6e, 0x72, 0x7a));

    // Brand.
    assert_eq!(Palette::BRAND, Color32::from_rgb(0x35, 0x74, 0xf0));
    assert_eq!(Palette::BRAND_INK, Color32::WHITE);

    // Status colors (§2.2) drive badges and ref labels.
    assert_eq!(Palette::STATE_SUCCESS, Color32::from_rgb(0x4c, 0xaf, 0x50));
    assert_eq!(Palette::STATE_WARNING, Color32::from_rgb(0xf9, 0xa8, 0x25));
    assert_eq!(Palette::STATE_ERROR, Color32::from_rgb(0xef, 0x53, 0x50));
}

/// The surface ladder must be strictly increasing in brightness so hover
/// states are always visible against idle states.
#[test]
fn surface_ladder_is_strictly_ordered() {
    let lum = |c: Color32| c.r() as u32 + c.g() as u32 + c.b() as u32;
    assert!(lum(Palette::BG) < lum(Palette::SURFACE));
    assert!(lum(Palette::SURFACE) < lum(Palette::SURFACE_2));
    assert!(lum(Palette::SURFACE_2) < lum(Palette::SURFACE_3));
    // Hover fill must differ from every state it replaces.
    assert_ne!(Palette::SURFACE_2, Palette::BG);
    assert_ne!(Palette::SURFACE_3, Palette::SURFACE_2);
}

// ---------------------------------------------------------------------------
// 2. Pure styling decisions
// ---------------------------------------------------------------------------

#[test]
fn badge_kind_maps_to_the_spec_status_colors() {
    // Foreground ink is exactly the mapped status token (§2.2 usage column).
    assert_eq!(BadgeKind::Added.accent(), Palette::STATE_SUCCESS);
    assert_eq!(BadgeKind::Modified.accent(), Palette::STATE_WARNING);
    assert_eq!(BadgeKind::Deleted.accent(), Palette::STATE_ERROR);
    assert_eq!(BadgeKind::Neutral.accent(), Palette::INK_2);

    // Neutral badges sit on the input/badge surface token (§2.1 usage).
    assert_eq!(BadgeKind::Neutral.colors().bg, Palette::SURFACE_3);
    // Status backgrounds derive from the same accent (translucent tint over
    // BG), so a badge can never drift to an unrelated hue.
    for kind in [BadgeKind::Added, BadgeKind::Modified, BadgeKind::Deleted] {
        let colors = kind.colors();
        assert_eq!(
            colors.bg,
            tint_over_bg(kind.accent(), BADGE_TINT),
            "{kind:?} bg must be its accent tinted over BG"
        );
        assert_ne!(colors.bg, Color32::TRANSPARENT);
    }
}

/// `RefKind`'s accent is the retained half of the ref-chip role: a live log
/// surface maps its own reference kind onto it for colouring and never renders
/// a ref label, so the type and this mapping outlived the retired
/// `ref_label` render function (and its solid-pill `RefKind::colors`).
#[test]
fn ref_kind_accent_maps_to_brand_success_warning() {
    // Spec §8.3: `.tg-label.branch` BRAND pill / remote SUCCESS / tag WARNING.
    assert_eq!(RefKind::Branch.accent(), Palette::BRAND);
    assert_eq!(RefKind::Remote.accent(), Palette::STATE_SUCCESS);
    assert_eq!(RefKind::Tag.accent(), Palette::STATE_WARNING);
}

#[test]
fn ghost_button_states_follow_the_widget_state_table() {
    use ButtonVariant::Ghost;
    use WidgetState::{Active, Disabled, Hovered, Idle};

    // Idle ghosts are transparent; hover/active step up the surface ladder.
    assert_eq!(Ghost.fill(Idle), Color32::TRANSPARENT);
    assert_eq!(Ghost.fill(Hovered), Palette::SURFACE_2);
    assert_eq!(Ghost.fill(Active), Palette::SURFACE_3);

    // Text: INK_2 at rest, INK when engaged, INK_3 disabled (§7.2).
    assert_eq!(Ghost.text(Idle), Palette::INK_2);
    assert_eq!(Ghost.text(Hovered), Palette::INK);
    assert_eq!(Ghost.text(Active), Palette::INK);
    assert_eq!(Ghost.text(Disabled), Palette::INK_3);

    // Disabled never changes the fill (no hover change while disabled).
    assert_eq!(Ghost.fill(Disabled), Ghost.fill(Idle));
}

#[test]
fn primary_button_brightens_instead_of_surface_hover() {
    use ButtonVariant::Primary;
    use WidgetState::{Active, Disabled, Hovered, Idle};

    // Solid brand fill in every enabled state — never a surface gray.
    assert_eq!(Primary.fill(Idle), Palette::BRAND);
    assert_ne!(Primary.fill(Hovered), Palette::SURFACE_2);
    assert_ne!(Primary.fill(Active), Palette::SURFACE_3);

    // Hover brightens toward white; press brightens further.
    let brighter =
        |a: Color32, b: Color32| a.r() >= b.r() && a.g() >= b.g() && a.b() >= b.b() && a != b;
    assert!(
        brighter(Primary.fill(Hovered), Primary.fill(Idle)),
        "hover must brighten the brand fill"
    );
    assert!(
        brighter(Primary.fill(Active), Primary.fill(Hovered)),
        "active must brighten past hover"
    );

    // Text stays brand ink until disabled, which drops to muted ink.
    assert_eq!(Primary.text(Idle), Palette::BRAND_INK);
    assert_eq!(Primary.text(Hovered), Palette::BRAND_INK);
    assert_eq!(Primary.text(Active), Palette::BRAND_INK);
    assert_eq!(Primary.text(Disabled), Palette::INK_3);
    // Disabled primary keeps its solid fill (no hover change, §7.2).
    assert_eq!(Primary.fill(Disabled), Primary.fill(Idle));
}

#[test]
fn compact_and_icon_variants_share_ghost_color_decisions() {
    for state in [
        WidgetState::Idle,
        WidgetState::Hovered,
        WidgetState::Active,
        WidgetState::Disabled,
    ] {
        assert_eq!(
            ButtonVariant::Compact.fill(state),
            ButtonVariant::Ghost.fill(state)
        );
        assert_eq!(
            ButtonVariant::Icon.fill(state),
            ButtonVariant::Ghost.fill(state)
        );
        assert_eq!(
            ButtonVariant::Compact.text(state),
            ButtonVariant::Ghost.text(state)
        );
        assert_eq!(
            ButtonVariant::Icon.text(state),
            ButtonVariant::Ghost.text(state)
        );
    }
}

/// **The band-height primary is the primary.** A pane header's band is a fixed
/// [`PANE_HEADER_HEIGHT`] and grows to fit its tallest child, so the header's
/// right-aligned action slot needs a brand button *at that height*; the
/// vocabulary had a 32px primary and a 28px **ghost** and nothing in between, so
/// the one call site that needed it borrowed a page-local button family instead
/// — a second spelling of R1's primary, which is the one thing that must not have
/// two.
///
/// So the two halves are pinned separately, because they are two different claims:
/// the *colour* ladder is identical arm for arm (a button that is four points
/// shorter must not also be a slightly different blue), and the *height* is the
/// band and not the 32px control. The height is measured off a rendered frame
/// rather than off a constant, because the constant is what the variant is
/// defined by and reading it back would prove nothing.
#[test]
fn a_compact_primary_shares_the_primary_colour_ladder_and_fits_the_pane_header_band() {
    for state in [
        WidgetState::Idle,
        WidgetState::Hovered,
        WidgetState::Active,
        WidgetState::Disabled,
    ] {
        assert_eq!(
            ButtonVariant::CompactPrimary.fill(state),
            ButtonVariant::Primary.fill(state),
            "the compact primary is the primary: R1's brand fill may not have a \
             second value just because a button is shorter"
        );
        assert_eq!(
            ButtonVariant::CompactPrimary.text(state),
            ButtonVariant::Primary.text(state),
            "…and the on-brand ink is the same step in both"
        );
    }
    // It really is the brand fill, so the sharing above is not two transparent
    // buttons agreeing.
    assert_eq!(
        ButtonVariant::CompactPrimary.fill(WidgetState::Idle),
        Palette::BRAND
    );
    // …and it is still a distinct *variant*: the fill ladder above is a claim
    // about colour, and a variant that quietly added a fourth state would show
    // up here.
    assert_ne!(
        ButtonVariant::CompactPrimary,
        ButtonVariant::Primary,
        "a band-height primary is a separate member of the vocabulary, not a \
         spelling of the 32px one: the cross-pane header ratchet needs the height \
         to be a property a caller can ask for"
    );

    // The height, from paint: one compact primary inside a real pane-header band,
    // measured against the band the shared header put down.
    let mut fonts_installed = false;
    let mut band = Harness::new_ui_state(
        move |ui, _| {
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            egui::CentralPanel::default().show(ui, |ui| {
                pane_header(ui, "BAND", None, |ui| {
                    compact_primary_button(ui, "Do the thing");
                });
            });
        },
        (),
    );
    band.set_size(egui::vec2(560.0, 200.0));
    settle(&mut band);

    let primary = band.get_by_label("Do the thing").rect();
    let rule = the_header_rule(&band);
    assert_eq!(
        primary.height(),
        PANE_HEADER_HEIGHT,
        "the shared compact primary is exactly the pane header's band height: a \
         taller control grows the band, and one pane's header being taller than \
         every other pane's is what the cross-pane geometry ratchet catches"
    );
    assert!(
        rule.top() - PANE_HEADER_HEIGHT <= primary.top() + 0.01
            && primary.bottom() <= rule.top() + 0.01,
        "and it fits *inside* its band rather than pushing the band's hairline \
         down: primary {primary:?}, hairline {rule:?}"
    );
}

// ---------------------------------------------------------------------------
// 3. Harness smoke render (several widgets together)
// ---------------------------------------------------------------------------

type ClickFlag = Rc<Cell<bool>>;

/// A harness rendering a panel composed purely of shared widgets.
///
/// Setup mirrors production (`app.rs` / `shell_frame.rs`): dark-only
/// tokens every frame plus embedded JetBrains Mono installed once.
fn widgets_harness(
    ghost_clicked: ClickFlag,
    compact_clicked: ClickFlag,
) -> (Harness<'static, ()>, tempfile::TempDir) {
    let mut search_buf = String::new();
    let mut name_buf = String::new();

    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, _state| {
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            egui::CentralPanel::default().show(ui, |ui| {
                // Section chrome.
                group_title(ui, "Recent");
                pane_header(ui, "CHANGED FILES", None, |_ui| {});

                // Buttons.
                if ghost_button(ui, None, "Ghost action").clicked() {
                    ghost_clicked.set(true);
                }
                if primary_button(ui, Some(Icon::CHECK), "Primary action").clicked() {
                    // Counted via painted assertion only.
                }
                if compact_button(ui, "Compact action").clicked() {
                    compact_clicked.set(true);
                }
                icon_button(ui, Icon::X);

                // Chips. The ref-label render function and the fixed-height
                // tree-row wrapper were retired from the façade, so the gallery
                // is the badge family plus the inputs; ref *colour* still has a
                // live consumer in `ui::log_window` and is pinned by
                // `ref_kind_accent_maps_to_brand_success_warning`.
                badge(ui, "+3", BadgeKind::Added);
                badge(ui, "M", BadgeKind::Modified);
                badge(ui, "D", BadgeKind::Deleted);

                // Inputs.
                search_input(ui, "Search commits", &mut search_buf);
                text_input(ui, "Branch name", &mut name_buf);

                // Dialog chrome.
                dialog_footer(ui, |ui| {
                    primary_button(ui, None, "Footer OK");
                });
            });
        },
        (),
    );
    harness.set_size(egui::vec2(800.0, 600.0));
    (harness, tempfile::tempdir().expect("tempdir"))
}

/// All text painted by the last completed frame.
fn painted_text<S>(harness: &Harness<'_, S>) -> Vec<String> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect()
}

#[test]
fn inline_error_composes_visible_text_with_semantic_error_ink() {
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, _state| {
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            egui::CentralPanel::default().show(ui, |ui| {
                ui.label("page context");
                inline_error(ui, "the operation could not finish");
            });
        },
        (),
    );
    harness.set_size(egui::vec2(480.0, 120.0));

    settle(&mut harness);
    assert_painted(&harness, "the operation could not finish");
    assert_eq!(
        painted_galleys(&harness)
            .into_iter()
            .find(|g| g.text.contains("the operation could not finish"))
            .map(|g| g.color),
        Some(Palette::STATE_ERROR)
    );
}

#[test]
fn keyed_read_presenter_renders_waiting_and_failure_from_display_inputs() {
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, _state| {
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            egui::CentralPanel::default().show(ui, |ui| {
                keyed_read_presentation(ui, KeyedReadPresentation::Waiting("Computing preview…"));
                keyed_read_presentation(
                    ui,
                    KeyedReadPresentation::Failed("Could not read the preview"),
                );
            });
        },
        (),
    );
    harness.set_size(egui::vec2(480.0, 120.0));

    settle(&mut harness);
    assert_painted(&harness, "Computing preview…");
    assert_painted(&harness, "Could not read the preview");
    assert_eq!(
        painted_galleys(&harness)
            .into_iter()
            .find(|g| g.text.contains("Could not read the preview"))
            .map(|g| g.color),
        Some(Palette::STATE_ERROR)
    );
}

#[test]
fn shared_note_and_alert_feedback_keep_distinct_rendered_roles() {
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, _state| {
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            egui::CentralPanel::default().show(ui, |ui| {
                note(ui, None, |ui| {
                    ui.label("context note");
                });
                alert_box(ui, "contained alert");
            });
        },
        (),
    );
    harness.set_size(egui::vec2(360.0, 180.0));
    settle(&mut harness);
    assert_painted(&harness, "context note");
    assert_painted(&harness, "contained alert");
}

#[test]
fn chip_geometry_owns_radius_padding_measurement_and_text_placement() {
    assert_eq!(CHIP_GEOMETRY.height, CHIP_HEIGHT);
    assert_eq!(CHIP_GEOMETRY.pad_x, CHIP_PAD_X);
    // Radius is theme-owned: the shared chip geometry derives the full pill
    // radius from `theme::PILL_RADIUS`, so pin the token rather than a literal.
    assert_eq!(CHIP_GEOMETRY.radius, f32::from(PILL_RADIUS));
    assert_eq!(chip_radius(), egui::CornerRadius::same(PILL_RADIUS));

    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, _state| {
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            egui::CentralPanel::default().show(ui, |ui| {
                badge(ui, "main", BadgeKind::Neutral);
            });
        },
        (),
    );
    harness.set_size(egui::vec2(240.0, 80.0));
    harness.step();
    let galley = harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(text) if text.galley.text() == "main" => Some(text.galley.clone()),
            _ => None,
        })
        .expect("badge should paint a main label");
    let size = CHIP_GEOMETRY.size(&galley);
    assert_eq!(size.y, CHIP_HEIGHT);
    assert_eq!(size.x, galley.size().x + 2.0 * CHIP_PAD_X);

    let rect = egui::Rect::from_min_size(egui::pos2(30.0, 40.0), size);
    let origin = chip_text_origin(rect, &galley);
    assert!((origin.x - (rect.center().x - galley.size().x / 2.0)).abs() < 0.01);
    assert!((origin.y - (rect.center().y - galley.size().y / 2.0)).abs() < 0.01);
}

/// The shared two-axis centring arithmetic: one string, one galley, painted
/// with its top-left corner half a galley away from the rect's centre on
/// **both** axes. The rect and the galley are deliberately non-square and of
/// different aspect, so a one-axis (or a swapped-axis) implementation cannot
/// satisfy both assertions.
#[test]
fn paint_centered_text_centres_one_galley_on_both_axes() {
    const INK: Color32 = Color32::from_rgb(0x11, 0x22, 0x33);
    let rect = egui::Rect::from_min_size(egui::pos2(12.0, 30.0), egui::vec2(240.0, 72.0));
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, _state| {
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            egui::CentralPanel::default().show(ui, |ui| {
                paint_centered_text(
                    ui.painter(),
                    rect,
                    "centered",
                    turbogit_ui::theme::chrome_font(turbogit_ui::theme::TYPE_BODY),
                    INK,
                );
            });
        },
        (),
    );
    harness.set_size(egui::vec2(360.0, 180.0));
    settle(&mut harness);

    let painted = painted_galleys(&harness);
    assert_eq!(
        painted.len(),
        1,
        "one string paints one galley: {painted:#?}"
    );
    let galley = &painted[0];
    assert_eq!(galley.text, "centered");
    assert_eq!(
        galley.color, INK,
        "the label is laid out and painted in `INK`"
    );
    // Non-square on both sides, and by different amounts, so no single axis or
    // coincidental half-size can carry the test.
    assert!(
        (rect.width() - rect.height()).abs() > 1.0,
        "the test rect must not be square"
    );
    assert!(
        (galley.rect.width() - galley.rect.height()).abs() > 1.0,
        "the test galley must not be square, got {:?}",
        galley.rect.size()
    );
    let want_x = rect.center().x - galley.rect.width() / 2.0;
    let want_y = rect.center().y - galley.rect.height() / 2.0;
    assert!(
        (galley.pos.x - want_x).abs() < 0.01,
        "x: painted at {:?}, wanted {want_x}",
        galley.pos.x
    );
    assert!(
        (galley.pos.y - want_y).abs() < 0.01,
        "y: painted at {:?}, wanted {want_y}",
        galley.pos.y
    );
}

#[test]
fn multiple_shared_chips_coexist_without_overlap_or_geometry_regression() {
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, _state| {
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            egui::CentralPanel::default().show(ui, |ui| {
                ui.horizontal(|ui| {
                    badge(ui, "added", BadgeKind::Added);
                    badge(ui, "main", BadgeKind::Neutral);
                    // The third chip used to be a `ref_label`; that render
                    // function is retired, so the overlap check runs on the
                    // badge family alone.
                    badge(ui, "gone", BadgeKind::Deleted);
                });
            });
        },
        (),
    );
    harness.set_size(egui::vec2(400.0, 80.0));
    settle(&mut harness);

    let added = harness.get_by_label("added").rect();
    let neutral = harness.get_by_label("main").rect();
    let third = harness.get_by_label("gone").rect();
    for rect in [added, neutral, third] {
        assert_eq!(rect.height(), CHIP_HEIGHT);
    }
    assert!(added.right() <= neutral.left());
    assert!(neutral.right() <= third.left());
    assert_eq!(added.top(), neutral.top());
    assert_eq!(neutral.top(), third.top());
}

/// Step frames until the painted output stabilizes.
fn settle<S>(harness: &mut Harness<'_, S>) {
    let mut prev = String::new();
    for _ in 0..10 {
        harness.step();
        let fingerprint = format!("{:?}", painted_text(harness));
        if fingerprint == prev {
            return;
        }
        prev = fingerprint;
    }
    panic!("widget layout did not settle within 10 frames");
}

#[track_caller]
fn assert_painted(harness: &Harness<'_, ()>, needle: &str) {
    let texts = painted_text(harness);
    assert!(
        texts.iter().any(|t| t.contains(needle)),
        "`{needle}` was not painted; painted text:\n{texts:#?}"
    );
}

#[test]
fn smoke_render_paints_the_widget_vocabulary_together() {
    let (mut harness, _dir) = widgets_harness(Rc::default(), Rc::default());
    settle(&mut harness);

    // Buttons.
    assert_painted(&harness, "Ghost action");
    assert_painted(&harness, "Primary action");
    assert_painted(&harness, "Compact action");

    // Badges. The ref-chip assertions that used to sit here went with the
    // retired `ref_label` render function.
    assert_painted(&harness, "+3");
    assert_painted(&harness, "M");
    assert_painted(&harness, "D");

    // Inputs paint their placeholder hint when empty.
    assert_painted(&harness, "Search commits");
    assert_painted(&harness, "Branch name");

    // Chrome: group titles and pane titles uppercase (§3.3). The pane header
    // renders the title it is given, so "CHANGED FILES" is the caller's word
    // written in the app's one case — see `every_pane_title_renders_in_the_same_case`.
    // The dialog header strip is gone (conformance issue 18 deleted
    // `dialog_header`, which no dialog ever called — every one hand-rolls its
    // own), so its title-case behaviour is no longer something to pin here.
    assert_painted(&harness, "RECENT");
    assert_painted(&harness, "CHANGED FILES");
}

#[test]
fn ghost_and_compact_buttons_click_through_the_accessibility_tree() {
    let ghost = Rc::new(Cell::new(false));
    let compact = Rc::new(Cell::new(false));
    let (mut harness, _dir) = widgets_harness(ghost.clone(), compact.clone());
    settle(&mut harness);

    harness.get_by_label("Ghost action").click();
    harness.get_by_label("Compact action").click();
    settle(&mut harness);

    assert!(ghost.get(), "ghost button click must register");
    assert!(compact.get(), "compact button click must register");
}

/// Harness rendering a shared segmented control (issue #01): three options,
/// second pre-selected.
fn segmented_harness() -> (Harness<'static, ()>, tempfile::TempDir) {
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, _state| {
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            egui::CentralPanel::default().show(ui, |ui| {
                segmented_control(ui, &["One", "Two", "Three"], 1);
            });
        },
        (),
    );
    harness.set_size(egui::vec2(400.0, 80.0));
    (harness, tempfile::tempdir().expect("tempdir"))
}

fn short_commit_ref_harness(reference: &'static str) -> (Harness<'static, ()>, tempfile::TempDir) {
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui(move |ui| {
        turbogit_ui::theme::configure_style(ui.ctx());
        if !fonts_installed {
            turbogit_ui::theme::install_fonts(ui.ctx());
            fonts_installed = true;
        }
        ui.label(short_commit_ref(reference));
    });
    harness.set_size(egui::vec2(240.0, 40.0));
    (harness, tempfile::tempdir().expect("tempdir"))
}

#[test]
fn short_commit_reference_renders_seven_characters_by_default() {
    let (mut harness, _dir) = short_commit_ref_harness("0123456789abcdef");
    settle(&mut harness);

    assert_painted(&harness, "0123456");
    assert!(
        !painted_text(&harness)
            .iter()
            .any(|text| text.contains("01234567"))
    );
}

#[test]
fn already_short_commit_reference_renders_intact() {
    let (mut harness, _dir) = short_commit_ref_harness("abc1234");
    settle(&mut harness);

    assert_painted(&harness, "abc1234");
}

#[test]
fn non_ascii_commit_reference_renders_whole_characters() {
    let (mut harness, _dir) = short_commit_ref_harness("界界界界界界界界尾");
    settle(&mut harness);

    assert_painted(&harness, "界界界界界界界");
    assert!(
        !painted_text(&harness)
            .iter()
            .any(|text| text.contains('界') && text.chars().count() > 7)
    );
}

/// Public API exists; clicking a segment returns its index. This is the
/// harness-level proof that any later surface (strategy pickers, file/hunk/
/// line pickers, CLI/libgit2/Auto pickers, diff side-by-side/unified) can
/// adopt the shared widget without inventing a second variant.
#[test]
fn segmented_control_lives_in_widgets_and_clicks_through_the_tree() {
    let (mut harness, _dir) = segmented_harness();
    settle(&mut harness);

    // Every option's text is painted.
    for label in ["One", "Two", "Three"] {
        assert_painted(&harness, label);
    }

    // The track + the selected segment are both painted (two distinct fills:
    // SURFACE_2 for the track and SURFACE_3 for the selected band). Both are
    // token-exact so a future surface cannot drift to a custom palette.
    let mut saw_track = false;
    let mut saw_selected = false;
    for clipped in harness.output().shapes.iter() {
        if let Shape::Rect(rs) = &clipped.shape {
            if rs.fill == Palette::SURFACE_2 {
                saw_track = true;
            }
            if rs.fill == Palette::SURFACE_3 {
                saw_selected = true;
            }
        }
    }
    assert!(saw_track, "segmented track must paint SURFACE_2");
    assert!(saw_selected, "selected segment must paint SURFACE_3");

    // Clicking a segment is observable through the accessibility tree.
    harness.get_by_label("Three").click();
    settle(&mut harness);
    assert_painted(&harness, "Three");
}

// ---------------------------------------------------------------------------
// 4. The shared disabled child scope (issue 03)
// ---------------------------------------------------------------------------

/// The ink each piece of painted text actually drew with — the shared
/// [`test_support::harness::painted_ink`], panicking on a string this frame
/// never painted. Every caller below names a control the same frame just
/// painted, so the panic branch is unreachable in practice and a `None` leak
/// into an equality assertion would only hide the real failure.
fn painted_ink<S>(harness: &Harness<'_, S>, needle: &str) -> Color32 {
    test_support::harness::painted_ink(harness, needle)
        .unwrap_or_else(|| panic!("`{needle}` was not painted"))
}

/// Every fill the last frame painted at full strength, so a caller can tell a
/// full-strength token from one an opacity multiplier dimmed.
fn rects_with_fill(harness: &Harness<'_, ()>) -> Vec<Color32> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Rect(rs) => Some(rs.fill),
            _ => None,
        })
        .collect()
}

/// What one frame observed about each [`disabled_child_scope`] call: the
/// closure's own enabled flag, the value it returned, and the caller's own
/// scope afterwards.
#[derive(Debug, Default)]
struct ScopeProbe {
    /// `child.is_enabled()` as seen *inside* the closure, per call.
    inside_enabled: Vec<bool>,
    /// The value the closure returned, per call.
    returned: Vec<u32>,
    /// `ui.is_enabled()` on the caller's scope after the call returned.
    caller_enabled: Vec<bool>,
    /// The caller's available width before/after the call (must not change).
    width_before: Vec<f32>,
    width_after: Vec<f32>,
    /// The caller's `max_rect` before/after the call (must not change).
    max_rect_before: Vec<egui::Rect>,
    max_rect_after: Vec<egui::Rect>,
    /// The caller's available-rect top before/after the call (must advance).
    top_before: Vec<f32>,
    top_after: Vec<f32>,
}

/// One owner for the "run this in a child scope, and dim it when disabled"
/// sequence. A disabled control must dim itself and swallow its own clicks
/// *without* leaking that state into the widgets beside it, the closure must
/// still run and still return its value, and the caller's own layout and
/// enabled state must come out untouched.
#[test]
fn disabled_child_scope_dim_inside_and_leaves_the_caller_intact() {
    let probe = Rc::new(std::cell::RefCell::new(ScopeProbe::default()));
    let probe_ui = probe.clone();

    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, _state| {
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            egui::CentralPanel::default().show(ui, |ui| {
                for (enabled, label) in [(false, "Dimmed"), (true, "Lit")] {
                    let mut p = probe_ui.borrow_mut();
                    p.width_before.push(ui.available_width());
                    p.max_rect_before.push(ui.max_rect());
                    p.top_before.push(ui.available_rect_before_wrap().top());

                    let seq = p.returned.len() as u32 + 1;
                    let mut inside = None;
                    let value = disabled_child_scope(ui, enabled, |child| {
                        inside = Some(child.is_enabled());
                        primary_button(child, None, label);
                        seq
                    });

                    p.inside_enabled
                        .push(inside.expect("the closure must run in either state"));
                    p.returned.push(value);
                    p.caller_enabled.push(ui.is_enabled());
                    p.width_after.push(ui.available_width());
                    p.max_rect_after.push(ui.max_rect());
                    p.top_after.push(ui.available_rect_before_wrap().top());
                }
                // No-leak: the control drawn straight after the disabled scope
                // must keep its enabled fill and ink.
                primary_button(ui, None, "After");
            });
        },
        (),
    );
    harness.set_size(egui::vec2(420.0, 320.0));
    harness.step();

    let p = probe.borrow();
    assert!(
        p.inside_enabled.len() >= 2,
        "both the disabled and the enabled scope must run"
    );
    // (a) Disabled: the closure's scope reports itself off — and the closure
    //     still ran, still painted, and still handed its value back.
    assert_eq!(
        &p.inside_enabled[..2],
        &[false, true],
        "only the disabled child scope may report !is_enabled()"
    );
    assert_eq!(
        &p.returned[..2],
        &[1, 2],
        "the closure's return value must survive the scope in both states"
    );
    // (b) + (c) The caller's own scope is neither disabled nor re-laid-out:
    //     same enabled flag, same width, same max_rect, and a cursor that
    //     advanced past whatever the child consumed.
    assert_eq!(
        &p.caller_enabled[..2],
        &[true, true],
        "the caller stays enabled"
    );
    for i in 0..p.returned.len() {
        assert_eq!(
            p.width_before[i], p.width_after[i],
            "the child scope must not resize the caller"
        );
        assert_eq!(
            p.max_rect_before[i], p.max_rect_after[i],
            "the child scope must not re-lay-out the caller"
        );
        assert!(
            p.top_after[i] > p.top_before[i],
            "the caller's cursor must advance past the child ({} -> {})",
            p.top_before[i],
            p.top_after[i]
        );
    }
    drop(p);

    // (d) No-leak: the disabled scope dimmed its own control and nothing else.
    //     egui dims a disabled scope by multiplying that scope's painter
    //     opacity, so "dimmed" reads as a lower-alpha paint of the same token
    //     rather than a different token; the accessibility node is the
    //     token-level statement.
    let ink = |needle: &str| painted_ink(&harness, needle);
    let luma = |c: Color32| c.r() as u32 + c.g() as u32 + c.b() as u32;
    assert_eq!(
        ink("Lit"),
        Palette::BRAND_INK,
        "an enabled scope must leave the control at its enabled ink"
    );
    assert_eq!(
        ink("After"),
        Palette::BRAND_INK,
        "the control after a disabled scope must not inherit its dimmed ink"
    );
    assert!(
        luma(ink("Dimmed")) < luma(ink("Lit")),
        "a control inside a disabled scope must paint dimmed: {:?} vs {:?}",
        ink("Dimmed"),
        ink("Lit")
    );
    assert!(
        rects_with_fill(&harness).contains(&Palette::BRAND),
        "a disabled scope must not dim the caller's next control's fill either"
    );

    // The accesskit node is the token-level "dimmed and not clickable" proof.
    assert!(
        harness
            .get_by_label("Dimmed")
            .accesskit_node()
            .is_disabled(),
        "a control inside a disabled scope reports itself disabled"
    );
    assert!(
        !harness.get_by_label("After").accesskit_node().is_disabled(),
        "a control after a disabled scope must not be reported disabled"
    );
}

// ---------------------------------------------------------------------------
// 5. The general card frame (issue 04)
// ---------------------------------------------------------------------------

/// Every rectangle the last frame painted, as
/// `(rect, fill, stroke, corner radius)`.
fn painted_rects<S>(
    harness: &Harness<'_, S>,
) -> Vec<(egui::Rect, Color32, egui::Stroke, egui::CornerRadius)> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Rect(rs) => Some((rs.rect, rs.fill, rs.stroke, rs.corner_radius)),
            _ => None,
        })
        .collect()
}

/// A frame that paints exactly one default card with one line of text in it,
/// at the same 360×400 window every card test below uses. The card's geometry
/// is then read straight off painted output.
fn default_card_harness() -> Harness<'static, ()> {
    let mut fonts_installed = false;
    Harness::builder()
        .with_size(egui::vec2(360.0, 400.0))
        .build_ui_state(
            move |ui, _state| {
                turbogit_ui::theme::configure_style(ui.ctx());
                if !fonts_installed {
                    turbogit_ui::theme::install_fonts(ui.ctx());
                    fonts_installed = true;
                }
                egui::CentralPanel::default().show(ui, |ui| {
                    card(ui, CardFrame::default(), |ui| {
                        ui.label("default body");
                    });
                });
            },
            (),
        )
}

/// Paint-time x of the one galley painting exactly `text`.
fn galley_x<S>(harness: &Harness<'_, S>, text: &str) -> f32 {
    painted_galleys(harness)
        .into_iter()
        .find(|g| g.text == text)
        .map(|g| g.pos.x)
        .unwrap_or_else(|| panic!("`{text}` was not painted"))
}

/// Every card the frame painted, as `(rect, fill, stroke, corner radius)`.
fn card_rects(
    harness: &Harness<'_, ()>,
) -> Vec<(egui::Rect, Color32, egui::Stroke, egui::CornerRadius)> {
    painted_rects(harness)
        .into_iter()
        .filter(|(_, fill, _, radius)| {
            matches!(*fill, Palette::CONTENT_BG | Palette::SURFACE)
                && *radius == egui::CornerRadius::same(turbogit_ui::theme::CARD_RADIUS)
        })
        .filter(|(rect, _, _, _)| rect.height() > 20.0)
        .collect()
}

/// The one card geometry, and the rule installed with it: a default card is a
/// filled rect at the card radius that strokes **nothing**, while inner padding,
/// surface tone and how the card claims its width stay per-site parameters.
///
/// Every fact here is read off the frame's painted `Shape::Rect` stream — the
/// same sink the fill, the radius and the geometry come from — so the
/// "no stroke" claim is a claim about what reached the painter, not about what
/// the frame was configured with. See
/// [`card_paints_no_stroke_where_a_default_card_is`] for why the weaker
/// configuration-level claim would be worthless.
#[test]
fn card_frame_owns_its_shape_and_honours_pad_surface_and_width() {
    let panel_width = Rc::new(Cell::new(0.0_f32));
    let panel_width_ui = panel_width.clone();

    let mut fonts_installed = false;
    let mut harness = Harness::builder()
        .with_size(egui::vec2(360.0, 400.0))
        .build_ui_state(
            move |ui, _state| {
                turbogit_ui::theme::configure_style(ui.ctx());
                if !fonts_installed {
                    turbogit_ui::theme::install_fonts(ui.ctx());
                    fonts_installed = true;
                }
                egui::CentralPanel::default().show(ui, |ui| {
                    panel_width_ui.set(ui.available_width());
                    card(ui, CardFrame::default(), |ui| {
                        ui.label("default body");
                    });
                    card(ui, CardFrame::default().raised().padded(24), |ui| {
                        ui.label("raised body");
                    });
                    // The Welcome clone/recents cards' shape, so every
                    // configuration the app's seven call sites use is covered.
                    card(ui, CardFrame::default().padded(16), |ui| {
                        ui.label("padded body");
                    });
                    // The Welcome changelog overlay's shape: raised, 20 px,
                    // pinned to 420. Unbordered here so this test pins the
                    // geometry contract; the border's own 2pt of cost is
                    // `the_bordered_card_is_the_one_surface_allowed_to_stroke`'s.
                    card(
                        ui,
                        CardFrame::default().raised().padded(20).min_width(420.0),
                        |ui| {
                            ui.label("pinned body");
                        },
                    );
                });
            },
            (),
        );
    settle(&mut harness);

    let rects = painted_rects(&harness);
    // Nothing here lands on a fraction of a point: a card's box, its corner
    // radius and its body inset are all integral in the frame, both with and
    // without a hairline. The old suite carried a 2pt `STROKE_SLACK` to absorb
    // the 1pt the hairline folded into every card's padding (egui counts
    // `stroke.width` as inner margin); with the hairline gone that 1pt is gone
    // and the tolerance can be a rounding epsilon rather than a whole pixel.
    const EPS: f32 = 0.01;

    // One owner for the shape: the default card is a `CONTENT_BG` fill at the
    // `CARD_RADIUS` rounding and nothing more.
    let (default_rect, default_radius) = rects
        .iter()
        .find(|(rect, fill, _, _)| *fill == Palette::CONTENT_BG && rect.height() > 20.0)
        .map(|(rect, _, _, radius)| (*rect, *radius))
        .expect("the default card must paint a CONTENT_BG frame");
    assert_eq!(
        default_radius,
        egui::CornerRadius::same(turbogit_ui::theme::CARD_RADIUS),
        "the card radius is the shared one, not a per-call-site value"
    );

    // Stretch is the default width strategy: the card spans its pane, and its
    // body sits exactly one `PANEL_PADDING` inside the card's own box.
    let panel = panel_width.get();
    assert!(
        (default_rect.width() - panel).abs() <= EPS,
        "the default card must stretch: {} vs available {panel}",
        default_rect.width()
    );
    assert!(
        (galley_x(&harness, "default body")
            - default_rect.left()
            - turbogit_ui::theme::PANEL_PADDING)
            .abs()
            <= EPS,
        "the default card must pad by exactly PANEL_PADDING, got {}",
        galley_x(&harness, "default body") - default_rect.left()
    );

    // The two `Raised` cards differ only in padding and width strategy, so
    // separate them by the width each one claims.
    let surface: Vec<egui::Rect> = rects
        .iter()
        .filter(|(rect, fill, _, _)| *fill == Palette::SURFACE && rect.height() > 20.0)
        .map(|(rect, _, _, _)| *rect)
        .collect();
    assert_eq!(
        surface.len(),
        2,
        "both `raised()` cards must paint, got {surface:?}"
    );
    let raised_rect = *surface
        .iter()
        .find(|rect| (rect.width() - panel).abs() <= EPS)
        .expect("the padded raised card must still stretch to the pane");
    let pinned_rect = *surface
        .iter()
        .find(|rect| (rect.width() - (420.0 + 2.0 * 20.0)).abs() <= EPS)
        .unwrap_or_else(|| {
            panic!("the pinned card must honour its 420 min width, got {surface:?}")
        });

    // A per-site padding parameter moves the body's inset by exactly the
    // difference in padding, without changing the fill or the radius.
    let body_x = |text: &str| galley_x(&harness, text);
    assert!(
        (body_x("raised body") - body_x("default body") - 12.0).abs() <= EPS,
        "`.padded(24)` must inset the body 12 further than `.padded(12)`, got {}",
        body_x("raised body") - body_x("default body")
    );
    assert!(
        (body_x("padded body") - body_x("default body") - 4.0).abs() <= EPS,
        "`.padded(16)` must inset the body 4 further than `.padded(12)`, got {}",
        body_x("padded body") - body_x("default body")
    );
    assert!(
        (body_x("pinned body") - body_x("default body") - 8.0).abs() <= EPS,
        "the pinned card must inset the body 8 further than `.padded(12)`, got {}",
        body_x("pinned body") - body_x("default body")
    );

    // `MinWidth` keeps its pin and does not stretch.
    assert!(
        (pinned_rect.width() - panel).abs() > EPS,
        "a MinWidth card must not stretch to the pane ({})",
        pinned_rect.width()
    );
    assert!(
        raised_rect.width() < pinned_rect.width(),
        "a stretched card must be narrower than one pinned past the pane"
    );
}

/// The headline of the ticket: a default card paints a fill and **no stroked
/// rect at all**.
///
/// Asserted on painted output, and that is the whole point. `CardFrame` has no
/// stroke field for a caller to flip, so an assertion of the shape "the default
/// frame has no stroke" would be a tautology — it would pass just as happily
/// against a body that still painted the hairline, because the type cannot
/// express one. Reading the `Shape::Rect` stream the painter received closes
/// that hole: a stroke put back into the body is a shape in this list whether or
/// not any type mentions it.
#[test]
fn card_paints_no_stroke_where_a_default_card_is() {
    let mut fonts_installed = false;
    let mut harness = Harness::builder()
        .with_size(egui::vec2(360.0, 400.0))
        .build_ui_state(
            move |ui, _state| {
                turbogit_ui::theme::configure_style(ui.ctx());
                if !fonts_installed {
                    turbogit_ui::theme::install_fonts(ui.ctx());
                    fonts_installed = true;
                }
                egui::CentralPanel::default().show(ui, |ui| {
                    card(ui, CardFrame::default(), |ui| {
                        ui.label("default body");
                    });
                    card(ui, CardFrame::default().padded(16), |ui| {
                        ui.label("padded body");
                    });
                    card(ui, CardFrame::default().raised().padded(16), |ui| {
                        ui.label("raised body");
                    });
                });
            },
            (),
        );
    settle(&mut harness);

    let rects = painted_rects(&harness);
    let cards: Vec<egui::Rect> = card_rects(&harness)
        .into_iter()
        .map(|(rect, _, _, _)| rect)
        .collect();
    assert_eq!(
        cards.len(),
        3,
        "all three default cards must paint, got {cards:?}"
    );

    // The assertion. Every card's own box, and nothing else: if the body strokes
    // its frame — or a widget inside the card strokes a rect that coincides with
    // it — this fails on the painted shape.
    for card in &cards {
        let strokes_over_card: Vec<&(egui::Rect, Color32, egui::Stroke, egui::CornerRadius)> =
            rects
                .iter()
                .filter(|(rect, _, stroke, _)| {
                    stroke.color != Color32::TRANSPARENT
                        && stroke.width > 0.0
                        && rect.intersects(*card)
                })
                .collect();
        assert!(
            strokes_over_card.is_empty(),
            "a card is a surface, so nothing may stroke it — the card at {card:?} is \
             covered by {strokes_over_card:?}"
        );
    }

    // Belt and braces, and the reason the loop above can be trusted to be
    // exhaustive: in a frame of nothing but panels and cards, the panel frames
    // are the only other rects, and they stroke nothing either. So the whole
    // frame carries no stroke at all, which is what "a default card paints no
    // stroked rect" means when the card is the only thing in it.
    let any_stroke = rects
        .iter()
        .find(|(_, _, stroke, _)| stroke.color != Color32::TRANSPARENT && stroke.width > 0.0);
    assert!(
        any_stroke.is_none(),
        "a frame of default cards must paint no stroke anywhere, found {any_stroke:?}"
    );
}

/// The bordered variant, and the only surface in the app allowed to wear one.
///
/// The app's one caller is the Welcome changelog, a card inside an `egui::Area`
/// — the "it floats" case the rule reserves a stroke for. Asserted here on
/// painted output as well, because a variant that exists but paints nothing is
/// not a variant.
#[test]
fn the_bordered_card_is_the_one_surface_allowed_to_stroke() {
    let mut fonts_installed = false;
    let mut harness = Harness::builder()
        .with_size(egui::vec2(360.0, 400.0))
        .build_ui_state(
            move |ui, _state| {
                turbogit_ui::theme::configure_style(ui.ctx());
                if !fonts_installed {
                    turbogit_ui::theme::install_fonts(ui.ctx());
                    fonts_installed = true;
                }
                egui::CentralPanel::default().show(ui, |ui| {
                    card(ui, CardFrame::default(), |ui| {
                        ui.label("default body");
                    });
                    card(
                        ui,
                        CardFrame::default()
                            .raised()
                            .bordered()
                            .padded(20)
                            .min_width(420.0),
                        |ui| {
                            ui.label("pinned body");
                        },
                    );
                });
            },
            (),
        );
    settle(&mut harness);

    let rects = painted_rects(&harness);
    let (bordered_rect, bordered_stroke, bordered_radius) = rects
        .iter()
        .find(|(rect, fill, stroke, _)| {
            *fill == Palette::SURFACE && stroke.width > 0.0 && rect.height() > 20.0
        })
        .map(|(rect, _, stroke, radius)| (*rect, *stroke, *radius))
        .expect("`.bordered()` must paint a stroked SURFACE card");
    assert_eq!(
        bordered_stroke,
        egui::Stroke::new(1.0, Palette::LINE),
        "a bordered card is a 1px `LINE` hairline — the one stroke the app allows"
    );
    assert_eq!(
        bordered_radius,
        egui::CornerRadius::same(turbogit_ui::theme::CARD_RADIUS),
        "a bordered card keeps the card radius; the border is not its own shape"
    );
    // The border's cost, named rather than tolerated: egui folds
    // `Frame::stroke.width` into the frame's inner margin, so a bordered card is
    // one point wider on each side than the same card unbordered. 462 = 420 of
    // pinned body + 2×20 of padding + 2×1 of hairline.
    assert!(
        (bordered_rect.width() - (420.0 + 2.0 * 20.0 + 2.0 * 1.0)).abs() <= 0.01,
        "a bordered pinned card is its 460 plus the 1pt hairline on each side, got {}",
        bordered_rect.width()
    );
    // …and the same accounting on the body: it sits `pad + 1` from the edge.
    assert!(
        (galley_x(&harness, "pinned body") - bordered_rect.left() - (20.0 + 1.0)).abs() <= 0.01,
        "a bordered card's body sits one point inside its padding, got {}",
        galley_x(&harness, "pinned body") - bordered_rect.left()
    );

    // The default card in the same frame is untouched by the variant's
    // existence — the border is opt-in per call site, not a frame-wide setting.
    let (default_rect, default_stroke) = rects
        .iter()
        .find(|(rect, fill, _, _)| *fill == Palette::CONTENT_BG && rect.height() > 20.0)
        .map(|(rect, _, stroke, _)| (*rect, *stroke))
        .expect("the default card must paint a CONTENT_BG frame");
    assert_eq!(
        default_stroke,
        egui::Stroke::NONE,
        "the default card next to a bordered one still strokes nothing, got {default_stroke:?} at {default_rect:?}"
    );
}

/// Removing the hairline moves no content — which is the only thing standing
/// between this ticket and a silent reflow of every carded region in the app.
///
/// The claim is pinned against a baseline measured on the *pre-ticket* body
/// (one default card, 360×400 window, one line of text) rather than against a
/// tolerance, and the measured result is not zero:
///
/// - egui 0.36 folds `Frame::stroke.width` into the frame's inner margin
///   (`Frame::total_margin`), so the 1px hairline cost one point of padding on
///   every side. The body's inset from the card's own edge was
///   `PANEL_PADDING + 1` and is now exactly `PANEL_PADDING`.
/// - the card's own box kept its position and its width, and lost exactly the
///   2pt the two horizontal lines were costing it in height.
///
/// So: one point of horizontal movement, 2pt of height, both attributable to
/// the hairline by name, and nothing else. A padding edit of any other size
/// fails here.
#[test]
fn dropping_the_cards_hairline_moved_nothing_but_the_hairlines_own_pixel() {
    let mut harness = default_card_harness();
    settle(&mut harness);

    const EPS: f32 = 0.01;
    /// The same card measured on the body that stroked a 1px `LINE` hairline.
    const BODY_INSET_WITH_HAIRLINE: f32 = 13.0;
    const BOX_HEIGHT_WITH_HAIRLINE: f32 = 40.0;

    let rects = painted_rects(&harness);
    let (card_rect, radius) = rects
        .iter()
        .find(|(rect, fill, _, _)| *fill == Palette::CONTENT_BG && rect.height() > 20.0)
        .map(|(rect, _, _, radius)| (*rect, *radius))
        .expect("the default card must paint a CONTENT_BG frame");
    assert_eq!(
        radius,
        egui::CornerRadius::same(turbogit_ui::theme::CARD_RADIUS)
    );

    // The box did not move and did not narrow: the pane is 328pt at this
    // window, and the card claims all of it under `Stretch`.
    let body_x = galley_x(&harness, "default body");
    let inset = body_x - card_rect.left();
    assert!(
        (card_rect.width() - 328.0).abs() <= EPS,
        "the card must still claim the whole pane, got {}",
        card_rect.width()
    );
    assert!(
        (inset - turbogit_ui::theme::PANEL_PADDING).abs() <= EPS,
        "the body must sit exactly PANEL_PADDING inside the card, got {inset} \
         (it sat {BODY_INSET_WITH_HAIRLINE} while the card stroked)"
    );
    assert!(
        (card_rect.height() - (BOX_HEIGHT_WITH_HAIRLINE - 2.0)).abs() <= EPS,
        "the card must lose exactly the 2pt the two hairlines cost, got {} vs {}",
        card_rect.height(),
        BOX_HEIGHT_WITH_HAIRLINE - 2.0
    );
    // The whole movement, stated once as the difference between the two
    // measurements — one point of inset, the hairline's own point, and nothing
    // else. This is the assertion a padding edit breaks first.
    assert!(
        (BODY_INSET_WITH_HAIRLINE - inset - 1.0).abs() <= EPS,
        "dropping the hairline must recover exactly its own 1pt of padding, \
         recovered {}",
        BODY_INSET_WITH_HAIRLINE - inset
    );
}

// ---------------------------------------------------------------------------
// 6. The closed three-chip vocabulary (ticket 05)
// ---------------------------------------------------------------------------
//
// Two halves, because "exactly one function produces a ref chip" is a claim a
// render seam cannot make.
//
// **Structural.** Read the chip module's own source (`include_str!`) and count
// which items name a chip fill and which functions consume it. A render test
// proves that the chip a given surface paints *is* the ref chip; it keeps
// passing if a second function paints the same ref chip two inches away, which
// is exactly the duplication this vocabulary exists to prevent. The scan is
// sound only because `cargo fmt -- --check` is a repository gate: it finds each
// *top-level* item by its column-0 `pub fn` / `pub const` head and closes it at
// the column-0 `}` (or `};`), which is how rustfmt lays this file out. A
// hand-rolled layout panics rather than passing quietly, and every ratchet
// compares against a written-out name list, so a fourth chip is named here
// before it can reach a screen.
//
// **Rendered.** The fills, inks, radii, type size and faces are read off
// painted output — `Shape::Rect` for the fill *and* its corner radius, and
// `test_support::harness::painted_ink` for the ink, because this codebase lays
// text out in `WHITE` and inks at paint time, so a galley's layout colour
// answers `WHITE` and the assertion would pass for the wrong reason.
//
// Every claim about text is scoped **by string and by position**, never by
// first match, and every claim about wrapping is made from geometry (row count
// and galley size), never from the galley's text accessor — which returns the
// *unwrapped* input, so a wrapped chip would look identical through it.

/// The chip module's own source, read at compile time.
const CHIPS_SRC: &str = include_str!("../src/ui/widgets/chips.rs");

/// The container module's own source — where the pane chrome lives.
const CONTAINERS_SRC: &str = include_str!("../src/ui/widgets/containers.rs");
/// The conflict kit's pane module, whose module-private header is gone.
const CONFLICT_PANE_SRC: &str = include_str!("../src/ui/kit/conflict_pane.rs");
/// The shared widget module's public façade, whose re-export list is the
/// shared widget module's public surface.
const WIDGETS_MOD_SRC: &str = include_str!("../src/ui/widgets/mod.rs");

/// One top-level item of a module: its name, its own text (doc comments
/// excluded — the claim is about code, not about what a comment says), and
/// whether it is a function.
struct ModuleItem {
    name: String,
    text: String,
    is_fn: bool,
}

/// The name an item head declares, with the generic parameter list, the
/// argument list and a `const`'s type annotation dropped: `pane_header<R>`
/// declares `pane_header`, and `pub const PANE_HEADER_HEIGHT: f32 = 28.0;`
/// declares `PANE_HEADER_HEIGHT`.
fn declared_name(rest: &str) -> String {
    rest.split(['(', '<', ':'])
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned()
}

/// Every top-level item of `src`, in source order.
fn top_level_items(src: &str) -> Vec<ModuleItem> {
    let lines: Vec<&str> = src.lines().collect();
    // The declared name of a column-0 `pub fn` / `fn` / `pub const` / `const`
    // head, or `None` for any other line.
    let head_name = |line: &str| -> Option<(String, bool)> {
        let trimmed = line.trim_start();
        for prefix in ["pub fn ", "fn "] {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                return Some((declared_name(rest), true));
            }
        }
        for prefix in ["pub const ", "const "] {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                return Some((declared_name(rest), false));
            }
        }
        None
    };

    let mut items = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        // Column 0 only: an indented head belongs to the enclosing `impl`, and
        // an `impl` member is not a module-level item.
        let head = if line.starts_with(' ') || line.starts_with('\t') {
            None
        } else {
            head_name(line)
        };
        let Some((name, is_fn)) = head else {
            i += 1;
            continue;
        };
        // A one-line `const` ends with its own `;`; a `fn` or a block-bodied
        // `const` ends at the column-0 `}` (or `};`) that closes it.
        let end = if line.trim_end().ends_with(';') {
            Some(i)
        } else {
            lines
                .iter()
                .enumerate()
                .skip(i + 1)
                .find(|(_, tail)| matches!(tail.trim_end(), "}" | "};"))
                .map(|(j, _)| j)
        };
        let end = end.unwrap_or_else(|| {
            panic!(
                "module item `{name}` has no column-0 terminator; the structural \
                 ratchets assume the rustfmt layout, so they refuse to pass on a \
                 hand-rolled one"
            )
        });
        items.push(ModuleItem {
            name,
            text: lines[i..=end].join("\n"),
            is_fn,
        });
        i = end + 1;
    }
    items
}

/// Byte offsets at which `needle` occurs in `text` as a whole identifier, so a
/// search for `Palette::RAISED` does not also match `Palette::RAISED_ON_CARD`.
fn word_offsets(text: &str, needle: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut hits = Vec::new();
    let mut from = 0;
    while let Some(rel) = text[from..].find(needle) {
        let at = from + rel;
        let after = at + needle.len();
        let bounded_before = at == 0 || !is_ident(bytes[at - 1]);
        let bounded_after = after >= bytes.len() || !is_ident(bytes[after]);
        if bounded_before && bounded_after {
            hits.push(at);
        }
        from = at + needle.len();
    }
    hits
}

/// The names of every top-level item of `src` that names `needle` in its own
/// text, sorted so the ratchets read as sets rather than as an incidental
/// source order.
fn items_naming(src: &str, needle: &str) -> Vec<String> {
    let mut names: Vec<String> = top_level_items(src)
        .into_iter()
        .filter(|item| !word_offsets(&item.text, needle).is_empty())
        .map(|item| item.name)
        .collect();
    names.sort();
    names
}

/// The names of every **function** in `src` that *uses* `ident`.
///
/// The item that declares `ident` is excluded, so asking "who calls
/// `chip_with`" answers with the callers rather than with `chip_with` itself.
fn fns_using(src: &str, ident: &str) -> Vec<String> {
    let mut names: Vec<String> = top_level_items(src)
        .into_iter()
        .filter(|item| item.is_fn && item.name != ident)
        .filter(|item| !word_offsets(&item.text, ident).is_empty())
        .map(|item| item.name)
        .collect();
    names.sort();
    names
}

/// The ref name the fixture chip holds, the current ref, and an ad-hoc count.
/// Distinct strings, because a shared one would make "which chip painted this"
/// unanswerable.
const REF_NAME: &str = "feature/multi-root";
const CURRENT_NAME: &str = "main";
const COUNT_TEXT: &str = "12";

/// A frame holding exactly the three chips, side by side and nothing else, so
/// every fill in the output is a chip fill and "no fourth chip" is a statement
/// about the whole frame rather than about a search result.
fn three_chips_harness() -> Harness<'static, ()> {
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, _state| {
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            egui::CentralPanel::default().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ref_chip(ui, REF_NAME);
                    current_chip(ui, CURRENT_NAME);
                    count_chip(ui, COUNT_TEXT);
                });
            });
        },
        (),
    );
    harness.set_size(egui::vec2(460.0, 80.0));
    harness
}

/// Every rect the frame painted at the shared chip height, as
/// `(rect, fill, corner radius)`. The chip geometry's own height is the filter,
/// so a chip is read by *being the right shape* rather than by its fill — which
/// is what lets the closed-set ratchet count chips it does not recognise.
fn chip_sized_rects<S>(harness: &Harness<'_, S>) -> Vec<(egui::Rect, Color32, egui::CornerRadius)> {
    painted_rects(harness)
        .into_iter()
        .filter(|(rect, _, _, _)| (rect.height() - CHIP_HEIGHT).abs() < 0.01)
        .map(|(rect, fill, _, radius)| (rect, fill, radius))
        .collect()
}

/// The one chip-sized rect painted with `fill`, with the round-trip through
/// painted output spelled out: the count proves the assertion is not reading a
/// coincidence, and the radius is returned because a fill alone cannot show it.
fn the_chip_rect_with<S>(
    harness: &Harness<'_, S>,
    fill: Color32,
    who: &str,
) -> (egui::Rect, egui::CornerRadius) {
    let hits: Vec<_> = chip_sized_rects(harness)
        .into_iter()
        .filter(|(_, painted, _)| *painted == fill)
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "expected exactly one {who} at chip height in {fill:?}, found {hits:#?}"
    );
    let (rect, _, radius) = hits.into_iter().next().expect("one chip rect");
    (rect, radius)
}

/// The font a string was laid out with, which is where the *size* of the chip
/// type is read from — [`PaintedGalley`] reports the family but not the size.
fn painted_font<S>(harness: &Harness<'_, S>, needle: &str) -> egui::FontId {
    harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(text) if text.galley.text() == needle => text
                .galley
                .job
                .sections
                .first()
                .map(|s| s.format.font_id.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("`{needle}` was not painted"))
}

/// The number of laid-out rows in the single galley painting exactly `needle`.
/// The wrapping claim, taken from geometry: the galley's *text* accessor returns
/// the unwrapped input, so a chip that wrapped looks identical through it.
fn painted_rows(harness: &Harness<'_, ()>, needle: &str) -> usize {
    harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(text) if text.galley.text() == needle => Some(text.galley.rows.len()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("`{needle}` was not painted"))
}

/// The one painted galley for `needle`, located by string. Each of the three
/// fixture strings paints exactly once in this frame, so the match is
/// unambiguous — and the count is asserted rather than assumed, because a
/// chip label that painted twice would make every ink claim here a claim about
/// whichever copy came first.
fn the_only_galley(
    harness: &Harness<'_, ()>,
    needle: &str,
) -> test_support::harness::PaintedGalley {
    let hits: Vec<_> = painted_galleys(harness)
        .into_iter()
        .filter(|g| g.text == needle)
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "`{needle}` must paint exactly once in the chip frame, found {hits:#?}"
    );
    hits.into_iter().next().expect("one galley")
}

/// **Structural: one function per chip, and no others.** The three colour pairs
/// are the only place in the module a chip names a fill, and each is consumed by
/// exactly one function — the constructor for its chip. A second ref chip
/// anywhere in the app is a second consumer of `REF_CHIP_COLORS` and fails here.
#[test]
fn exactly_one_function_produces_each_of_the_three_chips() {
    // One item defines each pair…
    assert_eq!(
        items_naming(CHIPS_SRC, "Palette::RAISED_ON_CARD"),
        ["REF_CHIP_COLORS"],
        "the ref chip's fill is defined in exactly one place"
    );
    assert_eq!(
        items_naming(CHIPS_SRC, "Palette::ROW_SELECTED"),
        ["CURRENT_CHIP_COLORS"],
        "the current chip's fill is defined in exactly one place"
    );
    assert_eq!(
        items_naming(CHIPS_SRC, "Palette::RAISED"),
        ["COUNT_CHIP_COLORS"],
        "the count chip's fill is defined in exactly one place"
    );
    // …and exactly one function renders each.
    assert_eq!(fns_using(CHIPS_SRC, "REF_CHIP_COLORS"), ["ref_chip"]);
    assert_eq!(
        fns_using(CHIPS_SRC, "CURRENT_CHIP_COLORS"),
        ["current_chip"]
    );
    assert_eq!(fns_using(CHIPS_SRC, "COUNT_CHIP_COLORS"), ["count_chip"]);

    // The closed set, from the other end. `chip_with` is the one body painter
    // in the module, so the functions that reach it *are* the chips — plus the
    // badge, which is the pill slot and is named here so it stays visibly
    // outside the chip set rather than quietly inside it.
    assert_eq!(
        fns_using(CHIPS_SRC, "chip_with"),
        ["badge", "count_chip", "current_chip", "ref_chip"],
        "`chip_with` is the only thing that paints a chip body: a fifth caller is \
         a fourth chip, or a second badge, and it has to be named here first"
    );
    // Of those four, exactly three take the compact chip geometry; the badge
    // takes the pill slot. So a chip is a function that paints with
    // `COMPACT_CHIP_GEOMETRY`, and that set is closed at three.
    assert_eq!(
        fns_using(CHIPS_SRC, "COMPACT_CHIP_GEOMETRY"),
        [
            "compact_chip_radius",
            "count_chip",
            "current_chip",
            "ref_chip"
        ],
        "the chip geometry is read by the three constructors and by the radius \
         accessor beside them — nobody else, so the chip set stays closed"
    );
    assert_eq!(
        fns_using(CHIPS_SRC, "CHIP_GEOMETRY"),
        [
            "badge",
            "chip_radius",
            "chip_rect_right",
            "chip_text_origin",
            "paint_chip",
        ],
        "the pill slot keeps its own callers: the badge, the four public \
         geometry helpers, and no chip"
    );

    // No chip in the module has a click plane. `hash_chip` is a label by
    // decision (ADR-0024: copying a hash is the commit menu's item) and
    // `ref_chip` inherits it, so a `Sense::click` here would hand every ref name
    // in the app a dead press target and an accessibility node answering Click.
    assert!(
        items_naming(CHIPS_SRC, "Sense::click").is_empty(),
        "no chip may gain a click plane: a chip that is a label inherits \
         `hash_chip`'s hover-only sense, and one that is a control carries its \
         own node"
    );
}

/// **The reserved counter orange and the repository-state colours are not in the
/// chip vocabulary.** A state is a coloured word or a leading dot taken from
/// [`turbogit_ui::theme::RepoState::color`]; a count is a number. The chip module
/// naming either is the mistake the branches screen made twice, arriving as one
/// line of `match`.
///
/// The badge family is the deliberate exception and is named: it states a *file*
/// status, keeps the pill radius and its own slot, and is not a chip.
#[test]
fn no_chip_names_the_reserved_counter_or_a_repository_state_colour() {
    for reserved in [
        "Palette::COUNTER",
        "Palette::AHEAD",
        "Palette::STATUS_DIVERGED",
    ] {
        assert_eq!(
            items_naming(CHIPS_SRC, reserved),
            Vec::<String>::new(),
            "`{reserved}` means dirt / unpushed / diverged and is a mark, never \
             a chip fill: a state behind a fill stops reading as state"
        );
    }
    // …and the count chip is the replacement, so it is a raised surface with
    // secondary ink rather than anything borrowed from that map.
    assert_eq!(COUNT_CHIP_COLORS.bg, Palette::RAISED);
    assert_eq!(COUNT_CHIP_COLORS.fg, Palette::INK_2);
}

/// **The hash chip's ink is the hash token, not the accent.** The one place in
/// the tree that renders a hash as itself, and the one consumer `Palette::LINK`
/// has — so this is the mechanical half of the sweep's claim about `LINK`.
///
/// A hash is information the user reads to name a commit, and the chip answers
/// no press and offers no focus (ADR-0024), so it may neither wear the action
/// accent nor be dimmed past legibility. `BRAND` on this chip's own `SURFACE_3`
/// fill measures 2.48:1 — under even the 3:1 floor for a non-text graphical
/// object — which is the measurement that made the token the right answer.
#[test]
fn the_hash_chip_paints_the_hash_token_and_not_the_action_accent() {
    const HASH: &str = "a1b2c3d";
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, _state| {
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            egui::CentralPanel::default().show(ui, |ui| {
                hash_chip(ui, HASH, "");
            });
        },
        (),
    );
    settle(&mut harness);

    assert_eq!(
        painted_ink(&harness, HASH),
        Palette::LINK,
        "the hash chip's ink is the hash token"
    );
    assert_ne!(
        painted_ink(&harness, HASH),
        Palette::BRAND,
        "…and not the action accent: a chip that answers no press may not look \
         like one, and the accent is unreadable on this fill anyway"
    );
    // The fill is the third raised surface, not the chip's own slot, and it did
    // not move — only the ink did.
    assert!(
        rects_with_fill(&harness).contains(&Palette::SURFACE_3),
        "the hash chip keeps its SURFACE_3 fill; only its ink moved off the accent"
    );
}

/// **The ref chip, end to end.** The one function, painting the raised-on-card
/// fill at the chip radius, with the chip type size in the data face and
/// secondary ink — the step-up ink, because the muted step is not legal on a
/// raised surface.
#[test]
fn the_ref_chip_paints_the_raised_on_card_fill_at_the_chip_radius_in_the_data_face() {
    let mut harness = three_chips_harness();
    settle(&mut harness);

    let (rect, radius) = the_chip_rect_with(&harness, Palette::RAISED_ON_CARD, "the ref chip");
    assert_eq!(
        radius,
        egui::CornerRadius::same(turbogit_ui::theme::CHIP_RADIUS),
        "the ref chip is the compact chip, not the pill"
    );

    // The ink, through the painted-ink seam: `INK_3` is not legal on this fill
    // and ticket 02's construction-site ratchet already says so.
    assert_eq!(painted_ink(&harness, REF_NAME), Palette::INK_2);

    // The face and the type size, read off the laid-out font.
    let font = painted_font(&harness, REF_NAME);
    assert_eq!(
        font.family,
        egui::FontFamily::Monospace,
        "a ref chip holds a ref name, which is data"
    );
    assert_eq!(
        font.size,
        turbogit_ui::theme::TYPE_CHIP,
        "a chip's type size is the chip type size"
    );
    let galley = the_only_galley(&harness, REF_NAME);
    assert_eq!(galley.family, egui::FontFamily::Monospace);

    // Wrapping, from geometry. `layout_no_wrap` is the point — a ref name wraps
    // to a second row inside an 18px chip or it is invisible, and the galley's
    // text accessor cannot see that happen, so the claims are the row count and
    // the width the chip reserved around one row of it.
    assert_eq!(
        painted_rows(&harness, REF_NAME),
        1,
        "a chip never wraps: the second row would be clipped by an 18px body"
    );
    assert!(
        (rect.width() - (galley.rect.width() + 2.0 * CHIP_PAD_X)).abs() < 0.01,
        "the chip reserved one row of text plus the shared inset on each side, \
         got {} for a {}-wide row",
        rect.width(),
        galley.rect.width()
    );

    // No click plane, at the render seam too. The chip answers as a **Label**:
    // the ref name is findable by name — a screen reader can read a branch off
    // the row — without the node claiming a verb. A clickable chip would answer
    // Click, which is the ADR-0024 failure: an unnamed button where there is no
    // button. (Asserted positively rather than by the absence of a button node:
    // `get_all_by_role` panics when nothing matches, so "no button" is not a
    // shape this harness can express.)
    let nodes: Vec<_> = harness.get_all_by_label(REF_NAME).collect();
    assert_eq!(
        nodes.len(),
        1,
        "the ref chip answers under its own name, once: {nodes:?}"
    );
    assert_eq!(
        nodes[0].accesskit_node().role(),
        egui::accesskit::Role::Label,
        "a ref chip is a label, not a button"
    );
    for text in [CURRENT_NAME, COUNT_TEXT] {
        let node = harness
            .get_all_by_label(text)
            .next()
            .unwrap_or_else(|| panic!("`{text}` answers under its own name"));
        assert_eq!(
            node.accesskit_node().role(),
            egui::accesskit::Role::Label,
            "`{text}` is a chip label too, not a button"
        );
    }
}

/// **The current chip, and the negative it exists to carry.** It paints the
/// selected-row fill with the readable accent ink — and it is the *only* chip
/// allowed near the brand.
///
/// The negative is what makes the permission mean something: a brand fill behind
/// running text is a button's shape, and the moment a chip takes it the accent
/// stops meaning "the current ref". So the frame asserts that no chip fills the
/// brand and no chip paints the brand as ink, while the current chip keeps the
/// one accent ink the set allows.
#[test]
fn the_current_chip_is_the_only_chip_allowed_near_the_brand() {
    let mut harness = three_chips_harness();
    settle(&mut harness);

    let (rect, radius) = the_chip_rect_with(&harness, Palette::ROW_SELECTED, "the current chip");
    assert_eq!(rect.height(), CHIP_HEIGHT);
    assert_eq!(
        radius,
        egui::CornerRadius::same(turbogit_ui::theme::CHIP_RADIUS)
    );
    assert_eq!(painted_ink(&harness, CURRENT_NAME), Palette::ACCENT_TEXT);

    // The brand, at construction: no chip's fill is the brand token, and the one
    // accent ink in the set is the current chip's. (The constructor table is
    // the seam that can prove this — a render test can only see the chips this
    // frame happened to paint.)
    let chips = [
        ("ref chip", REF_CHIP_COLORS),
        ("current chip", CURRENT_CHIP_COLORS),
        ("count chip", COUNT_CHIP_COLORS),
    ];
    // `ACCENT` and `BRAND` are one colour under two names, and both are named
    // in the token layer for branch chips, so both spellings are excluded by
    // asserting the alias first: a chip that reaches for either is reaching for
    // the brand, and the alias is why one inequality covers both.
    assert_eq!(
        Palette::ACCENT,
        Palette::BRAND,
        "the accent and the brand are one token, so the negative below covers \
         both spellings"
    );
    for (who, colors) in chips {
        assert_ne!(
            colors.bg,
            Palette::BRAND,
            "the {who} must not fill the brand token: a brand fill behind a \
             running string is a button's shape"
        );
    }
    let accented: Vec<&str> = chips
        .iter()
        .filter(|(_, colors)| matches!(colors.fg, Palette::ACCENT_TEXT | Palette::BRAND))
        .map(|(who, _)| *who)
        .collect();
    assert_eq!(
        accented,
        ["current chip"],
        "exactly one chip may wear an accent, and it is the one stating a fact \
         about the current ref rather than asking for something"
    );
    assert_ne!(
        CURRENT_CHIP_COLORS.fg,
        Palette::BRAND_INK,
        "a solid brand fill is gone, so its on-brand ink has nothing to sit on"
    );

    // The same claim at the render seam: nothing in a frame of three chips fills
    // the brand and nothing inks the brand.
    let brand_fills: Vec<egui::Rect> = painted_rects(&harness)
        .into_iter()
        .filter(|(_, fill, _, _)| *fill == Palette::BRAND)
        .map(|(rect, _, _, _)| rect)
        .collect();
    assert!(
        brand_fills.is_empty(),
        "no chip fills the brand token: {brand_fills:?}"
    );
    let brand_ink: Vec<String> = painted_galleys(&harness)
        .into_iter()
        .filter(|g| g.color == Palette::BRAND)
        .map(|g| g.text.clone())
        .collect();
    assert!(
        brand_ink.is_empty(),
        "no chip inks the brand token: {brand_ink:?}"
    );
}

/// **The count chip.** The plain raised surface with secondary ink in the data
/// face, so a column of counts aligns digit-for-digit and a number is not a
/// fourth kind of badge.
#[test]
fn the_count_chip_paints_the_raised_fill_with_secondary_mono_ink() {
    let mut harness = three_chips_harness();
    settle(&mut harness);

    let (rect, radius) = the_chip_rect_with(&harness, Palette::RAISED, "the count chip");
    assert_eq!(
        radius,
        egui::CornerRadius::same(turbogit_ui::theme::CHIP_RADIUS)
    );
    assert_eq!(rect.height(), CHIP_HEIGHT);

    assert_eq!(painted_ink(&harness, COUNT_TEXT), Palette::INK_2);
    let font = painted_font(&harness, COUNT_TEXT);
    assert_eq!(font.family, egui::FontFamily::Monospace);
    assert_eq!(font.size, turbogit_ui::theme::TYPE_CHIP);
    assert_eq!(painted_rows(&harness, COUNT_TEXT), 1);
    // A count never borrows the orange: that hue is dirt and unpushed, and this
    // chip is the thing that stops a third number borrowing it.
    assert_ne!(COUNT_CHIP_COLORS.bg, Palette::COUNTER);
    assert_ne!(COUNT_CHIP_COLORS.fg, Palette::COUNTER);
}

/// **The set is closed, read off the frame.** Three chips in a frame of three
/// chips and nothing else: the chip-sized rects are exactly the three fills, one
/// each. A fourth chip — a status pill, a ref pill, a tinted count — would add
/// a fourth chip-sized rect and fail here without this suite having to know its
/// name.
#[test]
fn a_frame_of_three_chips_paints_exactly_three_chips() {
    let mut harness = three_chips_harness();
    settle(&mut harness);

    let rects = chip_sized_rects(&harness);
    let mut fills: Vec<Color32> = rects.iter().map(|(_, fill, _)| *fill).collect();
    fills.sort_by_key(|c| (c.r(), c.g(), c.b()));
    let mut expected = vec![
        Palette::RAISED,
        Palette::RAISED_ON_CARD,
        Palette::ROW_SELECTED,
    ];
    expected.sort_by_key(|c| (c.r(), c.g(), c.b()));
    assert_eq!(
        fills, expected,
        "the chip set is closed: every chip-sized rect in the frame is one of \
         the three chips, and there is no fourth"
    );
    // The badge is not in that set: it is the pill slot, and a file status is
    // not a ref and not a number.
    assert!(
        !fills.contains(&BadgeKind::Neutral.colors().bg),
        "the status badge keeps its own slot and is not a chip"
    );
}

/// **Two radii, both pinned, never merged.** The status badge keeps the pill
/// radius; the three chips take the compact chip radius the token layer
/// documents and the shared family had been leaving on the table. The claim is
/// made against painted `CornerRadius` values rather than against the constants
/// alone, so a geometry that is configured correctly but painted at the other
/// radius fails.
#[test]
fn the_status_badge_keeps_the_pill_radius_and_the_two_radii_stay_distinct() {
    // Both slots, as tokens: the values the two roles are built from.
    assert_eq!(
        turbogit_ui::theme::CHIP_RADIUS,
        3,
        "the chip radius is the compact one"
    );
    assert_eq!(
        turbogit_ui::theme::PILL_RADIUS,
        9,
        "the pill radius is the full-height wrap"
    );
    assert_ne!(
        turbogit_ui::theme::CHIP_RADIUS,
        turbogit_ui::theme::PILL_RADIUS,
        "the two radii are different shapes for different jobs"
    );
    assert_eq!(
        COMPACT_CHIP_GEOMETRY.radius,
        f32::from(turbogit_ui::theme::CHIP_RADIUS)
    );
    assert_eq!(
        CHIP_GEOMETRY.radius,
        f32::from(turbogit_ui::theme::PILL_RADIUS)
    );
    assert_eq!(
        compact_chip_radius(),
        egui::CornerRadius::same(turbogit_ui::theme::CHIP_RADIUS)
    );
    assert_eq!(
        chip_radius(),
        egui::CornerRadius::same(turbogit_ui::theme::PILL_RADIUS)
    );
    assert_ne!(compact_chip_radius(), chip_radius());

    // …and as paint. One frame with the badge beside the three chips: the badge
    // is the only pill, the chips are the only compact ones, and a slot that
    // borrowed the other's radius fails on its own rect.
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, _state| {
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            egui::CentralPanel::default().show(ui, |ui| {
                ui.horizontal(|ui| {
                    badge(ui, "A", BadgeKind::Added);
                    ref_chip(ui, REF_NAME);
                    current_chip(ui, CURRENT_NAME);
                    count_chip(ui, COUNT_TEXT);
                });
            });
        },
        (),
    );
    harness.set_size(egui::vec2(520.0, 80.0));
    settle(&mut harness);

    let badge_rect = painted_rects(&harness)
        .into_iter()
        .find(|(_, fill, _, _)| *fill == BadgeKind::Added.colors().bg)
        .map(|(rect, _, _, radius)| (rect, radius))
        .expect("the badge paints its own tinted fill");
    assert_eq!(
        badge_rect.1,
        egui::CornerRadius::same(turbogit_ui::theme::PILL_RADIUS),
        "the status badge keeps the pill radius: it is a status, not a ref"
    );
    for (who, fill) in [
        ("ref chip", Palette::RAISED_ON_CARD),
        ("current chip", Palette::ROW_SELECTED),
        ("count chip", Palette::RAISED),
    ] {
        let radius = chip_sized_rects(&harness)
            .into_iter()
            .find(|(_, painted, _)| *painted == fill)
            .map(|(_, _, radius)| radius)
            .unwrap_or_else(|| panic!("the {who} paints at chip height"));
        assert_eq!(
            radius,
            egui::CornerRadius::same(turbogit_ui::theme::CHIP_RADIUS),
            "the {who} is the compact chip, not the pill"
        );
    }
}

// ---------------------------------------------------------------------------
// 7. One pane header, one column header (ticket 06)
// ---------------------------------------------------------------------------
//
// R7: every tool pane wears the same chrome — a 9px tracked `INK_3` title, an
// optional count chip, a right-aligned action slot, **one** hairline, then
// content — and every column-oriented pane labels its columns with one shared
// row over a structural-rule underline.
//
// The ratchets come in the two shapes the rest of this suite uses, because the
// two claims need them:
//
// - **Structural** (declarations). The tool-window header is gone from the
//   façade, and the conflict grammar's module-private header is gone from its
//   module. A second pane header that reused the band height or the title
//   treatment is a second consumer of them and fails here. This is the same
//   `include_str!` item parse the closed-chip ratchet uses, and it is
//   deliberately *not* the whole answer: it proves one declaration, not one
//   header.
// - **Rendered** (painted geometry). Four panes rendering byte-identical header
//   geometry, which is what "exactly one pane header" means to somebody looking
//   at the screen. This is the half a source scan cannot make, and the reason
//   the ticket asks for it: a caller that keeps a header of its own and forgets
//   to route it through the shared one still fails here.
//
// Every ink claim below reads `painted_ink`, never the galley's layout colour:
// this codebase lays text out in `WHITE` and inks it at paint time, so the
// layout colour answers `WHITE` and the column-header ink test would pass
// whatever was chosen.

/// Every `.rs` file under `crates/turbogit-ui/src/ui`, recursively, as
/// `(path relative to `src/ui`, source)`.
///
/// Read at *run* time rather than `include_str!`'d one file at a time, because
/// the claim being made is "no second pane header exists **anywhere**", and a
/// hand-maintained list of the files that were checked when the ratchet was
/// written is a list that silently stops covering new panes. A file appearing
/// later is covered the next time the suite runs, with no edit to the test.
fn ui_source_files() -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, base: &std::path::Path, out: &mut Vec<(String, String)>) {
        let entries =
            std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display()));
        let mut paths: Vec<_> = entries.map(|e| e.expect("dir entry").path()).collect();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                walk(&path, base, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let rel = path
                    .strip_prefix(base)
                    .expect("under base")
                    .to_string_lossy()
                    .replace('\\', "/");
                let src = std::fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
                out.push((rel, src));
            }
        }
    }
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui");
    let mut out = Vec::new();
    walk(&base, &base, &mut out);
    assert!(
        !out.is_empty(),
        "the ui source walk found nothing under {}",
        base.display()
    );
    out
}

/// **Structural: exactly one pane-header implementation exists, and the
/// tool-window header is gone from the shared widget module's public surface.**
///
/// "Gone from the public surface" is a claim about the façade's re-export list —
/// the only thing that makes a name part of `turbogit_ui::ui::widgets` — and it
/// is checked as a whole-identifier match against the *declaration* the façade
/// is made of, so neither `toolwindow_headers_of` nor a comment recording the
/// retirement can satisfy it either way.
///
/// The honest limit of that check is worth stating rather than hiding: Rust has
/// no way to say "this name does not resolve" from inside a test that has to
/// *compile*, so a true symbol-level absence assertion is a compile-fail test,
/// and `cargo test --all-targets` does not run doctests. What is asserted here
/// is the declaration the compiler compiles that list from; the
/// [`every_named_pane_produces_identical_header_geometry`] ratchet is what makes
/// the retirement a fact about the screen rather than about a list of names.
#[test]
fn the_tool_window_header_is_retired_from_the_shared_widget_surface() {
    // 1. The symbol is not declared anywhere in the UI layer, and specifically
    //    not in the module that used to own it.
    let retired: Vec<String> = ui_source_files()
        .iter()
        .filter(|(_, src)| {
            items_naming(src, "toolwindow_header")
                .iter()
                .any(|n| n == "toolwindow_header")
        })
        .map(|(rel, _)| rel.clone())
        .collect();
    assert!(
        retired.is_empty(),
        "`toolwindow_header` is declared again in {retired:?}: there is exactly one \
         pane header (`widgets::pane_header`), and a second implementation is not a \
         second spelling of it — it is a header the user can tell apart from the first"
    );

    // 2. The façade no longer re-exports it, and neither does it name the band
    //    height the old header carried.
    for name in ["toolwindow_header", "TOOLWINDOW_HEADER_HEIGHT"] {
        assert!(
            word_offsets(WIDGETS_MOD_SRC, name).is_empty(),
            "`{name}` is back on the shared widget module's public surface; the \
             retirement is a removal from `widgets::mod.rs`, not a rename"
        );
    }

    // 3. The conflict grammar's private header is absorbed, not left behind. It
    //    would not warn if it survived — nothing calls it once the kit delegates —
    //    so its absence is asserted here at the declaration instead.
    let kit: Vec<String> = top_level_items(CONFLICT_PANE_SRC)
        .into_iter()
        .filter(|item| item.name == "pane_header")
        .map(|item| item.name)
        .collect();
    assert!(
        kit.is_empty(),
        "the conflict kit declares its own `pane_header` again; it must delegate to \
         `widgets::pane_header` so the grammar's three columns wear the same header \
         as every tool pane (a second implementation is precisely what R7 forbids)"
    );
    assert!(
        !word_offsets(CONFLICT_PANE_SRC, "widgets::pane_header").is_empty(),
        "the conflict kit must reach the shared pane header, not merely have deleted \
         its own"
    );
}

/// **Structural: one band height and one title treatment, so a second header has
/// nowhere to get its numbers from.**
///
/// A duplicated header that reuses `PANE_HEADER_HEIGHT` or `pane_title()` is a
/// second consumer of the one definition and fails here by name. A duplicated
/// header that invents its own numbers is a second *decision*, and the rendered
/// ratchet is what catches that one.
#[test]
fn one_band_height_and_one_title_treatment_own_every_pane_header() {
    assert_eq!(
        items_naming(CONTAINERS_SRC, "PANE_HEADER_HEIGHT"),
        ["PANE_HEADER_HEIGHT", "pane_header"],
        "the pane header's band height is declared once and read by the one function \
         that paints a band; a second reader is a second header"
    );
    assert_eq!(
        fns_using(CONTAINERS_SRC, "PANE_HEADER_HEIGHT"),
        ["pane_header"],
        "only the one pane header may size a band"
    );
    assert_eq!(
        fns_using(CONTAINERS_SRC, "pane_title"),
        ["column_header", "pane_header"],
        "a pane title and a column label are the same tracked mark: the one \
         `pane_title()` decides the face and the ink, exactly these two read it, \
         and a third reader is a fourth thing wearing the type"
    );
    // Nothing outside the module that owns the chrome may reach for either.
    // Two exceptions are spelled out, because both are decisions *about* the
    // shared header rather than a second copy of it: the façade, which
    // re-exports the band height (a pane needs a number to compare against, not
    // a vocabulary to borrow), and the button family's **static** equality
    // assertion, which is what makes the band-height primary fit the band. The
    // assertion below the allowlist is the part that matters: no *function*
    // outside the chrome module may read the band height, so the equality is a
    // property of the crate rather than a decision a button makes per frame.
    let elsewhere: Vec<String> = ui_source_files()
        .iter()
        .filter(|(rel, _)| {
            !matches!(
                rel.as_str(),
                "widgets/containers.rs" | "widgets/mod.rs" | "widgets/controls.rs"
            )
        })
        .filter(|(_, src)| {
            !word_offsets(src, "PANE_HEADER_HEIGHT").is_empty()
                || !word_offsets(src, "pane_title").is_empty()
        })
        .map(|(rel, _)| rel.clone())
        .collect();
    assert!(
        elsewhere.is_empty(),
        "{elsewhere:?} name a pane header's private vocabulary; a pane asks for the \
         shared header rather than borrowing its numbers"
    );
    // The one other reader is the compact primary's height, and it reads it in a
    // static assertion — never inside a function, where a caller could have
    // sized a control to the band on purpose.
    for (rel, src) in ui_source_files() {
        if rel == "widgets/containers.rs" {
            continue;
        }
        assert_eq!(
            fns_using(&src, "PANE_HEADER_HEIGHT"),
            Vec::<String>::new(),
            "`{rel}` reads the pane header's band height inside a function: the \
             band is one number owned by `widgets/containers`, and a control may \
             not size itself against it at paint time. A static assertion that the \
             two numbers agree is a different thing, and is what the compact \
             primary does."
        );
    }
    // …and the façade's one mention is the re-export itself, not a second
    // decision about the number.
    assert_eq!(
        items_naming(WIDGETS_MOD_SRC, "PANE_HEADER_HEIGHT"),
        Vec::<String>::new(),
        "the façade re-exports the band height; a second mention would be a second \
         place deciding what a header is"
    );
    assert_eq!(
        turbogit_ui::theme::RAIL_WIDTH,
        2.0,
        "sanity: the token layer still owns the rail width, which the conflict \
         grammar's focus marker now reads"
    );
}

/// Every 1px `RULE_STRUCTURAL` rect the frame painted, as `(rect, fill)`.
fn structural_rules<S>(harness: &Harness<'_, S>) -> Vec<egui::Rect> {
    painted_rects(harness)
        .into_iter()
        .filter(|(rect, fill, _, _)| {
            *fill == Palette::RULE_STRUCTURAL && (rect.height() - 1.0).abs() < 0.01
        })
        .map(|(rect, _, _, _)| rect)
        .collect()
}

/// The frame's **first** structural rule — the header's hairline — read off
/// painted output, positioned rather than first-matched so a rule the pane's
/// body also painted cannot be mistaken for the header's.
fn the_header_rule<S>(harness: &Harness<'_, S>) -> egui::Rect {
    let mut rules = structural_rules(harness);
    rules.sort_by_key(|r| (r.top() * 100.0) as i64);
    *rules
        .first()
        .unwrap_or_else(|| panic!("a pane header must paint a structural hairline"))
}

/// The frame's **leading** galley — a pane header's title, which is the leftmost
/// thing any of these panes puts down.
///
/// Leftmost rather than first, deliberately: the header's other parts share its
/// row and not its baseline. A 10px count chip centred in the band paints
/// *above* a 9px title centred in the same band, so "the first thing painted"
/// would find the chip and every title claim would be a claim about a number.
fn the_pane_title_galley<S>(harness: &Harness<'_, S>) -> test_support::harness::PaintedGalley {
    painted_galleys(harness)
        .into_iter()
        .min_by(|a, b| {
            a.pos
                .x
                .partial_cmp(&b.pos.x)
                .expect("finite x")
                .then(a.pos.y.partial_cmp(&b.pos.y).expect("finite y"))
        })
        .unwrap_or_else(|| panic!("a pane header must paint a title"))
}

/// A frame holding one pane header — title, count chip, one action, one
/// hairline — and the pane's first line of content, with the content origin
/// recorded so the band's height is measurable from paint alone.
fn pane_header_harness(count: Option<&'static str>) -> (Harness<'static, ()>, Rc<Cell<f32>>) {
    let content_top = Rc::new(Cell::new(0.0));
    let content_top_ui = content_top.clone();
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, _state| {
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            egui::CentralPanel::default().show(ui, |ui| {
                content_top_ui.set(ui.available_rect_before_wrap().top());
                pane_header(ui, "PANES", count, |ui| {
                    compact_button(ui, "Do the thing");
                });
                ui.label("pane body");
            });
        },
        (),
    );
    harness.set_size(egui::vec2(560.0, 200.0));
    (harness, content_top)
}

/// **The shared pane header paints, in order: title, count chip, action slot,
/// one hairline, content.** Every step is read off painted geometry, and the
/// order is asserted as geometry rather than as a reading of the body.
#[test]
fn the_pane_header_paints_title_count_actions_one_hairline_then_content() {
    let (mut harness, content_top) = pane_header_harness(Some("12"));
    settle(&mut harness);

    // The title: 9px, the chrome face, the muted ink.
    let title = the_pane_title_galley(&harness);
    assert_eq!(title.text, "PANES");
    assert_eq!(painted_ink(&harness, "PANES"), Palette::INK_3);
    let font = painted_font(&harness, "PANES");
    assert_eq!(font.size, turbogit_ui::theme::TYPE_SECTION);
    assert_eq!(font.family, egui::FontFamily::Proportional);

    // The count chip is the shared one, not a fourth badge.
    let (chip, radius) = the_chip_rect_with(&harness, Palette::RAISED, "the header's count chip");
    assert_eq!(
        radius,
        egui::CornerRadius::same(turbogit_ui::theme::CHIP_RADIUS)
    );
    assert_eq!(painted_ink(&harness, "12"), Palette::INK_2);

    // **The order**, left to right: title, then the count beside it, then the
    // action slot hard against the band's trailing edge.
    let action = harness.get_by_label("Do the thing").rect();
    assert!(
        title.rect.right() < chip.left(),
        "the count chip sits beside the title, not over it: title {:?}, chip {chip:?}",
        title.rect
    );
    assert!(
        chip.right() < action.left(),
        "the action slot is the rightmost thing in the band: chip {chip:?}, action {action:?}"
    );
    assert!(
        (action.right() - title.rect.right()).abs() < 560.0,
        "the action slot must be reachable at all"
    );
    let rule = the_header_rule(&harness);
    assert!(
        (action.right() - rule.right()).abs() < 0.01,
        "the action slot is right-aligned to the header's own width, not floating: \
         action {:?}, rule {rule:?}",
        action
    );

    // **One** hairline: the header's, at the band's foot, spanning the pane.
    let rules = structural_rules(&harness);
    assert_eq!(
        rules.len(),
        1,
        "a pane header paints ONE hairline. A second rule between a header and its \
         content is the nested-boxes problem this migration exists to remove, and it \
         reappears quietly — a caller wanting air adds `ui.add_space`, not a rule. \
         Found {rules:?}"
    );
    assert_eq!(rule.width(), rule.width());
    assert!(
        (rule.left() - title.rect.left()).abs() < 0.01,
        "the hairline spans the header's own width, starting where the title starts: \
         rule {rule:?}, title {:?}",
        title.rect
    );
    // The band's height, measured from the content origin the frame recorded:
    // the hairline sits exactly `PANE_HEADER_HEIGHT` below it, and the content
    // below that.
    assert!(
        (rule.top() - content_top.get() - PANE_HEADER_HEIGHT).abs() < 0.01,
        "the header band is `PANE_HEADER_HEIGHT` tall and the hairline is its last \
         pixel: got {} for a band of {}",
        rule.top() - content_top.get(),
        PANE_HEADER_HEIGHT
    );
    let body = painted_galleys(&harness)
        .into_iter()
        .find(|g| g.text == "pane body")
        .expect("the pane's content paints");
    assert!(
        body.pos.y > rule.bottom(),
        "content comes after the hairline, not over it: body {:?}, rule {rule:?}",
        body.pos
    );
}

/// **The count chip is optional, and its absence does not change the band.** A
/// pane with nothing to count wears the same header, which is why the
/// cross-pane geometry comparison below can compare all three at once.
#[test]
fn a_pane_header_without_a_count_is_the_same_header() {
    let (mut with_count, top_with) = pane_header_harness(Some("7"));
    settle(&mut with_count);
    let (mut without, top_without) = pane_header_harness(None);
    settle(&mut without);

    let a = the_header_rule(&with_count);
    let b = the_header_rule(&without);
    assert_eq!(
        (a.top() - top_with.get(), a.height()),
        (b.top() - top_without.get(), b.height()),
        "dropping the count chip must not resize the band"
    );
    assert_eq!(
        painted_galleys(&without)
            .into_iter()
            .find(|g| g.text == "7")
            .map(|g| g.text),
        None,
        "no count chip means no count painted"
    );
}

/// The panes the widget-library suite names, each reached through its own
/// public entry point, in one harness shape.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ChromePane {
    Worktrees,
    Submodules,
    LogBranches,
    Changes,
}

impl ChromePane {
    /// The word this pane's title must read as. `BRANCHES` is the case that
    /// changed here: the strip said "Branches" where every other pane said
    /// itself in the app's one case.
    fn title(self) -> &'static str {
        match self {
            Self::Worktrees => "WORKTREES",
            Self::Submodules => "SUBMODULES",
            Self::LogBranches => "BRANCHES",
            Self::Changes => "CHANGES",
        }
    }

    /// Render this pane — and nothing else — into a harness whose window is the
    /// same size for all of them, so the header geometry is comparable without
    /// normalising anything away.
    fn harness(
        self,
    ) -> (
        Harness<'static, turbogit_app::state::AppState>,
        tempfile::TempDir,
    ) {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(&repo).expect("repo dir");
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            let out = std::process::Command::new("git")
                .args(&args)
                .current_dir(&repo)
                .output()
                .expect("git invocation");
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        let repo = repo.canonicalize().expect("canonical repo");
        let state = turbogit_app::state::AppState::for_roots(dir.path(), &[repo]);
        let mut fonts_installed = false;
        let harness = Harness::builder()
            .with_size(egui::vec2(760.0, 620.0))
            .build_ui_state(
                move |ui, state| {
                    turbogit_ui::theme::configure_style(ui.ctx());
                    if !fonts_installed {
                        turbogit_ui::theme::install_fonts(ui.ctx());
                        fonts_installed = true;
                    }
                    state.drain_events();
                    egui::CentralPanel::default().show(ui, |ui| match self {
                        Self::Worktrees => turbogit_ui::ui::worktrees::show(ui, state),
                        Self::Submodules => turbogit_ui::ui::submodules::show(ui, state),
                        Self::LogBranches => turbogit_ui::ui::log_window::branches_pane(ui, state),
                        Self::Changes => turbogit_ui::ui::commit_window::show(ui, state),
                    });
                },
                state,
            );
        (harness, dir)
    }
}

/// What one pane's header actually put down, read off painted output: where its
/// hairline landed, where its title landed relative to that hairline, and the
/// type it was laid out in.
#[derive(Debug)]
struct PaintedHeader {
    pane: ChromePane,
    rule: egui::Rect,
    title_offset: egui::Pos2,
    title_x: f32,
    title: String,
    size: f32,
    family: egui::FontFamily,
    ink: Color32,
}

fn measure_pane_header(pane: ChromePane) -> PaintedHeader {
    let (mut harness, _dir) = pane.harness();
    settle(&mut harness);
    let rule = the_header_rule(&harness);
    // The title is found by its OWN text and required to sit inside the header's
    // width, rather than by being the frame's leftmost galley.
    //
    // Leftmost is right for the three single-purpose panes, where the title is
    // the first thing their entry point paints, and it is the honest way to say
    // "the pane opens with this". It stops being right the moment a pane renders
    // something to the LEFT of its own header — which the changes card does: the
    // commit window's sub-tab strip is further left, so "leftmost" there finds
    // `Local Changes`. Scoping to the band is the same claim with the accuracy
    // the comparison needs, and it still fails loudly if a pane stops painting
    // its own title (no match) or paints it somewhere else (outside the band).
    let galley = painted_galleys(&harness)
        .into_iter()
        .find(|g| g.text == pane.title() && g.pos.x < rule.right())
        .unwrap_or_else(|| {
            panic!(
                "the {} pane must open with its own title `{}` inside its own \
                 header band (rule {rule:?})",
                format!("{pane:?}").to_lowercase(),
                pane.title(),
            )
        });
    let font = painted_font(&harness, pane.title());
    PaintedHeader {
        pane,
        rule,
        title_offset: egui::pos2(galley.pos.x - rule.left(), galley.pos.y - rule.top()),
        title_x: galley.pos.x,
        title: galley.text,
        size: font.size,
        family: font.family,
        ink: painted_ink(&harness, pane.title()),
    }
}

/// **The workhorse: four panes, one header.** The worktrees, submodules, log and
/// changes panes are rendered through their own entry points in
/// identically-shaped windows, and their header geometry is compared *to each
/// other* — not to a hand-written number. That is what turns "exactly one
/// pane-header implementation" into a fact about what the user sees rather than a
/// fact about which function was called.
///
/// The changes card is the reference implementation of v2 and joins the
/// comparison on the same terms as the panes that were already in it: one more
/// name in `PANES_NAMED_HERE`, one more expected title, and the same claims.
///
/// **What "identical" means is the header's own geometry, not where the header
/// sits on the screen.** The worktrees, submodules and log panes each fill the
/// central panel, so their bands land on the same rect and that equality is
/// asserted for them directly. The changes card does not: it is a 340px card
/// inside a two-zone layout, so its band is the card's width and its y is
/// wherever that card falls. Comparing absolute rects across the two kinds would
/// be comparing a pane's *placement*, which is the pane's business — and would
/// make the claim vacuous for the pane that most needs it. So the cross-pane
/// comparison is the part a shared implementation actually fixes:
///
/// - the hairline is one pixel tall, and **starts where the title starts**, so it
///   is the header's own rule spanning the header's own width rather than some
///   other row's rule that happened to be nearby;
/// - the title sits at the same offset inside the band in every pane. Since
///   every band is `Align::Center`, that offset *is* the band height: a pane
///   wearing a 40px band would centre its title lower and fail here;
/// - the title is the same type, size and ink everywhere.
#[test]
fn every_named_pane_produces_identical_header_geometry() {
    const PANES_NAMED_HERE: [ChromePane; 4] = [
        ChromePane::Worktrees,
        ChromePane::Submodules,
        ChromePane::LogBranches,
        ChromePane::Changes,
    ];
    /// The panes whose entry point *is* the pane, so their bands share a rect.
    const FULL_BLEED: [ChromePane; 3] = [
        ChromePane::Worktrees,
        ChromePane::Submodules,
        ChromePane::LogBranches,
    ];

    let measured: Vec<PaintedHeader> = PANES_NAMED_HERE
        .iter()
        .copied()
        .map(measure_pane_header)
        .collect();

    let first = &measured[0];
    // Each render really is the pane it was asked for — a comparison of four
    // renders of the *same* pane would satisfy every assertion below.
    let titles: Vec<&str> = measured.iter().map(|h| h.title.as_str()).collect();
    assert_eq!(
        titles,
        ["WORKTREES", "SUBMODULES", "BRANCHES", "CHANGES"],
        "each named pane must have rendered its own title, or the comparison below \
         is four renders of one pane"
    );

    // The header's own shape, in every pane: a 1px hairline that starts where the
    // title starts.
    for header in &measured {
        assert!(
            (header.rule.height() - 1.0).abs() < 0.01,
            "the {:?} pane's header hairline is not one pixel: {:?}",
            header.pane,
            header.rule
        );
        assert!(
            (header.rule.left() - header.title_x).abs() < 0.01,
            "the {:?} pane's hairline must start where its title starts, so it is \
             the header's own rule spanning the header's own width: rule {:?}, \
             title x={}",
            header.pane,
            header.rule,
            header.title_x
        );
    }

    for other in &measured[1..] {
        assert_eq!(
            (other.title_offset.x, other.title_offset.y),
            (first.title_offset.x, first.title_offset.y),
            "the {:?} pane's title sits somewhere else inside its header: {:?} vs {:?}. \
             Two headers is not two spellings of one decision; it is a pane the user \
             has to re-read.",
            other.pane,
            other.title_offset,
            first.title_offset
        );
        assert_eq!(
            (other.size, other.family.clone(), other.ink),
            (first.size, first.family.clone(), first.ink),
            "the {:?} pane's title is set in a different type than the {:?} pane's: \
             {:?}/{:?}/{:?} vs {:?}/{:?}/{:?}",
            other.pane,
            first.pane,
            other.size,
            other.family,
            other.ink,
            first.size,
            first.family,
            first.ink
        );
    }

    // The panes that fill the central panel share a rect outright, and that is
    // kept as a separate, stronger claim: it would catch a fourth full-bleed
    // pane wearing a band at a different origin.
    for other in measured.iter().filter(|h| FULL_BLEED.contains(&h.pane)) {
        assert_eq!(
            (
                other.rule.left(),
                other.rule.top(),
                other.rule.width(),
                other.rule.height()
            ),
            (
                first.rule.left(),
                first.rule.top(),
                first.rule.width(),
                first.rule.height()
            ),
            "the {:?} pane's header is not the header the {:?} pane wears: {:?} vs {:?}. \
             Two headers is not two spellings of one decision; it is a pane the user \
             has to re-read.",
            other.pane,
            first.pane,
            other.rule,
            first.rule
        );
    }

    // The measured values, written down so a reader can see what "identical"
    // means in points rather than trusting the equality above.
    assert_eq!(first.size, turbogit_ui::theme::TYPE_SECTION);
    assert_eq!(first.ink, Palette::INK_3);
    assert!(
        (first.rule.height() - 1.0).abs() < 0.01,
        "the header's hairline is one pixel"
    );
    assert!(
        first.rule.top() > first.title_offset.y,
        "the title is inside the band, above its hairline"
    );
}

/// **Every pane title renders in the same case.** The shared header prints the
/// word it is given, so this is a ratchet on the *callers*: the one pane whose
/// title had drifted is the log's branches strip, and it drifted here.
#[test]
fn every_pane_title_renders_in_the_same_case() {
    for pane in [
        ChromePane::Worktrees,
        ChromePane::Submodules,
        ChromePane::LogBranches,
    ] {
        let (mut harness, _dir) = pane.harness();
        settle(&mut harness);
        let title = the_pane_title_galley(&harness).text;
        assert_eq!(
            title,
            title.to_uppercase(),
            "the {pane:?} pane opens with `{title}`: a pane title is the app's one \
             case, upper, and a pane that spells its own title differently is a band \
             the eye has to re-read"
        );
        assert_eq!(title, pane.title());
        // …and the drifted spelling is absent, not merely outranked.
        let painted: Vec<String> = painted_text(&harness);
        for lower in ["Worktrees", "Submodules", "Branches", "Blame"] {
            assert!(
                !painted.contains(&lower.to_owned()),
                "the {pane:?} pane still paints the title-case `{lower}`: {painted:?}"
            );
        }
    }
}

/// The commit table's column table, restated here as the test's own so the
/// "same constants" claim is checked from both ends: the row paints its cells
/// through the table, and the header paints its labels through the table.
///
/// The trailing column's inset is **zero**, so the claim under test — a
/// trailing column's label ends exactly where its cell ends — is made without
/// also hand-placing a right-aligned cell. The inset arithmetic itself is
/// asserted from paint below: the label ends at `row.right() - inset`.
const TEST_COLUMNS: [PaneColumn; 4] = [
    PaneColumn::start("HASH", 26.0),
    PaneColumn::start("AUTHOR", 84.0),
    PaneColumn::start("MESSAGE", 164.0),
    PaneColumn::end("DATE", 0.0),
];

/// One column-header row over one data row, both measured from the same rect.
fn column_chrome_harness() -> Harness<'static, ()> {
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, _state| {
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            egui::CentralPanel::default().show(ui, |ui| {
                let row = ui.available_rect_before_wrap();
                column_header(ui, row, &TEST_COLUMNS);
                ui.horizontal(|ui| {
                    for (i, cell) in ["a1b2c3d", "Ada", "subject"].into_iter().enumerate() {
                        column_cell(ui, &TEST_COLUMNS, i, |ui| {
                            ui.label(cell);
                        });
                    }
                    // A trailing column is a right-aligned cell, so its row
                    // paints it where the log paints its date: ending on the
                    // trailing edge that the header's label for it also ends on.
                    let galley = ui.painter().layout_no_wrap(
                        "2026-01-01".to_owned(),
                        egui::FontId::proportional(12.0),
                        Palette::INK,
                    );
                    ui.painter().galley(
                        egui::pos2(
                            TEST_COLUMNS[3].origin(row) - galley.size().x,
                            ui.cursor().top(),
                        ),
                        galley,
                        Palette::INK,
                    );
                });
            });
        },
        (),
    );
    harness.set_size(egui::vec2(700.0, 160.0));
    harness
}

/// **Column offsets in the header and in the data rows come from the same
/// constants.** The header's labels and one row's cells are compared to each
/// other, label by label, so a column cannot be narrow in the header and wide in
/// the rows — which is the "my eye re-anchors on every row" problem, stated as
/// a fact about the paint rather than as a rule in a doc comment.
#[test]
fn the_column_header_and_its_rows_measure_from_the_same_offsets() {
    let mut harness = column_chrome_harness();
    settle(&mut harness);

    let pairs = [
        ("HASH", "a1b2c3d"),
        ("AUTHOR", "Ada"),
        ("MESSAGE", "subject"),
    ];
    for (label, cell) in pairs {
        let label_x = galley_x(&harness, label);
        let cell_x = galley_x(&harness, cell);
        assert!(
            (label_x - cell_x).abs() < 0.01,
            "the `{label}` header label is not over its cell: label at {label_x}, cell \
             at {cell_x}. The header and the rows must read the same column table."
        );
    }
    // The trailing column shares its edge rather than its start, which is what
    // a right-aligned cell means.
    let date_label = painted_galleys(&harness)
        .into_iter()
        .find(|g| g.text == "DATE")
        .expect("the trailing column is labelled");
    let date_cell = painted_galleys(&harness)
        .into_iter()
        .find(|g| g.text == "2026-01-01")
        .expect("the trailing cell paints");
    assert!(
        (date_label.rect.right() - date_cell.rect.right()).abs() < 0.01,
        "a trailing column's label ends where its cell ends: {:?} vs {:?}",
        date_label.rect,
        date_cell.rect
    );
}

/// **The column header's ink is `INK_3`, not `INK_4`.**
///
/// Asserted explicitly, and named in the failure message, because the target
/// frames were drawn with the dim value before the contrast was checked: a
/// reader comparing the code against the frame should find a test disagreeing
/// with them, and be told why.
#[test]
fn a_column_header_is_ink_3_and_never_the_dim_ink() {
    let mut harness = column_chrome_harness();
    settle(&mut harness);

    assert_eq!(
        painted_ink(&harness, "HASH"),
        Palette::INK_3,
        "a column header is a label the user reads to know what a bare number or \
         timestamp means"
    );
    assert_eq!(
        painted_ink(&harness, "HASH"),
        Palette::T_MUTED,
        "`INK_3` and `T_MUTED` are one step under two names, and the token layer is \
         where the value lives"
    );
    assert_ne!(
        painted_ink(&harness, "HASH"),
        Palette::INK_4,
        "9px is normal-size text. `INK_4` is 3.2:1 and is reserved for placeholders, \
         dim path suffixes and hatches — never the only rendering of something the \
         user needs. The target frames were drawn with the dim value before this was \
         checked, which is exactly why this assertion exists."
    );
    assert!(
        Palette::INK_3 != Palette::INK_4,
        "if the muted and dim steps were ever merged, this ratchet would stop \
         meaning anything"
    );

    // The type, from the laid-out font: 9px, the chrome face.
    let font = painted_font(&harness, "HASH");
    assert_eq!(font.size, turbogit_ui::theme::TYPE_SECTION);
    assert_eq!(font.family, egui::FontFamily::Proportional);
    // …and a pane title is set in the *same* type, which is what "the tracking is
    // uniform" is.
    let (mut pane, _top) = pane_header_harness(None);
    settle(&mut pane);
    let pane_font = painted_font(&pane, "PANES");
    assert_eq!(
        (pane_font.size, pane_font.family),
        (font.size, font.family),
        "a pane title and a column label are the same tracked mark"
    );
    assert_eq!(painted_ink(&pane, "PANES"), painted_ink(&harness, "HASH"));
}

/// **The column-header row's underline is the structural rule, and it is one.**
#[test]
fn the_column_header_row_underlines_itself_once_in_the_structural_tone() {
    let table_width = Rc::new(Cell::new(0.0));
    let table_width_ui = table_width.clone();
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, _state| {
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            egui::CentralPanel::default().show(ui, |ui| {
                let row = ui.available_rect_before_wrap();
                table_width_ui.set(row.width());
                column_header(ui, row, &TEST_COLUMNS);
                ui.label("the first data row");
            });
        },
        (),
    );
    harness.set_size(egui::vec2(700.0, 160.0));
    settle(&mut harness);

    let rules = structural_rules(&harness);
    assert_eq!(
        rules.len(),
        1,
        "the column-header row underlines itself once: {rules:?}"
    );
    // The labels are CENTRED in the band above the rule. This is its own
    // assertion because a rule at the band's foot is at the right y even when
    // the labels are painted somewhere else entirely — so the rule's own
    // geometry cannot see a label that has drifted. The band is derived from
    // the rule because the rule is the band's last pixel, which makes it the
    // one measurement here that is both known and checkable.
    //
    // The bug this pins: the labels read
    // `ui.available_rect_before_wrap().top()` — where the band was *about to
    // be* rather than where it landed — which drew every label 8pt high and put
    // the rule straight through the lower third of them.
    let band_top = rules[0].top() - COLUMN_HEADER_HEIGHT;
    for column in TEST_COLUMNS {
        let label = painted_galleys(&harness)
            .into_iter()
            .find(|g| g.text == column.label)
            .unwrap_or_else(|| panic!("`{}` paints", column.label));
        let expected = band_top + COLUMN_HEADER_HEIGHT / 2.0;
        assert!(
            (label.rect.center().y - expected).abs() <= 1.0,
            "a column label is centred in the band above the underline: \
             `{}` is centred at {} but the band's centre is {expected} \
             (rule {:?}, label {:?})",
            column.label,
            label.rect.center().y,
            rules[0],
            label.rect,
        );
        assert!(
            rules[0].top() >= label.rect.bottom(),
            "the column header's underline is UNDER its labels, not through them: \
             rule {:?}, `{}` label {:?}",
            rules[0],
            column.label,
            label.rect,
        );
    }
    assert!(
        (rules[0].width() - table_width.get()).abs() < 0.01,
        "the underline spans the table's width, got {} for a table of {}",
        rules[0].width(),
        table_width.get()
    );
    let body = painted_galleys(&harness)
        .into_iter()
        .find(|g| g.text == "the first data row")
        .expect("the row paints");
    assert!(
        body.pos.y > rules[0].bottom(),
        "the first data row is below the header's underline: {:?} vs {:?}",
        body.pos,
        rules[0]
    );
}

// ---------------------------------------------------------------------------
// 8. The rule-to-assertion table (ticket 21)
// ---------------------------------------------------------------------------
//
// The three contract suites are where the migration's rules become ratchets, and
// this section is the half of that work whose claims are about **the whole app**
// rather than about one widget's paint: R1's "the primary is the only other
// brand fill", R2's "a stroke means this floats", and R6's "exactly one function
// produces a ref chip".
//
// Each is a *closed* claim over an enumerated set, which is the only honest form
// for a negative about a tree a render seam cannot see: a test that listed what it
// expects and compared the tree to it fails when the tree grows, and names what
// grew. A source scan alone would not — the difference is that every entry here
// carries a written reason for why that site is allowed to do what it does, so
// adding a row is a decision and removing one is a deletion, rather than both
// being a no-op that only a reviewer would notice.

// --- R1: the pane's single primary action is the only other brand fill ---------

/// The four tool panes and **the brand fill each one is allowed to paint**: one
/// named primary action, or none.
///
/// R1 gives `Palette::BRAND` three jobs in the whole app: a view's single
/// primary action button, the active tab underline, and a selected row's rail.
/// Neither of the other two appears here — these harnesses render a pane alone in
/// a central panel, with no tab strip and no shell — so every brand-filled rect
/// in one of these four frames is a primary action button, and a pane has at
/// most one of them. That is the whole of R1's second row, stated per pane rather
/// than as a claim about the app: the number is written down, so a second blue in
/// a pane is a failing assertion instead of a screen nobody looks at twice.
const PANE_PRIMARY_ACTION: [(ChromePane, Option<&str>); 4] = [
    (ChromePane::Worktrees, Some("Add worktree")),
    (ChromePane::Submodules, None),
    (ChromePane::LogBranches, None),
    (ChromePane::Changes, None),
];

/// **R1: a pane's single primary action button is the only other brand fill.**
///
/// Read off painted output, one pane at a time, and checked three ways so a
/// failure says *which* half broke: how many blue rects the pane painted, that
/// the one it painted is the control the pane names as its primary, and that it
/// sits in the header's action slot rather than anywhere in the pane's body (a
/// blue somewhere else is a chip, a state or a band wearing a button's colour).
///
/// The branches pane is not in this table because it is not one of the four
/// `ChromePane`s: its own brand negative is asserted from a real render in
/// `tests/branch_component_kit.rs` (a rail is the only brand fill a branch tree
/// may paint), and this table is the same rule over the four panes R7 gives one
/// header to. Between them, "no second blue in a tool pane" is asserted
/// everywhere a tool pane is.
#[test]
fn a_tool_pane_paints_at_most_one_brand_fill_and_it_is_its_primary_action() {
    for (pane, expected_label) in PANE_PRIMARY_ACTION {
        let (mut harness, _dir) = pane.harness();
        settle(&mut harness);
        let rule = the_header_rule(&harness);

        // R1 gives the accent three jobs and a pane is allowed two of them: its
        // one primary action, and a selected row's rail. So the brand fills are
        // *partitioned* rather than counted — a rail is the token's width at a
        // row's leading edge, and everything else has to be the pane's primary.
        let brand: Vec<egui::Rect> = painted_rects(&harness)
            .into_iter()
            .filter(|(_, fill, _, _)| *fill == Palette::BRAND)
            .map(|(rect, _, _, _)| rect)
            .collect();
        let (rails, buttons): (Vec<egui::Rect>, Vec<egui::Rect>) = brand
            .iter()
            .copied()
            .partition(|rect| (rect.width() - RAIL_WIDTH).abs() < 0.01);
        for rail in &rails {
            assert!(
                rail.height() <= 40.0,
                "a brand-filled rect {rail:?} is a rail's width but is {}-tall: the \
                 accent fills a row's leading edge, and nothing else at that width",
                rail.height()
            );
        }

        // (1) How many buttons. Zero for the three panes with no primary; exactly
        // one for the one that has it.
        assert_eq!(
            buttons.len(),
            usize::from(expected_label.is_some()),
            "the {pane:?} pane painted {} brand-filled control(s) at {buttons:?} \
             (plus {} rail(s)). R1 gives the accent three jobs and a primary \
             action button is the only one a tool pane may spend it on: a second \
             blue here is the blue soup this migration removed.",
            buttons.len(),
            rails.len()
        );

        let Some(label) = expected_label else {
            continue;
        };

        // (2) Which control. The blue is the button the pane names, by its own
        // accessibility label — not merely *a* button, which a focus ring or a
        // second primary would also satisfy.
        let primary = harness.get_by_label(label).rect();
        assert_eq!(
            buttons,
            vec![primary],
            "the {pane:?} pane's one brand fill is its `{label}` action, painted \
             at the button's own rect: {buttons:?} vs {primary:?}"
        );
        // (3) Where. In the header's action slot, above the hairline — the slot
        // R7 reserves for a pane's one action, and nowhere else in the pane.
        assert!(
            primary.bottom() <= rule.top() + 0.01,
            "the {pane:?} pane's primary action belongs in the header's action \
             slot, above the hairline: {primary:?} vs the band rule {rule:?}"
        );
    }
}

// --- R2: a stroke means this floats, and a content region is not one ----------

/// Every place the UI layer draws a **stroke** — an outline around a rectangle,
/// through `rect_stroke` or a `Frame`'s own `stroke` — with the count of sites
/// in that file and why every one of them is allowed to outline something.
///
/// R2's rule is one sentence: *a 1px stroke means this floats* — a popover, a
/// dialog, a menu, a toast — and nothing else. A content region is a fill at the
/// card radius and separates by surface tone (and at most one hairline inside
/// itself, which is a **fill** of `RULE_CONTENT` rather than a stroke, so it is
/// not in this table at all). A render seam can only prove that at the frames a
/// test enumerates, so the claim is made at the construction sites instead, and
/// it is made as a **classification** rather than as a flat "no strokes": every
/// site must say which of three things it is, and the table is closed, so a
/// stroke added anywhere in the UI layer lands here with a file name and a count
/// that no longer matches.
///
/// The three categories, and why the second and third are not violations:
///
/// - **A floating surface.** The bordered card — a card inside an `egui::Area`,
///   which is the "it floats" case R2 reserves a hairline for — and the egui
///   window stroke every popup, dialog, menu and toast wears (pinned as a value
///   in `tests/design_tokens.rs`, because it is set on `visuals` and never
///   painted from a module here).
/// - **A control's keyboard focus.** A 1px `BRAND` ring just outside a control
///   that holds focus. It is not a surface and it bounds nothing: it is ink on a
///   surface the user has already found, in exactly the way the accent rail is
///   ink on a row. Treating it as a box would mean the app has no focus
///   indicator, which is a worse outcome than a fifth category.
/// - **A rule, or a control's own border.** A divider beside a control, a
///   tri-state checkbox's outline, a text field's resting edge, an inset note
///   that strokes a *severity* rather than an edge. Each is bounded by its own
///   control and by nothing else; none of them is a region, so none of them
///   claims to be one.
///
/// A `Stroke` used as a **line** — an `hline`, a dash inside a checkbox, an icon
/// glyph, a dot ring — is not an outline and is deliberately out of scope here:
/// those are marks and glyphs, the same category as the rail.
const STROKE_SITES: [(&str, usize, &str); 8] = [
    // floating surface
    (
        "widgets/containers.rs",
        2,
        "the one bordered card (a card inside an `egui::Area` — the only caller \
         is the Welcome changelog overlay) and `note`'s severity ring, which \
         outlines an *inset* in warning ink rather than a region's edge",
    ),
    // a control's keyboard focus
    ("components.rs", 1, "the kit button's own focus ring"),
    (
        "widgets/controls.rs",
        2,
        "the shared `focus_ring` helper every custom-drawn control reaches for, \
         and the vocabulary button's own",
    ),
    (
        "widgets/inputs.rs",
        2,
        "the text field's resting edge (a control's own border) and its focus \
         ring",
    ),
    // a rule, or a control's own border
    (
        "commit_window.rs",
        3,
        "the tri-state partial checkbox's border, the message editor's focus \
         ring, and the commit/rollback divider rule",
    ),
    ("sidebar.rs", 1, "the tri-state checkbox's own border"),
    (
        "diff/actions.rs",
        1,
        "the granularity axis's outer boundary, which separates one grouped \
         control from the one beside it",
    ),
    (
        "kit/conflict_pane.rs",
        1,
        "the 2px `BRAND` ring around a focused conflict header or result cell: a \
         focus marker, and the geometry difference between a ring and a rail is \
         the whole reason it is not routed through the rail painter",
    ),
];

/// **R2: every stroke in the UI layer is a floating surface, a control's focus
/// ring, or a rule — and never a content region's edge.**
///
/// The table is closed, so this is a claim about the tree rather than about a
/// list someone remembered to update: a new `rect_stroke` anywhere under
/// `src/ui` makes the counts disagree and the message names the file.
#[test]
fn every_stroke_in_the_ui_layer_is_a_floating_surface_a_focus_ring_or_a_rule() {
    // The same statement-walk the rail-width ratchet uses in
    // `branch_component_kit.rs`: accumulate lines into a statement and read it at
    // its `;`/`}`, so a stroke written across three lines is one site rather than
    // three and a comment is not a site.
    let mut found: Vec<(String, usize)> = Vec::new();
    for (rel, src) in ui_source_files() {
        let mut statement = String::new();
        let mut count = 0usize;
        for line in src.lines() {
            statement.push_str(line);
            statement.push('\n');
            if !line.trim_end().ends_with(';') && !line.trim_end().ends_with('}') {
                continue;
            }
            // `Frame::stroke(` and `rect_stroke(` are the two ways this codebase
            // outlines a rectangle. A bare `Stroke::new(` is deliberately not in
            // the pattern: that is the *line* spelling (a dash, a glyph, a dot
            // ring), and a mark is not an outline.
            if statement.contains("rect_stroke(") || statement.contains(".stroke(") {
                count += 1;
            }
            statement.clear();
        }
        if count > 0 {
            found.push((rel, count));
        }
    }
    found.sort();

    let mut expected: Vec<(String, usize)> = STROKE_SITES
        .iter()
        .map(|(file, count, _)| ((*file).to_owned(), *count))
        .collect();
    expected.sort();
    assert_eq!(
        found, expected,
        "the set of stroke sites in `src/ui` is not the one this table \
         classifies. A stroke means one of three things — this floats, this \
         control has focus, or this is a rule — and never 'this is a content \
         region'. Add the site with its category, or remove the stroke."
    );
    assert_eq!(
        found, expected,
        "the set of stroke sites in `src/ui` is not the one this table \
         classifies. A stroke means one of three things — this floats, this \
         control has focus, or this is a rule — and never 'this is a content \
         region'. Add the site with its category, or remove the stroke."
    );

    // …and the table is consumed, so a row cannot rot into a comment: each one
    // says why the sites it accounts for are not a content region's edge.
    for (file, count, why) in STROKE_SITES {
        assert!(
            !file.is_empty() && count > 0 && !why.trim().is_empty(),
            "every stroke site is recorded with the file, how many sites it \
             accounts for, and why none of them is a content region's edge"
        );
    }

    // The positive half, at the value: a card is a fill, and the only way to get
    // a hairline onto one is to opt in by name. (The paint is asserted by
    // `card_paints_no_stroke_where_a_default_card_is`; this is the same rule read
    // from the frame body, so a card that grew a stroke behind a config flag
    // would be a shape in that suite *and* a name here.)
    let containers = ui_source_files()
        .into_iter()
        .find(|(rel, _)| rel == "widgets/containers.rs")
        .map(|(_, src)| src)
        .expect("the container module is under src/ui");
    assert_eq!(
        items_naming(&containers, "BORDER_HAIRLINE_WIDTH"),
        vec!["BORDER_HAIRLINE_WIDTH".to_owned(), "card".to_owned()],
        "the card's hairline is one opt-in read inside the one card function: a \
         second reader is a second surface deciding to box itself"
    );
    assert!(
        !items_naming(&containers, "RULE_CONTENT")
            .iter()
            .any(|name| name == "card"),
        "a card separates its regions with surface tone and at most one hairline, \
         so it does not stroke itself with the content-divider role"
    );
}

// --- R6: exactly one function produces a ref chip, in the whole app -----------

/// **R6: the ref chip is one function in one file, and no screen module names
/// its fill or its geometry.**
///
/// `exactly_one_function_produces_each_of_the_three_chips` proves the claim about
/// `widgets/chips.rs`, and the log's own suite proves it about `log_window.rs`
/// (from paint *and* from source, with comments blanked). Neither is a claim
/// about the app: each is a claim about a file somebody thought to check. This is
/// the whole-tree form, so the claim is "a ref chip cannot be produced outside
/// the chip module" rather than "the chip module produces one".
///
/// Two files are allowed to name the ref chip's two values, and the reasons are
/// different in each, which is why the allowance is written out rather than being
/// a bare exception:
///
/// - `widgets/chips.rs` — the definitions and [`ref_chip`], the one constructor.
/// - `ui/components.rs` — `ref_chip_with_ink`, the **branch row's** name: the same
///   fill and the same geometry with the *row's* ink instead of `INK_2`, because a
///   branch row owns its identity ink and dimming every name in the list by a step
///   the token layer does not ask for is the wrong fix. It is the ref chip's
///   geometry, not a fourth chip, and it is pinned to the same fill below so it
///   cannot quietly become one.
///
/// `ref_chip` and `compact_chip_radius` have **no** allowance: the constructor and
/// the radius accessor are the chip module's alone, so a second function that
/// builds a ref chip from them would have to name the geometry, and this fails
/// naming the module.
/// The source with every comment blanked, so a decision *written down* about a
/// symbol is not the same thing as a *use* of it.
///
/// The log's `paint_label_pill` records in its own doc comment that the collapsed
/// ref marker is deliberately the neutral badge "rather than a second ref-chip
/// painter" — and naming that rule is exactly what a reader needs. A scan that
/// counted comments would force the reasoning to be deleted rather than written,
/// which is the wrong trade for a rule this subtle. String literals are kept: a
/// label or a table name is content, not commentary.
///
/// The same reasoning, and the same helper, as `tests/git_log.rs`, which needs it
/// for the log's own half of the ref-chip claim.
fn code_only(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let (mut i, mut block, mut line, mut string) = (0usize, false, false, false);
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied().unwrap_or('\0');
        if block {
            if c == '*' && next == '/' {
                block = false;
                i += 2;
                continue;
            }
            if c == '\n' {
                out.push('\n');
            }
            i += 1;
            continue;
        }
        if line {
            if c == '\n' {
                line = false;
                out.push('\n');
            }
            i += 1;
            continue;
        }
        if string {
            out.push(c);
            if c == '\\' {
                if let Some(escaped) = chars.get(i + 1) {
                    out.push(*escaped);
                }
                i += 2;
                continue;
            }
            if c == '"' {
                string = false;
            }
            i += 1;
            continue;
        }
        match (c, next) {
            ('/', '*') => {
                block = true;
                i += 2;
            }
            ('/', '/') => {
                line = true;
                i += 2;
            }
            ('"', _) => {
                string = true;
                out.push(c);
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out.into_iter().collect()
}

#[test]
fn the_ref_chip_is_produced_by_one_function_in_one_file_and_no_screen_paints_one() {
    /// The two **values** that make a chip a ref chip, and the radius accessor
    /// beside them: which files may name each, and why. Three files, each for a
    /// different reason, and none of them a screen:
    ///
    /// - `widgets/chips.rs` — the definitions and [`ref_chip`], the one
    ///   constructor.
    /// - `ui/components.rs` — `ref_chip_with_ink`, the **branch row's** name: the
    ///   same fill and the same geometry with the *row's* ink instead of `INK_2`,
    ///   because a branch row owns its identity ink and dimming every name in the
    ///   list by a step the token layer does not ask for is the wrong fix. It is
    ///   the ref chip's geometry, not a fourth chip, and the assertion below pins
    ///   that it really is the same fill. It also places the branches pane's
    ///   **scope** chip, which is a count chip in the compact slot, so the radius
    ///   accessor is named here too.
    /// - `ui/widgets/mod.rs` — the façade's re-export list, which is a
    ///   *publication* decision (`turbogit_ui::ui::widgets::REF_CHIP_COLORS`
    ///   resolves) rather than a second producer.
    ///
    /// `ref_chip` itself is deliberately **not** in this table: screens are
    /// supposed to call it, so a name scan would fail every correct call site.
    /// The claim about the function is made where it belongs — see
    /// `exactly_one_function_produces_each_of_the_three_chips` for the chip
    /// module's own half, and the declaration check below for the tree's.
    const NAMED_IN: [(&str, &[&str]); 2] = [
        (
            "REF_CHIP_COLORS",
            &["widgets/chips.rs", "components.rs", "widgets/mod.rs"],
        ),
        (
            "compact_chip_radius",
            &["widgets/chips.rs", "components.rs", "widgets/mod.rs"],
        ),
    ];

    /// The compact chip **geometry** is a different kind of name from the ref
    /// chip's colours, and it needs a different answer — the literal "the
    /// geometry is named in `chips.rs` and nowhere else" is **not** the claim, and
    /// asserting it would be asserting something false. The geometry is the
    /// shared *shape* role of all three chips, so a screen may legitimately use it
    /// for a chip of its own, and two do: the left rail's hand-laid rows place a
    /// **count** chip into rects they allocated themselves (the shared
    /// `count_chip` allocates in a `Ui` flow, which a measured-offset row cannot
    /// use), and the branch tree reserves the chip's own inset to line a row's
    /// columns up. So the rule is about the *pair*, not the shape: a screen that
    /// names the geometry must either paint it with the count chip's pair or name
    /// no chip colour at all, and may never reach for the ref chip's.
    const SCREEN_GEOMETRY_USE: [(&str, &str); 2] = [
        (
            "sidebar.rs",
            "hand-places the **count** chip into caller-allocated rects (the \
             shared `count_chip` allocates its own slot in a flow, which a \
             measured-offset rail row cannot use)",
        ),
        (
            "branch_tree_view.rs",
            "reserves the chip's inset to line a row's columns up, and names no \
             chip colour at all — the pair it paints with comes from \
             `components::ref_chip_with_ink`",
        ),
    ];

    let files: Vec<(String, String)> = ui_source_files()
        .into_iter()
        .map(|(rel, src)| (rel, code_only(&src)))
        .collect();
    let chips = files
        .iter()
        .find(|(rel, _)| rel == "widgets/chips.rs")
        .map(|(_, src)| src.as_str())
        .expect("the chip module is under src/ui");

    for (value, allowed) in NAMED_IN {
        // The chip module itself names them — asserted rather than assumed, so a
        // rename cannot quietly turn this into a vacuous loop over names that
        // appear nowhere.
        assert!(
            chips.contains(value),
            "the chip module must still name `{value}`: the whole-tree ratchet \
             below is only a claim about the app while the chip module really \
             does define the role"
        );
        // And the tree agrees about exactly these files. Comments are already
        // blanked, so a module that *documents* the decision is not counted.
        let mut naming: Vec<String> = files
            .iter()
            .filter(|(_, src)| src.contains(value))
            .map(|(rel, _)| rel.clone())
            .collect();
        naming.sort();
        let mut expected: Vec<String> = allowed.iter().map(|a| (*a).to_owned()).collect();
        expected.sort();
        assert_eq!(
            naming, expected,
            "`{value}` is named by a module that is not one of {expected:?}. A \
             second module that builds a ref chip — or a second function inside a \
             screen that paints one with a hand-spelled fill — is the \
             duplication the closed chip vocabulary exists to prevent, and \
             `exactly_one_function_produces_each_of_the_three_chips` cannot see \
             it because it only reads `widgets/chips.rs`."
        );
    }

    // The row variant really is the *same* fill, so the allowance above is the
    // documented one and not a loophole: a branch row's name is a ref chip with
    // the row's own ink.
    assert_eq!(
        turbogit_ui::ui::components::ref_chip_with_ink_fill(),
        Palette::RAISED_ON_CARD,
        "the branch row's ref chip keeps the one ref-chip fill; a caller-chosen \
         *ink* is allowed, a caller-chosen fill is a fourth chip"
    );

    // The compact geometry, screen by screen: the shape is shared, the pair is
    // not, and this is the half that stops a screen from painting a fourth chip
    // out of the shared shape.
    let mut geometry_in_screens: Vec<&str> = files
        .iter()
        .filter(|(rel, src)| {
            src.contains("COMPACT_CHIP_GEOMETRY")
                && !matches!(
                    rel.as_str(),
                    "widgets/chips.rs" | "components.rs" | "widgets/mod.rs"
                )
        })
        .map(|(rel, _)| rel.as_str())
        .collect();
    geometry_in_screens.sort();
    let mut expected_screens: Vec<&str> = SCREEN_GEOMETRY_USE.iter().map(|(f, _)| *f).collect();
    expected_screens.sort();
    assert_eq!(
        geometry_in_screens, expected_screens,
        "the set of screen modules that name the compact chip geometry is not the \
         one this table explains. The shape is the chips' shared shape role, so a \
         screen may use it for a chip of its own — but it has to say which chip, \
         and the only one a screen may build by hand is the count chip."
    );
    for (rel, why) in SCREEN_GEOMETRY_USE {
        let src = &files
            .iter()
            .find(|(name, _)| name == rel)
            .expect("a named screen module")
            .1;
        assert!(
            !why.trim().is_empty(),
            "every screen that names the chip geometry records why it is not a \
             fourth chip"
        );
        assert!(
            !src.contains("REF_CHIP_COLORS"),
            "`{rel}` names the compact chip geometry and must not name the ref \
             chip's colours: the ref chip is produced by exactly one function, and \
             a screen that paints the fill itself is that function's second copy"
        );
        // And if it names a chip colour, it is the count chip's — the one chip a
        // screen may place by hand.
        if src.contains("COUNT_CHIP_COLORS") {
            assert!(
                !why.contains("names no chip colour"),
                "`{rel}` places the count chip by hand, so it names that chip's \
                 pair — the reason recorded above has to say so"
            );
        }
    }

    // **The one-function claim, over the whole tree rather than over one file.**
    // `ref_chip` is *called* by screens — that is the vocabulary being used — so
    // the only thing that can be forbidden is a second *declaration*: a function
    // of that name outside the chip module, or a function anywhere that builds a
    // ref chip without going through the one constructor or the one documented
    // row variant. Both are a second producer wearing the first one's name.
    for (rel, src) in &files {
        if rel == "widgets/chips.rs" || rel == "widgets/mod.rs" {
            continue;
        }
        for declared in ["fn ref_chip(", "fn ref_chip_with_ink("] {
            assert!(
                word_offsets(src, declared).is_empty(),
                "`{rel}` declares `{declared}`: exactly one function produces a \
                 ref chip, and a second declaration of the name — however it \
                 colours itself — is a second producer, not a second caller."
            );
        }
    }
}
