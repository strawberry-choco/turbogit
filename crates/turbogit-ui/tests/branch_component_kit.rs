use std::cell::Cell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use egui::{Color32, Pos2, Rect};
use egui_kittest::{Harness, kittest::Queryable as _};
use test_support::git_seed::{commit, git};
use test_support::harness::{
    filled_circles, filled_rects, painted_galleys, settle, shell_harness_over_unstyled,
    widget_harness,
};
use test_support::srcscan;
use turbogit_app::state::{AppState, TreeState};
use turbogit_domain::model::{Branch, BranchKind, Remote, Root, RootId, RootStatus, Upstream};
use turbogit_ui::theme::{Palette, RAIL_WIDTH};
use turbogit_ui::ui::branch_tree_view::{TreeProps, branch_tree};
use turbogit_ui::ui::branches_tree::build_branch_view;
use turbogit_ui::ui::components::{
    CLICK_TARGET_MIN, KIT_ICON_LARGE, KitButton, RowShell, RowState, SyncKind, current_row_fill,
    middle_truncate, row_fill, row_ink, row_shell, sync_badge,
};
use turbogit_ui::ui::widgets::WidgetState;

const ALL_ROW_STATES: [RowState; 4] = [
    RowState::Default,
    RowState::Hover,
    RowState::RowSelected,
    RowState::FocusSelected,
];

#[test]
fn surface_tokens_match_design_doc() {
    assert_eq!(
        Palette::CONTENT_BG,
        egui::Color32::from_rgb(0x23, 0x25, 0x29)
    );
    assert_eq!(Palette::RAISED, egui::Color32::from_rgb(0x2b, 0x2d, 0x30));
    assert_eq!(
        Palette::SELECTION,
        egui::Color32::from_rgb(0x2e, 0x43, 0x69)
    );
    assert_eq!(Palette::RULE_CONTENT, Palette::RAISED);
    assert_ne!(Palette::RULE_CONTENT, Palette::LINE);
}

#[test]
fn meaning_colors_match_design_doc() {
    assert_eq!(Palette::ACCENT, egui::Color32::from_rgb(0x35, 0x74, 0xf0));
    assert_eq!(Palette::AHEAD, egui::Color32::from_rgb(0x5f, 0xa8, 0x6c));
    assert_eq!(Palette::DANGER, egui::Color32::from_rgb(0xdb, 0x5c, 0x5c));
    assert_eq!(Palette::LINK, egui::Color32::from_rgb(0x74, 0xa3, 0xe8));
}

#[test]
fn text_ramp_tokens_include_p0_accessibility_corrections() {
    assert_eq!(
        Palette::T_PRIMARY,
        egui::Color32::from_rgb(0xdf, 0xe1, 0xe5)
    );
    assert_eq!(
        Palette::T_SECONDARY,
        egui::Color32::from_rgb(0xb0, 0xb3, 0xbb)
    );
    assert_eq!(Palette::T_MUTED, egui::Color32::from_rgb(0x8a, 0x8e, 0x96));
    assert_eq!(Palette::T_DIM, egui::Color32::from_rgb(0x6e, 0x72, 0x7a));
    assert_eq!(Palette::INK, Palette::T_PRIMARY);
    assert_eq!(Palette::INK_2, Palette::T_SECONDARY);
    assert_eq!(Palette::INK_3, Palette::T_MUTED);
    assert_eq!(Palette::INK_4, Palette::T_DIM);
}

#[test]
fn shape_and_type_tokens_match_design_doc() {
    assert_eq!(turbogit_ui::theme::CHIP_RADIUS, 3);
    assert_eq!(turbogit_ui::theme::CONTROL_RADIUS, 4);
    assert_eq!(turbogit_ui::theme::TYPE_SECTION, 9.0);
    assert_eq!(turbogit_ui::theme::TYPE_CHIP, 10.0);
    assert_eq!(turbogit_ui::theme::TYPE_CONTROL, 11.0);
    assert_eq!(turbogit_ui::theme::TYPE_BODY, 12.0);
    assert_eq!(turbogit_ui::theme::TYPE_DETAIL_TITLE, 13.0);
}

#[test]
fn row_fill_states_are_distinct() {
    let idle = row_fill(RowState::Default);
    let hovered = row_fill(RowState::Hover);
    let selected = row_fill(RowState::RowSelected);
    assert_eq!(
        idle,
        egui::Color32::TRANSPARENT,
        "default rows paint nothing"
    );
    assert_eq!(
        hovered,
        Palette::RAISED_ON_CARD,
        "hover steps up from the surface the list sits on"
    );
    assert_eq!(
        selected,
        Palette::ROW_SELECTED,
        "a chosen list row takes the selected-row fill"
    );
    assert!(
        idle != hovered && hovered != selected && idle != selected,
        "default / hover / selected must be visually distinct (§17)"
    );
}

#[test]
fn the_three_surviving_selection_roles_are_three_distinct_fills() {
    let list_row = row_fill(RowState::RowSelected);
    let current_ref = current_row_fill(RowState::RowSelected);
    let focus_band = row_fill(RowState::FocusSelected);
    println!("list row {list_row:?}, current ref {current_ref:?}, focus {focus_band:?}");

    assert_eq!(
        list_row,
        Palette::ROW_SELECTED,
        "a chosen list row takes the selected-row fill"
    );
    assert_eq!(
        current_ref,
        Palette::SELECTION,
        "a selected current row takes the current-ref band"
    );
    assert_eq!(
        focus_band,
        Palette::selection_bg(),
        "the log/sidebar focus band keeps the translucent composite"
    );

    assert_ne!(
        list_row, current_ref,
        "the list-row fill and the current-ref band are two strengths on purpose"
    );
    assert_ne!(
        list_row, focus_band,
        "the opaque list-row fill is not the translucent focus band"
    );
    assert_ne!(
        current_ref, focus_band,
        "the current-ref band is not the translucent focus band"
    );
}

#[test]
fn no_list_row_resolves_to_the_current_ref_token() {
    for state in ALL_ROW_STATES {
        assert_ne!(
            row_fill(state),
            Palette::SELECTION,
            "{state:?} is a list row's fill and must not be the current-ref band: \
             that token belongs to `current_row_fill`. A list row takes \
             ROW_SELECTED."
        );
    }
    assert_eq!(
        current_row_fill(RowState::RowSelected),
        Palette::SELECTION,
        "the current-ref treatment is the selected current row's band"
    );
    assert_ne!(
        Palette::SELECTION,
        Palette::ROW_SELECTED,
        "the current-ref band and the list-row fill are two strengths on purpose"
    );
}

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
        row_fill(RowState::RowSelected),
        "current must not read as selection"
    );
    assert_ne!(
        current_row_fill(RowState::RowSelected),
        rest,
        "current-and-selected must render differently from current alone"
    );
    assert_ne!(
        current_row_fill(RowState::Hover),
        rest,
        "hovering a current row must still answer"
    );
}

#[test]
fn from_flags_maps_selection_onto_the_list_row_band_and_keeps_hover_below_it() {
    assert_eq!(
        RowState::from_flags(true, false),
        RowState::RowSelected,
        "a selected row is the shared tree/list selection fill"
    );
    assert_eq!(
        RowState::from_flags(true, true),
        RowState::RowSelected,
        "hovering a selected row must not demote it to the hover fill"
    );
    assert_ne!(
        RowState::from_flags(true, false),
        RowState::FocusSelected,
        "from_flags paints the opaque list-row fill, not the translucent focus band"
    );
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

    assert_eq!(
        row_fill(RowState::from_flags(true, false)),
        Palette::ROW_SELECTED,
        "the list-row band is the opaque selected-row fill"
    );
    assert_eq!(
        row_fill(RowState::from_flags(true, true)),
        Palette::ROW_SELECTED,
        "selection wins over hover in the painted fill too"
    );
    assert_ne!(
        row_fill(RowState::from_flags(true, false)),
        Palette::BRAND,
        "the loudest blue in the app is not a row's selection fill: selecting a \
         row must not cost the user the ability to read it"
    );
    assert_eq!(
        row_fill(RowState::from_flags(false, true)),
        Palette::RAISED_ON_CARD,
        "the unselected hover neighbour paints the raised-on-card hover fill"
    );
    assert_eq!(
        row_fill(RowState::from_flags(false, false)),
        egui::Color32::TRANSPARENT,
        "the resting neighbour paints nothing"
    );
}

#[test]
fn stale_rows_dim_without_hiding() {
    use turbogit_ui::ui::components::row_ink;
    assert_eq!(row_ink(false), Palette::T_PRIMARY);
    assert_eq!(row_ink(true), Palette::T_MUTED);
}

const SELECTED_ROW: &str = "beta";
const PLAIN_ROW: &str = "gamma";
const CURRENT_ROW: &str = "alpha";
const UPSTREAM: &str = "origin/main";

