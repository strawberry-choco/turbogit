//! The one column-table grammar, shared by the tool panes and the fleet
//! matrices.
//!
//! Two shapes live here, and both exist so the offset arithmetic is stated
//! once. [`column_table_pane`] runs the tool-pane sequence — the R7 header, the
//! column-header row, a scroller of one row function — for the Worktrees and
//! Submodules panes and for the Commit window's Shelf and Stash panes;
//! [`matrix_table`] paints one band-and-offset matrix for the five fleet
//! matrices (the preflight modal's three variants, the run monitor's table, and
//! the selection summary's per-repo table), and [`paint_cell`] paints what goes
//! inside a matrix row's band. Callers keep their columns, their strings and
//! their rows, which are the things that actually differ between them.
//!
//! **Crate-private on purpose.** It is a grammar shared by sibling feature
//! surfaces, not a general widget and not a feature API — the same position
//! `ui::components` holds for the branch screens and `ui::kit::conflict_pane` for
//! the conflict ones. Nothing outside `turbogit_ui::ui` names it, so no caller can
//! read "column-oriented pane" as a general container role.

use egui::{Color32, Painter, Pos2, Rect, ScrollArea, Sense, Ui, Vec2};
use turbogit_app::state::AppState;
use turbogit_domain::model::RootId;

use super::widgets::{self, PaneColumn};

/// The row source a column-table pane hands this module: the focused root's own
/// rows, or `None` while that answer has not arrived.
///
/// A named shape rather than a generic because the caller fixes the row type; a
/// function pointer keeps `T` out of the signature instead of making it an
/// inference variable the call sites have to be identical to satisfy.
///
/// **Owned, not borrowed**, because a pane's rows are not always a cache's
/// slice: the Shelf pane shows one root's shelves out of a project-wide store,
/// which is a selection rather than a view. `None` stays reserved for "no answer
/// yet", so an owned empty `Vec` is how a pane says it has looked and found
/// nothing.
pub(crate) type CachedRows<T> = for<'a> fn(&'a AppState, &'a RootId) -> Option<Vec<T>>;

/// The four things a column-table pane decides for itself, and the other four
/// this module does not ask about.
///
/// `loading` and `empty` are **this pane's own words**, not shared ones: a
/// worktree pane that has no answer says so, and a submodule pane that has one
/// says so, and a caller that cannot word its own absence is describing a pane
/// that does not exist.
pub(crate) struct PaneTable<'a, T> {
    /// The pane's own title, in the app's one upper case.
    pub title: &'a str,
    /// The column table the header row and every data row both read.
    pub columns: &'a [PaneColumn],
    /// The pane's own wording for the two words it can show in place of a list:
    /// what it says while the answer is not known, and what it says when the
    /// answer is "none". They are **this pane's own words**, not shared ones: a
    /// worktree pane that has no answer says so, and a submodule pane that has one
    /// says so, and a caller that cannot word its own absence is describing a pane
    /// that does not exist. Both go through [`widgets::empty_state`], so the ink
    /// is one decision even though the sentence is not.
    pub loading: &'a str,
    /// Painted when the cache is warm and the list is empty.
    pub empty: &'a str,
    /// The focused root's cached rows.
    pub cached: CachedRows<T>,
    /// The pane's right-aligned action slot. Empty is a legitimate choice.
    pub actions: &'a mut dyn FnMut(&mut Ui, &mut AppState),
    /// One row of the list.
    pub row: &'a mut dyn FnMut(&mut Ui, &mut AppState, &T),
}

