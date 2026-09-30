//! The one column-table tool pane, shared by Worktrees and Submodules.
//!
//! Both panes are the same surface over different rows: read the focused
//! root's cached list, paint the shared pane header with a count chip that is
//! that list's own length, then the shared column-header row over the pane's
//! column table, then a scroller whose empty branch is a word and whose body is
//! one row function. This module owns that sequence so neither pane restates it;
//! the two panes keep their columns, their strings, their header actions and
//! their rows, which are the four things that actually differ between them.
//!
//! **Crate-private on purpose.** It is a grammar shared by two sibling feature
//! surfaces, not a general widget and not a feature API — the same position
//! `ui::components` holds for the branch screens and `ui::kit::conflict_pane` for
//! the conflict ones. Nothing outside `turbogit_ui::ui` names it, so no caller can
//! read "column-oriented pane" as a general container role.

use egui::{ScrollArea, Ui};
use turbogit_app::state::AppState;
use turbogit_domain::model::RootId;

use super::widgets::{self, PaneColumn};

/// The cache accessor a column-table pane hands this module: the focused root's
/// own row list, or `None` while that answer has not arrived.
///
/// A named shape rather than a generic because the two accessors return
/// differently-typed rows and the caller fixes the row type; a function pointer
/// keeps `T` out of the signature instead of making it an inference variable the
/// two call sites have to be identical to satisfy.
pub(crate) type CachedRows<T> = for<'a> fn(&'a AppState, &'a RootId) -> Option<&'a [T]>;

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
    let loaded: Option<Vec<T>> = root
        .as_ref()
        .and_then(|id| (spec.cached)(state, id))
        .map(<[T]>::to_vec);
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
