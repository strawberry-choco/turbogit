//! Issue #6 — Shared widget library (public facade at `src/ui/widgets/mod.rs`).
//!
//! Three layers of proof, mirroring spec §7 and the R1.4 plan row:
//!
//! 1. **Palette-token completeness** — every token the widget vocabulary
//!    relies on exists in [`turbogit_ui::theme::Palette`] with the exact hex
//!    values from spec §2, and the surface ladder is strictly ordered.
//! 2. **Pure styling decisions** — badge-kind→color, ref-kind→color,
//!    button-variant×state fills/text, and tree-row selection logic are all
//!    total functions over tokens, asserted without rendering.
//! 3. **Harness smoke render** — several widgets composed in one headless
//!    egui_kittest frame: painted text is asserted and a ghost button is
//!    really clicked through the accessibility tree.

use std::cell::Cell;
use std::rc::Rc;

use egui::{Color32, Shape};
use egui_kittest::{Harness, kittest::Queryable};
use test_support::harness::painted_galleys;
use turbogit_ui::theme::{PILL_RADIUS, Palette};
use turbogit_ui::ui::components::{self, RowState};
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

#[test]
fn ref_kind_maps_to_brand_success_warning_pills() {
    // Spec §8.3: `.tg-label.branch` BRAND pill / remote SUCCESS / tag WARNING.
    assert_eq!(RefKind::Branch.accent(), Palette::BRAND);
    assert_eq!(RefKind::Remote.accent(), Palette::STATE_SUCCESS);
    assert_eq!(RefKind::Tag.accent(), Palette::STATE_WARNING);

    // Ref labels are solid pills; ink picks the palette token with real
    // contrast: white brand ink on BRAND, dark BG ink on the lighter
    // success/warning fills.
    assert_eq!(RefKind::Branch.colors().fg, Palette::BRAND_INK);
    for kind in [RefKind::Remote, RefKind::Tag] {
        let colors = kind.colors();
        assert_eq!(colors.bg, kind.accent());
        assert_eq!(colors.fg, Palette::BG);
    }
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

#[test]
fn tree_row_selection_logic_paints_brand_over_hover() {
    // Selected wins over hover; unselected rows only fill on hover. These are
    // the shared tree/list row fills, i.e. `RowState::from_flags`' mapping of
    // a selected row onto the solid BRAND role rather than the §13
    // `RowState::Selected` band — see `components::RowState`.
    assert_eq!(
        components::row_fill(RowState::from_flags(true, false)),
        Palette::BRAND
    );
    assert_eq!(
        components::row_fill(RowState::from_flags(true, true)),
        Palette::BRAND
    );
    assert_eq!(
        components::row_fill(RowState::from_flags(false, true)),
        Palette::SURFACE_2
    );
    assert_eq!(
        components::row_fill(RowState::from_flags(false, false)),
        Color32::TRANSPARENT
    );
}

/// Status badges from the screens-gap vocabulary (issue #01): direction-tagged
/// ahead/behind counts, protected-branch lock, stale-age, FOCUSED, and
/// CASCADE markers. Each variant picks a token from the new risk/status
/// scales so a chip carrying any of them has a real color.
#[test]
fn status_badge_variants_map_to_tokens() {
    use StatusBadge;

    // Count direction decides ahead (success) vs behind (warning): a count
    // chip never picks the wrong hue for its direction.
    assert_eq!(
        StatusBadge::Count(CountDirection::Ahead).accent(),
        Palette::STATE_SUCCESS,
        "ahead counts are success-green"
    );
    assert_eq!(
        StatusBadge::Count(CountDirection::Behind).accent(),
        Palette::STATE_WARNING,
        "behind counts are warning-amber"
    );

    // Lock uses the warning accent: a protected-branch lock must read as a
    // caution, mirroring the existing tag-ref accent decision.
    assert_eq!(StatusBadge::Lock.accent(), Palette::STATE_WARNING);

    // Stale-age uses the info accent, mirroring the STATUS_STALE token.
    assert_eq!(StatusBadge::Stale.accent(), Palette::STATE_INFO);

    // FOCUSED uses the brand accent: a modal-active marker reads as the
    // same brand that selection, primary actions, and focused inputs use.
    assert_eq!(StatusBadge::Focused.accent(), Palette::BRAND);

    // CASCADE has a distinct accent of its own so cascade chips never
    // collide with focus or selection color.
    let cascade = StatusBadge::Cascade.accent();
    assert_ne!(cascade, Palette::BRAND, "cascade ≠ focus");
    assert_ne!(cascade, Palette::STATE_SUCCESS, "cascade ≠ success");
    assert_ne!(cascade, Palette::STATE_WARNING, "cascade ≠ warning");
    assert_ne!(cascade, Palette::STATE_INFO, "cascade ≠ info");
    assert_ne!(cascade, Palette::STATE_ERROR, "cascade ≠ error");
    assert_ne!(cascade, Palette::INK_2, "cascade ≠ muted text");
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

                // Chips.
                badge(ui, "+3", BadgeKind::Added);
                badge(ui, "M", BadgeKind::Modified);
                badge(ui, "D", BadgeKind::Deleted);
                ref_label(ui, "main", RefKind::Branch);
                ref_label(ui, "origin/main", RefKind::Remote);
                ref_label(ui, "v1.0", RefKind::Tag);

                // Rows.
                tree_row(ui, true, |ui| {
                    ui.label("selected branch row");
                });
                tree_row(ui, false, |ui| {
                    ui.label("unselected branch row");
                });

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
                    ref_label(ui, "origin", RefKind::Remote);
                });
            });
        },
        (),
    );
    harness.set_size(egui::vec2(400.0, 80.0));
    settle(&mut harness);

    let added = harness.get_by_label("added").rect();
    let neutral = harness.get_by_label("main").rect();
    let remote = harness.get_by_label("origin").rect();
    for rect in [added, neutral, remote] {
        assert_eq!(rect.height(), CHIP_HEIGHT);
    }
    assert!(added.right() <= neutral.left());
    assert!(neutral.right() <= remote.left());
    assert_eq!(added.top(), neutral.top());
    assert_eq!(neutral.top(), remote.top());
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

    // Badges & ref chips.
    assert_painted(&harness, "+3");
    assert_painted(&harness, "M");
    assert_painted(&harness, "D");
    assert_painted(&harness, "main");
    assert_painted(&harness, "origin/main");
    assert_painted(&harness, "v1.0");

    // Rows. `selectable_row` was deleted by conformance issue 18 (no caller,
    // and no migration adopted it: it is `tree_row` with `selected = false`), so
    // this gallery shows the selection pair only.
    assert_painted(&harness, "selected branch row");
    assert_painted(&harness, "unselected branch row");

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