/// Run one column-table tool pane: the R7 header, the column-header row, and a
/// scroller of `spec.row`.
///
/// **The list is read before the header, and copied.** Two separate reasons, and
/// the second is load-bearing:
///
/// - the header's count chip is that list's own length and not a second opinion
///   about it, and it is painted only once the list has landed — a count that
///   appears as 0 and becomes 3 a frame later is a flickering number;
/// - the copy is what lets the header's action slot take `&mut AppState` while
///   the rows below still hold the answer. Borrowing the cache across the header
///   is the shape that forced the header's `&Ui` and the row loop's separate
///   borrow apart in the first place.
///
/// With no focused root the pane is **header and nothing else**: there is no
/// cache to ask, so there is no count to show and no list to describe.
pub(crate) fn column_table_pane<T: Clone>(
    ui: &mut Ui,
    state: &mut AppState,
    spec: PaneTable<'_, T>,
) {
    let root = state.selected_root.clone();
    let loaded: Option<Vec<T>> = root.as_ref().and_then(|id| (spec.cached)(state, id));
    let count = loaded.as_ref().map(|rows| rows.len().to_string());

    widgets::pane_header(ui, spec.title, count.as_deref(), |ui| {
        (spec.actions)(ui, state)
    });
    ui.add_space(4.0);

    let Some(id) = root else {
        return;
    };
    let rows = loaded.unwrap_or_default();
    let answered = (spec.cached)(state, &id).is_some();
    if !answered {
        widgets::empty_state(ui, spec.loading);
    }

    widgets::column_header(ui, ui.available_rect_before_wrap(), spec.columns);
    ui.add_space(4.0);

    ScrollArea::vertical().show(ui, |ui| {
        // A cold cache is **not** the empty state: the pane has already said
        // "Loading …" above, and printing the empty word under it would report
        // an absence the cache has not established.
        if rows.is_empty() && answered {
            // The loading line and the empty line are one role — a word about
            // what the pane has to show — and go through the one empty state so
            // the two never read as two different decisions.
            widgets::empty_state(ui, spec.empty);
        }
        for row in &rows {
            (spec.row)(ui, state, row);
        }
    });
}

/// Paint one fleet matrix: a band of column labels over one row band per row,
/// at the offsets the caller's own [`PaneColumn`] table states.
///
/// Takes geometry only — a width, one column table, two band heights and the
/// rows — and names no feature, no state and no operation. The caller keeps its
/// column list, its strings and its cell contents; what stops being five
/// hand-written painters is the arithmetic every one of them was restating
/// twice: once in its `[("LABEL", x), …]` literal and once in every
/// `cell(x, …)` call, with nothing comparing the two numbers.
///
/// **The row closure allocates the band its row claims.** The painter computes
/// the rect and hands it over; it does not allocate it, because the band's
/// *width* is the row's business and one row of one of the five does not fill
/// it. A painter that allocated the full pane width itself would force that
/// row's trailing control into either a taller child or a reserved trailing
/// slot, and both are visible.
///
/// Returns the header band it allocated, so a caller that owns a mark under it
/// measures the same rect instead of deriving a second one.
///
/// **Not [`widgets::column_header`], and not a caller of it.** That band is
/// [`widgets::COLUMN_HEADER_HEIGHT`] (16pt) over a `RULE_STRUCTURAL` underline,
/// in the one tracked micro type; these matrices are 22/24pt with no rule, in
/// the chrome control face, and rewiring them onto the shared header is a
/// visible change that has not been decided.
pub(crate) fn matrix_table<T>(
    ui: &mut Ui,
    width: f32,
    columns: &[PaneColumn],
    header_h: f32,
    row_h: f32,
    rows: &[T],
    row: &mut dyn FnMut(&mut Ui, Rect, &T),
) -> Rect {
    let header = Rect::from_min_size(ui.cursor().left_top(), Vec2::new(width, header_h));
    ui.allocate_exact_size(Vec2::new(width, header_h), Sense::hover());
    for column in columns {
        let galley = ui.painter().layout_no_wrap(
            column.label.to_owned(),
            crate::theme::chrome_font(crate::theme::TYPE_CONTROL),
            crate::theme::Palette::INK_3,
        );
        ui.painter().galley_with_override_text_color(
            egui::Pos2::new(
                column.origin(header),
                header.center().y - galley.size().y / 2.0,
            ),
            galley,
            crate::theme::Palette::INK_3,
        );
    }
    for r in rows {
        let rect = Rect::from_min_size(ui.cursor().left_top(), Vec2::new(width, row_h));
        row(ui, rect, r);
    }
    header
}

