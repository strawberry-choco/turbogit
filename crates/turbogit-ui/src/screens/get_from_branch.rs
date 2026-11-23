//! Get from branch: the comparison of a branch against the working tree,
//! its status rungs, the per-row take that arms the confirm, and the swap
//! that exchanges the two sides.

use nsh_base::theme;
use nsh_core::prelude::*;
use turbogit_app::integrate_service::{BranchFileRow, BranchFileStatus};
use turbogit_app::state::{Dialog, PendingConfirm};
use turbogit_domain::model::{BranchKind, RootId};

use crate::bridge::AppHandle;
use crate::test_ids;
use crate::theme as data;
use crate::ui::button::{ButtonProps, ButtonVariant, button};
use crate::ui::data_table::{DataTableColumnProps, DataTableProps, data_table};
use crate::ui::dialog::{DialogProps, dialog};

pub fn layer(handle: AppHandle) -> impl IntoElement {
    let show = handle.clone();
    when(
        move || show.read(|s| s.ui.dialog == Some(Dialog::GetFromBranch)),
        move || {
            let body = handle.clone();
            dynamic(move || {
                body.read(|_| ());
                get_dialog(body.clone()).into_element()
            })
        },
    )
}

fn close(handle: &AppHandle) {
    handle.update(|s| s.ui.dialog = None);
}

fn status_word(status: BranchFileStatus) -> &'static str {
    match status {
        BranchFileStatus::Deleted => "Deleted",
        BranchFileStatus::Added => "Added",
        BranchFileStatus::Modified => "Modified",
    }
}

fn status_ink(status: BranchFileStatus) -> AdaptiveColor {
    match status {
        BranchFileStatus::Deleted => data::STATUS_DELETED,
        BranchFileStatus::Added => data::STATUS_ADDED,
        BranchFileStatus::Modified => data::STATUS_MODIFIED,
    }
}

fn get_dialog(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let (source, current, swapped, rows, error, root) = handle.peek(|s| {
        (
            s.ui.dlg.get_from_branch_source.clone(),
            s.ui.dlg.get_from_branch_current.clone(),
            s.ui.dlg.get_from_branch_swapped,
            s.ui.dlg.get_from_branch_rows.clone(),
            s.ui.dlg.get_from_branch_error.clone(),
            s.ui.dlg.get_from_branch_root.clone(),
        )
    });
    let (base, other) = if swapped {
        (current.as_str(), source.as_str())
    } else {
        (source.as_str(), current.as_str())
    };
    let mut body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .test_id(&test_ids::get_from_branch("body"))
        .child(
            text(format!("Comparing {base} with {other} (working tree)"))
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.foreground),
        )
        .child(
            text("FILES THAT DIFFER")
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        );
    if let Some(message) = error {
        body = body.child(
            text(format!("Could not read the comparison — {message}"))
                .text_size(px(data::TYPE_CONTROL))
                .text_color(data::STATE_ERROR),
        );
    } else if rows.is_empty() {
        body = body.child(
            text("No file differs between the two sides.")
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground),
        );
    } else {
        body = body.child(view().h(px(320.0)).w_full().child(files_table(
            handle.clone(),
            &root,
            &source,
            &rows,
        )));
    }
    let swap = handle.clone();
    let cancel = handle.clone();
    let close_handle = handle.clone();
    body = body.child(
        view()
            .flex_row()
            .items_center()
            .gap(px(data::CELL_GAP))
            .child(button(
                ButtonProps::new("Swap Branches")
                    .variant(ButtonVariant::Secondary)
                    .test_id(&test_ids::get_from_branch("swap"))
                    .on_press(move || swap.update(|s| s.swap_get_from_branch())),
            ))
            .child(button(
                ButtonProps::new("Close")
                    .variant(ButtonVariant::Ghost)
                    .test_id(&test_ids::get_from_branch("close"))
                    .on_press(move || close(&cancel)),
            )),
    );
    dialog(
        DialogProps::new("get-from-branch", format!("Get from Branch · {source}"))
            .width(680.0)
            .on_cancel(move || close(&close_handle))
            .child(body)
            .action("Close")
            .on_action(move || close(&handle)),
    )
}