fn fixture_now() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(1_780_315_200, 0).expect("valid now")
}

fn fresh_local(name: &str) -> Branch {
    Branch {
        name: name.to_string(),
        kind: BranchKind::Local,
        tracking: Some(Upstream::from_git_ref(UPSTREAM).expect("a valid git ref")),
        favorite: false,
        protected: false,
        exists: true,
        ahead: 0,
        behind: 0,
        gone: false,
        last_touched: Some(fixture_now() - chrono::Duration::days(5)),
        tip: None,
        remote: None,
    }
}

fn fixture_root() -> Root {
    Root {
        id: RootId(Arc::from(std::path::PathBuf::from("/alpha"))),
        path: std::path::PathBuf::from("/alpha"),
        remotes: vec![Remote {
            name: "origin".to_string(),
            fetch_url: None,
            push_url: None,
        }],
        branches: vec![
            fresh_local(CURRENT_ROW),
            fresh_local(SELECTED_ROW),
            fresh_local(PLAIN_ROW),
        ],
        current_branch: Some(CURRENT_ROW.to_string()),
        head: Some("abc".to_string()),
        status: RootStatus::default(),
    }
}

struct RowFixture {
    roots: Vec<Root>,
    tags: HashMap<RootId, Vec<(String, turbogit_domain::model::RefState)>>,
    tree: TreeState,
    now: chrono::DateTime<chrono::Utc>,
}

fn selected_row_harness() -> Harness<'static, RowFixture> {
    let tree = TreeState {
        show_remotes: false,
        selected: Some(SELECTED_ROW.to_string()),
        selected_root: Some(RootId(Arc::from(std::path::PathBuf::from("/alpha")))),
        ..TreeState::default()
    };
    branch_tree_harness(vec![fixture_root()], tree, "row_fill_fixture")
}

fn dirty_repo_header_harness() -> Harness<'static, RowFixture> {
    let mut dirty = fixture_root();
    dirty.id = RootId(Arc::from(std::path::PathBuf::from("/beta")));
    dirty.path = std::path::PathBuf::from("/beta");
    dirty.branches = vec![fresh_local("dirty-current"), fresh_local("dirty-feature")];
    dirty.current_branch = Some("dirty-current".to_string());
    dirty.status.changes = vec![turbogit_domain::model::Change {
        path: std::path::PathBuf::from("dirty.rs"),
        status: turbogit_domain::model::ChangeStatus::Modified,
        chunks: vec![],
        staged: false,
        unstaged: true,
        orig_path: None,
    }];

    let tree = TreeState {
        show_remotes: false,
        ..TreeState::default()
    };
    branch_tree_harness(vec![fixture_root(), dirty], tree, "state_text_fixture")
}

fn branch_tree_harness(
    roots: Vec<Root>,
    tree: TreeState,
    id_salt: &'static str,
) -> Harness<'static, RowFixture> {
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, fx: &mut RowFixture| {
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            let view = build_branch_view(
                &fx.roots,
                &fx.tags,
                &std::collections::HashMap::new(),
                &|_root: &RootId| false,
            );
            let props = TreeProps {
                view: &view,
                filter: "",
                repo_filter: None,
                tags_by_root: &fx.tags,
                multi_repo: fx.roots.len() > 1,
                busy: false,
                has_any_data: true,
                merge_in_progress: false,
                last_fetch: None,
                now: fx.now,
                allows_rename: false,
                allows_context_menu: false,
                id_salt,
                full_height: true,
                collapse_remotes_by_default: true,
            };
            branch_tree(ui, &props, &mut fx.tree);
        },
        RowFixture {
            roots,
            tags: HashMap::new(),
            tree,
            now: fixture_now(),
        },
    );
    harness.set_size(egui::vec2(720.0, 480.0));
    harness
}

fn selected_row_rect(harness: &Harness<'_, RowFixture>) -> Rect {
    row_rect_painted_with(harness, Palette::ROW_SELECTED, "the selected list row")
}

fn comparison_row_rect(harness: &Harness<'_, RowFixture>) -> Rect {
    row_rect_painted_with(
        harness,
        current_row_fill(RowState::Default),
        "the unselected comparison row (the current row)",
    )
}

fn row_rect_painted_with(harness: &Harness<'_, RowFixture>, fill: Color32, who: &str) -> Rect {
    let hits: Vec<Rect> = filled_rects(harness)
        .into_iter()
        .filter(|(_, painted)| *painted == fill)
        .map(|(rect, _)| rect)
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "expected exactly one row band for {who} in {fill:?}, found {hits:?}"
    );
    hits.into_iter().next().expect("one row band")
}

fn text_in_row(
    harness: &Harness<'_, RowFixture>,
    row: Rect,
    needle: &str,
) -> test_support::harness::PaintedGalley {
    let hits: Vec<_> = painted_galleys(harness)
        .into_iter()
        .filter(|g| g.text == needle && row.contains(g.pos))
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "expected exactly one `{needle}` inside the row {row:?}, found {} of \
         the frame's {} painted galleys",
        hits.len(),
        painted_galleys(harness).len()
    );
    hits.into_iter().next().expect("one hit")
}

#[test]
fn a_selected_list_row_paints_the_selected_row_fill_and_not_the_current_ref_band() {
    let mut harness = selected_row_harness();
    settle(&mut harness);

    let selected = selected_row_rect(&harness);
    let comparison = comparison_row_rect(&harness);
    println!("selected row {selected:?}, unselected row {comparison:?}");
    assert!(
        selected != comparison,
        "the selected row and the comparison row are different rows"
    );

    let painted: Vec<Color32> = filled_rects(&harness).into_iter().map(|(_, f)| f).collect();
    assert!(
        !painted.contains(&Palette::SELECTION),
        "no row band may paint the current-ref band: a selected list row's fill \
         is ROW_SELECTED, and SELECTION belongs to the current row's selected \
         band only"
    );
    assert!(
        !painted.contains(&Palette::selection_bg()),
        "the branch list is not a focus band surface: the translucent composite \
         belongs to the log table, the sidebar tree and blame"
    );
    let painted_rails = rails(&harness);
    let brand_fills: Vec<Rect> = filled_rects(&harness)
        .into_iter()
        .filter(|(_, fill)| *fill == Palette::BRAND)
        .map(|(rect, _)| rect)
        .collect();
    let not_a_rail: Vec<Rect> = brand_fills
        .iter()
        .copied()
        .filter(|rect| !painted_rails.contains(rect))
        .collect();
    assert!(
        not_a_rail.is_empty(),
        "the loudest blue in the app is a row's 2px leading rail and nothing else: \
         the branch tree painted {not_a_rail:?} in the brand, which is not the \
         token's {RAIL_WIDTH}px rail ({painted_rails:?} are). A brand fill behind \
         running text is a button's shape, and this pane has one primary action."
    );
    assert_eq!(
        painted_rails.len(),
        2,
        "the selected row and the current row each carry their leading rail: \
         {painted_rails:?}"
    );
    assert!(
        painted_rails.iter().any(|rail| {
            (rail.left() - selected.left()).abs() < 0.01
                && (rail.top() - selected.top()).abs() < 0.01
                && (rail.bottom() - selected.bottom()).abs() < 0.01
        }),
        "the selected row {selected:?} carries a rail at its own leading edge: \
         {painted_rails:?}"
    );
}

#[test]
fn a_selected_list_rows_text_origin_equals_an_unselected_rows() {
    let mut harness = selected_row_harness();
    settle(&mut harness);

    let selected = selected_row_rect(&harness);
    let comparison = comparison_row_rect(&harness);
    let selected_name = text_in_row(&harness, selected, SELECTED_ROW);
    let comparison_name = text_in_row(&harness, comparison, CURRENT_ROW);
    println!(
        "selected name at {:?} (row {:?}), unselected name at {:?} (row {:?})",
        selected_name.pos, selected, comparison_name.pos, comparison
    );

    assert_eq!(
        selected_name.pos.x - selected.left(),
        comparison_name.pos.x - comparison.left(),
        "selecting a row must not indent its content: the rail is paint, not padding"
    );
    assert_eq!(
        selected_name.pos.x, comparison_name.pos.x,
        "both rows share one name column, selected or not"
    );
    assert_eq!(
        selected.size(),
        comparison.size(),
        "selecting a row must not resize it"
    );
    let selected_meta = text_in_row(&harness, selected, UPSTREAM);
    let comparison_meta = text_in_row(&harness, comparison, UPSTREAM);
    assert_eq!(
        selected_meta.pos.x, comparison_meta.pos.x,
        "a selected row's metadata column does not move either"
    );
}

