//! The Submodules route: every registered root's submodules, their checked
//! out and recorded commits, the status word, and Init, Update, and Deinit.

use std::time::Duration;

use nsh_base::theme;
use nsh_core::prelude::*;
use turbogit_app::state::PendingConfirm;
use turbogit_domain::model::{DisplayName, RootId, Submodule, SubmoduleState};

use crate::bridge::AppHandle;
use crate::screens::shelf::counted;
use crate::test_ids;
use crate::theme as data;
use crate::ui::data_table::{DataTableColumnProps, DataTableProps, data_table};
use crate::ui::pane_header::{PaneHeaderProps, pane_header};
use turbogit_app::state::AppState;

#[derive(Clone)]
struct Row {
    root: RootId,
    path: std::path::PathBuf,
    head: Option<String>,
    recorded: Option<String>,
    state: SubmoduleState,
    name: String,
    diverged: bool,
}

fn rows(s: &AppState) -> Vec<Row> {
    s.multi
        .roots
        .iter()
        .flat_map(|root| {
            s.caches
                .submodules(&root.id)
                .into_iter()
                .flatten()
                .map(row_of)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn row_of(sub: &Submodule) -> Row {
    let diverged = diverged(sub);
    let recorded = match (&sub.head, &sub.recorded) {
        (Some(head), Some(recorded)) if head == recorded => None,
        _ => sub.recorded.clone(),
    };
    Row {
        root: sub.root.clone(),
        path: sub.path.clone(),
        head: sub.head.clone(),
        recorded: recorded.or_else(|| sub.recorded.clone()),
        state: sub.state,
        name: sub.path.display_name_or("<submodule>"),
        diverged,
    }
}

/// Both sides have to be present: an uninitialised submodule is an absence,
/// not a divergence.
fn diverged(sub: &Submodule) -> bool {
    matches!((&sub.head, &sub.recorded), (Some(head), Some(recorded)) if head != recorded)
}

fn status_word(state: SubmoduleState) -> &'static str {
    match state {
        SubmoduleState::UpToDate => "Up to date",
        SubmoduleState::NeedsUpdate => "Needs update",
        SubmoduleState::Uninitialized => "Uninitialized",
        SubmoduleState::Conflicted => "Conflicts",
    }
}

fn status_ink(state: SubmoduleState) -> AdaptiveColor {
    match state {
        SubmoduleState::UpToDate => data::RepoState::Clean.color(),
        SubmoduleState::NeedsUpdate => data::RepoState::Diverged.color(),
        SubmoduleState::Conflicted => data::RepoState::Conflict.color(),
        SubmoduleState::Uninitialized => data::RepoState::Uninitialized.color(),
    }
}

/// The row action's verb: an uninitialised submodule's update is an init.
fn action_verb(state: SubmoduleState) -> &'static str {
    match state {
        SubmoduleState::Uninitialized => "Init",
        _ => "Update",
    }
}

/// The Submodules route.
pub fn submodules_screen(handle: AppHandle) -> impl IntoElement {
    handle.update(ensure_submodule_data);
    let retry = handle.clone();
    set_interval(Duration::from_millis(160), move || {
        if retry.peek(|s: &AppState| {
            s.multi
                .roots
                .iter()
                .any(|root| s.caches.submodules(&root.id).is_none())
        }) {
            retry.update(ensure_submodule_data);
        }
    });
    let live = handle.clone();
    dynamic(move || {
        live.read(|_| ());
        submodules_body(handle.clone()).into_element()
    })
}

/// Reads the submodule status of every root whose cache is still empty.
fn ensure_submodule_data(state: &mut AppState) {
    for id in state
        .multi
        .roots
        .iter()
        .map(|root| root.id.clone())
        .collect::<Vec<_>>()
    {
        if state.caches.submodules(&id).is_none() {
            state.fetch_submodules(id);
        }
    }
}

fn submodules_body(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let rows = handle.peek(rows);
    let total = rows.len();
    let roots = handle.peek(|s| s.multi.roots.len());
    let _ = roots;
    view()
        .flex_1()
        .flex_col()
        .bg(t.background)
        .p(px(data::PANEL_PADDING))
        .gap(px(data::PANEL_PADDING))
        .test_id(&test_ids::route("submodules"))
        .child(pane_header(
            PaneHeaderProps::new("Submodules")
                .count(counted(total, "submodule", "submodules"))
                .test_id(&test_ids::submodule("header")),
        ))
        .child(if total == 0 {
            view()
                .p(px(data::PANEL_PADDING))
                .test_id(&test_ids::submodule("empty"))
                .child(
                    text("No registered submodules in this repository.")
                        .text_size(px(data::TYPE_BODY))
                        .text_color(t.muted_foreground),
                )
                .into_element()
        } else {
            submodules_table(handle, rows).into_element()
        })
}

fn submodules_table(handle: AppHandle, rows: Vec<Row>) -> impl IntoElement {
    let t = theme();
    let paths: Vec<String> = rows
        .iter()
        .map(|row| row.path.display().to_string())
        .collect();
    let heads: Vec<String> = rows
        .iter()
        .map(|row| {
            row.head
                .as_deref()
                .map(crate::screens::short)
                .unwrap_or_default()
        })
        .collect();
    let records: Vec<String> = rows
        .iter()
        .map(|row| {
            row.recorded
                .as_deref()
                .map(crate::screens::short)
                .unwrap_or_default()
        })
        .collect();
    let states: Vec<String> = rows
        .iter()
        .map(|row| status_word(row.state).to_string())
        .collect();
    let actions: Vec<String> = rows.iter().map(|row| row.name.clone()).collect();
    let data = nsh_base::DataTableRows::new(rows.len())
        .texts("path", paths)
        .texts("head", heads)
        .texts("recorded", records)
        .texts("status", states)
        .texts("actions", actions);
    let mut props = DataTableProps::new(
        vec![
            DataTableColumnProps::new("path", "PATH").width(280.0),
            DataTableColumnProps::new("head", "CHECKED OUT").width(120.0),
            DataTableColumnProps::new("recorded", "RECORDED").width(120.0),
            DataTableColumnProps::new("status", "STATUS").width(140.0),
            DataTableColumnProps::new("actions", "ACTIONS").width(180.0),
        ],
        data,
    )
    .test_id(&test_ids::submodule("table"));
    let cells = rows.clone();
    let action_handle = handle;
    props = props.render_cell(move |cell: &nsh_base::DataTableCell| {
        let Some(row) = cells.get(cell.row as usize) else {
            return text(cell.text.clone()).into_element();
        };
        if cell.key == "actions" {
            let name = row.name.clone();
            let verb = action_verb(row.state);
            let root = row.root.clone();
            let path = row.path.clone();
            let init = row.state == SubmoduleState::Uninitialized;
            let action_root = root.clone();
            let action_path = path.clone();
            let action_handle = action_handle.clone();
            let deinit_handle = action_handle.clone();
            return view()
                .flex_row()
                .items_center()
                .justify_end()
                .gap(px(4.0))
                .child(
                    view()
                        .role(AccessibilityRole::Button)
                        .label(format!("{verb} {name}"))
                        .test_id(&test_ids::submodule(format!("{verb}-{name}")))
                        .px(px(data::DENSITY_COMPACT_BUTTON_PAD_X))
                        .py(px(data::DENSITY_COMPACT_BUTTON_PAD_Y))
                        .rounded(px(data::CONTROL_RADIUS))
                        .hover(|s| s.bg(theme().muted))
                        .on_press(move || {
                            action_handle.update(|s| {
                                s.update_submodule(action_root.clone(), action_path.clone(), init)
                            });
                        })
                        .child(
                            text(format!("{verb} {name}"))
                                .text_size(px(data::TYPE_CONTROL))
                                .text_color(theme().foreground),
                        ),
                )
                .child(
                    view()
                        .role(AccessibilityRole::Button)
                        .label(format!("Deinit {name}"))
                        .test_id(&test_ids::submodule(format!("Deinit-{name}")))
                        .px(px(data::DENSITY_COMPACT_BUTTON_PAD_X))
                        .py(px(data::DENSITY_COMPACT_BUTTON_PAD_Y))
                        .rounded(px(data::CONTROL_RADIUS))
                        .hover(|s| s.bg(theme().muted))
                        .on_press(move || {
                            let root = root.clone();
                            let path = path.clone();
                            deinit_handle.update(move |s| {
                                s.ui.confirm = Some(PendingConfirm::DeinitSubmodule {
                                    root: root.clone(),
                                    path: path.clone(),
                                });
                            });
                        })
                        .child(
                            text(format!("Deinit {name}"))
                                .text_size(px(data::TYPE_CONTROL))
                                .text_color(theme().foreground),
                        ),
                )
                .into_element();
        }
        let ink = if cell.key == "status" {
            status_ink(row.state)
        } else if cell.key == "head" && row.diverged {
            data::RepoState::Diverged.color()
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
