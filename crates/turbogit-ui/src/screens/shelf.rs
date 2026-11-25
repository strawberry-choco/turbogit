//! The Shelf and Stash panes: one row per parked item, the keep/remove verb
//! pairs, the delete confirms, and the dialogs the headers open. The rows
//! are keyed by repository and name (or index), never by paint order.

use std::sync::Arc;
use std::time::Duration;

use nsh_base::theme;
use nsh_core::prelude::*;
use turbogit_app::shell_writes;
use turbogit_app::state::{AppState, Dialog, PendingConfirm};
use turbogit_domain::model::{RootId, Shelf, Stash};

use crate::bridge::AppHandle;
use crate::test_ids;
use crate::theme as data;
use crate::ui::button::{ButtonProps, ButtonVariant, button};
use crate::ui::pane_header::{PaneHeaderProps, pane_header};
use crate::ui::tab::{TabItem, TabsProps, tabs};

const TICK_MS: u64 = 120;

const SHELF_EMPTY: &str = "Nothing is shelved in this repository. Use “Shelve…” to park the \
                          selected changes and take the working tree back to clean.";
const STASH_EMPTY: &str = "Nothing is stashed in this repository. Use “Stash…” to park every \
                          uncommitted change, in or out of this application.";

/// The one place the stash row's verbs are explained. Spelled on every row,
/// the parentheticals read as "this is what happens to my changes" — and pop
/// keeps the changes — while pushing each message onto a second line.
const STASH_VERB_LEGEND: &str =
    "Apply keeps the entry · Pop applies and drops it · Delete discards the changes too";

/// One shelf row and its repository.
#[derive(Clone, Debug, PartialEq)]
struct ShelfRow {
    root: RootId,
    root_name: String,
    name: String,
    files: usize,
    shelved: String,
}

/// One stash row and its repository.
#[derive(Clone, Debug, PartialEq)]
struct StashRow {
    root: RootId,
    root_name: String,
    reference: String,
    message: String,
    index: usize,
}

#[derive(Clone)]
struct ShelfWindow(Arc<Vec<ShelfRow>>);

impl PartialEq for ShelfWindow {
    fn eq(&self, other: &Self) -> bool {
        *self.0 == *other.0
    }
}

impl std::ops::Deref for ShelfWindow {
    type Target = Vec<ShelfRow>;

    fn deref(&self) -> &Vec<ShelfRow> {
        &self.0
    }
}

#[derive(Clone)]
struct StashWindow(Arc<Vec<StashRow>>);

impl PartialEq for StashWindow {
    fn eq(&self, other: &Self) -> bool {
        *self.0 == *other.0
    }
}

impl std::ops::Deref for StashWindow {
    type Target = Vec<StashRow>;

    fn deref(&self) -> &Vec<StashRow> {
        &self.0
    }
}

fn shelf_rows(state: &AppState) -> Vec<ShelfRow> {
    let mut rows = Vec::new();
    for root in &state.multi.roots {
        for shelf in state.shelves_for(&root.id) {
            rows.push(shelf_row_of(&shelf));
        }
    }
    rows
}

fn shelf_row_of(shelf: &Shelf) -> ShelfRow {
    ShelfRow {
        root: shelf.root.clone(),
        root_name: shelf.root.name(),
        name: shelf.name.clone(),
        files: shelf.changes.len(),
        shelved: shelf.created_at.format("%Y-%m-%d %H:%M").to_string(),
    }
}

fn stash_rows(state: &AppState) -> Vec<StashRow> {
    let mut rows = Vec::new();
    for root in &state.multi.roots {
        let Some(stashes) = state.caches.stashes(&root.id) else {
            continue;
        };
        for stash in stashes {
            rows.push(stash_row_of(root.id.clone(), stash));
        }
    }
    rows
}