/// Paint one fleet-matrix body cell: `text` in the chrome data face, ink
/// `color`, starting at `base_x + x` and centred on `cy`.
///
/// The second half of the grammar [`matrix_table`] owns — the header band and
/// the row band are there, and this is what a row paints inside its band. A
/// **layout mechanic, not a widget**: it allocates nothing, answers nothing, and
/// takes no `Id`. Crate-private with the module it belongs to, because nothing
/// outside `turbogit_ui::ui` has a matrix row to paint.
///
/// The caller keeps its columns, strings and offsets; only the face, the
/// centring and the paint are decided here. Each site keeps its own one-line
/// `let cell = |x, text, color| …` forwarder so its `cell(x, …)` calls read
/// unchanged.
///
/// **`galley_with_override_text_color`, not `Painter::galley`.** `Painter::galley`
/// takes its colour as a *fallback* for `PLACEHOLDER` spans only — a galley laid
/// out in `color` would keep painting `color` today, so the two coincide here and
/// the coincidence is **not** the contract: collapsing the override would make the
/// helper silently assume nothing else ever lays a galley out in a different ink
/// from the one it paints. `ui::components`' own centred label keeps the override
/// for the same stated reason.
///
/// **`base_x` and `cy`, not a `Rect`.** The run monitor's row callback is
/// `|ui, _rect, row|`: it deliberately ignores the rect [`matrix_table`] hands it
/// and derives `left`/`cy` from `ui.cursor()` inside a `ui.horizontal` child,
/// which is not `r.left()` / `r.center().y` by any guarantee. A `Rect`-shaped
/// signature would move that table's cells. The base is therefore a parameter
/// *expression*, because that one site's base is cursor-derived.
///
/// `text: String` mirrors the closure parameter this replaced, so the migration
/// is a substitution and `layout_no_wrap` receives what it received before.
pub(crate) fn paint_cell(
    painter: &Painter,
    base_x: f32,
    x: f32,
    cy: f32,
    text: String,
    color: Color32,
) {
    let galley = painter.layout_no_wrap(
        text,
        crate::theme::chrome_font(crate::theme::TYPE_BODY),
        color,
    );
    painter.galley_with_override_text_color(
        Pos2::new(base_x + x, cy - galley.size().y / 2.0),
        galley,
        color,
    );
}

/// The PATH cell a column table's first column holds: the row's **primary
/// column**, in the data face, at a **derived** width.
///
/// Primary ink because the path is what tells one row from another — it is the
/// one thing in the row the reader is looking for. The monospaced data face
/// because a path is data, and because a monospaced column of paths is
/// scannable in a way a proportional one is not (same rule `ref_chip` and
/// `hash_chip` follow).
///
/// The width is the gap from this cell's origin to the next column's, both read
/// from the pane's own column table, so the path takes exactly the room the
/// other columns leave it and no more, less [`crate::theme::CELL_GAP`]. A long
/// path elides rather than pushing the columns beside it out of line.
///
/// The band and the gap are read here rather than passed in, because neither is
/// the pane's: the band is the shared chip height every one of these cells shares
/// with the chip and the state mark beside it, and the gap is the one clearance
/// token. A caller that could pass either would be able to make its path cell
/// taller than the rest of its row, which is the one thing this cell must not be.
///
/// Laid out in a band of its own rather than through `add_sized`, because
/// `add_sized` **centres** its widget in the size it is given: a path would then
/// start wherever its own length left it, and a column whose cells do not start
/// at the column are not a column. `left_to_right(Align::Center)` is the one
/// layout that starts the text on the cell's left edge and still centres it
/// against the chip and the state mark in the same band.
pub(crate) fn path_cell(ui: &mut Ui, columns: &[PaneColumn], path: &std::path::Path) {
    let next_column = columns[1].origin(ui.max_rect());
    let width = (next_column - ui.cursor().left() - crate::theme::CELL_GAP).max(0.0);
    ui.allocate_ui_with_layout(
        egui::Vec2::new(width, widgets::CHIP_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(path.display().to_string())
                        .font(crate::theme::data_font(crate::theme::TYPE_BODY))
                        .color(crate::theme::Palette::INK),
                )
                .truncate(),
            );
        },
    );
}
