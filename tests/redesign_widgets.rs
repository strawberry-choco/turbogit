//! Issue #6 — Shared widget library (`src/ui/widgets.rs`).
//!
//! Three layers of proof, mirroring spec §7 and the R1.4 plan row:
//!
//! 1. **Palette-token completeness** — every token the widget vocabulary
//!    relies on exists in [`turbogit::theme::Palette`] with the exact hex
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
use egui_kittest::{kittest::Queryable, Harness};
use turbogit::theme::Palette;
use turbogit::ui::icons::Icon;
use turbogit::ui::widgets::{self, BadgeKind, ButtonVariant, RefKind, WidgetState};

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

    // Ink.
    assert_eq!(Palette::INK, Color32::from_rgb(0xbc, 0xbe, 0xc4));
    assert_eq!(Palette::INK_2, Color32::from_rgb(0xa0, 0xa3, 0xab));
    assert_eq!(Palette::INK_3, Color32::from_rgb(0x80, 0x80, 0x80));

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
            widgets::tint_over_bg(kind.accent(), widgets::BADGE_TINT),
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
    // Selected wins over hover; unselected rows only fill on hover.
    assert_eq!(widgets::row_fill(true, false), Palette::BRAND);
    assert_eq!(widgets::row_fill(true, true), Palette::BRAND);
    assert_eq!(widgets::row_fill(false, true), Palette::SURFACE_2);
    assert_eq!(widgets::row_fill(false, false), Color32::TRANSPARENT);
}

// ---------------------------------------------------------------------------
// 3. Harness smoke render (several widgets together)
// ---------------------------------------------------------------------------

type ClickFlag = Rc<Cell<bool>>;

/// A harness rendering a panel composed purely of shared widgets.
///
/// Setup mirrors production (`app.rs` / `redesign_harness.rs`): dark-only
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
            turbogit::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            egui::CentralPanel::default().show(ui, |ui| {
                // Section chrome.
                widgets::group_title(ui, "Recent");
                widgets::toolwindow_header(ui, "Changed files", |_ui| {});

                // Buttons.
                if widgets::ghost_button(ui, None, "Ghost action").clicked() {
                    ghost_clicked.set(true);
                }
                if widgets::primary_button(ui, Some(Icon::CHECK), "Primary action").clicked() {
                    // Counted via painted assertion only.
                }
                if widgets::compact_button(ui, "Compact action").clicked() {
                    compact_clicked.set(true);
                }
                widgets::icon_button(ui, Icon::X);

                // Chips.
                widgets::badge(ui, "+3", BadgeKind::Added);
                widgets::badge(ui, "M", BadgeKind::Modified);
                widgets::badge(ui, "D", BadgeKind::Deleted);
                widgets::ref_label(ui, "main", RefKind::Branch);
                widgets::ref_label(ui, "origin/main", RefKind::Remote);
                widgets::ref_label(ui, "v1.0", RefKind::Tag);

                // Rows.
                widgets::tree_row(ui, true, |ui| {
                    ui.label("selected branch row");
                });
                widgets::tree_row(ui, false, |ui| {
                    ui.label("unselected branch row");
                });
                widgets::selectable_row(ui, |ui| {
                    ui.label("plain selectable row");
                });

                // Inputs.
                widgets::search_input(ui, "Search commits", &mut search_buf);
                widgets::text_input(ui, "Branch name", &mut name_buf);

                // Dialog chrome.
                widgets::dialog_header(ui, "Push Confirmation");
                widgets::dialog_footer(ui, |ui| {
                    widgets::primary_button(ui, None, "Footer OK");
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

    // Rows.
    assert_painted(&harness, "selected branch row");
    assert_painted(&harness, "unselected branch row");
    assert_painted(&harness, "plain selectable row");

    // Inputs paint their placeholder hint when empty.
    assert_painted(&harness, "Search commits");
    assert_painted(&harness, "Branch name");

    // Chrome: group titles and tool-window headers uppercase (§3.3);
    // dialog header titles stay title-case.
    assert_painted(&harness, "RECENT");
    assert_painted(&harness, "CHANGED FILES");
    assert_painted(&harness, "Push Confirmation");
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