fn stash_row_of(root: RootId, stash: &Stash) -> StashRow {
    StashRow {
        root_name: root.name(),
        root,
        reference: turbogit_app::shelve_stash::stash_ref(stash.index),
        message: stash.message.clone(),
        index: stash.index,
    }
}

fn fetch_cold_stashes(state: &mut AppState) {
    let cold: Vec<RootId> = state
        .multi
        .roots
        .iter()
        .map(|root| root.id.clone())
        .filter(|id| state.caches.stashes(id).is_none())
        .collect();
    for id in cold {
        state.fetch_stashes(id);
    }
}

/// The Shelf route: its own two-pane strip over the same bodies the Commit
/// tab mounts.
pub fn shelf_screen(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let subtab = signal(0_usize);
    let items = vec![
        TabItem::new("Shelf").test_id(&test_ids::shelf("tab:shelf")),
        TabItem::new("Stash").test_id(&test_ids::shelf("tab:stash")),
    ];
    let strip_handle = handle.clone();
    let body_handle = handle;
    let body_subtab = subtab;
    view()
        .flex_1()
        .flex_col()
        .bg(t.background)
        .child(
            view()
                .flex_row()
                .px(px(data::PANEL_PADDING))
                .py(px(data::SPACING_Y))
                .child(tabs(
                    TabsProps::new(items, move || body_subtab.get()).on_change(move |at| {
                        subtab.set(at);
                        strip_handle.update(move |s| {
                            s.ui.commit_subtab = if at == 1 {
                                turbogit_app::state::CommitSubTab::Stash
                            } else {
                                turbogit_app::state::CommitSubTab::Shelf
                            };
                        });
                    }),
                )),
        )
        .child(dynamic(move || match body_subtab.get() {
            1 => stash_pane(body_handle.clone()).into_element(),
            _ => shelf_pane(body_handle.clone()).into_element(),
        }))
}

/// The Shelf pane: the group header per repository, one row per shelf, and
/// the keep/remove verbs.
pub fn shelf_pane(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let window = signal(ShelfWindow(Arc::new(handle.peek(shelf_rows))));
    let tick = handle.clone();
    let tick_window = window;
    set_interval(Duration::from_millis(TICK_MS), move || {
        let rows = tick.read(shelf_rows);
        if *tick_window.get().0 != rows {
            tick_window.set(ShelfWindow(Arc::new(rows)));
        }
    });

    let shelve_handle = handle.clone();
    let import_handle = handle.clone();
    let unshelve_handle = handle.clone();
    let row_handle = handle.clone();
    let row_window = window;
    let empty_cond = window;
    view()
        .flex_1()
        .flex_col()
        .bg(t.card)
        .test_id(&test_ids::route("shelf"))
        .child(pane_header(
            PaneHeaderProps::new("SHELVES")
                .test_id(&test_ids::shelf("shelves-header"))
                .action(button(
                    ButtonProps::new("Shelve…")
                        .variant(ButtonVariant::Ghost)
                        .size(crate::ui::ControlSize::Compact)
                        .test_id(&test_ids::shelf("shelve"))
                        .on_press(move || shelve_handle.update(|s| s.open_shelve_dialog(None))),
                ))
                .action(button(
                    ButtonProps::new("Import Patches…")
                        .variant(ButtonVariant::Ghost)
                        .size(crate::ui::ControlSize::Compact)
                        .test_id(&test_ids::shelf("import"))
                        .on_press(move || {
                            import_handle.update(|s| s.open_dialog(Dialog::ImportPatches, None))
                        }),
                ))
                .action(button(
                    ButtonProps::new("Unshelve…")
                        .variant(ButtonVariant::Ghost)
                        .size(crate::ui::ControlSize::Compact)
                        .test_id(&test_ids::shelf("unshelve-dialog"))
                        .on_press(move || unshelve_handle.update(|s| s.open_unshelve_dialog())),
                )),
        ))
        .child(each(
            move || row_window.with(|rows| rows.as_slice().to_vec()),
            |row| format!("{}:{}", row.root_name, row.name),
            move |row, _| shelf_row_view(row_handle.clone(), row.get()),
        ))
        .child(when(
            move || empty_cond.get().is_empty(),
            move || {
                let t = theme();
                text(SHELF_EMPTY)
                    .text_size(px(data::TYPE_BODY))
                    .text_color(t.muted_foreground)
                    .p(px(data::PANEL_PADDING))
            },
        ))
}

