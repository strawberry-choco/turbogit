//! The Worktrees route: every registered root's linked worktrees, the add
//! dialog's trigger, the dirty probe's three states, and the remove confirm.

use std::time::Duration;

use nsh_base::theme;
use nsh_core::prelude::*;
use turbogit_app::state::{AppState, Dialog, PendingConfirm};
use turbogit_domain::model::{DisplayName, RootId, Worktree};

use crate::bridge::AppHandle;
use crate::screens::shelf::counted;
use crate::test_ids;
use crate::theme as data;
use crate::ui::button::{ButtonProps, ButtonVariant, button};
use crate::ui::data_table::{DataTableColumnProps, DataTableProps, data_table};
use crate::ui::pane_header::{PaneHeaderProps, pane_header};

const PROBING: &str = "probing…";

/// One worktree row's data, kept beside the table so a cell's action reaches
/// the root and path it names.
#[derive(Clone)]
struct Row {
    root: RootId,
    path: std::path::PathBuf,
    branch: String,
    head: String,
    state: String,
    name: String,
}

fn worktree_rows(s: &AppState) -> Vec<Row> {
    s.multi
        .roots
        .iter()
        .flat_map(|root| {
            s.caches
                .worktrees(&root.id)
                .into_iter()
                .flatten()
                .map(row_of)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn row_of(wt: &Worktree) -> Row {
    let (word, _) = probe_state(wt.dirty);
    Row {
        root: wt.root.clone(),
        path: wt.path.clone(),
        branch: wt.branch.clone(),
        head: String::new(),
        state: word.to_string(),
        name: wt.path.display_name_or("<worktree>"),
    }
}

fn probe_state(dirty: Option<bool>) -> (&'static str, AdaptiveColor) {
    match dirty {
        Some(true) => ("dirty", data::STATE_WARNING),
        Some(false) => ("clean", theme().muted_foreground),
        None => (PROBING, theme().muted_foreground),
    }
}

/// The Worktrees route.
pub fn worktrees_screen(handle: AppHandle) -> impl IntoElement {
    handle.update(ensure_worktree_data);
    let retry = handle.clone();
    set_interval(Duration::from_millis(160), move || {
        if retry.peek(worktrees_need_data) {
            retry.update(ensure_worktree_data);
        }
    });
    let live = handle.clone();
    dynamic(move || {
        live.read(|_| ());
        worktrees_body(handle.clone()).into_element()
    })
}

/// Whether a cache read is still missing or a row's probe has not settled.
fn worktrees_need_data(state: &AppState) -> bool {
    state.multi.roots.iter().any(|root| {
        state
            .caches
            .worktrees(&root.id)
            .is_none_or(|rows| rows.iter().any(|row| row.dirty.is_none()))
    })
}

/// Refills an invalidated cache and probes the rows that have no verdict yet.
fn ensure_worktree_data(state: &mut AppState) {
    for id in state
        .multi
        .roots
        .iter()
        .map(|root| root.id.clone())
        .collect::<Vec<_>>()
    {
        if state.caches.worktrees(&id).is_none() {
            state.fetch_worktrees(id);
        }
    }
    state.ensure_worktree_probes();
}

fn worktrees_body(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let rows_handle = handle.clone();
    let rows = rows_handle.peek(worktree_rows);
    let total = rows.len();
    let add_handle = handle.clone();
    let empty = handle.peek(|s| {
        s.multi
            .roots
            .iter()
            .all(|root| s.caches.worktrees(&root.id).is_some())
    }) && total == 0;
    let body = worktrees_table(handle.clone(), rows);
    view()
        .flex_1()
        .flex_col()
        .bg(t.background)
        .p(px(data::PANEL_PADDING))
        .gap(px(data::PANEL_PADDING))
        .test_id(&test_ids::route("worktrees"))
        .child(pane_header(
            PaneHeaderProps::new("Worktrees")
                .count(counted(total, "worktree", "worktrees"))
                .test_id(&test_ids::worktree("header"))
                .action(button(
                    ButtonProps::new("Add worktree")
                        .variant(ButtonVariant::Primary)
                        .test_id(&test_ids::worktree("add"))
                        .on_press(move || {
                            add_handle.update(|s| {
                                s.ui.dlg.wt_path.clear();
                                s.ui.dlg.wt_branch.clear();
                                s.open_dialog(Dialog::NewWorktree, None);
                            });
                        }),
                )),
        ))
        .child(if empty {
            view()
                .p(px(data::PANEL_PADDING))
                .test_id(&test_ids::worktree("empty"))
                .child(
                    text("No linked worktrees. Use “Add worktree” to check a branch out into its own directory.")
                        .text_size(px(data::TYPE_BODY))
                        .text_color(t.muted_foreground),
                )
                .into_element()
        } else {
            body.into_element()
        })
}

fn worktrees_table(handle: AppHandle, rows: Vec<Row>) -> impl IntoElement {
    let t = theme();
    let paths: Vec<String> = rows
        .iter()
        .map(|row| row.path.display().to_string())
        .collect();
    let branches: Vec<String> = rows.iter().map(|row| row.branch.clone()).collect();
    let heads: Vec<String> = rows.iter().map(|row| row.head.clone()).collect();
    let states: Vec<String> = rows.iter().map(|row| row.state.clone()).collect();
    let actions: Vec<String> = rows.iter().map(|row| row.name.clone()).collect();
    let data = nsh_base::DataTableRows::new(rows.len())
        .texts("path", paths)
        .texts("branch", branches)
        .texts("head", heads)
        .texts("state", states)
        .texts("actions", actions);
    let mut props = DataTableProps::new(
        vec![
            DataTableColumnProps::new("path", "PATH").width(300.0),
            DataTableColumnProps::new("branch", "BRANCH").width(180.0),
            DataTableColumnProps::new("head", "HEAD").width(100.0),
            DataTableColumnProps::new("state", "STATE").width(100.0),
            DataTableColumnProps::new("actions", "ACTIONS").width(140.0),
        ],
        data,
    )
    .test_id(&test_ids::worktree("table"));
    let cells = rows.clone();
    let action_handle = handle.clone();
    props = props.render_cell(move |cell: &nsh_base::DataTableCell| {
        let Some(row) = cells.get(cell.row as usize) else {
            return text(cell.text.clone()).into_element();
        };
        if cell.key == "actions" {
            let name = row.name.clone();
            let remove_handle = action_handle.clone();
            let root = row.root.clone();
            let path = row.path.clone();
            return view()
                .flex_row()
                .items_center()
                .justify_end()
                .child(
                    view()
                        .role(AccessibilityRole::Button)
                        .label(format!("Remove {name}"))
                        .test_id(&test_ids::worktree(format!("remove-{name}")))
                        .px(px(data::DENSITY_COMPACT_BUTTON_PAD_X))
                        .py(px(data::DENSITY_COMPACT_BUTTON_PAD_Y))
                        .rounded(px(data::CONTROL_RADIUS))
                        .hover(|s| s.bg(theme().muted))
                        .on_press(move || {
                            let root = root.clone();
                            let path = path.clone();
                            remove_handle.update(move |s| {
                                s.ui.confirm = Some(PendingConfirm::RemoveWorktree {
                                    root: root.clone(),
                                    path: path.clone(),
                                });
                            });
                        })
                        .child(
                            text(format!("Remove {name}"))
                                .text_size(px(data::TYPE_CONTROL))
                                .text_color(theme().foreground),
                        ),
                )
                .into_element();
        }
        let ink = if cell.key == "state" {
            let dirty = row.state == "dirty";
            if dirty {
                data::STATE_WARNING
            } else {
                t.muted_foreground
            }
        } else {
            t.foreground
        };
        text(cell.text.clone())
            .text_size(px(data::TYPE_CONTROL))
            .text_color(ink)
            .into_element()
    });
    data_table(props)
}

/// The route is the table; a root with no cache paints the loading state.
pub fn worktrees_loading(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let _ = handle;
    view().p(px(data::PANEL_PADDING)).child(
        text("Loading worktrees…")
            .text_size(px(data::TYPE_BODY))
            .text_color(t.muted_foreground),
    )
}
