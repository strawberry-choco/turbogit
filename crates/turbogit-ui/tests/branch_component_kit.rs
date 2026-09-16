//! Issue 01 — Branch-screen design tokens & component kit (design doc §12–§14).
//!
//! The theme gains the Branches-screen visual vocabulary: the §13 surfaces
//! (window/panel/content/raised/selection/divider), the small meaning-color
//! set (accent, ahead, behind, danger, link), the three-level text ramp, the
//! §14 component kit (branch row states, section header, sync badge, four
//! button variants, overflow cluster), and §12 geometry. Expected values are
//! read from the design reference `docs/branches-screen-behavior.md` —
//! independent of the implementation.
//!
//! Pure decisions are asserted directly; the kit renders once through the
//! harness to prove the clickable-target and painted-surface rules hold.

use std::cell::Cell;
use std::rc::Rc;

use egui_kittest::{Harness, kittest::Queryable as _};
use turbogit_ui::theme::Palette;
use turbogit_ui::ui::components::{
    CLICK_TARGET_MIN, KIT_ICON_LARGE, KitButton, RowState, SyncKind, middle_truncate, row_fill,
    sync_badge,
};
use turbogit_ui::ui::widgets::WidgetState;

// --- §13 surfaces ------------------------------------------------------------

#[test]
fn surface_tokens_match_design_doc() {
    assert_eq!(
        Palette::WINDOW_BG,
        egui::Color32::from_rgb(0x1a, 0x1b, 0x1e)
    ); // title bar, strips
    assert_eq!(Palette::PANEL_BG, egui::Color32::from_rgb(0x1e, 0x20, 0x23)); // sidebar, detail panel, inputs
    assert_eq!(
        Palette::CONTENT_BG,
        egui::Color32::from_rgb(0x23, 0x25, 0x29)
    ); // branch list, tab strip
    assert_eq!(Palette::RAISED, egui::Color32::from_rgb(0x2b, 0x2d, 0x30)); // secondary buttons, chips, active tab
    assert_eq!(
        Palette::SELECTION,
        egui::Color32::from_rgb(0x2e, 0x43, 0x69)
    ); // active row/pill
    assert_eq!(Palette::DIVIDER, egui::Color32::from_rgb(0x2b, 0x2d, 0x30)); // 1px dividers
}

// --- §13 meaning colors -------------------------------------------------------

#[test]
fn meaning_colors_match_design_doc() {
    assert_eq!(Palette::ACCENT, egui::Color32::from_rgb(0x35, 0x74, 0xf0)); // primary action / branch chips
    assert_eq!(Palette::AHEAD, egui::Color32::from_rgb(0x5f, 0xa8, 0x6c)); // ahead counts, current branch, in sync
    assert_eq!(Palette::BEHIND, egui::Color32::from_rgb(0xdc, 0xa3, 0x4e)); // behind counts, dirty markers
    assert_eq!(Palette::DANGER, egui::Color32::from_rgb(0xdb, 0x5c, 0x5c)); // delete only
    assert_eq!(Palette::LINK, egui::Color32::from_rgb(0x74, 0xa3, 0xe8)); // commit hash chips
}

// --- §13 text ramp ------------------------------------------------------------

#[test]
fn text_ramp_tokens_match_design_doc() {
    assert_eq!(
        Palette::T_PRIMARY,
        egui::Color32::from_rgb(0xdf, 0xe1, 0xe5)
    ); // branch names, body
    assert_eq!(
        Palette::T_SECONDARY,
        egui::Color32::from_rgb(0x9d, 0xa0, 0xa8)
    ); // section labels, secondary actions
    assert_eq!(Palette::T_MUTED, egui::Color32::from_rgb(0x6f, 0x73, 0x7b)); // counts, timestamps, placeholders
}

// --- §13 shape + type sizes ---------------------------------------------------

#[test]
fn shape_and_type_tokens_match_design_doc() {
    assert_eq!(Palette::RADIUS_CHIP, 3); // chips & badges
    assert_eq!(Palette::RADIUS_CONTROL, 4); // buttons, inputs, panels
    assert_eq!(turbogit_ui::theme::TYPE_SECTION, 9.0); // uppercase section labels
    assert_eq!(turbogit_ui::theme::TYPE_CHIP, 10.0); // chips/badges
    assert_eq!(turbogit_ui::theme::TYPE_CONTROL, 11.0); // controls and metadata
    assert_eq!(turbogit_ui::theme::TYPE_BODY, 12.0); // branch names and body
    assert_eq!(turbogit_ui::theme::TYPE_DETAIL_TITLE, 13.0); // detail title
}