fn files_table(
    handle: AppHandle,
    root: &Option<RootId>,
    source: &str,
    rows: &[BranchFileRow],
) -> impl IntoElement {
    let mut data = nsh_base::data_table::DataTableRows::new(rows.len());
    data = data.texts(
        "path",
        rows.iter().map(|row| row.path.display().to_string()),
    );
    data = data.texts("status", rows.iter().map(|row| status_word(row.status)));
    data = data.texts("action", rows.iter().map(|_| "Get from Branch"));

    let columns = vec![
        DataTableColumnProps::new("path", "PATH"),
        DataTableColumnProps::new("status", "STATUS").width(110.0),
        DataTableColumnProps::new("action", "").width(130.0),
    ];
    let render_handle = handle.clone();
    let render_rows = rows.to_vec();
    let render_root = root.clone();
    let render_source = source.to_string();
    data_table(
        DataTableProps::new(columns, data)
            .row_height(data::GROUP_ROW_HEIGHT)
            .accessibility_label("Files that differ")
            .test_id(&test_ids::get_from_branch("files"))
            .render_cell(move |cell| {
                let Some(row) = render_rows.get(cell.row as usize) else {
                    return view().into_element();
                };
                if cell.key.as_str() == "action" {
                    if row.status != BranchFileStatus::Modified {
                        return view().into_element();
                    }
                    let Some(root) = render_root.clone() else {
                        return view().into_element();
                    };
                    let press = render_handle.clone();
                    let source = render_source.clone();
                    let path = row.path.clone();
                    return button(
                        ButtonProps::new("Get from Branch")
                            .size(crate::ui::ControlSize::Compact)
                            .variant(ButtonVariant::Primary)
                            .test_id(&test_ids::get_from_branch(format!(
                                "take:{}",
                                row.path.display()
                            )))
                            .on_press(move || {
                                let root = root.clone();
                                let source = source.clone();
                                let path = path.clone();
                                press.update(move |s| {
                                    s.ui.confirm = Some(PendingConfirm::GetFileFromBranch {
                                        root: root.clone(),
                                        source: source.clone(),
                                        path: path.clone(),
                                    });
                                });
                            }),
                    )
                    .into_element();
                }
                let (line, color) = if cell.key.as_str() == "status" {
                    (status_word(row.status).to_string(), status_ink(row.status))
                } else {
                    (row.path.display().to_string(), theme().foreground)
                };
                text(line)
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(color)
                    .into_element()
            }),
    )
}

pub fn open(handle: &AppHandle) {
    let found = handle.peek(|s| {
        s.multi.roots.first().and_then(|repo| {
            let current = repo.current_branch.clone();
            repo.branches
                .iter()
                .find(|b| b.kind == BranchKind::Local && Some(&b.name) != current.as_ref())
                .map(|b| (repo.id.clone(), b.name.clone()))
        })
    });
    let Some((root, source)) = found else {
        return;
    };
    handle.update(move |s| {
        s.open_get_from_branch(&root, &source);
    });
}

pub fn get_from_branch_screen(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let open_handle = handle.clone();
    view()
        .flex_1()
        .flex_col()
        .gap(px(data::PANEL_PADDING))
        .bg(t.background)
        .p(px(data::PANEL_PADDING))
        .test_id(&test_ids::route("get_from_branch"))
        .child(
            text("Get from branch")
                .text_size(px(data::TYPE_PANE_TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.foreground),
        )
        .child(button(
            ButtonProps::new("Compare a branch with the working tree…")
                .variant(ButtonVariant::Primary)
                .test_id(&test_ids::get_from_branch("open"))
                .on_press(move || open(&open_handle)),
        ))
        .child(
            text("Pick a branch to compare against the working tree.")
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground),
        )
}
