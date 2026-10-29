use egui::{Color32, Painter, Pos2, Rect, ScrollArea, Sense, Ui, Vec2};
use turbogit_app::state::AppState;
use turbogit_domain::model::RootId;

use super::widgets::{self, PaneColumn};

pub(crate) type CachedRows<T> = for<'a> fn(&'a AppState, &'a RootId) -> Option<Vec<T>>;

pub(crate) struct PaneTable<'a, T> {
    pub title: &'a str,
    pub columns: &'a [PaneColumn],
    pub loading: &'a str,
    pub empty: &'a str,
    pub cached: CachedRows<T>,
    pub actions: &'a mut dyn FnMut(&mut Ui, &mut AppState),
    pub row: &'a mut dyn FnMut(&mut Ui, &mut AppState, &T),
}

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

    // The scroll's persistent id comes from the pane title, not from layout:
    // egui's default salt is a constant, so two unsalted ScrollAreas nested at
    // the same anonymous-child depth share one id, one scroll moves the other
    // and egui paints its id-clash error over both. The title is the pane's
    // name, and the titles of every `column_table_pane` call site are distinct;
    // a duplicate would re-collide and fail `shell_scroll_id_clash.rs` first.
    ScrollArea::vertical()
        .id_salt(format!("pane_table_{}", spec.title.to_lowercase()))
        .show(ui, |ui| {
            if rows.is_empty() && answered {
                widgets::empty_state(ui, spec.empty);
            }
            for row in &rows {
                (spec.row)(ui, state, row);
            }
        });
}

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
/// The second half of the grammar [`matrix_table`] owns, the header band and
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
/// takes its colour as a *fallback* for `PLACEHOLDER` spans only, a galley laid
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
/// Primary ink because the path is what tells one row from another, it is the
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