#[test]
fn selecting_a_row_does_not_invert_its_name_or_its_metadata() {
    let mut harness = selected_row_harness();
    settle(&mut harness);

    let selected = selected_row_rect(&harness);
    let comparison = comparison_row_rect(&harness);
    let selected_name = text_in_row(&harness, selected, SELECTED_ROW);
    let comparison_name = text_in_row(&harness, comparison, CURRENT_ROW);
    assert_eq!(
        selected_name.color, comparison_name.color,
        "a selected row's branch name wears the same ink as an unselected row's"
    );
    assert_eq!(
        selected_name.color,
        row_ink(false),
        "…which is the row's own ink, not a selection ink"
    );
    assert_ne!(
        selected_name.color,
        Palette::BRAND_INK,
        "selection does not flip text to the on-brand ink: that inversion is \
         what the opaque list-row fill replaced"
    );

    let selected_meta = text_in_row(&harness, selected, UPSTREAM);
    let comparison_meta = text_in_row(&harness, comparison, UPSTREAM);
    assert_eq!(
        selected_meta.color, comparison_meta.color,
        "a selected row's metadata wears the same ink as an unselected row's"
    );
    assert_eq!(
        selected_meta.color,
        Palette::T_MUTED,
        "the tracking line is the row's metadata ink, unchanged by selection"
    );
}

#[test]
fn the_row_shell_paints_the_handed_fill_at_the_handed_rect_and_nothing_else() {
    let mut harness = Harness::new_ui(|ui| {
        turbogit_ui::theme::configure_style(ui.ctx());
        turbogit_ui::theme::install_fonts(ui.ctx());
        egui::CentralPanel::default().show(ui, |ui| {
            egui::Grid::new("row_shell")
                .num_columns(1)
                .spacing(egui::vec2(0.0, 0.0))
                .show(ui, |ui| {
                    let (_, selected) =
                        ui.allocate_exact_size(egui::vec2(300.0, 28.0), egui::Sense::hover());
                    row_shell(ui, selected.rect, RowState::RowSelected, RowShell::Fill);
                    ui.end_row();
                    let (_, plain) =
                        ui.allocate_exact_size(egui::vec2(300.0, 28.0), egui::Sense::hover());
                    row_shell(ui, plain.rect, RowState::Hover, RowShell::Fill);
                    ui.end_row();
                });
        });
    });
    harness.set_size(egui::vec2(360.0, 120.0));
    settle(&mut harness);

    let rects = filled_rects(&harness);
    let selected = rects
        .iter()
        .find(|(_, f)| *f == Palette::ROW_SELECTED)
        .map(|(r, _)| *r)
        .expect("the shell paints the selected-row fill it was handed");
    let plain = rects
        .iter()
        .find(|(_, f)| *f == Palette::RAISED_ON_CARD)
        .map(|(r, _)| *r)
        .expect("the shell paints the raised-on-card fill it was handed");
    assert_eq!(selected.size(), plain.size(), "the shell adds no geometry");
    assert_eq!(
        selected.height(),
        28.0,
        "the row keeps the caller's height: the shell reserves nothing"
    );
}

#[test]
fn mid_operation_label_is_first_class() {
    use turbogit_ui::ui::components::mid_op_label;
    assert_eq!(mid_op_label("merging"), "merging…");
    assert_eq!(mid_op_label("rebasing"), "rebasing…");
}

const STATE_SEVERITY_COLORS: [(&str, Color32); 3] = [
    ("dirt / unpushed orange", Palette::COUNTER),
    ("ahead green", Palette::AHEAD),
    ("conflicted / diverged red", Palette::STATUS_DIVERGED),
];

const THE_NON_SEVERITY_STATE: (turbogit_ui::theme::RepoState, &str, Color32) = (
    turbogit_ui::theme::RepoState::Uninitialized,
    "work the user is owed, not a severity",
    Palette::INK_3,
);

/// Every repository state.
///
/// **A new variant is a failing test here, not a compile error, and this comment
/// used to claim the opposite.** The old note said "a new variant is a compile
/// error here rather than a silent gap", and that was false: a fixed-length
/// `[RepoState; 6]` is an ordinary array, so the seventh variant
/// (`Uninitialized`, which a real change landed) compiled silently and every
/// "all states" loop below quietly stopped covering it. Rust has no way to say
/// "this enum gained a variant" from inside a test that has to compile, no
/// reflection, no `variant_count` on stable, so the closure is checked the only
/// way it can be: [`every_repo_state_variant_is_enumerated_here`] reads the
/// variants out of `theme.rs`'s own declaration and compares them with the ones
/// written here, and a mismatch names the variant that arrived.
///
/// That is weaker than a compile error and it is the honest version of this
/// table: a *removed* variant is still a compile error (the constant names a
/// path that no longer exists), and an *added* one is a red suite.
const ALL_REPO_STATES: [turbogit_ui::theme::RepoState; 7] = [
    turbogit_ui::theme::RepoState::Clean,
    turbogit_ui::theme::RepoState::Dirty,
    turbogit_ui::theme::RepoState::Conflict,
    turbogit_ui::theme::RepoState::Diverged,
    turbogit_ui::theme::RepoState::Unpushed,
    turbogit_ui::theme::RepoState::Unpulled,
    turbogit_ui::theme::RepoState::Uninitialized,
];

fn declared_repo_state_variants() -> Vec<String> {
    let lines = srcscan::code_lines(Path::new(CRATE_SRC).join("theme.rs"));
    let start = lines
        .iter()
        .position(|line| line.trim_start().starts_with("pub enum RepoState"))
        .unwrap_or_else(|| panic!("theme.rs no longer declares `pub enum RepoState`"));
    let mut out = Vec::new();
    for line in &lines[start + 1..] {
        let trimmed = line.trim();
        if trimmed == "}" {
            break;
        }
        let looks_like_a_variant = !trimmed.is_empty()
            && !trimmed.starts_with('#')
            && trimmed
                .trim_end_matches(',')
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
            && trimmed.ends_with(',');
        if looks_like_a_variant {
            out.push(trimmed.trim_end_matches(',').to_owned());
        }
    }
    assert!(
        !out.is_empty(),
        "no variants parsed out of `pub enum RepoState`: the ratchet that keeps \
         ALL_REPO_STATES honest would otherwise pass on an empty set"
    );
    out
}

#[test]
fn every_repo_state_variant_is_enumerated_here() {
    let declared = declared_repo_state_variants();
    let enumerated: Vec<String> = ALL_REPO_STATES.iter().map(|s| format!("{s:?}")).collect();

    let missing: Vec<&String> = declared
        .iter()
        .filter(|name| !enumerated.contains(name))
        .collect();
    assert!(
        missing.is_empty(),
        "RepoState declares {missing:?} and this suite does not enumerate it, so \
         every \"all states\" rule below quietly stops covering it — which is \
         exactly what happened when `Uninitialized` landed. Add it to \
         ALL_REPO_STATES and say what colour it is: either a name in \
         STATE_SEVERITY_COLORS or a row in THE_NON_SEVERITY_STATE. Declared: \
         {declared:?}. Enumerated: {enumerated:?}."
    );

    let extra: Vec<&String> = enumerated
        .iter()
        .filter(|name| !declared.contains(name))
        .collect();
    assert!(
        extra.is_empty(),
        "this suite enumerates {extra:?}, which is not a `RepoState` variant \
         (declared: {declared:?}). A state that does not exist in the map cannot \
         be a chip colour, and keeping it in the list would make the sweeps look \
         broader than they are."
    );
}

#[test]
fn no_chip_in_the_set_is_a_repository_state() {
    use turbogit_ui::ui::widgets::{COUNT_CHIP_COLORS, CURRENT_CHIP_COLORS, REF_CHIP_COLORS};
    let chips = [
        ("ref chip", REF_CHIP_COLORS),
        ("current chip", CURRENT_CHIP_COLORS),
        ("count chip", COUNT_CHIP_COLORS),
    ];
    for (who, colors) in chips {
        for (state_name, state_color) in STATE_SEVERITY_COLORS {
            assert_ne!(
                colors.bg, state_color,
                "the {who} must not fill the {state_name}: `RepoState::color()` \
                 is for a coloured word or a leading dot, and a chip is for a ref \
                 or a number"
            );
            assert_ne!(colors.fg, state_color);
        }
    }
}