fn shelf_row_view(handle: AppHandle, row: ShelfRow) -> impl IntoElement {
    let t = theme();
    let row_name = row.name.clone();
    let unshelve_handle = handle.clone();
    let unshelve_root = row.root.clone();
    let unshelve_name = row.name.clone();
    let remove_handle = handle.clone();
    let remove_root = row.root.clone();
    let remove_name = row.name.clone();
    let delete_handle = handle.clone();
    let delete_root = row.root.clone();
    let delete_name = row.name.clone();
    view()
        .flex_row()
        .items_center()
        .gap(px(data::SPACING_X))
        .px(px(data::PANEL_PADDING))
        .py(px(data::SPACING_Y))
        .test_id(&test_ids::shelf(format!(
            "row:{}:{}",
            row.root_name, row.name
        )))
        .child(
            text(row.root_name.clone())
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        )
        .child(
            text(row.name.clone())
                .text_size(px(data::TYPE_BODY))
                .text_color(t.card_foreground)
                .flex_1(),
        )
        .child(
            text(counted(row.files, "file", "files"))
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground),
        )
        .child(
            text(row.shelved.clone())
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground),
        )
        .child(button(
            ButtonProps::new("Unshelve")
                .variant(ButtonVariant::Secondary)
                .size(crate::ui::ControlSize::Compact)
                .test_id(&test_ids::shelf(format!("unshelve:{row_name}")))
                .on_press(move || {
                    let root = unshelve_root.clone();
                    let name = unshelve_name.clone();
                    unshelve_handle.update(move |s| s.unshelve(root, name, false));
                }),
        ))
        .child(button(
            ButtonProps::new("Unshelve and Remove")
                .variant(ButtonVariant::Danger)
                .size(crate::ui::ControlSize::Compact)
                .test_id(&test_ids::shelf(format!("unshelve-remove:{row_name}")))
                .on_press(move || {
                    let root = remove_root.clone();
                    let name = remove_name.clone();
                    remove_handle.update(move |s| s.unshelve(root, name, true));
                }),
        ))
        .child(button(
            ButtonProps::new("Delete Shelf")
                .variant(ButtonVariant::Ghost)
                .size(crate::ui::ControlSize::Compact)
                .test_id(&test_ids::shelf(format!("delete:{row_name}")))
                .on_press(move || {
                    let root = delete_root.clone();
                    let name = delete_name.clone();
                    delete_handle.update(move |s| {
                        s.ui.confirm = Some(PendingConfirm::DeleteShelf { root, name });
                    });
                }),
        ))
}

