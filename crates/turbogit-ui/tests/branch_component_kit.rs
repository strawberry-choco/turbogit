//! Issue 01 — Branch-screen design tokens & component kit (design doc §12–§14).
//!
//! The theme gains the Branches-screen visual vocabulary: the §13 surfaces
//! (window/panel/content/raised/selection/divider), the small meaning-color
//! set (accent, ahead, behind, danger, link), the three-level text ramp, the
//! §14 component kit (branch row states, section header, sync badge, four
//! button variants, overflow cluster), and §12 geometry. The § numbers name
//! the constants' roles in the design vocabulary; there is no longer a design
//! document behind them to read expected values from, so each expected value
//! below is stated here as the contract this suite pins.
//!
//! Pure decisions are asserted directly; the kit renders once through the
//! harness to prove the clickable-target and painted-surface rules hold.

use std::cell::Cell;
use std::rc::Rc;

use egui_kittest::{Harness, kittest::Queryable as _};
use turbogit_ui::theme::Palette;
use turbogit_ui::ui::components::{
    CLICK_TARGET_MIN, KIT_ICON_LARGE, KitButton, RowState, SyncKind, current_row_fill,
    middle_truncate, row_fill, sync_badge,
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
fn text_ramp_tokens_include_p0_accessibility_corrections() {
    assert_eq!(
        Palette::T_PRIMARY,
        egui::Color32::from_rgb(0xdf, 0xe1, 0xe5)
    ); // branch names, body
    assert_eq!(
        Palette::T_SECONDARY,
        egui::Color32::from_rgb(0xb0, 0xb3, 0xbb)
    ); // section labels, secondary actions
    assert_eq!(Palette::T_MUTED, egui::Color32::from_rgb(0xae, 0xb2, 0xba)); // counts, timestamps, placeholders
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

/// The current branch is a fact about the repository, not about the pointer, so
/// its row owns a resting band. That band must not be mistakable for either
/// interaction state, and a current row that is also selected must not collapse
/// into one muddy third state.
#[test]
fn current_row_band_is_distinct_from_hover_and_selection() {
    let rest = current_row_fill(RowState::Default);
    assert_ne!(
        rest,
        row_fill(RowState::Hover),
        "current must not read as hover"
    );
    assert_ne!(
        rest,
        row_fill(RowState::Selected),
        "current must not read as selection"
    );
    assert_ne!(
        current_row_fill(RowState::Selected),
        rest,
        "current-and-selected must render differently from current alone"
    );
    assert_ne!(
        current_row_fill(RowState::Hover),
        rest,
        "hovering a current row must still answer"
    );
}

/// Every hand-painted row in the crate tracks selection and hover as two
/// booleans and calls [`RowState::from_flags`] to turn them into a state
/// (`log_window`, `settings_modal`, `welcome`, `interactive_rebase`,
/// `multi_selection`). So the mapping `from_flags` performs *is* what those five
/// surfaces paint, and it is pinned here: `selected` resolves to
/// [`RowState::BrandSelected`] — the solid brand band, not the §13
/// [`RowState::Selected`] band `row_fill_states_are_distinct` covers — and it
/// wins over `hovered`, because a selected row keeps its selection fill while
/// the pointer is on it. The two unselected neighbours resolve to the hover and
/// rest states that [`row_fill_states_are_distinct`] already names, so the
/// whole function is covered by assertion here rather than by leaving it to the
/// five call sites to get right implicitly.
#[test]
fn from_flags_maps_selection_onto_the_brand_band_and_keeps_hover_below_it() {
    // Selected wins over hover, in both pointer positions.
    assert_eq!(
        RowState::from_flags(true, false),
        RowState::BrandSelected,
        "a selected row is the shared tree/list brand band"
    );
    assert_eq!(
        RowState::from_flags(true, true),
        RowState::BrandSelected,
        "hovering a selected row must not demote it to the hover fill"
    );
    // The brand band is its own role, distinct from the §13 tool-window band.
    assert_ne!(
        RowState::from_flags(true, false),
        RowState::Selected,
        "from_flags paints the brand band, not the §13 selection band"
    );
    assert_ne!(
        RowState::from_flags(true, false),
        RowState::FocusSelected,
        "from_flags paints the brand band, not the translucent focus band"
    );
    // The two unselected neighbours.
    assert_eq!(
        RowState::from_flags(false, true),
        RowState::Hover,
        "an unselected hovered row is the hover state"
    );
    assert_eq!(
        RowState::from_flags(false, false),
        RowState::Default,
        "an untouched row is the default state"
    );

    // And the fills those states actually paint, so the mapping cannot be
    // re-pointed at a different selection role without failing here.
    assert_eq!(
        row_fill(RowState::from_flags(true, false)),
        Palette::BRAND,
        "the brand band is the solid BRAND fill"
    );
    assert_eq!(
        row_fill(RowState::from_flags(true, true)),
        Palette::BRAND,
        "selection wins over hover in the painted fill too"
    );
    assert_eq!(
        row_fill(RowState::from_flags(false, true)),
        Palette::SURFACE_2,
        "the unselected hover neighbour paints the surface-2 hover fill"
    );
    assert_eq!(
        row_fill(RowState::from_flags(false, false)),
        egui::Color32::TRANSPARENT,
        "the resting neighbour paints nothing"
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
fn section_label_is_uppercase_and_carries_no_count() {
    use turbogit_ui::ui::components::section_label;
    // The live count is a badge beside the label (§3.3), never characters
    // appended to it.
    assert_eq!(section_label("Local"), "LOCAL");
    assert_eq!(section_label("Remote"), "REMOTE");
    assert_eq!(section_label("Tags"), "TAGS");
}

// --- §14.3 sync badge ----------------------------------------------------------

/// A destructive action is separated at rest, not only by a divider.
#[test]
fn danger_rests_tinted_and_grows_stronger() {
    use turbogit_ui::ui::widgets::WidgetState::{Active, Disabled, Hovered, Idle};

    let rest = KitButton::Danger.fill(Idle);
    assert_ne!(rest, egui::Color32::TRANSPARENT, "Delete rests tinted");
    assert_ne!(
        KitButton::Danger.fill(Disabled),
        rest,
        "a disabled destructive action does not keep the resting tint"
    );
    for stronger in [
        KitButton::Danger.fill(Hovered),
        KitButton::Danger.fill(Active),
    ] {
        assert_ne!(stronger, rest, "hover and press stay distinct from rest");
    }
    assert_ne!(
        KitButton::Danger.fill(Hovered),
        KitButton::Danger.fill(Active),
        "hover and press are their own states"
    );
}

#[test]
fn sync_badge_contract_matches_design() {
    // The badge states the relationship in words, one entry per direction; the
    // arrow itself is the chip's icon, so it never duplicates it in the label.
    assert_eq!(
        sync_badge(2, 0, false),
        vec![(SyncKind::Ahead, "2 ahead".to_string())]
    );
    assert_eq!(
        sync_badge(0, 1, false),
        vec![(SyncKind::Behind, "1 behind".to_string())]
    );
    // Diverged is the pair, not one combined marker.
    assert_eq!(
        sync_badge(2, 1, false),
        vec![
            (SyncKind::Ahead, "2 ahead".to_string()),
            (SyncKind::Behind, "1 behind".to_string()),
        ]
    );
    // In sync says nothing — an empty list, not a label.
    assert_eq!(sync_badge(0, 0, false), vec![]);
    // A deleted upstream wins over any count.
    assert_eq!(
        sync_badge(3, 0, true),
        vec![(SyncKind::Gone, "gone".to_string())]
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
fn danger_button_keeps_red_ink_on_a_tinted_block() {
    assert_eq!(KitButton::Danger.ink(WidgetState::Idle), Palette::DANGER);
    assert_eq!(KitButton::Danger.ink(WidgetState::Hovered), Palette::DANGER);
    // Ticket 07: Delete no longer rests fully transparent — a destructive
    // action separated only by a divider was still not separated at rest.
    assert_ne!(
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
        BRANCH_ROW_H, CLICK_TARGET_MIN, KIT_ICON, SECTION_H, SIDE_PANEL_W, TOOLBAR_H,
    };
    assert_eq!(BRANCH_ROW_H, 30.0); // dense IDE list row
    assert_eq!(SECTION_H, 26.0); // Local / Remote / Tags header
    assert_eq!(TOOLBAR_H, 36.0); // branch toolbar: one centered row of controls
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

    // The section label paints on its own; the strip (accessible label "Local")
    // is the click target.
    let _ = harness.get_by_label("LOCAL");
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