// --- §14.1 branch row states ---------------------------------------------------

#[test]
fn row_fill_states_are_distinct() {
    let idle = row_fill(RowState::Default);
    let hovered = row_fill(RowState::Hover);
    let selected = row_fill(RowState::Selected);
    assert_eq!(
        idle,
        egui::Color32::TRANSPARENT,
        "default rows paint nothing"
    );
    assert_eq!(
        hovered,
        Palette::SURFACE_2,
        "hover uses the surface-2 hover fill"
    );
    assert_eq!(
        selected,
        Palette::SELECTION,
        "selected uses the §13 selection token"
    );
    assert!(
        idle != hovered && hovered != selected && idle != selected,
        "default / hover / selected must be visually distinct (§17)"
    );
}

/// Stale rows dim (never hide); current rows keep primary ink.
#[test]
fn stale_rows_dim_without_hiding() {
    use turbogit_ui::ui::components::row_ink;
    assert_eq!(row_ink(false), Palette::T_PRIMARY);
    assert_eq!(row_ink(true), Palette::T_MUTED);
}

/// Mid-operation rows render a first-class label (design doc §10).
#[test]
fn mid_operation_label_is_first_class() {
    use turbogit_ui::ui::components::mid_op_label;
    assert_eq!(mid_op_label("merging"), "merging…");
    assert_eq!(mid_op_label("rebasing"), "rebasing…");
}

// --- §14.2 section header -----------------------------------------------------

#[test]
fn section_label_renders_uppercase_with_count() {
    use turbogit_ui::ui::components::section_label;
    assert_eq!(section_label("Local", 3), "LOCAL 3");
    assert_eq!(section_label("Remote", 0), "REMOTE 0");
    assert_eq!(section_label("Tags", 12), "TAGS 12");
}

// --- §14.3 sync badge ----------------------------------------------------------

#[test]
fn sync_badge_contract_matches_design() {
    // Ahead / behind render as icon + count pairs, never the bare arrow (§4).
    assert_eq!(
        sync_badge(2, 0, false),
        Some((SyncKind::Ahead, "↑2".to_string()))
    );
    assert_eq!(
        sync_badge(0, 1, false),
        Some((SyncKind::Behind, "↓1".to_string()))
    );
    assert_eq!(
        sync_badge(2, 1, false),
        Some((SyncKind::Diverged, "↑2 ↓1".to_string()))
    );
    // In sync is said quietly, never silent.
    assert_eq!(
        sync_badge(0, 0, false),
        Some((SyncKind::InSync, "in sync".to_string()))
    );
    // A deleted upstream wins over any count.
    assert_eq!(
        sync_badge(3, 0, true),
        Some((SyncKind::Gone, "gone".to_string()))
    );
}

// --- §14.4/§14.7 truncation ----------------------------------------------------

#[test]
fn middle_truncate_keeps_identifying_ends() {
    // `mre`-style names stay readable on both ends (design doc §14.7).
    assert_eq!(
        middle_truncate("feature/multi-root-executor", 22),
        "feature/multi…executor"
    );
    // Short names are returned whole.
    assert_eq!(middle_truncate("main", 22), "main");
    // The rule "never a bare end ellipsis" holds at tiny widths too.
    let t = middle_truncate("features", 4);
    assert!(
        t.starts_with("fe") && t.ends_with('s') && t.contains('…'),
        "got {t:?}"
    );
}

// --- §14.5 buttons: four variants, four states -----------------------------------

#[test]
fn primary_button_uses_accent_and_brightens_on_engagement() {
    use turbogit_ui::ui::widgets::mix;
    assert_eq!(KitButton::Primary.fill(WidgetState::Idle), Palette::ACCENT);
    assert_eq!(
        KitButton::Primary.fill(WidgetState::Hovered),
        mix(Palette::ACCENT, egui::Color32::WHITE, 0.10)
    );
    assert_eq!(
        KitButton::Primary.fill(WidgetState::Active),
        mix(Palette::ACCENT, egui::Color32::WHITE, 0.20)
    );
    assert_eq!(
        KitButton::Primary.ink(WidgetState::Idle),
        Palette::BRAND_INK
    );
}