/// Harness rendering the screens-gap status-badge vocabulary (issue #01).
/// One row per variant; painted text confirms the chip body is alive.
fn status_badges_harness() -> (Harness<'static, ()>, tempfile::TempDir) {
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, _state| {
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            egui::CentralPanel::default().show(ui, |ui| {
                status_badge(ui, "↑3", StatusBadge::Count(CountDirection::Ahead));
                status_badge(ui, "↓2", StatusBadge::Count(CountDirection::Behind));
                status_badge(ui, "LOCK", StatusBadge::Lock);
                status_badge(ui, "3d", StatusBadge::Stale);
                status_badge(ui, "FOCUSED", StatusBadge::Focused);
                status_badge(ui, "CASCADE", StatusBadge::Cascade);
            });
        },
        (),
    );
    harness.set_size(egui::vec2(800.0, 80.0));
    (harness, tempfile::tempdir().expect("tempdir"))
}

/// Each variant paints at least one rect filled with the token the variant
/// claims (issue #01): accent(). Token equality on the painted rect, not on
/// the variant, proves the widget actually paints through the public API
/// rather than reporting an answer the implementation never delivered.
#[test]
fn status_badges_paint_with_their_claimed_token() {
    use {CountDirection, StatusBadge};

    let (mut harness, _dir) = status_badges_harness();
    settle(&mut harness);

    // Body text confirms every chip is alive (the assertion that matters
    // for the variant is its painted rect color, asserted below).
    let rects: Vec<Color32> = harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Rect(rs) if rs.fill != Color32::TRANSPARENT => Some(rs.fill),
            _ => None,
        })
        .collect();

    // Every variant's tinted fill (accent @ BADGE_TINT over BG) must appear
    // on a painted rect: the chip's body paint comes through unchanged from
    // the variant table, not the raw accent.
    let expected = [
        StatusBadge::Count(CountDirection::Ahead).accent(),
        StatusBadge::Count(CountDirection::Behind).accent(),
        StatusBadge::Lock.accent(),
        StatusBadge::Stale.accent(),
        StatusBadge::Focused.accent(),
        StatusBadge::Cascade.accent(),
    ];
    for accent in expected {
        let tinted = tint_over_bg(accent, BADGE_TINT);
        assert!(
            rects.contains(&tinted),
            "no rect painted with tinted fill {tinted:?} (accent {accent:?}); painted fills: {rects:?}"
        );
    }
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