#[test]
fn repository_state_is_one_map_and_the_chip_set_holds_none_of_it() {
    use turbogit_ui::ui::widgets::{COUNT_CHIP_COLORS, CURRENT_CHIP_COLORS, REF_CHIP_COLORS};

    for state in ALL_REPO_STATES {
        if state == turbogit_ui::theme::RepoState::Clean {
            assert_eq!(
                state.words(),
                "",
                "{state:?} is the state that says nothing: its band paints nothing"
            );
        } else {
            assert!(
                !state.words().is_empty(),
                "{state:?} must have wording: a state nobody can read is not a state"
            );
        }
        println!("{state:?}: {:?} in {:?}", state.words(), state.color());
    }

    let (odd_state, odd_reason, odd_color) = THE_NON_SEVERITY_STATE;
    for state in ALL_REPO_STATES {
        let color = state.color();
        let in_severity = STATE_SEVERITY_COLORS
            .iter()
            .any(|(_, table_color)| *table_color == color);
        if state == odd_state {
            assert_eq!(
                color, odd_color,
                "{state:?} is the one state that is not a severity ({odd_reason}), \
                 so it wears the muted ink and nothing else. Give it a severity \
                 colour and the distinction this variant exists to draw is gone."
            );
            assert!(
                !in_severity,
                "{state:?} wears the muted ink, which must therefore NOT also be a \
                 severity tone: if it were, the exclusion above would be a comment \
                 rather than a decision"
            );
            continue;
        }
        assert!(
            in_severity,
            "{state:?} resolves to {color:?}, which is neither one of the three \
             mapped severities nor the one documented non-severity state. Name it \
             in STATE_SEVERITY_COLORS, or make it the exception — but say which, \
             here: this suite stops covering the map otherwise."
        );
    }
    let outside: Vec<turbogit_ui::theme::RepoState> = ALL_REPO_STATES
        .iter()
        .copied()
        .filter(|state| {
            !STATE_SEVERITY_COLORS
                .iter()
                .any(|(_, color)| *color == state.color())
        })
        .collect();
    assert_eq!(
        outside,
        vec![odd_state],
        "exactly one state wears a colour outside the severity table, and it is \
         {odd_state:?} ({odd_reason}). A second one is a second decision that has \
         not been made."
    );

    for (who, colors) in [
        ("ref chip", REF_CHIP_COLORS),
        ("current chip", CURRENT_CHIP_COLORS),
        ("count chip", COUNT_CHIP_COLORS),
    ] {
        for state in ALL_REPO_STATES {
            let color = state.color();
            assert_ne!(
                colors.bg, color,
                "the {who} must not fill {state:?}'s colour {color:?}: a \
                 repository state is coloured text or a leading dot, and a chip is \
                 for a ref or a number"
            );
            assert_ne!(
                colors.fg, color,
                "the {who} must not ink itself {state:?}'s colour either — the ink \
                 says what the text is, not what state the repository is in"
            );
        }
    }
}

#[test]
fn a_dirty_repository_reaches_the_branches_screen_as_words_never_as_a_chip_fill() {
    let mut harness = dirty_repo_header_harness();
    settle(&mut harness);

    let words = "dirty";
    let expected_ink = turbogit_ui::theme::RepoState::Dirty.color();
    assert_eq!(
        expected_ink,
        Palette::COUNTER,
        "this fixture's whole point is the reserved orange: a dirt count is one \
         of the two things that colour is for"
    );

    let hits: Vec<_> = painted_galleys(&harness)
        .into_iter()
        .filter(|g| g.text == words)
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "the header states the state in words exactly once, found {hits:#?}"
    );
    assert_eq!(
        hits[0].color, expected_ink,
        "the state word wears the one repository-state colour map's ink"
    );

    let dots: Vec<_> = filled_circles(&harness)
        .into_iter()
        .filter(|(_, _, fill)| *fill == expected_ink)
        .collect();
    assert_eq!(
        dots.len(),
        0,
        "the band states the state in words alone, with no leading dot: {dots:?}"
    );

    for (state_name, state_color) in STATE_SEVERITY_COLORS {
        let chip_fills: Vec<Rect> = filled_rects(&harness)
            .into_iter()
            .filter(|(_, fill)| *fill == state_color)
            .map(|(rect, _)| rect)
            .collect();
        assert!(
            chip_fills.is_empty(),
            "a state must never become a chip fill, and the {state_name} painted \
             as a fill at {chip_fills:?}: dirty / behind / up-to-date / \
             needs-update are coloured text or a leading dot"
        );
    }
}

#[test]
fn section_label_is_uppercase_and_carries_no_count() {
    use turbogit_ui::ui::components::section_label;
    assert_eq!(section_label("Local"), "LOCAL");
    assert_eq!(section_label("Remote"), "REMOTE");
    assert_eq!(section_label("Tags"), "TAGS");
}

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
    assert_eq!(
        sync_badge(2, 0, false),
        vec![(SyncKind::Ahead, "↑2".to_string())]
    );
    assert_eq!(
        sync_badge(0, 1, false),
        vec![(SyncKind::Behind, "↓1".to_string())]
    );
    assert_eq!(
        sync_badge(2, 1, false),
        vec![
            (SyncKind::Ahead, "↑2".to_string()),
            (SyncKind::Behind, "↓1".to_string()),
        ]
    );
    assert_eq!(sync_badge(0, 0, false), vec![]);
    assert_eq!(
        sync_badge(3, 0, true),
        vec![(SyncKind::Gone, "gone".to_string())]
    );
}

/// The face a sync badge is laid out in must own both of its glyphs. A badge is
/// arrow plus digit, and in the chrome chain the arrow only resolves through a
/// fallback cut, two typefaces inside a three-glyph mark. This pins the half
/// that does not depend on which fallbacks the chain happens to carry: the data
/// face covers both arrows itself. `a_current_row_keeps_its_state_marks` pins the
/// other half, that a painted badge really is monospace.
#[test]
fn the_data_face_owns_both_direction_arrows() {
    use turbogit_ui::theme::{TYPE_CHIP, data_font};

    let mut harness = Harness::new_ui_state(
        |ui, _| {
            turbogit_ui::theme::configure_style(ui.ctx());
            turbogit_ui::theme::install_fonts(ui.ctx());
        },
        (),
    );
    settle(&mut harness);
    // The frame has to actually render: the font atlas queues texture deltas for
    // the glyphs it lays out, and an unrendered context drops them on the floor.
    let _image = harness.render().expect("renderer produced no image");
    for arrow in ["↑", "↓"] {
        assert!(
            harness
                .ctx
                .fonts_mut(|f| f.has_glyphs(&data_font(TYPE_CHIP), arrow)),
            "{arrow} must be covered by the face the badge is painted in, or the \
             mark loses its direction and the row states nothing"
        );
    }
}

/// The band's summary paints its counts through the chrome words
/// (`state_words`), so the arrows ride whatever the proportional chain resolves
/// them through. That
/// chain changing is invisible to every text assertion, tofu still lays out as
/// text, so the coverage the band now depends on is pinned the way the data
/// face's is.
#[test]
fn the_chrome_chain_resolves_the_arrows_the_band_summary_needs() {
    use turbogit_ui::theme::{TYPE_CHIP, chrome_font};

    let mut harness = Harness::new_ui_state(
        |ui, _| {
            turbogit_ui::theme::configure_style(ui.ctx());
            turbogit_ui::theme::install_fonts(ui.ctx());
        },
        (),
    );
    settle(&mut harness);
    let _image = harness.render().expect("renderer produced no image");
    for arrow in ["↑", "↓"] {
        assert!(
            harness
                .ctx
                .fonts_mut(|f| f.has_glyphs(&chrome_font(TYPE_CHIP), arrow)),
            "{arrow} must resolve somewhere in the chrome chain, or the band's \
             summary paints a box where its count belongs"
        );
    }
}

#[test]
fn middle_truncate_keeps_identifying_ends() {
    assert_eq!(
        middle_truncate("feature/multi-root-executor", 22),
        "feature/multi…executor"
    );
    assert_eq!(middle_truncate("main", 22), "main");
    let t = middle_truncate("features", 4);
    assert!(
        t.starts_with("fe") && t.ends_with('s') && t.contains('…'),
        "got {t:?}"
    );
}

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

#[test]
fn geometry_constants_match_spec() {
    use turbogit_ui::ui::components::{
        BRANCH_ROW_H, CLICK_TARGET_MIN, KIT_ICON, SECTION_H, SIDE_PANEL_W, TOOLBAR_H,
    };
    assert_eq!(BRANCH_ROW_H, 30.0);
    assert_eq!(SECTION_H, 26.0);
    assert_eq!(TOOLBAR_H, 36.0);
    assert_eq!(SIDE_PANEL_W, 220.0);
    assert_eq!(KIT_ICON, 12.0);
    assert_eq!(KIT_ICON_LARGE, 13.0);
    assert_eq!(CLICK_TARGET_MIN, 24.0);
}

type ClickFlag = Rc<Cell<bool>>;

fn kit_harness(primary_clicked: ClickFlag, danger_clicked: ClickFlag) -> Harness<'static, ()> {
    use turbogit_ui::ui::components::{KitButton, kit_button, overflow_button, section_header};
    widget_harness(egui::vec2(600.0, 400.0), move |ui| {
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
    })
}

