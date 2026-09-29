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
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use egui::{Color32, Pos2, Rect};
use egui_kittest::{Harness, kittest::Queryable as _};
use test_support::harness::{filled_circles, filled_rects, painted_galleys, settle};
use turbogit_app::state::{AppState, TreeState};
use turbogit_domain::model::{Branch, BranchKind, Remote, Root, RootId, RootStatus, Upstream};
use turbogit_ui::theme::{Palette, RAIL_WIDTH};
use turbogit_ui::ui::branch_tree_view::{TreeProps, branch_tree};
use turbogit_ui::ui::branches_tree::build_branch_view;
use turbogit_ui::ui::components::{
    CLICK_TARGET_MIN, KIT_ICON_LARGE, KitButton, RowState, SyncKind, current_row_fill, fill,
    middle_truncate, row_fill, row_ink, sync_badge,
};
use turbogit_ui::ui::widgets::WidgetState;

/// Every state the row-state enum carries, enumerated in one place.
///
/// The enum is `pub` but not iterable, so a test that wants to say something
/// *about all of the states* has to write them out — and writing them out is
/// the point: a new variant that is not added here is invisible to the
/// all-states rules (the "no list row resolves to the current-ref token" one
/// above all), and a variant dropped from the enum fails to compile here rather
/// than quietly narrowing what those rules cover.
const ALL_ROW_STATES: [RowState; 4] = [
    RowState::Default,
    RowState::Hover,
    RowState::RowSelected,
    RowState::FocusSelected,
];

// --- §13 surfaces ------------------------------------------------------------

#[test]
fn surface_tokens_match_design_doc() {
    assert_eq!(
        Palette::CONTENT_BG,
        egui::Color32::from_rgb(0x23, 0x25, 0x29)
    ); // branch list, tab strip
    assert_eq!(Palette::RAISED, egui::Color32::from_rgb(0x2b, 0x2d, 0x30)); // secondary buttons, chips, active tab
    assert_eq!(
        Palette::SELECTION,
        egui::Color32::from_rgb(0x2e, 0x43, 0x69)
    ); // active row/pill
    // The 1px separator's tone is the raised surface, not the LINE border, and
    // the hairline role is the one spelling of it.
    assert_eq!(Palette::RULE_CONTENT, Palette::RAISED);
    assert_ne!(Palette::RULE_CONTENT, Palette::LINE);
}

// --- §13 meaning colors -------------------------------------------------------

#[test]
fn meaning_colors_match_design_doc() {
    assert_eq!(Palette::ACCENT, egui::Color32::from_rgb(0x35, 0x74, 0xf0)); // primary action / branch chips
    assert_eq!(Palette::AHEAD, egui::Color32::from_rgb(0x5f, 0xa8, 0x6c)); // ahead counts, current branch, in sync
    assert_eq!(Palette::DANGER, egui::Color32::from_rgb(0xdb, 0x5c, 0x5c)); // delete only
    assert_eq!(Palette::LINK, egui::Color32::from_rgb(0x74, 0xa3, 0xe8)); // the commit hash chip's ink
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
    // The muted step is `#8A8E96`, not `#AEB2BA`: the old value differed from
    // `T_SECONDARY` by two, two and one per channel (1.01:1 against it), so
    // counts, timestamps and metadata all read at one value. It is legal on
    // the panel and content surfaces, and NOT on a raised or selected one —
    // there the caller steps up to `T_SECONDARY`.
    assert_eq!(Palette::T_MUTED, egui::Color32::from_rgb(0x8a, 0x8e, 0x96)); // metadata, refs
    // The fourth step: placeholders, dim path suffixes, hatches. Sub-AA on
    // every audited surface by design, and therefore never the only rendering
    // of something the user needs.
    assert_eq!(Palette::T_DIM, egui::Color32::from_rgb(0x6e, 0x72, 0x7a));
    // The shell-facing names are aliases of the authoritative ramp, so a
    // central change reaches both spellings.
    assert_eq!(Palette::INK, Palette::T_PRIMARY);
    assert_eq!(Palette::INK_2, Palette::T_SECONDARY);
    assert_eq!(Palette::INK_3, Palette::T_MUTED);
    assert_eq!(Palette::INK_4, Palette::T_DIM);
}

// --- §13 shape + type sizes ---------------------------------------------------

