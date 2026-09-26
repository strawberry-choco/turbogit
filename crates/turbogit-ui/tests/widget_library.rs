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
use turbogit_ui::theme::{PILL_RADIUS, Palette};
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

    // Ink (C3: INK aliases the authoritative T_PRIMARY ramp; INK_2/INK_3
    // alias the lifted T_SECONDARY/T_MUTED levels).
    assert_eq!(Palette::INK, Palette::T_PRIMARY);
    assert_eq!(Palette::INK_2, Palette::T_SECONDARY);
    assert_eq!(Palette::INK_3, Palette::T_MUTED);

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
                toolwindow_header(ui, "Changed files", |_ui| {});

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
fn painted_text(harness: &Harness<'_, ()>) -> Vec<String> {
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
fn settle(harness: &mut Harness<'_, ()>) {
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

    // Chrome: group titles and tool-window headers uppercase (§3.3).
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

/// The ink each piece of painted text actually drew with.
///
/// A galley keeps the color it was *laid out* with, so a widget that overrides
/// the color at paint time — which is how every shared button takes its ink —
/// is only visible through the text shape's override. Prefer the override, and
/// fall back to the laid-out color.
fn painted_ink(harness: &Harness<'_, ()>, needle: &str) -> Color32 {
    harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(text) if text.galley.text() == needle => Some(
                text.override_text_color
                    .or_else(|| text.galley.job.sections.first().map(|s| s.format.color))
                    .unwrap_or(Color32::TRANSPARENT),
            ),
            _ => None,
        })
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
fn painted_rects(
    harness: &Harness<'_, ()>,
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

/// The one card geometry. The fill, the hairline `LINE` stroke and the
/// `CARD_RADIUS` rounding stay owned by the shared frame, while inner padding,
/// surface tone and how the card claims its width are per-site parameters.
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
    let galley_x = |needle: &str| -> f32 {
        painted_galleys(&harness)
            .into_iter()
            .find(|g| g.text == needle)
            .map(|g| g.pos.x)
            .unwrap_or_else(|| panic!("`{needle}` was not painted"))
    };
    // egui paints a `Frame`'s inside stroke half outside its own rect and then
    // rounds outward to whole pixels, so a card's painted box is a point or two
    // proud of the box its geometry asked for. Every measurement below allows
    // that slack and nothing more.
    const STROKE_SLACK: f32 = 2.0;

    // One owner for the shape: the default card is a `CONTENT_BG` fill, a 1px
    // `LINE` hairline, and the `CARD_RADIUS` rounding — asserted on the painted
    // shapes, not on the configuration that produced them.
    let (default_rect, default_stroke, default_radius) = rects
        .iter()
        .find(|(rect, fill, _, _)| *fill == Palette::CONTENT_BG && rect.height() > 20.0)
        .map(|(rect, _, stroke, radius)| (*rect, *stroke, *radius))
        .expect("the default card must paint a CONTENT_BG frame");
    assert_eq!(default_stroke, egui::Stroke::new(1.0, Palette::LINE));
    assert_eq!(
        default_radius,
        egui::CornerRadius::same(turbogit_ui::theme::CARD_RADIUS)
    );

    // Stretch is the default width strategy: the card spans its pane, and its
    // body sits one `PANEL_PADDING` inside the card's own box.
    let panel = panel_width.get();
    assert!(
        (default_rect.width() - panel).abs() <= STROKE_SLACK,
        "the default card must stretch: {} vs available {panel}",
        default_rect.width()
    );
    assert!(
        (galley_x("default body") - default_rect.left() - turbogit_ui::theme::PANEL_PADDING).abs()
            <= STROKE_SLACK,
        "the default card must pad by PANEL_PADDING, got {}",
        galley_x("default body") - default_rect.left()
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
        .find(|rect| (rect.width() - panel).abs() <= STROKE_SLACK)
        .expect("the padded raised card must still stretch to the pane");
    let pinned_rect = *surface
        .iter()
        .find(|rect| (rect.width() - (420.0 + 2.0 * 20.0)).abs() <= STROKE_SLACK)
        .unwrap_or_else(|| {
            panic!("the pinned card must honour its 420 min width, got {surface:?}")
        });

    // A per-site padding parameter moves the body's inset by exactly the
    // difference in padding, without changing the fill, stroke or radius.
    assert!(
        (galley_x("raised body") - galley_x("default body") - 12.0).abs() <= 0.01,
        "`.padded(24)` must inset the body 12 further than `.padded(12)`, got {}",
        galley_x("raised body") - galley_x("default body")
    );
    assert!(
        (galley_x("pinned body") - galley_x("default body") - 8.0).abs() <= 0.01,
        "the pinned card must inset the body 8 further than `.padded(12)`, got {}",
        galley_x("pinned body") - galley_x("default body")
    );

    // `MinWidth` keeps its pin and does not stretch.
    assert!(
        (pinned_rect.width() - panel).abs() > STROKE_SLACK,
        "a MinWidth card must not stretch to the pane ({})",
        pinned_rect.width()
    );
    assert!(
        raised_rect.width() < pinned_rect.width(),
        "a stretched card must be narrower than one pinned past the pane"
    );
}
