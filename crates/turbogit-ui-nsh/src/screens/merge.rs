//! The Merge dialog: the source branch, the four strategies, the option
//! rows, the preview totals, the cascade banner, and the Merge verb.

use nsh_base::{SelectItem, theme};
use nsh_core::prelude::*;
use turbogit_app::state::AppState;
use turbogit_domain::model::{BranchKind, MergeStrategy};

use crate::bridge::AppHandle;
use crate::screens::branch_dialogs::{STRATEGIES, dispatch_merge, merge_flags};
use crate::screens::shelf::counted;
use crate::test_ids;
use crate::theme as data;
use crate::ui::checkbox::{CheckboxProps, CheckboxState, checkbox};
use crate::ui::dialog::{DialogProps, dialog};
use crate::ui::row::{RowProps, RowState, row};
use crate::ui::select::{SelectProps, select};

/// The Merge dialog the branch menu opens.
pub fn merge_dialog(handle: AppHandle) -> AnyElement {
    let close_handle = handle.clone();
    let merge_handle = handle.clone();
    dialog(
        DialogProps::new("merge", "Merge into current branch")
            .on_cancel(move || close_handle.update(|s| s.ui.dialog = None))
            .child(merge_body(handle.clone()))
            .action("Merge")
            .on_action(move || dispatch_merge(&merge_handle)),
    )
    .into_element()
}

fn current_branch(state: &AppState) -> Option<String> {
    let id = state
        .ui
        .dlg
        .merge_root
        .as_ref()
        .or_else(|| state.ui.dlg.repo_scope.first())?;
    state.multi.by_id(id)?.current_branch.clone()
}

fn merge_body(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let (root, target, scope) = handle.peek(|s| {
        let root =
            s.ui.dlg
                .merge_root
                .clone()
                .or_else(|| s.ui.dlg.repo_scope.first().cloned());
        (
            root,
            s.ui.dlg.merge_target.clone(),
            s.ui.dlg.repo_scope.clone(),
        )
    });
    let repo_name = root.as_ref().map(|id| id.name()).unwrap_or_default();
    let current = handle.peek(current_branch);
    let branches: Vec<String> = root
        .as_ref()
        .and_then(|id| {
            handle.peek(|s| {
                s.multi.by_id(id).map(|repo| {
                    repo.branches
                        .iter()
                        .filter(|b| b.kind == BranchKind::Local)
                        .map(|b| b.name.clone())
                        .collect::<Vec<_>>()
                })
            })
        })
        .unwrap_or_default();

    let mut body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .test_id(&test_ids::merge("body"))
        .child(
            text("SOURCE BRANCH")
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        );

    let items: Vec<SelectItem> = branches
        .iter()
        .map(|name| SelectItem::new(name.clone(), name.clone()))
        .collect();
    let shown = if target.trim().is_empty() {
        "Select source branch…".to_string()
    } else {
        target.clone()
    };
    let source_handle = handle.clone();
    body = body.child(
        view()
            .flex_row()
            .items_center()
            .gap(px(data::CELL_GAP))
            .child(
                text(format!("⑂ {shown}"))
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.foreground)
                    .flex_1(),
            )
            .child(select(
                SelectProps::new(items)
                    .label(move || SharedString::from("Change…"))
                    .test_id(&test_ids::merge("source"))
                    .on_change(move |value: &SharedString| {
                        let value = value.to_string();
                        source_handle.update(move |s| {
                            s.ui.dlg.merge_target = value;
                            s.ui.dlg.merge_preview = None;
                            s.ui.dlg.merge_preview_key = None;
                            s.ui.dlg.merge_source_picker_open = false;
                        });
                    }),
            )),
    );

    body = body.child(
        text("STRATEGY")
            .text_size(px(data::TYPE_SECTION))
            .text_color(t.muted_foreground),
    );
    let strategy = handle.peek(|s| s.ui.dlg.merge_strategy);
    let mut strategies = view().flex_row().gap(px(4.0));
    for (value, label) in STRATEGIES {
        strategies = strategies.child(strategy_row(
            handle.clone(),
            value,
            label,
            strategy == value,
        ));
    }
    body = body.child(strategies);

    body = body.child(
        text("OPTIONS")
            .text_size(px(data::TYPE_SECTION))
            .text_color(t.muted_foreground),
    );
    let (no_ff, verify, unrelated, no_verify) = handle.peek(|s| {
        (
            s.ui.dlg.merge_no_ff,
            s.ui.dlg.merge_verify_signatures,
            s.ui.dlg.merge_allow_unrelated,
            s.ui.dlg.merge_no_verify,
        )
    });
    body = body
        .child(option_row(
            handle.clone(),
            "No fast-forward (--no-ff)",
            no_ff,
            |s| s.ui.dlg.merge_no_ff = !s.ui.dlg.merge_no_ff,
        ))
        .child(option_row(
            handle.clone(),
            "Verify signatures on incoming",
            verify,
            |s| s.ui.dlg.merge_verify_signatures = !s.ui.dlg.merge_verify_signatures,
        ))
        .child(option_row(
            handle.clone(),
            "Allow unrelated histories (--allow-unrelated-histories)",
            unrelated,
            |s| s.ui.dlg.merge_allow_unrelated = !s.ui.dlg.merge_allow_unrelated,
        ))
        .child(option_row(
            handle.clone(),
            "Skip hooks (--no-verify)",
            no_verify,
            |s| s.ui.dlg.merge_no_verify = !s.ui.dlg.merge_no_verify,
        ));

    // The preview is a read per (target, strategy), never per frame.
    let target_now = handle.peek(|s| s.ui.dlg.merge_target.trim().to_string());
    if !target_now.is_empty() {
        let needs = handle.peek(|s| {
            s.ui.dlg.merge_preview_key.as_ref()
                != Some(&(target_now.clone(), s.ui.dlg.merge_strategy))
        });
        if needs && let Some(root) = root.clone() {
            let strategy = handle.peek(|s| s.ui.dlg.merge_strategy);
            let preview = handle.peek(|s| s.merge_preview(&root, &target_now, strategy));
            handle.update(move |s| {
                s.ui.dlg.merge_preview = preview;
                s.ui.dlg.merge_preview_key = Some((target_now.clone(), strategy));
            });
        }
        if let Some(preview) = handle.peek(|s| s.ui.dlg.merge_preview) {
            let commit_line =
                if preview.files == 0 && preview.insertions == 0 && preview.deletions == 0 {
                    "Already up to date".to_string()
                } else if strategy == MergeStrategy::FastForward {
                    "Will fast-forward — no merge commit".to_string()
                } else {
                    format!(
                        "Will create {}",
                        counted(preview.merge_commits, "merge commit", "merge commits")
                    )
                };
            body = body
                .child(
                    text(commit_line)
                        .text_size(px(data::TYPE_CONTROL))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.foreground)
                        .test_id(&test_ids::merge("preview-commits")),
                )
                .child(
                    text(format!(
                        "{} changed · {} · {}",
                        counted(preview.files, "file", "files"),
                        counted(preview.insertions, "insertion", "insertions"),
                        counted(preview.deletions, "deletion", "deletions")
                    ))
                    .text_size(px(data::TYPE_CHIP))
                    .text_color(t.muted_foreground)
                    .test_id(&test_ids::merge("preview-counts")),
                );
        }
    }

    let siblings = root
        .as_ref()
        .map(|root| handle.peek(|s| s.merge_cascade_siblings(root)))
        .unwrap_or_default();
    if !siblings.is_empty() {
        let plan_handle = handle.clone();
        let root = root.clone();
        body = body.child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::SPACING_X))
                .test_id(&test_ids::merge("cascade"))
                .child(
                    text(format!(
                        "⚠ Cascade with {} after this merge",
                        counted(siblings.len(), "other repo", "other repos")
                    ))
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(data::STATE_WARNING),
                )
                .child(
                    view()
                        .role(AccessibilityRole::Button)
                        .label("View plan →")
                        .test_id(&test_ids::merge("view-plan"))
                        .px(px(data::DENSITY_COMPACT_BUTTON_PAD_X))
                        .py(px(data::DENSITY_COMPACT_BUTTON_PAD_Y))
                        .rounded(px(data::CONTROL_RADIUS))
                        .on_press(move || {
                            if let Some(root) = root.clone() {
                                plan_handle.update(move |s| s.open_merge_cascade_plan(&root));
                            }
                        })
                        .child(
                            text("View plan →")
                                .text_size(px(data::TYPE_CONTROL))
                                .text_color(t.foreground),
                        ),
                ),
        );
    }
    body = body.child(
        text(format!(
            "Merging into {} in {}",
            current.unwrap_or_else(|| "…".to_string()),
            repo_name
        ))
        .text_size(px(data::TYPE_CHIP))
        .text_color(t.muted_foreground),
    );
    let _ = scope;
    let _ = merge_flags(&handle);
    body
}