#[test]
fn kit_smoke_renders_and_targets_stay_clickable() {
    let primary = Rc::new(Cell::new(false));
    let danger = Rc::new(Cell::new(false));
    let mut harness = kit_harness(primary.clone(), danger.clone());
    harness.step();

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

const CRATE_SRC: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src");

const NOT_A_SCREEN: [&str; 5] = ["components", "widgets", "kit", "icons", "theme"];

fn screens_calling(name: &str) -> (Vec<String>, Vec<(String, usize)>) {
    let src_root = Path::new(CRATE_SRC);
    let mut sites: Vec<(String, String, usize)> = Vec::new();
    for (rel, line) in srcscan::call_sites(src_root, name) {
        let screen = rel
            .strip_prefix("ui/")
            .unwrap_or(&rel)
            .split('/')
            .next()
            .unwrap_or_default()
            .trim_end_matches(".rs")
            .to_owned();
        if NOT_A_SCREEN.contains(&screen.as_str()) {
            continue;
        }
        sites.push((screen, rel, line));
    }
    let mut screens: Vec<String> = sites.iter().map(|(screen, _, _)| screen.clone()).collect();
    screens.sort();
    screens.dedup();
    let where_ = sites
        .iter()
        .map(|(_, path, line)| (path.clone(), *line))
        .collect();
    (screens, where_)
}

fn rect_primitives(rel: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut pending: Option<(String, String)> = None;
    for line in srcscan::code_lines(Path::new(CRATE_SRC).join(rel)) {
        match pending.take() {
            Some((name, signature)) => {
                let signature = format!("{signature} {line}");
                if line.contains('{') {
                    if signature.contains("Rect") {
                        out.push(name);
                    }
                } else {
                    pending = Some((name, signature));
                }
            }
            None => {
                if let Some(rest) = line.trim_start().strip_prefix("pub fn ") {
                    let name = rest.split('(').next().unwrap_or_default().trim().to_owned();
                    if line.contains('{') {
                        if line.contains("Rect") {
                            out.push(name);
                        }
                    } else {
                        pending = Some((name, line.clone()));
                    }
                }
            }
        }
    }
    out
}

const ROW_PRIMITIVES: [(&str, &str, &[&str]); 2] = [
    (
        "paint_row",
        "ui/widgets/rows.rs",
        &[
            "commit_window",
            "diff",
            "interactive_rebase",
            "multi_selection",
            "settings_modal",
            "welcome",
        ],
    ),
    (
        "paint_rail",
        "ui/components.rs",
        &["branch_tree_view", "sidebar"],
    ),
];

const ROW_CONSTRUCTION_SITES: [&str; 1] = ["row_shell"];

#[test]
fn every_shared_row_primitive_is_used_by_at_least_two_screens() {
    assert_eq!(
        srcscan::declared_fns(&srcscan::read_source(
            Path::new(CRATE_SRC).join("ui/widgets/rows.rs"),
        )),
        vec!["paint_row".to_owned()],
        "the shared row module's primitives are enumerated in ROW_PRIMITIVES; a \
         primitive that is not two screens' worth of use does not live here"
    );

    let components = Path::new(CRATE_SRC).join("ui/components.rs");
    let free = |name: &str| {
        srcscan::code_lines(&components)
            .iter()
            .any(|line| line.starts_with(&format!("pub fn {name}(")))
    };
    assert!(
        !free("fill"),
        "`components::fill` is back: the row fill's construction site is \
         `row_shell`'s `Fill` arm, and a second spelling of it is the \
         two-opinion case the row vocabulary exists to prevent"
    );
    assert!(
        free("row_shell"),
        "`components::row_shell` must still be declared in ui/components.rs"
    );
    for (home, primitives) in [
        (
            "ui/widgets/rows.rs",
            srcscan::declared_fns(&srcscan::read_source(
                Path::new(CRATE_SRC).join("ui/widgets/rows.rs"),
            )),
        ),
        ("ui/components.rs", rect_primitives("ui/components.rs")),
    ] {
        for name in primitives {
            let declared = ROW_PRIMITIVES
                .iter()
                .any(|(primitive, _, _)| *primitive == name)
                || ROW_CONSTRUCTION_SITES.contains(&name.as_str());
            assert!(
                declared,
                "{home}::{name} paints into a row rect and is classified as \
                 neither. Either it is a shared row primitive and belongs in \
                 ROW_PRIMITIVES with the screens that call it, or it is a \
                 construction site reached through another spelling and belongs \
                 in ROW_CONSTRUCTION_SITES with the reason. A row painter with \
                 one caller does not live in the shared vocabulary."
            );
        }
    }

    for (name, home, screens) in ROW_PRIMITIVES {
        let homes: Vec<String> = srcscan::rust_files(Path::new(CRATE_SRC))
            .into_iter()
            .filter(|path| {
                srcscan::code_lines(path)
                    .iter()
                    .any(|line| srcscan::declares(line, name))
            })
            .map(|path| {
                path.strip_prefix(Path::new(CRATE_SRC))
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert_eq!(
            homes,
            vec![home.to_owned()],
            "`{name}` must be declared exactly once, in {home}"
        );

        let (found, sites) = screens_calling(name);
        assert!(
            found.len() >= 2,
            "`{name}` is called from {} screen(s) {found:?} (at {sites:?}). A \
             primitive with one caller does not live in the shared row module — it \
             belongs to that screen.",
            found.len()
        );
        assert_eq!(
            found,
            screens.to_vec(),
            "the screens calling `{name}` changed. The rule is about *screens*, so a \
             screen that adopts the primitive (or drops it) has to be written down \
             here; the call sites are {sites:?}."
        );
    }
}

#[test]
fn the_row_shell_paints_a_caller_allocated_rect_and_never_measures_one() {
    let src = srcscan::fn_code(
        &srcscan::read_source(Path::new(CRATE_SRC).join("ui/components.rs")),
        "row_shell",
    );

    assert!(
        src.contains("rect: egui::Rect"),
        "the row shell takes the caller's rect: {src}"
    );
    assert!(
        !src.contains("->"),
        "the row shell paints into a rect the caller already has; it hands \
         nothing back, because a shell that allocated a row would have to answer \
         with one: {src}"
    );
    for measure in [
        "allocate",
        "available_",
        "interact_size",
        "spacing()",
        "max_rect",
        "min_rect",
        "preferred_",
        "desired_",
        "layout_no_wrap",
        "FontId",
        "FontFamily",
        "text_style",
        "ui.add(",
        "measure",
    ] {
        assert!(
            !src.contains(measure),
            "the row shell must not measure, allocate or lay out — it paints into \
             the rect it is handed, which is what lets a virtualised list adopt it. \
             It names `{measure}`: {src}"
        );
    }
}

#[test]
fn the_rail_marks_exactly_the_two_selection_roles() {
    for (state, is_a_selection) in [
        (RowState::Default, false),
        (RowState::Hover, false),
        (RowState::RowSelected, true),
        (RowState::FocusSelected, true),
    ] {
        assert_eq!(
            state.is_selected(),
            is_a_selection,
            "{state:?} is {}the two selection roles the rail marks",
            if is_a_selection { "" } else { "not " }
        );
    }
}

fn shell_harness() -> Harness<'static, ()> {
    use turbogit_ui::ui::components::{RowShell, row_shell};
    let mut harness = widget_harness(egui::vec2(400.0, 400.0), move |ui| {
        for (index, (state, variant)) in [
            (RowState::FocusSelected, RowShell::Railed),
            (RowState::RowSelected, RowShell::Railed),
            (RowState::FocusSelected, RowShell::Fill),
            (RowState::Hover, RowShell::Railed),
        ]
        .into_iter()
        .enumerate()
        {
            let rect = egui::Rect::from_min_size(
                egui::pos2(20.0, 20.0 + index as f32 * 40.0),
                egui::vec2(300.0, 30.0),
            );
            row_shell(ui, rect, state, variant);
        }
    });
    harness.step();
    harness
}

#[test]
fn the_row_shells_railed_variant_adds_the_one_rail_and_its_fill_variant_never_does() {
    let harness = shell_harness();
    let rect = |index: usize| {
        egui::Rect::from_min_size(
            egui::pos2(20.0, 20.0 + index as f32 * 40.0),
            egui::vec2(300.0, 30.0),
        )
    };
    let (focus_railed, row_railed, focus_filled, hover_railed) =
        (rect(0), rect(1), rect(2), rect(3));

    for (row, state) in [
        (focus_railed, RowState::FocusSelected),
        (row_railed, RowState::RowSelected),
        (focus_filled, RowState::FocusSelected),
        (hover_railed, RowState::Hover),
    ] {
        let band = filled_with(&harness, row_fill(state))
            .into_iter()
            .find(|rect| *rect == row)
            .unwrap_or_else(|| {
                panic!(
                    "the shell must paint {:?}'s own rect {row:?}, not a measured one",
                    row_fill(state)
                )
            });
        assert_eq!(band, row);
    }

    let painted_rails = rails(&harness);
    assert_eq!(
        painted_rails.len(),
        2,
        "exactly the two chosen rows carry a rail: {painted_rails:?}"
    );
    for row in [focus_railed, row_railed] {
        let rail = painted_rails
            .iter()
            .find(|rail| rail.top() == row.top())
            .unwrap_or_else(|| panic!("the chosen row at {row:?} carries no rail"));
        assert_eq!(rail.left(), row.left(), "the rail is at the leading edge");
        assert_eq!(rail.width(), RAIL_WIDTH, "the rail is the token's width");
        assert_eq!(rail.height(), row.height(), "the rail spans the row");
    }
    for row in [focus_filled, hover_railed] {
        assert!(
            !painted_rails.iter().any(|rail| row.contains_rect(*rail)),
            "a row that is not chosen carries no rail, whichever variant painted \
             it: {row:?} vs {painted_rails:?}"
        );
    }
}

#[test]
fn the_rail_width_is_defined_in_the_token_layer_and_consumed_by_one_painter() {
    assert_eq!(RAIL_WIDTH, 2.0, "the rail width dimension");
    let theme = srcscan::code_lines(Path::new(CRATE_SRC).join("theme.rs"));
    assert_eq!(
        theme
            .iter()
            .filter(|line| line.contains("pub const RAIL_WIDTH"))
            .count(),
        1,
        "the rail width is defined in the token layer and nowhere else"
    );
    assert_eq!(
        theme
            .iter()
            .filter(|line| line.contains("pub const MARK_RADIUS"))
            .count(),
        1,
        "the 2px *radius* role stays its own declaration"
    );

    let naming: Vec<String> = srcscan::rust_files(Path::new(CRATE_SRC))
        .into_iter()
        .filter(|path| {
            srcscan::code_lines(path)
                .iter()
                .any(|line| line.contains("RAIL_WIDTH"))
        })
        .map(|path| {
            path.file_name()
                .expect("a file name")
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    let mut expected = vec![
        "components.rs".to_owned(),
        "tabs.rs".to_owned(),
        "theme.rs".to_owned(),
    ];
    let mut found = naming.clone();
    expected.sort();
    found.sort();
    assert_eq!(
        found, expected,
        "exactly three files may name RAIL_WIDTH: theme.rs, which defines it; \
         components.rs, the one rail painter that consumes it; and tabs.rs, whose \
         tab underline is the accent's other 2px job and reads the same width \
         dimension. A fourth is a second definition of the width under another \
         name — or a rail that grew one."
    );
    let tabs = srcscan::code_lines(Path::new(CRATE_SRC).join("ui/widgets/tabs.rs"));
    assert!(
        tabs.iter()
            .any(|line| line.contains("crate::theme::RAIL_WIDTH")),
        "the tab underline must take its width from theme::RAIL_WIDTH: two 2px \
         accents that are one decision cannot be two literals"
    );
}

#[test]
fn no_surface_paints_an_accent_rail_at_a_hard_coded_width() {
    let mut offenders: Vec<String> = Vec::new();
    for path in srcscan::rust_files(Path::new(CRATE_SRC).join("ui")) {
        let lines = srcscan::code_lines(&path);
        let rel = path
            .strip_prefix(Path::new(CRATE_SRC))
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let mut statement = String::new();
        let mut start = 1usize;
        for (index, line) in lines.iter().enumerate() {
            if statement.is_empty() {
                start = index + 1;
            }
            statement.push_str(line);
            statement.push('\n');
            if !line.trim_end().ends_with(';') && !line.trim_end().ends_with('}') {
                continue;
            }
            let compact: String = statement.split_whitespace().collect::<Vec<_>>().join(" ");
            let dense = compact.replace(' ', "");
            let brands = dense.contains("Palette::BRAND,")
                || dense.contains("Palette::BRAND)")
                || dense.contains("Palette::ACCENT,")
                || dense.contains("Palette::ACCENT)");
            let hard_coded_2px = ["(2.0", ",2.0", "=2.0"].iter().any(|p| dense.contains(p));
            if brands && hard_coded_2px && !dense.contains("rect_stroke") {
                offenders.push(format!("{rel}:{start}: {compact}"));
            }
            statement.clear();
        }
    }
    assert!(
        offenders.is_empty(),
        "these sites paint a brand 2px bar from a literal instead of going through \
         components::paint_rail + theme::RAIL_WIDTH: {offenders:#?}"
    );
}

fn temp_repo(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    std::fs::create_dir_all(&path).expect("repo dir");
    git(&path, &["init", "-q", "-b", "main"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    commit(&path, "base.txt", "base\n", "init");
    path
}

/// A two-repository workspace in a fresh temp dir: `alpha` and `beta`, each a
/// git repository, side by side under the project root.
///
/// A `TempDir` rather than a fixed shared scratch path on purpose: a shared
/// path is a `config.lock` race rather than a test.
fn two_repo_workspace() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("temp workspace");
    let alpha = temp_repo(dir.path(), "alpha");
    let beta = temp_repo(dir.path(), "beta");
    (dir, alpha, beta)
}

fn focused_sidebar() -> (Harness<'static, AppState>, tempfile::TempDir) {
    let (dir, alpha, beta) = two_repo_workspace();
    let state = AppState::for_roots(dir.path(), &[alpha, beta]);
    let mut harness = shell_harness_over_unstyled(state, egui::vec2(1280.0, 800.0), 1024);
    settle(&mut harness);
    harness.get_by_label("beta").click();
    settle(&mut harness);
    (harness, dir)
}

fn filled_with<S>(harness: &Harness<'_, S>, color: Color32) -> Vec<Rect> {
    filled_rects(harness)
        .into_iter()
        .filter(|(_, painted)| *painted == color)
        .map(|(rect, _)| rect)
        .collect()
}

fn focus_bands(harness: &Harness<'_, AppState>, row: Rect) -> Vec<Rect> {
    filled_with(harness, row_fill(RowState::FocusSelected))
        .into_iter()
        .filter(|band| band.intersect(row) == *band)
        .collect()
}

fn rails<S>(harness: &Harness<'_, S>) -> Vec<Rect> {
    filled_with(harness, Palette::BRAND)
        .into_iter()
        .filter(|rect| (rect.width() - RAIL_WIDTH).abs() < 0.01)
        .collect()
}

fn label_in_row(
    harness: &Harness<'_, AppState>,
    row: Rect,
    text: &str,
) -> test_support::harness::PaintedGalley {
    let hits: Vec<_> = painted_galleys(harness)
        .into_iter()
        .filter(|g| g.text == text && row.contains(g.pos))
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "expected exactly one `{text}` inside {row:?}, found {hits:#?}"
    );
    hits.into_iter().next().expect("one hit")
}

fn fills_in_row(harness: &Harness<'_, AppState>, row: Rect) -> Vec<Color32> {
    filled_rects(harness)
        .into_iter()
        .filter(|(rect, _)| row.contains_rect(*rect))
        .map(|(_, color)| color)
        .collect()
}

#[test]
fn a_selected_row_paints_exactly_one_rail_of_the_token_width_at_its_leading_edge() {
    let (harness, _workspace) = focused_sidebar();

    let row = harness.get_by_label("beta").rect();
    let bands = focus_bands(&harness, row);
    assert_eq!(
        bands.len(),
        1,
        "one focused row paints one band; found {bands:?}"
    );
    let band = bands[0];
    assert!(
        row.intersect(band) == band,
        "the focus band must sit inside the focused row: row {row:?}, band {band:?}"
    );

    let painted_rails = rails(&harness);
    assert_eq!(
        painted_rails.len(),
        1,
        "the one selected row carries exactly one rail: {painted_rails:?}"
    );
    let rail = painted_rails[0];
    println!("focus band {band:?}, rail {rail:?}");
    assert_eq!(
        rail.width(),
        RAIL_WIDTH,
        "the rail is the token's width, not a literal at the call site"
    );
    assert_eq!(
        rail.height(),
        band.height(),
        "the rail spans the row's full height"
    );
    assert_eq!(
        rail.left(),
        band.left(),
        "the rail sits at the row's leading edge"
    );
    assert_eq!(rail.top(), band.top(), "the rail starts at the row's top");
    assert_eq!(
        rail.right(),
        band.left() + RAIL_WIDTH,
        "the rail is RAIL_WIDTH wide and no more"
    );

    let plain = harness.get_by_label("alpha").rect();
    for other in rails(&harness) {
        assert!(
            !plain.contains_rect(other),
            "an unselected row must not carry a rail: {other:?}"
        );
    }
}

#[test]
fn a_selected_row_indents_neither_its_text_nor_its_band() {
    let (harness, _workspace) = focused_sidebar();
    let selected = harness.get_by_label("beta").rect();
    let plain = harness.get_by_label("alpha").rect();
    let band = focus_bands(&harness, selected)
        .into_iter()
        .next()
        .expect("the focused row paints its band");
    let selected_name = label_in_row(&harness, selected, "beta");
    let plain_name = label_in_row(&harness, plain, "alpha");
    println!(
        "selected name at {:?} (row {selected:?}), unselected name at {:?} (row {plain:?}), band {band:?}",
        selected_name.pos, plain_name.pos,
    );

    assert_eq!(
        selected_name.pos.x - selected.left(),
        plain_name.pos.x - plain.left(),
        "selecting a row must not indent its content: the rail is paint, not padding"
    );
    assert_eq!(
        selected_name.pos.x, plain_name.pos.x,
        "both rows share one name column, selected or not"
    );
    assert_eq!(
        band.left(),
        plain.left(),
        "the selected row's band starts at the row's leading edge: a rail taken out \
         of the content region would indent the selected row and not the \
         unselected one"
    );
    assert_eq!(
        selected.size(),
        plain.size(),
        "selecting a row must not resize it"
    );
}

#[test]
fn every_list_row_fill_comes_from_the_one_row_fill_decision() {
    let (mut harness, _workspace) = focused_sidebar();
    let selected = harness.get_by_label("beta").rect();
    let plain = harness.get_by_label("alpha").rect();

    assert_eq!(
        row_fill(RowState::Default),
        Color32::TRANSPARENT,
        "a resting row's decision is to paint nothing"
    );
    let resting = fills_in_row(&harness, plain);
    for (name, state) in [
        ("hover", RowState::Hover),
        ("row-selected", RowState::RowSelected),
        ("focus-selected", RowState::FocusSelected),
    ] {
        assert!(
            !resting.contains(&row_fill(state)),
            "an unselected, unhovered row paints no {name} band: {resting:?}"
        );
    }

    let selected_painted = fills_in_row(&harness, selected);
    assert!(
        selected_painted.contains(&row_fill(RowState::FocusSelected)),
        "the selected row's band is the one row-fill decision's focus fill ({:?}); \
         the row painted {selected_painted:?}",
        row_fill(RowState::FocusSelected)
    );

    harness.get_by_label("alpha").hover();
    settle(&mut harness);
    let hovered = fills_in_row(&harness, plain);
    assert!(
        hovered.contains(&row_fill(RowState::Hover)),
        "a hovered row's fill is the one decision's hover fill ({:?}), the \
         raised-on-card role; the row painted {hovered:?}",
        row_fill(RowState::Hover)
    );
    assert_eq!(
        row_fill(RowState::Hover),
        Palette::RAISED_ON_CARD,
        "hover steps up from the surface the list sits on"
    );
    let still_selected = fills_in_row(&harness, selected);
    assert!(
        still_selected.contains(&row_fill(RowState::FocusSelected)),
        "hovering a row's neighbour must not change the selected row's fill: \
         {still_selected:?}"
    );
}

#[test]
fn a_standard_list_row_takes_its_named_row_height() {
    {
        let (harness, _workspace) = focused_sidebar();
        for label in ["alpha", "beta"] {
            let rect = harness.get_by_label(label).rect();
            assert_eq!(
                rect.height(),
                turbogit_ui::theme::GROUP_ROW_HEIGHT,
                "the `{label}` row is a repository row, so it is GROUP_ROW_HEIGHT \
                 tall — a standard row takes the named height for its kind, and \
                 selection does not resize it"
            );
        }
    }

    let mut harness = selected_row_harness();
    settle(&mut harness);
    let selected = selected_row_rect(&harness);
    assert_eq!(
        selected.height(),
        turbogit_ui::ui::components::BRANCH_ROW_H,
        "the branch list row is the named branch row height, on a selected row \
         just as on a resting one"
    );
}

#[test]
fn a_section_label_is_ink_3_and_never_the_dim_ink() {
    let mut harness = selected_row_harness();
    settle(&mut harness);

    let mut labels: Vec<test_support::harness::PaintedGalley> = painted_galleys(&harness)
        .into_iter()
        .filter(|g| g.text == "LOCAL" || g.text == "TAGS")
        .collect();
    assert!(
        !labels.is_empty(),
        "the branch tree paints its section labels, or this ratchet is asserting \
         nothing: the frame painted {:#?}",
        painted_galleys(&harness)
    );
    labels.sort_by(|a, b| a.pos.y.total_cmp(&b.pos.y));
    for label in &labels {
        println!("section label `{}` in {:?}", label.text, label.color);
        assert_ne!(
            label.color,
            Palette::INK_4,
            "9px is normal-size text and `INK_4` is 3.2:1 with no legal surface \
             in the palette. The target frames were drawn with the dim value \
             before the contrast was checked, so a reader comparing the code with \
             the frame finds a test disagreeing with them — and the disagreement \
             is the point."
        );
        assert_ne!(
            label.color,
            Palette::T_DIM,
            "…and the two spellings of the dim step stay distinct, or this \
             ratchet would be checking a name rather than a value"
        );
        assert_eq!(
            label.color,
            Palette::INK_2,
            "the branches tree's section label reads at `INK_2` (see the comment \
             above: one step above R3's role table, which names `INK_3` for \
             section labels). If it moved, move it on purpose and say why here."
        );
    }
    for label in &labels {
        assert!(
            is_a_label_bearing_step(label.color),
            "a section label is chrome, so its ink is a ramp step: `{}` painted \
             {:?}, which is neither `INK`, `INK_2` nor `INK_3`",
            label.text,
            label.color
        );
    }
    assert_ne!(
        Palette::INK_3,
        Palette::INK_4,
        "if the muted and dim steps were merged, this ratchet would stop meaning \
         anything"
    );
}

fn is_a_label_bearing_step(ink: Color32) -> bool {
    [Palette::INK, Palette::INK_2, Palette::INK_3].contains(&ink)
}

#[test]
fn the_sidebar_active_band_is_the_focus_role_and_the_two_fills_are_one_step_apart() {
    use turbogit_ui::theme::Palette;
    use turbogit_ui::ui::components::{RowState, row_fill};

    let (harness, _workspace) = focused_sidebar();
    let row = harness.get_by_label("beta").rect();
    let bands = focus_bands(&harness, row);
    assert_eq!(
        bands.len(),
        1,
        "one focused row paints one band; found {bands:?}"
    );
    let fills = fills_in_row(&harness, row);
    assert!(
        fills.contains(&row_fill(RowState::FocusSelected)),
        "the sidebar's active band is the one row-fill decision's **focus** fill \
         ({:?}); the row painted {fills:?}",
        row_fill(RowState::FocusSelected)
    );
    assert!(
        !fills.contains(&Palette::ROW_SELECTED),
        "…and it is not the list row's opaque `ROW_SELECTED` ({:?}): a full-bleed \
         tree selection spanning a workspace tree is not a row in a list, and R4's \
         composite-plus-rail belongs to rows in lists. Changing this is a decision \
         with a reason, and the reason is written down in `ui/sidebar.rs` and in \
         `docs/design-system-roles.md`. The row painted {fills:?}",
        Palette::ROW_SELECTED
    );

    let focus = row_fill(RowState::FocusSelected);
    assert!(
        !focus.is_opaque(),
        "the focus band is a composite; a call that handed this an opaque fill \
         would silently make the measurement below vacuous"
    );
    let alpha = 1.0 - f64::from(focus.a()) / 255.0;
    for (host_name, host, tolerance) in [
        (
            "the app background (the log table's host)",
            Palette::BG,
            1.0_f64,
        ),
        ("the left rail's own surface", Palette::SIDEBAR, 4.0_f64),
    ] {
        let deltas: Vec<f64> = vec![
            (f64::from(focus.r()) + f64::from(host.r()) * alpha
                - f64::from(Palette::ROW_SELECTED.r()))
            .abs(),
            (f64::from(focus.g()) + f64::from(host.g()) * alpha
                - f64::from(Palette::ROW_SELECTED.g()))
            .abs(),
            (f64::from(focus.b()) + f64::from(host.b()) * alpha
                - f64::from(Palette::ROW_SELECTED.b()))
            .abs(),
        ];
        println!(
            "focus band {focus:?} over {host_name}: composite \
             #{:02X?} against the list row's #{:02X?} (deltas {deltas:?})",
            (
                (f64::from(focus.r()) + f64::from(host.r()) * alpha) as u8,
                (f64::from(focus.g()) + f64::from(host.g()) * alpha) as u8,
                (f64::from(focus.b()) + f64::from(host.b()) * alpha) as u8,
            ),
            (
                Palette::ROW_SELECTED.r(),
                Palette::ROW_SELECTED.g(),
                Palette::ROW_SELECTED.b(),
            ),
        );
        assert!(
            deltas.iter().all(|d| *d <= tolerance),
            "the focus band and the list-row fill are within a rounding step of \
             each other on {host_name} (deltas {deltas:?} per channel, tolerance \
             {tolerance}). That is the measurement behind keeping the focus band \
             on a tree selection: the swap would change which role the band claims \
             and nothing the user can see. If a future change makes the two \
             visibly different, this is the assertion that has to be re-decided — \
             and the re-decision has to be about the band, not about the number."
        );
    }
}

#[test]
fn the_mark_pair_paints_a_dot_and_words_and_borrows_no_chip_geometry() {
    let components = Path::new(CRATE_SRC).join("ui/components.rs");
    let src = srcscan::fn_code(&srcscan::read_source(&components), "mark");

    assert!(
        !src.contains("rect_filled"),
        "the mark pair paints no fill rect: it is a dot and a word, and a rect \
         behind them is a chip. {src}"
    );
    assert!(
        !src.contains("ChipGeometry") && !src.contains("CHIP_GEOMETRY"),
        "the mark pair borrows no chip geometry — no radius, no chip height, no \
         chip body. It reserves a band to centre a dot in, and that is all. {src}"
    );
    for own_map in [
        "Palette::COUNTER",
        "Palette::AHEAD",
        "Palette::STATUS_DIVERGED",
    ] {
        assert!(
            !src.contains(own_map),
            "the mark pair holds no state vocabulary of its own — it must not \
             name `{own_map}`. The colour arrives from the one \
             `RepoState::color()` map at the call site. {src}"
        );
    }
    let dot = srcscan::fn_code(&srcscan::read_source(&components), "mark");
    assert!(
        dot.contains("circle_filled"),
        "the mark's leading dot is a filled circle and nothing else: {dot}"
    );
    assert!(
        !dot.contains("Palette::"),
        "the dot takes its colour as an argument; a literal here would be a \
         second state map. {dot}"
    );

    let components = Path::new(CRATE_SRC).join("ui/components.rs");
    assert!(
        !srcscan::code_lines(&components)
            .iter()
            .any(|line| line.starts_with("pub fn state_dot(")),
        "`components::state_dot` is back: the dot's one caller is `state_mark`, \
         and a one-caller forwarder over `Painter::circle_filled` is a spelling \
         of the primitive rather than a role"
    );

    let mut harness = Harness::new_ui_state(
        |ui, _| {
            turbogit_ui::theme::configure_style(ui.ctx());
            turbogit_ui::theme::install_fonts(ui.ctx());
            egui::CentralPanel::default().show(ui, |ui| {
                turbogit_ui::ui::components::state_mark(
                    ui,
                    "unpulled commits",
                    turbogit_ui::theme::RepoState::Unpulled.color(),
                );
            });
        },
        (),
    );
    settle(&mut harness);
    let dots: Vec<Pos2> = filled_circles(&harness)
        .into_iter()
        .map(|(center, _radius, _fill)| center)
        .collect();
    assert_eq!(dots.len(), 1, "the mark pair paints one dot: {dots:?}");
    let words: Vec<String> = painted_galleys(&harness)
        .into_iter()
        .map(|g| g.text.clone())
        .collect();
    assert_eq!(
        words,
        vec!["unpulled commits".to_owned()],
        "…and the state in words, with no chip body behind either of them: {words:?}"
    );
}

#[test]
fn the_branch_view_builder_is_git_free_styling_free_and_egui_free() {
    let src = srcscan::code_lines(Path::new(CRATE_SRC).join("ui/branches_tree.rs"));

    for (index, line) in src.iter().enumerate() {
        for banned in ["egui", "Ui", "Painter", "Color32", "RichText", "Visuals"] {
            assert!(
                !line.contains(banned),
                "the pure branch view builder names `{banned}` at line {}: it \
                 builds a view model from a view model, and a styling reference \
                 here is a second place a colour, a font or a radius gets \
                 decided. Draw from the model, do not decide here.",
                index + 1
            );
        }
    }
    for banned in ["theme", "Palette", "components", "widgets"] {
        assert!(
            !src.iter()
                .any(|line| line.trim_start().starts_with(&format!("use {banned}"))),
            "the pure branch view builder imports `{banned}`: the tree builder is \
             the one place in the branches feature that decides nothing about \
             how anything looks, and an import is where that would start."
        );
    }
    for banned in ["std::process", "Command::", "git2", "Repository"] {
        assert!(
            !src.iter().any(|line| line.contains(banned)),
            "the pure branch view builder reaches for git (`{banned}`): the \
             fixture above hands it a finished view model, and a read here would \
             make the branches screen's tree depend on a subprocess"
        );
    }
    let code = src.join("\n");
    for expected in [
        "pub fn build_branch_view(",
        "roots: &[Root]",
        "tags_by_root:",
        "show_remotes:",
        ") -> BranchView {",
    ] {
        assert!(
            code.contains(expected),
            "the pure branch view builder must still declare `build_branch_view` \
             taking the view model and returning a view model — this suite renders \
             every painted ratchet above through it, so a signature change is the \
             one thing that would make them vacuous. Missing `{expected}`."
        );
    }
}

#[test]
fn the_count_helper_owns_the_noun_pair_and_the_six_former_bypasses() {
    let code = srcscan::fn_code(
        &srcscan::read_source(Path::new(CRATE_SRC).join("ui/components.rs")),
        "counted",
    );
    let params: Vec<&str> = code
        .split_once('(')
        .expect("counted's declaration carries a parameter list")
        .1
        .split(')')
        .next()
        .expect("the parameter list is closed")
        .split(',')
        .filter(|p| !p.trim().is_empty())
        .collect();
    assert_eq!(
        params.len(),
        3,
        "`counted` must take the singular and the plural, got {params:?}"
    );

    use turbogit_ui::ui::components::counted;
    assert_eq!(counted(1, "repo", "repos"), "1 repo");
    assert_eq!(counted(2, "repo", "repos"), "2 repos");
    assert_eq!(counted(0, "branch", "branches"), "0 branches");
}

#[test]
fn no_file_under_ui_answers_pluralisation_for_itself() {
    fn suffix_pluraliser_lines(lines: &[String]) -> Vec<usize> {
        let mut hits = Vec::new();
        for (index, line) in lines.iter().enumerate() {
            if !line.contains("== 1 {") {
                continue;
            }
            let mut arms = String::new();
            for follower in &lines[index..] {
                arms.push_str(follower);
                arms.push('\n');
                if arms.matches('{').count() <= arms.matches('}').count() {
                    break;
                }
            }
            if arms.contains("} else {") {
                hits.push(index + 1);
            }
        }
        hits
    }

    let src_root = Path::new(CRATE_SRC).join("ui");
    let offenders: Vec<(String, Vec<usize>)> = srcscan::rust_files(&src_root)
        .into_iter()
        .filter_map(|path| {
            let hits = suffix_pluraliser_lines(&srcscan::code_lines(&path));
            (!hits.is_empty()).then(|| {
                (
                    path.file_name().unwrap().to_string_lossy().into_owned(),
                    hits,
                )
            })
        })
        .collect();
    let keeps: [&str; 2] = ["branch_divergence.rs", "components.rs"];
    let survivors: Vec<&(String, Vec<usize>)> = offenders
        .iter()
        .filter(|(file, _)| !keeps.contains(&file.as_str()))
        .collect();
    assert!(
        survivors.is_empty(),
        "a module under src/ui/ answers pluralisation for itself: {survivors:?}. \
         The two allowed keepers are branch_divergence.rs, whose `repositories` \
         helper builds a bare noun with no count — which `counted` does not \
         return — for its rollback-reason sentence, and components.rs, which is \
         where `counted`'s own `if n == 1` lives."
    );

    let divergence = srcscan::code_lines(Path::new(CRATE_SRC).join("ui/branch_divergence.rs"));
    let suffix: Vec<usize> = suffix_pluraliser_lines(&divergence);
    assert_eq!(
        suffix.len(),
        1,
        "branch_divergence.rs carries more than its recorded `repositories` keep: \
         the suffix pluraliser lines are {suffix:?}"
    );
}

#[test]
fn the_shell_has_no_private_alias_for_the_edge_painter() {
    let shell = srcscan::read_source(Path::new(CRATE_SRC).join("ui/shell.rs"));
    let items = srcscan::top_level_items(&shell);
    assert!(
        !items.iter().any(|item| item.name == "paint_edge_line"),
        "`shell::paint_edge_line` is back: a private wrapper over the shared \
         painter with one caller is a second spelling of the call, and the \
         half-pixel offset it forwards is exactly what a shell-local copy \
         eventually loses"
    );

    let calls: Vec<usize> = shell
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains("widgets::edge_rule("))
        .map(|(index, _)| index + 1)
        .collect();
    assert_eq!(
        calls.len(),
        1,
        "the shell must call `widgets::edge_rule` exactly once, direct. \
         Found {calls:?} — two is a second edge spelling creeping back in, \
         zero is the status bar's edge losing its shared tone."
    );
}