#[test]
fn secondary_button_is_raised_surface() {
    assert_eq!(
        KitButton::Secondary.fill(WidgetState::Idle),
        Palette::RAISED
    );
    assert_eq!(
        KitButton::Secondary.ink(WidgetState::Idle),
        Palette::T_SECONDARY
    );
    assert_eq!(
        KitButton::Secondary.ink(WidgetState::Hovered),
        Palette::T_PRIMARY
    );
}

#[test]
fn quiet_button_is_text_only_and_quiet() {
    assert_eq!(
        KitButton::Quiet.fill(WidgetState::Idle),
        egui::Color32::TRANSPARENT
    );
    assert_eq!(
        KitButton::Quiet.fill(WidgetState::Hovered),
        Palette::SURFACE_2
    );
    assert_eq!(
        KitButton::Quiet.ink(WidgetState::Idle),
        Palette::T_SECONDARY
    );
}

#[test]
fn danger_button_is_red_text_only() {
    assert_eq!(KitButton::Danger.ink(WidgetState::Idle), Palette::DANGER);
    assert_eq!(KitButton::Danger.ink(WidgetState::Hovered), Palette::DANGER);
    assert_eq!(
        KitButton::Danger.fill(WidgetState::Idle),
        egui::Color32::TRANSPARENT
    );
}

#[test]
fn disabled_buttons_dim_to_muted_ink() {
    for kind in [
        KitButton::Primary,
        KitButton::Secondary,
        KitButton::Quiet,
        KitButton::Danger,
    ] {
        assert_eq!(kind.ink(WidgetState::Disabled), Palette::T_MUTED);
    }
}

// --- §12 geometry -----------------------------------------------------------------

#[test]
fn geometry_constants_match_spec() {
    use turbogit_ui::ui::components::{
        BRANCH_ROW_H, CLICK_TARGET_MIN, DETAIL_W, KIT_ICON, SECTION_H, SIDE_PANEL_W, TOOLBAR_H,
    };
    assert_eq!(BRANCH_ROW_H, 30.0); // dense IDE list row
    assert_eq!(SECTION_H, 26.0); // Local / Remote / Tags header
    assert_eq!(TOOLBAR_H, 44.0); // branch toolbar
    assert_eq!(DETAIL_W, 280.0); // branch detail panel
    assert_eq!(SIDE_PANEL_W, 220.0); // left repo tree / metadata panel
    assert_eq!(KIT_ICON, 12.0); // icons draw at 12–13px (§14)
    assert_eq!(KIT_ICON_LARGE, 13.0);
    assert_eq!(CLICK_TARGET_MIN, 24.0); // every clickable target ≥24px tall (§14)
}

// --- Render smoke: the kit paints and targets stay clickable ----------------------

type ClickFlag = Rc<Cell<bool>>;

fn kit_harness(primary_clicked: ClickFlag, danger_clicked: ClickFlag) -> Harness<'static, ()> {
    use turbogit_ui::ui::components::{KitButton, kit_button, overflow_button, section_header};
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, _| {
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            egui::CentralPanel::default().show(ui, |ui| {
                section_header(ui, "Local", 3, true, |ui| {
                    overflow_button(ui, "Local header action");
                });
                if kit_button(ui, KitButton::Primary, "New Branch").clicked() {
                    primary_clicked.set(true);
                }
                if kit_button(ui, KitButton::Danger, "Delete").clicked() {
                    danger_clicked.set(true);
                }
                if kit_button(ui, KitButton::Quiet, "Quiet").clicked() {}
            });
        },
        (),
    );
    harness.set_size(egui::vec2(600.0, 400.0));
    harness
}

#[test]
fn kit_smoke_renders_and_targets_stay_clickable() {
    let primary = Rc::new(Cell::new(false));
    let danger = Rc::new(Cell::new(false));
    let mut harness = kit_harness(primary.clone(), danger.clone());
    harness.step();

    // The section label paints; the strip (accessible label "Local") is the
    // click target.
    let _ = harness.get_by_label("LOCAL 3");
    for label in ["Local", "New Branch", "Delete", "Quiet"] {
        let node = harness.get_by_label(label);
        let rect = node.rect();
        assert!(
            rect.height() >= CLICK_TARGET_MIN,
            "clickable target `{label}` must be ≥24px tall, was {}",
            rect.height()
        );
    }

    harness.get_by_label("New Branch").click();
    harness.step();
    assert!(primary.get(), "primary kit button must be clickable");
    harness.get_by_label("Delete").click();
    harness.step();
    assert!(danger.get(), "danger kit button must be clickable");
}