#[test]
fn shape_and_type_tokens_match_design_doc() {
    // The chip and control radii are named once, at module level: the
    // `Palette`-side duplicate spellings went with the sweep, so a chip radius
    // has one source — `CHIP_RADIUS`, with no second `Palette` alias left to
    // drift away from it.
    assert_eq!(turbogit_ui::theme::CHIP_RADIUS, 3); // chips & badges
    assert_eq!(turbogit_ui::theme::CONTROL_RADIUS, 4); // buttons, inputs, panels
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

/// The three selection roles that survived are three *distinct* fills: the list
/// row, the current ref, and the focus band the log table and the sidebar tree
/// still use.
///
/// This is the ratchet that fires if the new selected-row fill is re-pointed at
/// the current-ref token — the mistake the four-to-three split exists to
/// prevent, and the one a reader cannot see without the numbers side by side.
#[test]
fn the_three_surviving_selection_roles_are_three_distinct_fills() {
    let list_row = row_fill(RowState::RowSelected);
    let current_ref = current_row_fill(RowState::RowSelected);
    let focus_band = row_fill(RowState::FocusSelected);
    println!("list row {list_row:?}, current ref {current_ref:?}, focus {focus_band:?}");

    // Each role is the value it is documented as, so a role cannot be swapped
    // for another and stay green.
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

    // ... and the three are pairwise distinct. A bare `assert_ne!` on the first
    // pair is the one that matters: the current-ref band and the list-row fill
    // are the pair that got merged once already.
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

/// The negative rule, pinned at the construction site that decides the fills
/// (`components::row_fill`): **no list row resolves to the selection token**.
///
/// [`Palette::SELECTION`] is the current-ref treatment — it belongs to
/// `current_row_fill`, not to the row-fill decision. A render seam can only
/// prove this at the sites a test enumerates, so the enumeration is every
/// variant of the enum: a new state has to be added to this list, and a state
/// re-pointed at the current-ref band fails even if nothing paints it yet.
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
    // The current-ref band is still a real band, reachable exactly where the
    // rule says: a current row that is also selected.
    assert_eq!(
        current_row_fill(RowState::RowSelected),
        Palette::SELECTION,
        "the current-ref treatment is the selected current row's band"
    );
    // And the current-ref band is a *different* value from the list row's fill,
    // which is the reason the two are kept apart rather than merged.
    assert_ne!(
        Palette::SELECTION,
        Palette::ROW_SELECTED,
        "the current-ref band and the list-row fill are two strengths on purpose"
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

/// Every hand-painted row in the crate tracks selection and hover as two
/// booleans and calls [`RowState::from_flags`] to turn them into a state
/// (`log_window`, `settings_modal`, `welcome`, `interactive_rebase`,
/// `multi_selection`). So the mapping `from_flags` performs *is* what those five
/// surfaces paint, and it is pinned here: `selected` resolves to
/// [`RowState::RowSelected`] — the one list-row selection fill, the opaque
/// `ROW_SELECTED` a chosen row takes — and it wins over `hovered`, because a
/// selected row keeps its selection fill while the pointer is on it. The two
/// unselected neighbours resolve to the hover and rest states that
/// [`row_fill_states_are_distinct`] already names, so the whole function is
/// covered by assertion here rather than by leaving it to the five call sites
/// to get right implicitly.
#[test]
fn from_flags_maps_selection_onto_the_list_row_band_and_keeps_hover_below_it() {
    // Selected wins over hover, in both pointer positions.
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
    // The list-row band is its own role, distinct from the focus band the log
    // table and the sidebar tree take.
    assert_ne!(
        RowState::from_flags(true, false),
        RowState::FocusSelected,
        "from_flags paints the opaque list-row fill, not the translucent focus band"
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

/// Stale rows dim (never hide); current rows keep primary ink.
#[test]
fn stale_rows_dim_without_hiding() {
    use turbogit_ui::ui::components::row_ink;
    assert_eq!(row_ink(false), Palette::T_PRIMARY);
    assert_eq!(row_ink(true), Palette::T_MUTED);
}

// --- §14.1 painted: a selected list row is a fill, not an inversion (ticket 03)
//
// The rules above are about the *decision*; these three are about what a frame
// actually paints, through the production row: the branch tree's branch row is
// the app's canonical list row, and it is the row the dropped duplicate used to
// be. So the fixture below is the real component (a bare view model plus a
// `TreeState`, no git and no application state) with one row selected, and the
// assertions read painted output — filled rects and galleys — rather than
// calling the decision functions again.
//
// Everything is scoped **by row**, never by first match: a branch name paints
// in its row and often again in a header chip, and "origin/main" paints in
// every row that has an upstream, so each galley is located by the row rect that
// contains it.

/// The row the fixture selects. Named so the assertions read as the sentence
/// they are.
const SELECTED_ROW: &str = "beta";
/// A second local row, unselected and not current — the comparison row.
const PLAIN_ROW: &str = "gamma";
/// The checked-out branch. Deliberately *not* the selected row: a current row
/// answers through `current_row_fill`, so selecting it would prove nothing
/// about `row_fill`.
const CURRENT_ROW: &str = "alpha";
/// The tracking branch every fixture row carries, so a row has metadata to
/// compare as well as a name.
const UPSTREAM: &str = "origin/main";

/// The fixed "now" the fixture renders against, so painted staleness text is
/// deterministic. 2026-06-01T12:00:00Z.
fn fixture_now() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(1_780_315_200, 0).expect("valid now")
}

/// A local branch that is five days old relative to [`fixture_now`] — fresh, so
/// the row reads at primary ink and the fixture's ink assertions are about
/// *selection* rather than about staleness.
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

/// The harness state: the view model inputs, the tree's own state (with the
/// selection made up front — this suite is about what a *selected* row paints,
/// not about what clicking one does) and the fixed clock.
struct RowFixture {
    roots: Vec<Root>,
    tags: HashMap<RootId, Vec<(String, turbogit_domain::model::RefState)>>,
    tree: TreeState,
    now: chrono::DateTime<chrono::Utc>,
}

/// The branch tree with one row selected, rendered through the real component.
fn selected_row_harness() -> Harness<'static, RowFixture> {
    // Remotes off: the fixture's subject is local rows, and a remote group adds
    // rows whose names repeat the local ones. The selection is made up front —
    // this suite is about what a *selected* row paints, not about what clicking
    // one does.
    let tree = TreeState {
        show_remotes: false,
        selected: Some(SELECTED_ROW.to_string()),
        selected_root: Some(RootId(Arc::from(std::path::PathBuf::from("/alpha")))),
        ..TreeState::default()
    };
    branch_tree_harness(vec![fixture_root()], tree, "row_fill_fixture")
}

/// A **multi-root** branch tree, with one repository in sync and one with a
/// local edit, over distinct branch names so no row is ambiguous.
///
/// Multi-root is not decoration here. A single-root project suppresses the repo
/// header's identity cluster — the status dot, the repo name and the current
/// chip — and the state word rides the same header, so a one-root frame contains
/// **no repository state at all** and an assertion about a state being coloured
/// text would be asserting nothing. Two roots put the state on screen.
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

/// One branch tree over `roots`, through the real component, with the given tree
/// state. Shared by the row fixtures and the state fixture so they differ only
/// in their inputs.
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
            let view = build_branch_view(&fx.roots, &fx.tags, &|_root: &RootId| false);
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

/// The row rect the selected row painted, as identified by its own fill.
///
/// Located by the fill rather than by counting rows: this is the assertion that
/// says *which* row is selected, so deriving it from the row order would make
/// the geometry and ink assertions below depend on the very thing they check.
fn selected_row_rect(harness: &Harness<'_, RowFixture>) -> Rect {
    row_rect_painted_with(harness, Palette::ROW_SELECTED, "the selected list row")
}

/// The unselected comparison row's rect, found the same way: the current row
/// answers through [`current_row_fill`], so it paints a band of its own and is
/// the one unselected row in this fixture whose rect is legible from painted
/// output. It carries the `current` pill, which is right-anchored and cannot
/// reach the name's left edge — the very edge these tests compare.
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

/// The painted text of `needle` inside `row`, matched exactly and scoped to the
/// row rather than taken first: a branch name also paints in the repo header's
/// chip, and a tracking branch paints in *every* row that has one, so a
/// first-match read would compare the selected row against its neighbour.
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

/// The fill of the selected list row is the one opaque selected-row value, and
/// the current-ref band is nowhere in the frame.
///
/// This is the "list rows actually use the new fill" half of the ticket: the
/// token existing is the easy half, and this is the assertion that reaches the
/// call site through a real render. A row that still resolved to
/// `Palette::SELECTION` — or to the translucent composite — fails here.
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
    // **R1: the only brand fill in a branch tree is a rail.** Every brand-filled
    // rect in the frame has to be the token's width at a row's leading edge, so
    // the check is "is it a rail?" rather than "is it wide?" — the old version of
    // this assertion only rejected brand bands *wider* than 200px, which quietly
    // permitted exactly the regression this rule exists for: the v1 current-branch
    // pill, a small brand-filled chip standing on every current row. Ticket 16
    // removed it, and this is the ratchet that now notices if it comes back.
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
    // The rail is really there, so the negative above is a claim about a frame
    // that has rails rather than a frame with nothing in it. Two of them is the
    // right number for this fixture: the selected row and the current row are
    // different rows, and both are chosen.
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

/// The geometry ratchet: a selected list row's text origin is an unselected
/// row's text origin. The rail is paint — reserving its width as padding shifts
/// every row's content the moment it is selected, and the two rows' names stop
/// lining up down the column.
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

    // Origin-for-origin: the name sits the same distance from its row's left
    // edge, at the same x, whether or not the row is selected. This is the
    // assertion that a rail reserved as *layout* breaks.
    assert_eq!(
        selected_name.pos.x - selected.left(),
        comparison_name.pos.x - comparison.left(),
        "selecting a row must not indent its content: the rail is paint, not padding"
    );
    assert_eq!(
        selected_name.pos.x, comparison_name.pos.x,
        "both rows share one name column, selected or not"
    );
    // The row itself is unchanged too: a selection that resized the row would
    // move the name as surely as one that padded it.
    assert_eq!(
        selected.size(),
        comparison.size(),
        "selecting a row must not resize it"
    );
    // The same for the metadata column: the tracking branch starts where it
    // starts on an unselected row.
    let selected_meta = text_in_row(&harness, selected, UPSTREAM);
    let comparison_meta = text_in_row(&harness, comparison, UPSTREAM);
    assert_eq!(
        selected_meta.pos.x, comparison_meta.pos.x,
        "a selected row's metadata column does not move either"
    );
}

/// The ink ratchet: selecting a row does not invert its ink. The branch name and
/// the metadata are painted with exactly the inks an unselected row uses.
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

    // The metadata: the tracking branch, which paints in every row that has an
    // upstream, so this is the case the row-scoped lookup exists for.
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

/// The kit's own row shell, at the construction site the ratchets above name:
/// one fill behind the text, at the caller's rect, and nothing else. It is the
/// smallest possible proof that the shell is *only* a fill — a shell that also
/// shifted content or picked an ink would show up here as a second rect or a
/// changed galley.
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
                    fill(ui, selected.rect, RowState::RowSelected);
                    ui.end_row();
                    let (_, plain) =
                        ui.allocate_exact_size(egui::vec2(300.0, 28.0), egui::Sense::hover());
                    fill(ui, plain.rect, RowState::Hover);
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

/// Mid-operation rows render a first-class label (design doc §10).
#[test]
fn mid_operation_label_is_first_class() {
    use turbogit_ui::ui::components::mid_op_label;
    assert_eq!(mid_op_label("merging"), "merging…");
    assert_eq!(mid_op_label("rebasing"), "rebasing…");
}

// --- State is coloured text or a leading dot, never a chip (ticket 05) -------
//
// R6's other half, and the reason the blue soup leaves the branches screen at
// all. A repository state is a *fact about the repository*, not a ref and not a
// number, so it has no business being one of the three chips — and behind a
// fill it stops reading as state altogether and starts reading as a category.
//
// The rule is asserted where it can be: at the three chip constructors (no chip
// names a state colour, so there is no chip for a state to reach for), and at a
// real render of the branches tree over a **dirty** root, where the state the
// screen reports is `Dirty` and therefore owns the reserved counter orange. An
// in-sync fixture would be worse than useless here: its state colour never
// enters the frame, so a chip fill carrying it would not be distinguishable
// from a frame with no state at all.
//
// The words themselves come from the one map — `RepoState::words()` beside
// `RepoState::color()` — and the dot is the *mark*: a filled circle, with no
// fill rect, no radius, no chip geometry and no node of its own.

/// The state colours the one repository-state map produces for a **severity**,
/// as a closed list of three.
///
/// **The seventh state is not in this table, and that is the decision.** A
/// repository state is one of three severities — dirt, upstream drift, a
/// conflict — or it is none of them. `RepoState::Uninitialized` (a submodule
/// registered but never checked out) is *work the user is owed*, not something
/// that went wrong, so it wears the **muted ink** rather than inventing a fourth
/// tone: the ramp's own contract is what makes that safe, because `INK_3` is
/// legal on the app, panel and content surfaces — the three the submodules pane
/// and the sidebar's repository rows actually sit on — and measurably not legal
/// on a raised or selected one.
///
/// The alternative was to add `INK_3` here as a fourth "state colour". Rejected:
/// this table's role is *severity*, and the one place a chip's legality is
/// decided already asks the better question — "is this colour **any** state the
/// map can produce?", which covers `INK_3` without calling it a severity. Naming
/// it here would make every future state colour a fourth row of a severity table
/// and lose the distinction the seventh variant exists to draw. The exclusion is
/// therefore *asserted* below rather than merely absent: the loop over
/// [`ALL_REPO_STATES`] states both halves, and the painted negative in
/// `a_dirty_repository_reaches_the_branches_screen_as_a_dot_and_words_never_as_a_chip_fill`
/// sweeps the severities.
const STATE_SEVERITY_COLORS: [(&str, Color32); 3] = [
    ("dirt / unpushed orange", Palette::COUNTER),
    ("ahead green", Palette::AHEAD),
    ("conflicted / diverged red", Palette::STATUS_DIVERGED),
];

/// The one state the severity table deliberately excludes, and the colour it is
/// allowed to wear instead.
///
/// Written as a table row of its own rather than a comment, because "the map can
/// produce a colour that is not in the severity table" is the fact a future
/// variant has to be able to check itself against.
const THE_NON_SEVERITY_STATE: (turbogit_ui::theme::RepoState, &str, Color32) = (
    turbogit_ui::theme::RepoState::Uninitialized,
    "work the user is owed, not a severity",
    Palette::INK_3,
);

/// Every repository state.
///
/// **A new variant is a failing test here, not a compile error — and this comment
/// used to claim the opposite.** The old note said "a new variant is a compile
/// error here rather than a silent gap", and that was false: a fixed-length
/// `[RepoState; 6]` is an ordinary array, so the seventh variant
/// (`Uninitialized`, which a real change landed) compiled silently and every
/// "all states" loop below quietly stopped covering it. Rust has no way to say
/// "this enum gained a variant" from inside a test that has to compile — no
/// reflection, no `variant_count` on stable — so the closure is checked the only
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

/// Every variant `theme.rs` declares for `RepoState`, in declaration order.
///
/// The declaration is read from the source rather than restated, because a list
/// that has to be restated is a list that can fall behind — and this one already
/// did. Comments are blanked first (`code_lines`), so a doc comment on a variant
/// is not mistaken for one, and a `#[derive]`-style attribute line is skipped
/// rather than read as a name.
fn declared_repo_state_variants() -> Vec<String> {
    let lines = code_lines(&crate_src().join("theme.rs"));
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
        // A variant is a bare identifier on its own line. Anything else inside
        // the braces (a doc comment is already blanked, an attribute carries
        // `#[`, a `}` closes the enum) is not one.
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

/// **The enumeration is closed.** Every variant `RepoState` declares is one of
/// the states the rules below sweep, and every state the rules sweep is a
/// declared variant.
///
/// Two halves, and both are needed: the first says a variant cannot be added
/// without appearing here (so the sweeps cover it), the second says the list has
/// not quietly grown a name the enum does not have (so the sweeps are not
/// covering a fiction). The failure message names the variants on both sides,
/// because "the loop does not cover the new state" is a much more expensive
/// thing to discover than "you added a state and did not say what colour it is".
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

/// The chip set, from the branches screen's side: the three chips the tree may
/// reach for, and the assertion that none of them is a state.
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

/// The state vocabulary the branches screen paints: one map, one wording, and
/// **every colour the map can produce** accounted for — the three severities, or
/// the one state that is deliberately not one.
///
/// The map's own app-wide test is `p0_repo_state_shared_mapping_is_app_wide` in
/// `design_tokens.rs`, which pins the mapping against the sidebar's dot and the
/// sync badge too. What this adds is the chip set's half of the same claim, so
/// the two suites cannot disagree about whether a state has become a chip — and
/// it asks the stronger question, looping over `state.color()` for every state
/// rather than over a transcribed list of colours, so the seventh variant cannot
/// slip past the way it did the first time.
#[test]
fn repository_state_is_one_map_and_the_chip_set_holds_none_of_it() {
    use turbogit_ui::ui::widgets::{COUNT_CHIP_COLORS, CURRENT_CHIP_COLORS, REF_CHIP_COLORS};

    // Every state has words and a colour, from the same map — a dot and the
    // summary beside it can never disagree, because there is only one source for
    // both halves.
    for state in ALL_REPO_STATES {
        assert!(
            !state.words().is_empty(),
            "{state:?} must have wording: a state nobody can read is not a state"
        );
        println!("{state:?}: {:?} in {:?}", state.words(), state.color());
    }

    // **(1) The severity table is closed over the map.** Every state either takes
    // one of the three severity colours or it is the one documented exception,
    // which is named rather than absent — so "the severity table covers the map"
    // is a claim with both halves checked rather than a coincidence.
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
    // …and the exception really is the only one: exactly one state wears a
    // colour the severity table does not carry, and it is the one named.
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

    // **(2) No chip is any state**, asked over the whole map rather than over the
    // severity table: a chip that filled the muted ink to carry a state would be
    // the same mistake as a chip that filled the orange, and asking the map's own
    // answer is the only form of the question that covers the seventh variant.
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

/// **The render half.** A dirty root's state reaches the branches screen as two
/// things and two only: a leading dot (a *circle* — no fill rect, no radius, no
/// chip geometry) and the state in words, inked from the one map. No filled
/// rectangle anywhere in the frame wears a repository-state colour, which is
/// what "state is not a chip" means once it reaches pixels.
#[test]
fn a_dirty_repository_reaches_the_branches_screen_as_a_dot_and_words_never_as_a_chip_fill() {
    let mut harness = dirty_repo_header_harness();
    settle(&mut harness);

    // The state the fixture actually produced. Read from the words the frame
    // painted rather than assumed, so a fixture that stopped being dirty fails
    // here for the honest reason instead of passing vacuously.
    let words = "dirty worktree";
    let expected_ink = turbogit_ui::theme::RepoState::Dirty.color();
    assert_eq!(
        expected_ink,
        Palette::COUNTER,
        "this fixture's whole point is the reserved orange: a dirt count is one \
         of the two things that colour is for"
    );

    // (1) Coloured text. The words paint, once, in the map's colour. Scoped by
    // string, and the count is asserted rather than assumed: a second copy of
    // the summary elsewhere in the tree would make "which one" unanswerable.
    let hits: Vec<_> = painted_galleys(&harness)
        .into_iter()
        .filter(|g| g.text.contains(words))
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

    // (2) A leading dot, and specifically a *circle*. The dot is a mark: it has
    // no fill rect, so the negative below can be stated about rects alone.
    let dots: Vec<_> = filled_circles(&harness)
        .into_iter()
        .filter(|(_, _, fill)| *fill == expected_ink)
        .collect();
    assert_eq!(
        dots.len(),
        1,
        "the state reaches the screen as a leading dot too, in the same colour \
         as the word beside it: found {dots:?}"
    );

    // (3) And never as a fill. No filled rectangle in the whole frame carries a
    // repository-state colour — the orange included. This is the claim the two
    // parts above exist to make unnecessary, and it is the one a future "let me
    // just badge this dirty" breaks.
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

// --- 07 — the shared row module: its primitives, the one rail, the one fill ---
//
// Everything above is the row *decision*. This section is the row module's own
// surface and the geometry a frame actually paints, in the order the ticket
// states them:
//
// 1. **the primitive set and its caller counts** — structural, because a caller
//    count is not something a render can observe, and a comment asserting one is
//    worth nothing. This section reads the crate's own `src` tree, in the house
//    style of `tests/menu_host.rs` and `tests/git_log.rs`.
// 2. **the rail** — one painter, one width definition site, both pre-existing
//    rail sites absorbed, and no surface left painting one from a literal.
// 3. **painted geometry** — the rail's width and its position at the leading
//    edge, and a selected row's text origin against an unselected row's.
// 4. **the fills** — every list row's resting, hover and selected fill, read off
//    painted output and identified with the one row-fill decision.
//
// The painted ratchets render the **sidebar**, one of the two screens that own
// an absorbed rail, through a real workspace and a real click on a repo row.

// --- the structural ratchets: reading the crate's own source ----------------

/// The crate's `src` directory — the tree the structural ratchets read.
fn crate_src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Every `.rs` file under `dir`, recursively.
fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(rust_files(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    out
}

/// One source file with every comment blanked out, line structure kept so a
/// finding can still be reported as `file:line`.
///
/// The comments are the point, and it is this migration's own lesson rather than
/// a general one: the row-shell retirement left a **live** function whose name
/// merely *contained* the retired name, alive because a comment says so
/// (`worktrees::worktree_row`). A source ratchet that counted words would count
/// that comment, and a human reading its output could act on it.
fn code_lines(path: &Path) -> Vec<String> {
    let src = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let (mut i, mut in_block, mut in_line, mut in_string) = (0usize, false, false, false);
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied().unwrap_or('\0');
        if in_block {
            if c == '*' && next == '/' {
                in_block = false;
                out.push(' ');
                out.push(' ');
                i += 1;
            } else {
                out.push(if c == '\n' { '\n' } else { ' ' });
            }
        } else if in_line {
            if c == '\n' {
                in_line = false;
                out.push('\n');
            } else {
                out.push(' ');
            }
        } else if in_string {
            out.push(c);
            if c == '\\' {
                if let Some(escaped) = chars.get(i + 1) {
                    out.push(*escaped);
                    i += 1;
                }
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '/' && next == '/' {
            in_line = true;
            out.push(' ');
            out.push(' ');
            i += 1;
        } else if c == '/' && next == '*' {
            in_block = true;
            out.push(' ');
            out.push(' ');
            i += 1;
        } else {
            if c == '"' {
                in_string = true;
            }
            out.push(c);
        }
        i += 1;
    }
    out.lines().map(str::to_owned).collect()
}

/// Whether `line` **declares** `name` as a function, rather than calling one.
fn declares(line: &str, name: &str) -> bool {
    let trimmed = line.trim_start();
    for prefix in ["pub fn ", "pub(crate) fn ", "fn "] {
        if let Some(rest) = trimmed.strip_prefix(prefix)
            && let Some(ident) = rest.split('(').next()
        {
            return ident.trim() == name;
        }
    }
    false
}

/// Whether `line` **calls** `name`: the name followed by `(`, and not part of a
/// longer identifier — `paint_rail(` and `rows::paint_rail(` are calls,
/// `my_paint_rail(` and `paint_rail_rect(` are not.
fn calls(line: &str, name: &str) -> bool {
    let mut from = 0;
    while let Some(offset) = line[from..].find(name) {
        let at = from + offset;
        let after = line[at + name.len()..].chars().next();
        let before = line[..at].chars().next_back().unwrap_or(' ');
        if after == Some('(') && !before.is_alphanumeric() && before != '_' {
            return true;
        }
        from = at + name.len();
    }
    false
}

/// Modules that are vocabulary rather than a screen. A call from one of these is
/// the shared grammar reaching for its own primitive, not a second screen
/// adopting a row, so it does not count towards "at least two screens".
const NOT_A_SCREEN: [&str; 5] = ["components", "widgets", "kit", "icons", "theme"];

/// Every call of `name` in the crate's `src` tree, as `(screen, path, line)`.
///
/// `screen` is the first path component under `src/ui/`, so `diff/actions.rs` is
/// the diff pane and `sidebar.rs` is the sidebar. That is the granularity the
/// "at least two screens" rule is written in: three call sites inside one screen
/// are one screen.
fn call_sites(name: &str) -> Vec<(String, String, usize)> {
    let src_root = crate_src();
    let mut out = Vec::new();
    for path in rust_files(&src_root) {
        let rel = path
            .strip_prefix(&src_root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let under_ui = rel.strip_prefix("ui/").unwrap_or("");
        let screen = under_ui
            .split('/')
            .next()
            .unwrap_or_default()
            .trim_end_matches(".rs")
            .to_owned();
        if NOT_A_SCREEN.contains(&screen.as_str()) {
            continue;
        }
        for (index, line) in code_lines(&path).iter().enumerate() {
            if declares(line, name) || !calls(line, name) {
                continue;
            }
            out.push((screen.clone(), rel.clone(), index + 1));
        }
    }
    out
}

/// The distinct screens a primitive is called from (sorted), with its call sites.
fn screens_calling(name: &str) -> (Vec<String>, Vec<(String, usize)>) {
    let sites = call_sites(name);
    let mut screens: Vec<String> = sites.iter().map(|(screen, _, _)| screen.clone()).collect();
    screens.sort();
    screens.dedup();
    let where_ = sites
        .iter()
        .map(|(_, path, line)| (path.clone(), *line))
        .collect();
    (screens, where_)
}

/// The `pub fn`s a module declares, in source order — the module's public
/// surface, read out of the module rather than restated beside it.
fn declared_primitives(rel: &str) -> Vec<String> {
    code_lines(&crate_src().join(rel))
        .iter()
        .filter_map(|line| {
            line.trim_start()
                .strip_prefix("pub fn ")
                .and_then(|rest| rest.split('(').next())
                .map(|ident| ident.trim().to_owned())
        })
        .collect()
}

/// The `pub fn`s in the row vocabulary that take a `Rect` — the painting
/// primitives, as opposed to the kit's decision functions (`row_fill`,
/// `row_ink`, `middle_truncate`, …) and its controls (`pill`, `kit_button`,
/// `section_header`), none of which paint into a caller-allocated row rect.
///
/// The classification is mechanical rather than a hand list, which is what makes
/// the ratchet total: a primitive added to either row home is either two screens'
/// worth of use or a declared construction site, with no third option and
/// nothing to remember. Signatures are accumulated to their opening brace, so a
/// wrapped parameter list is read whole.
fn rect_primitives(rel: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut pending: Option<(String, String)> = None;
    for line in code_lines(&crate_src().join(rel)) {
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

/// Every primitive the shared row vocabulary exports: the name, the module that
/// declares it, and the screens that call it.
///
/// All three parts are asserted exhaustively by
/// `every_shared_row_primitive_is_used_by_at_least_two_screens`, so this table
/// cannot quietly fall behind the tree: a screen that starts calling a primitive
/// fails until it is written down here, a caller that goes away fails until the
/// row is corrected, and a primitive added to either module fails until it is
/// added here. That is the difference between "used by at least two screens"
/// being a ratchet and being a claim in a doc comment.
const ROW_PRIMITIVES: [(&str, &str, &[&str]); 2] = [
    (
        "paint_row",
        "ui/widgets/rows.rs",
        &[
            "commit_window",
            "interactive_rebase",
            "multi_selection",
            "settings_modal",
            "welcome",
        ],
    ),
    // The rail painter lives in the row-state grammar rather than in
    // `widgets/rows.rs` because that module is private and its public surface is
    // exactly what `widgets/mod.rs` re-exports: a `pub fn` added there is
    // unreachable from every screen, which this workspace's `deny(warnings)`
    // turns into a compile error rather than an invisible helper.
    //
    // `branch_tree_view` joined the list in ticket 16: the branches pane's
    // selected and current rows take the rail at their leading edge, through this
    // one painter, instead of the brand band they used to paint. A screen that
    // adopts the primitive is written down here — that is what this list is for.
    //
    // `log_window` is deliberately **absent**, and that is a fact about how it
    // adopted the rail rather than about whether it has one (ticket 14): the
    // log's rows go through the shared shell (`components::row_shell`, the
    // `Railed` variant), which is where this painter lives one call away. The
    // rule above counts *callers*, so a surface that reaches the rail through
    // the shell contributes to the primitive's use without appearing as a
    // screen of its own. The painted proof that the log's rows carry a rail at
    // all is `tests/git_log.rs`'s.
    (
        "paint_rail",
        "ui/components.rs",
        &["branch_tree_view", "diff", "sidebar"],
    ),
];

/// The row-vocabulary painters that are **not** screen-facing primitives, named
/// here so the classification above stays total.
///
/// `fill` is the row fill's construction site: the fill decision, the control
/// radius and the "no text geometry" rule in one place, which is exactly what
/// ticket 03 moved it there for. It is reached through the shared row module's
/// `widgets::paint_row` — the seven-call-site spelling screens actually call — so
/// it has no screen caller of its own and is not a primitive a second screen
/// could adopt or drift from.
///
/// `row_shell` is its sibling (ticket 14): the one painter that lays a row's
/// shell down — the fill, and the leading-edge rail for a chosen row — for both
/// of the shell's variants. It is reached two ways, neither of which is a screen
/// calling a screen-facing primitive: through `widgets::paint_row` for
/// `RowShell::Fill` (the five call sites above), and directly by the log's own
/// rows for `RowShell::Railed`, which is the escape hatch that lets a
/// virtualised list adopt the vocabulary without a shell that re-measures per
/// row. One caller of the railed variant is not a case for a shared primitive
/// *yet* — the shell is where the two variants are defined, and the rail itself
/// is a primitive with three screens behind it either way.
const ROW_CONSTRUCTION_SITES: [&str; 2] = ["fill", "row_shell"];

/// Every primitive the shared row module exports is used by at least two
/// screens, and the set is pinned against the module's own source.
#[test]
fn every_shared_row_primitive_is_used_by_at_least_two_screens() {
    // 1. The shared row module's whole public surface, read from the module, so a
    //    second `pub fn` cannot be added without this failing.
    assert_eq!(
        declared_primitives("ui/widgets/rows.rs"),
        vec!["paint_row".to_owned()],
        "the shared row module's primitives are enumerated in ROW_PRIMITIVES; a \
         primitive that is not two screens' worth of use does not live here"
    );

    // 2. …and the classification is *total*: every painter in either row home is
    //    a declared primitive or a declared construction site. This is the half
    //    that catches a primitive added *with* a caller, which the table alone
    //    would not notice.
    for (home, primitives) in [
        (
            "ui/widgets/rows.rs",
            declared_primitives("ui/widgets/rows.rs"),
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
        // 3. Exactly one declaration, in the module this table names.
        let homes: Vec<String> = rust_files(&crate_src())
            .into_iter()
            .filter(|path| code_lines(path).iter().any(|line| declares(line, name)))
            .map(|path| {
                path.strip_prefix(crate_src())
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

        // 4. The caller count, counted over the tree, in screens.
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

/// The code of one `fn` in `rel`, from its declaration line to its closing brace.
///
/// Comment-stripped through [`code_lines`], so a claim about what a body *does*
/// cannot be satisfied by a doc comment describing the opposite, and a `//`
/// inside a string literal cannot hide a line from it. Brace-counted rather
/// than matched on a column, so a wrapped signature or a nested block is read
/// whole.
fn fn_source(rel: &str, name: &str) -> String {
    let lines = code_lines(&crate_src().join(rel));
    let start = lines
        .iter()
        .position(|line| declares(line, name))
        .unwrap_or_else(|| panic!("{rel} declares no `fn {name}`"));
    let mut depth = 0i32;
    let mut body = String::new();
    for line in &lines[start..] {
        for c in line.chars() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
        }
        body.push_str(line);
        body.push('\n');
        if depth == 0 {
            return body;
        }
    }
    panic!("`fn {name}` in {rel} never closes");
}

/// **The row shell's contract: it accepts an already-allocated rect and never
/// measures one.**
///
/// This is the half of the log's adoption that no frame can show. A shell that
/// re-measured per row would still paint a correct-looking row — a *pitch* out
/// of step with the virtualised list that asked for it, which is exactly the
/// regression the log's virtualisation suites are there to catch, and exactly
/// the rewrite the variant exists to avoid. So the claim is asserted from the
/// shell's own source: the rect arrives as a parameter, nothing is handed back,
/// and the body names no way of measuring, allocating or laying out.
///
/// The list of measurement APIs is spelled rather than inferred, because the
/// mutation it has to catch is a shell that grew *one* of them.
#[test]
fn the_row_shell_paints_a_caller_allocated_rect_and_never_measures_one() {
    let src = fn_source("ui/components.rs", "row_shell");

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

/// The rail marks a **chosen** row: exactly the two selection roles, and neither
/// of the two that are not.
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

/// A harness painting four rows — one per (`variant`, `state`) pair the shell is
/// reached with — so the two variants are pinned by what they paint rather than
/// by what the log happens to ask for. The rows are at known y offsets and the
/// whole frame is nothing but shell, so every fill in the output is a row's.
fn shell_harness() -> Harness<'static, ()> {
    use turbogit_ui::ui::components::{RowShell, row_shell};
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, _| {
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            egui::CentralPanel::default().show(ui, |ui| {
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
        },
        (),
    );
    harness.set_size(egui::vec2(400.0, 400.0));
    harness.step();
    harness
}

/// The shell's two variants, read off painted output: the railed one adds the
/// one rail at the leading edge of a **chosen** row, and the fill-only one never
/// does — whatever state it is handed.
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

    // Each row's band is the one row-fill decision's answer, painted at the
    // rect the caller allocated — which is the visible half of "it does not
    // measure": the painted band *is* the caller's rect, to the point.
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

/// The rail width is a token-layer dimension: defined once, and read by the
/// two places that draw one — the row rail painter and the tab underline.
#[test]
fn the_rail_width_is_defined_in_the_token_layer_and_consumed_by_one_painter() {
    // The value.
    assert_eq!(RAIL_WIDTH, 2.0, "the rail width dimension");
    // The definition: one `pub const`, in the token layer.
    let theme = code_lines(&crate_src().join("theme.rs"));
    assert_eq!(
        theme
            .iter()
            .filter(|line| line.contains("pub const RAIL_WIDTH"))
            .count(),
        1,
        "the rail width is defined in the token layer and nowhere else"
    );
    // A width, not a radius. The two 2px roles are two declarations, and the
    // coincidence of their numbers is exactly the trap.
    assert_eq!(
        theme
            .iter()
            .filter(|line| line.contains("pub const MARK_RADIUS"))
            .count(),
        1,
        "the 2px *radius* role stays its own declaration"
    );

    // Who reads it. Exactly three files may name it, and each for a stated
    // reason:
    //
    // - `theme.rs` defines it.
    // - `components.rs` is the one rail **painter**: a 2px vertical bar at a
    //   row's leading edge, which is what the width is for.
    // - `shell.rs` is the one other place the *number* is the whole point: the
    //   active tab's 2px accent underline, which is R1's second non-button job
    //   for the brand and is the same thickness as the rail. It reads the token
    //   rather than restating `2.0` precisely so the two cannot drift apart — and
    //   it is still not a *rail site*: the underline is a horizontal rule at a
    //   tab's bottom edge, so it does not go through `components::paint_rail`,
    //   which paints a vertical bar. Sharing the width is not sharing the
    //   painter, and this ratchet pins the set of *readers of the width*, not a
    //   set of painters.
    //
    // A fourth file is a second opinion about how thick the accent is, which is
    // the drift the token exists to prevent.
    let naming: Vec<String> = rust_files(&crate_src())
        .into_iter()
        .filter(|path| {
            code_lines(path)
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
        "shell.rs".to_owned(),
        "theme.rs".to_owned(),
    ];
    let mut found = naming.clone();
    expected.sort();
    found.sort();
    assert_eq!(
        found, expected,
        "exactly three files may name RAIL_WIDTH: theme.rs, which defines it; \
         components.rs, the one rail painter that consumes it; and shell.rs, whose \
         tab underline is the accent's other 2px job and reads the same width \
         dimension. A fourth is a second definition of the width under another \
         name — or a rail that grew one."
    );
    // The third reader really does read the *token*, not a literal that happens
    // to equal it: this is the distinction the comment above turns on, and it is
    // the mutation that a hand-rolled rail would introduce.
    let shell = code_lines(&crate_src().join("ui/shell.rs"));
    assert!(
        shell
            .iter()
            .any(|line| line.contains("crate::theme::RAIL_WIDTH")),
        "the tab underline must take its width from theme::RAIL_WIDTH: two 2px \
         accents that are one decision cannot be two literals"
    );
}

/// No surface paints an accent rail at a hard-coded width.
///
/// The negative half of "the width is written down in exactly one place", and it
/// is deliberately narrower than "no `2.0`": four things in the tree are 2px and
/// are not rails — the two BRAND rings around a focused conflict-pane header and
/// result cell, the BRAND_INK dash inside the tri-state partial checkbox, and the
/// icon stroke weight. So the rule is scoped to a **2px extent in the same
/// statement as a brand/Accent colour** (`Stroke::new(2.0, …)`,
/// `Vec2::new(2.0, …)`), which is the form both absorbed rail sites used.
/// Centring arithmetic (`size().y / 2.0`) is a division, not an extent.
#[test]
fn no_surface_paints_an_accent_rail_at_a_hard_coded_width() {
    let mut offenders: Vec<String> = Vec::new();
    for path in rust_files(&crate_src().join("ui")) {
        let lines = code_lines(&path);
        let rel = path
            .strip_prefix(crate_src())
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        // Statements, not lines: the diff pane's rail spanned three of them, and
        // a line-based rule would have missed it entirely.
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
            // A hard-coded 2px *extent*: `2.0` as a direct call argument or a
            // local binding (`Stroke::new(2.0, …)`, `Vec2::new(2.0, …)`,
            // `let rail = 2.0;`) — never `… / 2.0`, which is centring.
            let hard_coded_2px = ["(2.0", ",2.0", "=2.0"].iter().any(|p| dense.contains(p));
            // A focused conflict-pane cell's 2px BRAND *ring* is a border, not a
            // rail: it strokes the whole cell rather than filling a bar at one
            // edge, and it is a real geometry difference rather than a second
            // definition of the rail.
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

// --- the painted ratchets: the sidebar, which owns one of the two rails ------

/// Run `git` in `repo`, asserting success, and return stdout.
fn git(repo: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git invocation");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 stdout")
}

/// One initialised repository with a commit, under `parent`.
fn temp_repo(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    std::fs::create_dir_all(&path).expect("repo dir");
    git(&path, &["init", "-q", "-b", "main"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("base.txt"), "base\n").expect("seed file");
    git(&path, &["add", "."]);
    git(&path, &["commit", "-q", "-m", "init"]);
    path
}

/// A two-repository workspace in a fresh temp dir: `alpha` and `beta`, each a
/// git repository, side by side under the project root.
///
/// A `TempDir` rather than a fixed `.scratch/...` path on purpose: several suites
/// build fixtures under fixed scratch paths, and a shared one is a `config.lock`
/// race rather than a test.
fn two_repo_workspace() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("temp workspace");
    let alpha = temp_repo(dir.path(), "alpha");
    let beta = temp_repo(dir.path(), "beta");
    (dir, alpha, beta)
}

/// The sidebar screen with `beta` focused: the real shell, at the width that keeps
/// the left rail visible, driven exactly as `tests/workspace_sidebar.rs` drives
/// it. The focus is made by clicking the row, so the fixture exercises the real
/// selection path rather than poking `AppState` internals.
fn focused_sidebar() -> (Harness<'static, AppState>, tempfile::TempDir) {
    let (dir, alpha, beta) = two_repo_workspace();
    let state = AppState::for_roots(dir.path(), &[alpha, beta]);
    let mut harness = Harness::builder().with_max_steps(1024).build_ui_state(
        |ui, state| {
            state.drain_events();
            turbogit_ui::ui::render(ui, state);
        },
        state,
    );
    harness.set_size(egui::vec2(1280.0, 800.0));
    settle(&mut harness);
    harness.get_by_label("beta").click();
    settle(&mut harness);
    (harness, dir)
}

/// Filled rects painting exactly `color`.
///
/// Generic over the harness's state so the vocabulary's own ratchets can read a
/// frame that is nothing but the shell under test, and the sidebar's can read the
/// shell through it.
fn filled_with<S>(harness: &Harness<'_, S>, color: Color32) -> Vec<Rect> {
    filled_rects(harness)
        .into_iter()
        .filter(|(_, painted)| *painted == color)
        .map(|(rect, _)| rect)
        .collect()
}

/// The painted focus bands **inside `row`** — the sidebar's active-row band, read
/// off the token through the one row-fill decision so it keeps tracking the
/// theme, and scoped to the row because a focus-band fill is not unique to the
/// tree: the shell's own selected surfaces paint the same composite.
fn focus_bands(harness: &Harness<'_, AppState>, row: Rect) -> Vec<Rect> {
    filled_with(harness, row_fill(RowState::FocusSelected))
        .into_iter()
        .filter(|band| band.intersect(row) == *band)
        .collect()
}

/// The painted accent rails: brand-filled rects exactly a rail wide.
fn rails<S>(harness: &Harness<'_, S>) -> Vec<Rect> {
    filled_with(harness, Palette::BRAND)
        .into_iter()
        .filter(|rect| (rect.width() - RAIL_WIDTH).abs() < 0.01)
        .collect()
}

/// The one painted galley of exactly `text` inside `row`.
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

/// The colours painted inside `row`.
fn fills_in_row(harness: &Harness<'_, AppState>, row: Rect) -> Vec<Color32> {
    filled_rects(harness)
        .into_iter()
        .filter(|(rect, _)| row.contains_rect(*rect))
        .map(|(_, color)| color)
        .collect()
}

/// The selected row is marked by the one rail: the token's width, at the row's
/// leading edge, for the row's full height — and by nothing else.
#[test]
fn a_selected_row_paints_exactly_one_rail_of_the_token_width_at_its_leading_edge() {
    let (harness, _workspace) = focused_sidebar();

    // The band is painted at the row's own rect, so the rail's position is
    // measured against the row's real geometry rather than against a value this
    // test invented.
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

    // The unselected sibling carries no rail: one rail, for one selection.
    let plain = harness.get_by_label("alpha").rect();
    for other in rails(&harness) {
        assert!(
            !plain.contains_rect(other),
            "an unselected row must not carry a rail: {other:?}"
        );
    }
}

/// The rail is **paint**: selecting a row indents neither its text nor the
/// content region of its band.
///
/// A rail reserved as layout padding shifts every row's content the moment it is
/// selected, and the rows' names stop lining up down the column. Both halves are
/// asserted because they are the same mistake: the band's leading edge *is* the
/// row's content region, so a band that starts at `row.left() + RAIL_WIDTH` is a
/// selected row whose content is indented whether or not the label itself moved.
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

/// Every list row's resting, hover and selected fill comes from the one row-fill
/// decision, read off painted output.
#[test]
fn every_list_row_fill_comes_from_the_one_row_fill_decision() {
    let (mut harness, _workspace) = focused_sidebar();
    let selected = harness.get_by_label("beta").rect();
    let plain = harness.get_by_label("alpha").rect();

    // Resting: the decision for an untouched row is to paint nothing.
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

    // Selected: the focused row takes the decision's focus band, painted.
    let selected_painted = fills_in_row(&harness, selected);
    assert!(
        selected_painted.contains(&row_fill(RowState::FocusSelected)),
        "the selected row's band is the one row-fill decision's focus fill ({:?}); \
         the row painted {selected_painted:?}",
        row_fill(RowState::FocusSelected)
    );

    // Hover: the pointer on the unselected row takes the decision's hover fill.
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
    // And the selected row is still selected while the pointer is on its
    // neighbour: the two states never swap.
    let still_selected = fills_in_row(&harness, selected);
    assert!(
        still_selected.contains(&row_fill(RowState::FocusSelected)),
        "hovering a row's neighbour must not change the selected row's fill: \
         {still_selected:?}"
    );
}

/// A standard list row takes the named row height for its kind, on both screens
/// this suite can render: a repository row in the sidebar is the group-row
/// height, and the branch tree's list row is the branch row height.
///
/// This is the seam-available half of "the row module owns row height per row
/// kind". The kinds are the named roles (`theme::GROUP_ROW_HEIGHT`,
/// `components::BRANCH_ROW_H`, and `theme::FILE_ROW_HEIGHT` for a group/folder
/// row), so a call site that picked a literal of its own shows up here as a row
/// of the wrong height — and a selection that resized a row would too.
#[test]
fn a_standard_list_row_takes_its_named_row_height() {
    // The sidebar's repository row.
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

    // The branch tree's list row, from the fixture above.
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

// --- R3: a section label is the muted ink, never the dim step (ticket 21) -----
//
// The dim step's whole reason for existing is that it is a **named
// restriction**: "placeholders, dim path suffixes and hatches, and never the only
// rendering of something the user needs". A name is the only thing in this
// codebase that can enforce a restriction, so the ramp's geometry half is as
// load-bearing as the contrast half — and the column header already has a
// painted ratchet saying so (`a_column_header_is_ink_3_and_never_the_dim_ink` in
// `tests/widget_library.rs`). The *section* label is the other 9px tracked mark
// in the app, it was drawn with the dim value in the same target frames, and it
// had no such assertion. This is it.
///
/// Read off painted output through the real branch tree, and located by the
/// fixture's own rows rather than by a hard-coded position: the label paints
// once per section, so a second copy would make "which one" unanswerable.
#[test]
fn a_section_label_is_ink_3_and_never_the_dim_ink() {
    let mut harness = selected_row_harness();
    settle(&mut harness);

    // The fixture's sections. `show_remotes` is off, so LOCAL and TAGS are the
    // two that paint, and the local one is there twice over (two roots' worth of
    // grouping is not in play — one root, three local branches).
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
        // **The rule: never the dim step.** `INK_4` is the ramp's one step with
        // no legal surface — placeholders, dim path suffixes and hatches, and
        // never the only rendering of something the user needs. A 9px section
        // label is something the user needs, so this is the assertion; the
        // column header has its twin in `tests/widget_library.rs` and the target
        // frames were drawn with the dim value in both places, which is why
        // neither is left to a comment.
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
        // **The value it paints today, pinned with its gap named.** R3's role
        // table assigns `INK_3` to section labels, and the branches tree's own
        // `section_header` paints them one step higher, at `INK_2`
        // (`components.rs::section_header`, the shared header the branches
        // screen wears). That is a legal ink on this surface — it clears AA by a
        // wide margin — so it is a *brighter* label than the role table asks
        // for rather than an illegible one, and this ticket's job is to ratchet
        // the contract rather than to re-colour a screen. So the value is pinned
        // as it paints, with the gap written down: aligning it with the table is
        // a deliberate edit to this assertion plus one line in
        // `components::section_header`, and it is not a silent improvement.
        assert_eq!(
            label.color,
            Palette::INK_2,
            "the branches tree's section label reads at `INK_2` (see the comment \
             above: one step above R3's role table, which names `INK_3` for \
             section labels). If it moved, move it on purpose and say why here."
        );
    }
    // Whatever the value, it has to be one of the ramp's steps that may carry a
    // label — which is the part of the rule that is not a matter of taste.
    for label in &labels {
        assert!(
            is_a_label_bearing_step(label.color),
            "a section label is chrome, so its ink is a ramp step: `{}` painted \
             {:?}, which is neither `INK`, `INK_2` nor `INK_3`",
            label.text,
            label.color
        );
    }
    // The two restrictions are different restrictions, restated on the geometry
    // side: a section label is a *ramp* step, and the dim step stays the one
    // with no legal surface.
    assert_ne!(
        Palette::INK_3,
        Palette::INK_4,
        "if the muted and dim steps were merged, this ratchet would stop meaning \
         anything"
    );
}

/// Whether a colour is one of the ramp's steps that may carry a label.
///
/// Deliberately not a contrast measurement: the *legality* claim is the ramp's
/// (see `design_tokens.rs`), and re-deriving it here would be a second copy of
/// the ink sweep that could disagree with the first. This only asks "is it a step
/// the ramp has", so a label that switched to a surface colour or a state tone
/// fails here.
fn is_a_label_bearing_step(ink: Color32) -> bool {
    [Palette::INK, Palette::INK_2, Palette::INK_3].contains(&ink)
}

// --- R4: the sidebar's active band is the focus role, and why (ticket 21) ------
//
// Ticket 19 found that the left rail's active row takes
// `RowState::FocusSelected` (the translucent `selection_bg()` composite) where
// R4's criterion says "a selected list row is `ROW_SELECTED` plus a rail", and
// parked the question with its answer written down. The answer is **keep the
// focus band**, and this is the ratchet that holds it — with the measurement
// that settles the "but which one is it really?" part of the question, so nobody
// has to re-derive it before spending a change on the swap.
//
// Both halves matter. The first says the sidebar still paints the focus role
// (so a swap is a deliberate edit to this assertion, not a silent drift). The
// second says *why the swap would be invisible*: composited over the app
// background, the two fills are within one 8-bit step per channel of each other,
// so the choice is about which role the band claims and not about what the user
// can see. A change that cannot be seen is a change that must earn its place
// some other way, and this is the note that says it did not.
#[test]
fn the_sidebar_active_band_is_the_focus_role_and_the_two_fills_are_one_step_apart() {
    use turbogit_ui::theme::Palette;
    use turbogit_ui::ui::components::{RowState, row_fill};

    // (1) The decision, from the sidebar's own paint.
    let (harness, _workspace) = focused_sidebar();
    let row = harness.get_by_label("beta").rect();
    let bands = focus_bands(&harness, row);
    assert_eq!(
        bands.len(),
        1,
        "one focused row paints one band; found {bands:?}"
    );
    // The row paints more than the band — the rail, and the rail's own surface
    // behind it — so the claim is about *which selection fill* it paints, not
    // about the whole list.
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

    // (2) The measurement that settles it. `selection_bg()` is a **premultiplied**
    // translucent composite, so what it looks like depends entirely on what it is
    // composited over, and that is not the same answer everywhere. Composited over
    // the app background (the log table's host) it lands on `#233455` against the
    // list row's `#243456` — one 8-bit step on two channels and none on the third.
    // Over the left rail's own surface, which is a shade darker than the app
    // background, it lands on `#213252`: three, two and four steps. Both are
    // inside a rounding step of the same colour, which is the point: the two
    // fills are the same object to the eye, so the choice is about which *role*
    // the band claims and not about what the user can see. A change that cannot be
    // seen is a change that has to earn its place some other way, and this is the
    // note that says it did not.
    //
    // The arithmetic is done in floating point and never rounded, because the
    // rounding is exactly the size of the thing being measured: `premultiplied +
    // host × (1 − a)` is 35.5 and 85.5 on the app background, and asking whether
    // that is 35 or 36 is asking which way a compositor rounds.
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

// --- R6: the mark pair is a dot and words, at its construction site (ticket 21) -
//
// The render half of "state is not a chip" is
// `a_dirty_repository_reaches_the_branches_screen_as_a_dot_and_words_never_as_a_chip_fill`:
// in a real frame, no filled rect wears a state colour. The construction half is
// here, because a frame only shows the states a fixture happens to produce, and
// the failure this guards is `state_mark` itself growing a chip body — which
// would put a filled rect *behind* the state word in every state the app shows
// and in every state it does not.
#[test]
fn the_mark_pair_paints_a_dot_and_words_and_borrows_no_chip_geometry() {
    let src = fn_source("ui/components.rs", "state_mark");

    // The mark is a circle: a filled circle, no fill rect. A state behind a rect
    // stops reading as state and starts reading as a category, which is the
    // whole of R6's second half.
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
    // The colours arrive as arguments, so a second state vocabulary cannot start
    // here; the one map is `RepoState::color()` at the call site.
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
    // …and it really is a dot, from the helper it delegates to.
    let dot = fn_source("ui/components.rs", "state_dot");
    assert!(
        dot.contains("circle_filled"),
        "the mark's leading dot is a filled circle and nothing else: {dot}"
    );
    assert!(
        !dot.contains("Palette::"),
        "the dot takes its colour as an argument; a literal here would be a \
         second state map. {dot}"
    );

    // And the painted half, at the render seam too: one state mark, one circle,
    // one galley, no rect.
    let mut harness = Harness::new_ui_state(
        |ui, _| {
            turbogit_ui::theme::configure_style(ui.ctx());
            turbogit_ui::theme::install_fonts(ui.ctx());
            egui::CentralPanel::default().show(ui, |ui| {
                turbogit_ui::ui::components::state_mark(
                    ui,
                    "2 behind",
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
        vec!["2 behind".to_owned()],
        "…and the state in words, with no chip body behind either of them: {words:?}"
    );
}

// --- Purity: the branch view builder is git-free and egui-free (ticket 21) ----
//
// `build_branch_view` is a pure function over a view model: it asks the caller
// per repository and returns a tree, with no git, no `egui::Ui`, and no styling
// import. That property is currently enforced by a doc comment and by the
// dependency shape, and neither fails: a sweep adding `use egui::Color32;` to
// decide a row's colour would compile, look reasonable, and quietly turn the
// builder into a second place a colour is decided — which is the one thing the
// view sweep exists to prevent.
//
// So the property is asserted, at the source, which is the one altitude that can
// see it: a comment saying "no egui here" is not a ratchet, and this is. The
// fixture in this suite already calls the builder (every painted ratchet above
// goes through it), so the claim and its user live in the same file.
#[test]
fn the_branch_view_builder_is_git_free_styling_free_and_egui_free() {
    let src = code_lines(&crate_src().join("ui/branches_tree.rs"));

    // No egui: not a type, not a path, not an import. `egui` as a whole word, so
    // a use of any part of the framework is caught.
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
    // No styling import and no token: the same argument, spelled for the module
    // the tokens live in, because `use crate::theme::Palette` is the most likely
    // way the property would be lost and it does not contain the word `egui`.
    for banned in ["theme", "Palette", "components", "widgets"] {
        assert!(
            !src.iter()
                .any(|line| line.trim_start().starts_with(&format!("use {banned}"))),
            "the pure branch view builder imports `{banned}`: the tree builder is \
             the one place in the branches feature that decides nothing about \
             how anything looks, and an import is where that would start."
        );
    }
    // No git either — the model is the input, and a `std::process` or a
    // `git2` reference in here would mean the view is doing a read.
    for banned in ["std::process", "Command::", "git2", "Repository"] {
        assert!(
            !src.iter().any(|line| line.contains(banned)),
            "the pure branch view builder reaches for git (`{banned}`): the \
             fixture above hands it a finished view model, and a read here would \
             make the branches screen's tree depend on a subprocess"
        );
    }
    // …and the claim is not vacuous: the function this suite renders through is
    // really declared there, takes the model, and hands a model back.
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