fn strategy_row(
    handle: AppHandle,
    value: MergeStrategy,
    label: &'static str,
    active: bool,
) -> impl IntoElement {
    let t = theme();
    row(RowProps::new()
        .state(if active {
            RowState::Selected
        } else {
            RowState::Default
        })
        .test_id(&test_ids::merge(format!("strategy-{label}")))
        .on_press(move || {
            handle.update(move |s| {
                s.ui.dlg.merge_strategy = value;
                s.ui.dlg.merge_preview = None;
                s.ui.dlg.merge_preview_key = None;
            });
        })
        .child(
            text(label)
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.foreground),
        ))
}

fn option_row(
    handle: AppHandle,
    label: &'static str,
    on: bool,
    toggle: impl Fn(&mut AppState) + Copy + 'static,
) -> impl IntoElement {
    let press = handle;
    let shared = std::rc::Rc::new(toggle);
    let change = shared.clone();
    view()
        .flex_row()
        .items_center()
        .gap(px(data::CELL_GAP))
        .child(checkbox(
            CheckboxProps::new(move || {
                let _ = &change;
                if on {
                    CheckboxState::Checked
                } else {
                    CheckboxState::Unchecked
                }
            })
            .accessibility_label(label)
            .test_id(&test_ids::merge(format!("option-{label}")))
            .on_change(move |_| {
                let toggle = shared.clone();
                press.update(move |s| {
                    toggle(s);
                    s.ui.dlg.merge_preview = None;
                    s.ui.dlg.merge_preview_key = None;
                });
            }),
        ))
        .child(
            text(label)
                .text_size(px(data::TYPE_CONTROL))
                .text_color(theme().foreground),
        )
}

/// The Merge route: the dialog's own body on a page, for the route table.
pub fn merge_screen(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    view()
        .flex_1()
        .flex_col()
        .gap(px(data::PANEL_PADDING))
        .bg(t.background)
        .p(px(data::PANEL_PADDING))
        .test_id(&test_ids::route("merge"))
        .child(
            text("Merge")
                .text_size(px(data::TYPE_PANE_TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.foreground),
        )
        .child(
            view()
                .flex_col()
                .gap(px(data::SPACING_Y))
                .rounded(px(data::CARD_RADIUS))
                .border(px(1.0))
                .border_color(t.border)
                .bg(t.card)
                .p(px(data::PANEL_PADDING))
                .child(merge_body(handle)),
        )
}