/// The Stash pane: the entry's git ref, its message, and the apply/pop/delete
/// verbs.
pub fn stash_pane(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let window = signal(StashWindow(Arc::new(handle.peek(stash_rows))));
    let tick = handle.clone();
    let tick_window = window;
    set_interval(Duration::from_millis(TICK_MS), move || {
        let rows = tick.update_with(|s| {
            fetch_cold_stashes(s);
            stash_rows(s)
        });
        if *tick_window.get().0 != rows {
            tick_window.set(StashWindow(Arc::new(rows)));
        }
    });

    let stash_handle = handle.clone();
    let row_handle = handle.clone();
    let row_window = window;
    let empty_cond = window;
    view()
        .flex_1()
        .flex_col()
        .bg(t.card)
        .test_id(&test_ids::route("shelf"))
        .child(pane_header(
            PaneHeaderProps::new("STASHES")
                .test_id(&test_ids::shelf("stashes-header"))
                .action(button(
                    ButtonProps::new("Stash…")
                        .variant(ButtonVariant::Ghost)
                        .size(crate::ui::ControlSize::Compact)
                        .test_id(&test_ids::shelf("stash"))
                        .on_press(move || {
                            stash_handle.update(|s| s.open_dialog(Dialog::Stash, None))
                        }),
                )),
        ))
        // Under the header's hairline rather than in it: the header's slot
        // vocabulary is fixed (ADR-0027 R7), so the pane body carries this.
        .child(
            text(STASH_VERB_LEGEND)
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground)
                .w_full()
                .truncate()
                .px(px(data::PANEL_PADDING))
                .py(px(data::SPACING_Y)),
        )
        .child(each(
            move || row_window.with(|rows| rows.as_slice().to_vec()),
            |row| format!("{}:{}", row.root_name, row.index),
            move |row, _| stash_row_view(row_handle.clone(), row.get()),
        ))
        .child(when(
            move || empty_cond.get().is_empty(),
            move || {
                let t = theme();
                text(STASH_EMPTY)
                    .text_size(px(data::TYPE_BODY))
                    .text_color(t.muted_foreground)
                    .p(px(data::PANEL_PADDING))
            },
        ))
}

fn stash_row_view(handle: AppHandle, row: StashRow) -> impl IntoElement {
    let t = theme();
    let root = row.root.clone();
    let reference = row.reference.clone();
    let index = row.index;
    let apply_handle = handle.clone();
    let apply_root = root.clone();
    let pop_handle = handle.clone();
    let pop_root = root.clone();
    let delete_handle = handle.clone();
    let delete_root = root;
    let reference_for_pop = reference.clone();
    view()
        .flex_row()
        .items_center()
        .gap(px(data::SPACING_X))
        .px(px(data::PANEL_PADDING))
        .py(px(data::SPACING_Y))
        .test_id(&test_ids::shelf(format!(
            "row:{}:{}",
            row.root_name, row.reference
        )))
        .child(
            text(row.root_name.clone())
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        )
        .child(
            text(row.reference.clone())
                .text_size(px(data::TYPE_CONTROL))
                .text_color(data::LINK),
        )
        .child(
            text(row.message.clone())
                .text_size(px(data::TYPE_BODY))
                .text_color(t.card_foreground)
                .flex_1(),
        )
        .child(button(
            ButtonProps::new("Apply")
                .variant(ButtonVariant::Secondary)
                .size(crate::ui::ControlSize::Compact)
                .test_id(&test_ids::shelf(format!("apply:{index}")))
                .on_press(move || {
                    let operation = shell_writes::stash::apply_at(
                        &apply_root,
                        &turbogit_app::shelve_stash::stash_ref(index),
                        index,
                    );
                    apply_handle.update(move |s| s.dispatch(operation));
                }),
        ))
        .child(button(
            ButtonProps::new("Pop")
                .variant(ButtonVariant::Ghost)
                .size(crate::ui::ControlSize::Compact)
                .test_id(&test_ids::shelf(format!("pop:{index}")))
                .on_press(move || {
                    let operation =
                        shell_writes::stash::pop_at(&pop_root, &reference_for_pop, index);
                    pop_handle.update(move |s| s.dispatch(operation));
                }),
        ))
        .child(button(
            ButtonProps::new("Delete")
                .variant(ButtonVariant::DangerGhost)
                .size(crate::ui::ControlSize::Compact)
                .test_id(&test_ids::shelf(format!("delete:{index}")))
                .on_press(move || {
                    let root = delete_root.clone();
                    delete_handle.update(move |s| {
                        s.ui.confirm = Some(PendingConfirm::DeleteStash { root, index });
                    });
                }),
        ))
}

/// The count words the shelf row prints, the egui `components::counted`.
pub fn counted(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("{n} {one}")
    } else {
        format!("{n} {many}")
    }
}
