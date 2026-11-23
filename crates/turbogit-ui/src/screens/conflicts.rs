//! Conflicts: the file list, the three-pane resolver with its per-conflict
//! choices and bulk resolve, and the merge editor for a conflicted file.

use std::path::{Path, PathBuf};

use nsh_base::theme;
use nsh_core::prelude::*;
use turbogit_app::OwnedOperation;
use turbogit_app::conflict::{
    self, CHOICE_BOTH_THEIRS_FIRST, CHOICE_OURS, CHOICE_THEIRS, resolved_block_text,
};
use turbogit_app::operation::{LifecycleVerb, Operation};
use turbogit_app::shell_writes;
use turbogit_app::state::{AppState, BulkApplyScope, ConflictBulkReport, Toast};
use turbogit_domain::model::{ConflictKind, RootId};

use crate::bridge::AppHandle;
use crate::test_ids;
use crate::theme as data;
use crate::ui::button::{ButtonProps, ButtonVariant, button};
use crate::ui::checkbox::{CheckboxProps, CheckboxState, checkbox};
use crate::ui::dialog::{DialogProps, dialog};
use crate::ui::row::{RowProps, RowState, row};

const PANE_HEADER: f32 = 26.0;
const RESOLVER_WIDTH: f32 = 1100.0;
const MERGE_WIDTH: f32 = 980.0;
const BLOCK_ROWS: f32 = 180.0;

fn kind_word(kind: ConflictKind) -> &'static str {
    match kind {
        ConflictKind::BothDeleted => "both deleted",
        ConflictKind::BothAdded => "both added",
        ConflictKind::BothModified => "both modified",
        ConflictKind::DeletedByUs => "deleted by you",
        ConflictKind::DeletedByThem => "deleted by them",
        ConflictKind::AddedByUs => "added by you",
        ConflictKind::AddedByThem => "added by them",
    }
}

/// One conflicted file of a root.
pub fn conflicted_files(state: &AppState) -> Vec<(RootId, PathBuf, Option<ConflictKind>)> {
    state
        .multi
        .roots
        .iter()
        .flat_map(|root| {
            root.status.conflicted.iter().map(|path| {
                (
                    root.id.clone(),
                    path.clone(),
                    root.status.conflict_kind(path),
                )
            })
        })
        .collect()
}

fn segments(
    state: &AppState,
    root: &Path,
    path: &Path,
    content: &str,
) -> Vec<(String, String, bool)> {
    if state.settings.in_process_diffs
        && let Some(versions) = state.merge_versions(&RootId(root.to_path_buf().into()), path)
    {
        return turbogit_app::diff_engine::merge_segments(
            versions.base.as_deref().unwrap_or_default(),
            versions.ours.as_deref().unwrap_or_default(),
            versions.theirs.as_deref().unwrap_or_default(),
        );
    }
    conflict::parse_conflict_markers(content).0
}

pub fn compose(segs: &[(String, String, bool)], res: &[Option<u8>]) -> String {
    compose_impl(segs, res, false)
}

fn compose_display(segs: &[(String, String, bool)], res: &[Option<u8>]) -> String {
    compose_impl(segs, res, true)
}

fn compose_impl(segs: &[(String, String, bool)], res: &[Option<u8>], placeholder: bool) -> String {
    let mut out = String::new();
    let mut at = 0usize;
    for (ours, theirs, is_conflict) in segs {
        if *is_conflict {
            at += 1;
            match res
                .get(at - 1)
                .copied()
                .flatten()
                .and_then(|choice| resolved_block_text(ours, theirs, choice))
            {
                Some(text) => out.push_str(&text),
                None if placeholder => out.push_str("<< unresolved >>\n"),
                None => out.push_str(ours),
            }
        } else {
            out.push_str(ours);
        }
    }
    out
}

fn seed_state(state: &mut AppState, root: &Path, path: &Path, placeholder: bool) -> bool {
    let full = root.join(path);
    let Ok(content) = std::fs::read_to_string(&full) else {
        state.ui.toast = Some(Toast::error("Could not read conflicted file."));
        return false;
    };
    let segs = segments(state, root, path, &content);
    let res: Vec<Option<u8>> = segs
        .iter()
        .filter(|(_, _, is_conflict)| *is_conflict)
        .map(|_| None)
        .collect();
    let text = if placeholder {
        compose_display(&segs, &res)
    } else {
        compose(&segs, &res)
    };
    state.ui.conflict_segs = segs;
    state.ui.conflict_res = res;
    state.ui.conflict_text = text;
    state.ui.conflict_resolver_edited = false;
    true
}

/// Opens the resolver over `root`'s conflicted file. Purely a state change,
/// so a fixture can open it before the root mounts.
pub fn open_resolver_in(state: &mut AppState, root: &RootId, path: &Path) {
    if !seed_state(state, root.as_path(), path, false) {
        return;
    }
    let files = conflicted_files(state);
    let held = state.holding_operation(root.as_path());
    let paths: Vec<PathBuf> = files.iter().map(|(_, path, _)| path.clone()).collect();
    let auto = state.auto_merged_files(root, &paths).unwrap_or_default();
    let res_len = state.ui.conflict_res.len();
    state.ui.conflict_resolver_root = Some(root.clone());
    state.ui.conflict_resolver_undo = vec![None; res_len];
    state.ui.conflict_resolver_active_idx = 0;
    state.ui.conflict_resolver_selected = Some(path.to_path_buf());
    state.ui.conflict_overwrite_pending = None;
    state.ui.conflict_bulk_report = None;
    state.ui.merge_in_progress = held == Some(OwnedOperation::Merge);
    state.ui.auto_merged_files = if held == Some(OwnedOperation::Merge) {
        auto
    } else {
        Vec::new()
    };
    if !files.is_empty() {
        state.ui.conflict_resolver_open = true;
    }
}

pub fn open_resolver(handle: &AppHandle, root: &RootId, path: &Path) {
    handle.update(move |s| open_resolver_in(s, root, path));
}

/// Opens the merge editor over `root`'s conflicted file.
pub fn open_merge_editor_in(state: &mut AppState, root: &RootId, path: &Path) {
    if seed_state(state, root.as_path(), path, true) {
        state.ui.conflict_open = Some((root.clone(), path.to_path_buf()));
    }
}

pub fn open_merge_editor(handle: &AppHandle, root: &RootId, path: &Path) {
    handle.update(move |s| open_merge_editor_in(s, root, path));
}

/// The layer: the resolver, the merge editor, and their overwrite gate.
pub fn layer(handle: AppHandle) -> impl IntoElement {
    let show = handle.clone();
    when(
        move || show.read(|s| s.ui.conflict_resolver_open || s.ui.conflict_open.is_some()),
        move || {
            let paint = handle.clone();
            dynamic(move || {
                paint.read(|_| ());
                let (resolver, merge) =
                    paint.peek(|s| (s.ui.conflict_resolver_open, s.ui.conflict_open.clone()));
                let mut out = view().into_element();
                if resolver {
                    out = resolver_dialog(paint.clone()).into_element();
                }
                if merge.is_some() {
                    out = view()
                        .child(out)
                        .child(merge_dialog(paint.clone()))
                        .into_element();
                }
                out
            })
        },
    )
}

fn held_operation(handle: &AppHandle) -> OwnedOperation {
    handle
        .peek(|s| {
            s.ui.conflict_resolver_root
                .as_ref()
                .and_then(|root| s.holding_operation(root.as_path()))
        })
        .unwrap_or(OwnedOperation::Merge)
}

fn abort_merge(handle: &AppHandle) {
    let owner = held_operation(handle);
    let root = handle.peek(|s| s.ui.conflict_resolver_root.clone());
    if let Some(root) = root {
        handle.update(move |s| {
            s.dispatch(Operation::Resolve {
                root: root.clone(),
                owner,
                verb: LifecycleVerb::Abort,
            });
            s.ui.conflict_resolver_open = false;
        });
    }
}

fn continue_merge(handle: &AppHandle) {
    let owner = held_operation(handle);
    let root = handle.peek(|s| s.ui.conflict_resolver_root.clone());
    if let Some(root) = root {
        handle.update(move |s| {
            s.dispatch(Operation::Resolve {
                root: root.clone(),
                owner,
                verb: LifecycleVerb::Continue,
            });
            s.ui.conflict_resolver_open = false;
        });
    }
}

fn resolve_block(handle: &AppHandle, at: usize, choice: u8) {
    handle.update(move |s| {
        while s.ui.conflict_resolver_undo.len() <= at {
            s.ui.conflict_resolver_undo.push(None);
        }
        let previous = s.ui.conflict_res.get(at).copied().flatten();
        s.ui.conflict_resolver_undo[at] = previous;
        if let Some(slot) = s.ui.conflict_res.get_mut(at) {
            *slot = Some(choice);
        }
        if !s.ui.conflict_resolver_edited {
            s.ui.conflict_text = compose(&s.ui.conflict_segs, &s.ui.conflict_res);
        }
        if s.ui.conflict_resolver_auto_advance {
            advance_active(s);
        }
    });
}

fn undo_block(handle: &AppHandle, at: usize) {
    handle.update(move |s| {
        if let Some(slot) = s.ui.conflict_res.get_mut(at) {
            *slot = None;
        }
        if !s.ui.conflict_resolver_edited {
            s.ui.conflict_text = compose(&s.ui.conflict_segs, &s.ui.conflict_res);
        }
    });
}

fn advance_active(state: &mut AppState) {
    let count = state.ui.conflict_res.len();
    if count == 0 {
        return;
    }
    let current = state.ui.conflict_resolver_active_idx.min(count - 1);
    let mut at = (current + 1) % count;
    let start = at;
    loop {
        if state.ui.conflict_res[at].is_none() {
            state.ui.conflict_resolver_active_idx = at;
            return;
        }
        at = (at + 1) % count;
        if at == start {
            return;
        }
    }
}

fn take_side(handle: &AppHandle, at: usize, choice: u8) {
    let merged = handle
        .peek(|s| s.ui.conflict_resolver_edited || s.ui.conflict_res.iter().any(|r| r.is_some()));
    if merged {
        handle.update(move |s| s.ui.conflict_overwrite_pending = Some((at, choice)));
    } else {
        resolve_block(handle, at, choice);
    }
}

fn apply_resolution(handle: &AppHandle) {
    let (root, path, content, merge) = handle.peek(|s| {
        (
            s.ui.conflict_resolver_root.clone(),
            s.ui.conflict_resolver_selected.clone(),
            s.ui.conflict_text.clone(),
            s.ui.merge_in_progress,
        )
    });
    if let (Some(root), Some(path)) = (root, path) {
        handle.update(move |s| {
            s.dispatch(shell_writes::conflict::apply_merge_resolution(
                &root, &path, &content,
            ));
            if !merge {
                s.ui.conflict_resolver_open = false;
            }
        });
    }
}

fn apply_to_all_remaining(handle: &AppHandle) {
    handle.update(|s| {
        let active = s.ui.conflict_resolver_active_idx;
        let choice = match s.ui.conflict_resolver_scope {
            BulkApplyScope::Ours => CHOICE_OURS,
            BulkApplyScope::Theirs => CHOICE_THEIRS,
            BulkApplyScope::ActiveChoice => {
                s.ui.conflict_resolver_undo
                    .get(active)
                    .copied()
                    .flatten()
                    .or_else(|| s.ui.conflict_res.get(active).copied().flatten())
                    .unwrap_or(CHOICE_OURS)
            }
        };
        while s.ui.conflict_resolver_undo.len() < s.ui.conflict_res.len() {
            s.ui.conflict_resolver_undo.push(None);
        }
        let mut filled = 0usize;
        for at in 0..s.ui.conflict_res.len() {
            if s.ui.conflict_res[at].is_none() {
                s.ui.conflict_resolver_undo[at] = s.ui.conflict_res[at];
                s.ui.conflict_res[at] = Some(choice);
                filled += 1;
            }
        }
        if !s.ui.conflict_resolver_edited {
            s.ui.conflict_text = compose(&s.ui.conflict_segs, &s.ui.conflict_res);
        }
        s.ui.conflict_bulk_report = Some(if filled == 0 {
            ConflictBulkReport::NoneResolved
        } else {
            ConflictBulkReport::AllResolved
        });
    });
}

fn block_side(segs: &[(String, String, bool)], index: usize, ours: bool) -> String {
    segs.iter()
        .filter(|(_, _, is_conflict)| *is_conflict)
        .nth(index)
        .map(|(ours_text, theirs_text, _)| {
            if ours {
                ours_text.clone()
            } else {
                theirs_text.clone()
            }
        })
        .unwrap_or_default()
}

fn block_result(segs: &[(String, String, bool)], res: &[Option<u8>], index: usize) -> String {
    let Some((ours_text, theirs_text, _)) = segs
        .iter()
        .filter(|(_, _, is_conflict)| *is_conflict)
        .nth(index)
    else {
        return String::new();
    };
    match res
        .get(index)
        .copied()
        .flatten()
        .and_then(|choice| resolved_block_text(ours_text, theirs_text, choice))
    {
        Some(text) => text,
        None => "<< unresolved >>\n".to_string(),
    }
}

fn pane_header(label: &str) -> impl IntoElement {
    let t = theme();
    view()
        .h(px(PANE_HEADER))
        .flex_row()
        .items_center()
        .px(px(data::DENSITY_COMPACT_BUTTON_PAD_X))
        .bg(t.muted)
        .child(
            text(label.to_string())
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.foreground),
        )
}

fn result_editor(handle: AppHandle, label: &'static str) -> impl IntoElement {
    let change = handle.clone();
    let read = handle.clone();
    nsh_base::editor()
        .value(move || SharedString::from(read.peek(|s| s.ui.conflict_text.clone())))
        .read_only(false)
        .line_numbers(true)
        .soft_wrap(true)
        .accessibility_label(label)
        .flex_1()
        .on_change(move |text: &SharedString| {
            let text = text.to_string();
            change.update(move |s| {
                s.ui.conflict_text = text;
                s.ui.conflict_resolver_edited = true;
            });
        })
}

fn read_only_editor(text: String, label: &'static str) -> impl IntoElement {
    nsh_base::editor()
        .default_value(text)
        .read_only(true)
        .line_numbers(true)
        .soft_wrap(true)
        .accessibility_label(label)
        .flex_1()
}

fn resolver_dialog(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let (path, unresolved, total, conflicted, active, auto_advance, scope, report, merge) = handle
        .peek(|s| {
            (
                s.ui.conflict_resolver_selected
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "(no file)".to_string()),
                s.ui.conflict_res.iter().filter(|r| r.is_none()).count(),
                s.ui.conflict_res.len(),
                conflicted_files(s),
                s.ui.conflict_resolver_active_idx,
                s.ui.conflict_resolver_auto_advance,
                s.ui.conflict_resolver_scope,
                s.ui.conflict_bulk_report,
                s.ui.merge_in_progress,
            )
        });
    let (ours, theirs, result, edited) = handle.peek(|s| {
        (
            block_side(&s.ui.conflict_segs, active, true),
            block_side(&s.ui.conflict_segs, active, false),
            block_result(&s.ui.conflict_segs, &s.ui.conflict_res, active),
            s.ui.conflict_resolver_edited,
        )
    });
    let counter = if total == 0 {
        "0 conflicts".to_string()
    } else if unresolved == 1 {
        "1 conflict remaining".to_string()
    } else {
        format!("{unresolved} conflicts remaining")
    };
    let mut body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .test_id(&test_ids::conflicts("resolver"))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .child(
                    text(path.clone())
                        .text_size(px(data::TYPE_CONTROL))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.foreground)
                        .flex_1(),
                )
                .child(
                    text(counter)
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(t.muted_foreground),
                ),
        );
    if merge {
        let abort = handle.clone();
        let cont = handle.clone();
        body = body.child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .child(button(
                    ButtonProps::new("Abort merge")
                        .variant(ButtonVariant::Danger)
                        .test_id(&test_ids::conflicts("abort-merge"))
                        .on_press(move || abort_merge(&abort)),
                ))
                .child(button(
                    ButtonProps::new("Continue merge")
                        .variant(ButtonVariant::Primary)
                        .disabled(unresolved > 0)
                        .test_id(&test_ids::conflicts("continue-merge"))
                        .on_press(move || continue_merge(&cont)),
                )),
        );
    }
    let mut main = view()
        .flex_row()
        .gap(px(data::SPACING_X))
        .h(px(360.0))
        .child(file_list(handle.clone(), &conflicted, &path))
        .child(
            view()
                .flex_col()
                .flex_1()
                .gap(px(data::DENSITY_LIST_GAP))
                .child(
                    nsh_base::h_resizable()
                        .handle_class("w-px")
                        .flex_1()
                        .child(
                            nsh_base::resizable_panel().child(
                                view()
                                    .flex_col()
                                    .flex_1()
                                    .child(pane_header(&format!(
                                        "Local (Yours) · conflict {} of {}",
                                        active + 1,
                                        total
                                    )))
                                    .child(read_only_editor(ours, "Local (Yours)")),
                            ),
                        )
                        .child(
                            nsh_base::resizable_panel().child(
                                view()
                                    .flex_col()
                                    .flex_1()
                                    .child(pane_header(&format!(
                                        "Result · conflict {} of {}",
                                        active + 1,
                                        total
                                    )))
                                    .child(if edited {
                                        result_editor(handle.clone(), "Result").into_element()
                                    } else {
                                        read_only_editor(result, "Result").into_element()
                                    }),
                            ),
                        )
                        .child(
                            nsh_base::resizable_panel().child(
                                view()
                                    .flex_col()
                                    .flex_1()
                                    .child(pane_header(&format!(
                                        "Incoming (Theirs) · conflict {} of {}",
                                        active + 1,
                                        total
                                    )))
                                    .child(read_only_editor(theirs, "Incoming (Theirs)")),
                            ),
                        ),
                )
                .child(block_controls(handle.clone())),
        );
    let overwrite = handle.peek(|s| s.ui.conflict_overwrite_pending);
    if let Some((at, choice)) = overwrite {
        let discard = handle.clone();
        let keep = handle.clone();
        main = main.child(
            view()
                .flex_col()
                .gap(px(data::DENSITY_LIST_GAP))
                .p(px(data::PANEL_PADDING))
                .rounded(px(data::CARD_RADIUS))
                .border(px(1.0))
                .border_color(data::STATE_WARNING)
                .test_id(&test_ids::conflicts("overwrite"))
                .child(
                    text("This file already has merged changes.")
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.foreground),
                )
                .child(
                    text(format!(
                        "Accepting the {} side will discard and overwrite them.",
                        if choice == CHOICE_THEIRS {
                            "theirs"
                        } else {
                            "yours"
                        }
                    ))
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.foreground),
                )
                .child(
                    view()
                        .flex_row()
                        .gap(px(data::SPACING_X))
                        .child(button(
                            ButtonProps::new("Discard and Accept")
                                .variant(ButtonVariant::Primary)
                                .test_id(&test_ids::conflicts("discard-accept"))
                                .on_press(move || {
                                    discard.update(move |s| s.ui.conflict_overwrite_pending = None);
                                    resolve_block(&discard, at, choice);
                                }),
                        ))
                        .child(button(
                            ButtonProps::new("Keep Merged Changes")
                                .variant(ButtonVariant::Secondary)
                                .test_id(&test_ids::conflicts("keep-merged"))
                                .on_press(move || {
                                    keep.update(|s| s.ui.conflict_overwrite_pending = None)
                                }),
                        )),
                ),
        );
    }
    body = body.child(main);
    body = body.child(
        view()
            .flex_row()
            .items_center()
            .gap(px(data::CELL_GAP))
            .child(
                text(format!("Conflict {}", active + 1))
                    .text_size(px(data::TYPE_CHIP))
                    .text_color(t.muted_foreground),
            )
            .child(button(
                ButtonProps::new("< Prev")
                    .variant(ButtonVariant::Ghost)
                    .disabled(active == 0)
                    .test_id(&test_ids::conflicts("prev"))
                    .on_press({
                        let prev = handle.clone();
                        move || {
                            prev.update(|s| {
                                s.ui.conflict_resolver_active_idx =
                                    s.ui.conflict_resolver_active_idx.saturating_sub(1)
                            })
                        }
                    }),
            ))
            .child(button(
                ButtonProps::new("Next >")
                    .variant(ButtonVariant::Ghost)
                    .disabled(active + 1 >= total)
                    .test_id(&test_ids::conflicts("next"))
                    .on_press({
                        let next = handle.clone();
                        move || {
                            next.update(|s| {
                                s.ui.conflict_resolver_active_idx =
                                    (s.ui.conflict_resolver_active_idx + 1)
                                        .min(s.ui.conflict_res.len().saturating_sub(1))
                            })
                        }
                    }),
            ))
            .child(button(
                ButtonProps::new("Edit manually")
                    .variant(ButtonVariant::Secondary)
                    .test_id(&test_ids::conflicts("edit-manually"))
                    .on_press({
                        let edit = handle.clone();
                        move || edit.update(|s| s.ui.conflict_resolver_edited = true)
                    }),
            ))
            .child(checkbox(
                CheckboxProps::new(move || {
                    if auto_advance {
                        CheckboxState::Checked
                    } else {
                        CheckboxState::Unchecked
                    }
                })
                .accessibility_label("Auto-advance to next conflict")
                .test_id(&test_ids::conflicts("auto-advance"))
                .on_change({
                    let auto = handle.clone();
                    move |_| {
                        auto.update(|s| {
                            s.ui.conflict_resolver_auto_advance =
                                !s.ui.conflict_resolver_auto_advance
                        })
                    }
                }),
            ))
            .child(
                text("Auto-advance to next conflict")
                    .text_size(px(data::TYPE_CHIP))
                    .text_color(t.foreground),
            )
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(data::CELL_GAP))
                    .flex_1()
                    .child(
                        view().flex_1().child(button(
                            ButtonProps::new(format!("Apply to all {unresolved} remaining"))
                                .variant(ButtonVariant::Secondary)
                                .test_id(&test_ids::conflicts("apply-all"))
                                .on_press({
                                    let all = handle.clone();
                                    move || apply_to_all_remaining(&all)
                                }),
                        )),
                    )
                    .child(button(
                        ButtonProps::new("Apply")
                            .variant(ButtonVariant::Primary)
                            .disabled(unresolved > 0)
                            .test_id(&test_ids::conflicts("apply"))
                            .on_press({
                                let apply = handle.clone();
                                move || apply_resolution(&apply)
                            }),
                    )),
            ),
    );
    body = body.child(scope_row(handle.clone(), scope));
    match report {
        Some(ConflictBulkReport::AllResolved) => {
            body = body.child(
                text("All conflicts were resolved automatically")
                    .text_size(px(data::TYPE_CHIP))
                    .text_color(t.muted_foreground),
            );
        }
        Some(ConflictBulkReport::NoneResolved) => {
            body = body.child(
                text("No conflicts were resolved automatically")
                    .text_size(px(data::TYPE_CHIP))
                    .text_color(t.muted_foreground),
            );
        }
        None => {}
    }
    let cancel = handle.clone();
    body = body.child(button(
        ButtonProps::new("Close")
            .variant(ButtonVariant::Ghost)
            .test_id(&test_ids::conflicts("close"))
            .on_press(move || cancel.update(|s| s.ui.conflict_resolver_open = false)),
    ));
    dialog(
        DialogProps::new("resolve-conflicts", "Resolve conflicts")
            .width(RESOLVER_WIDTH)
            .on_cancel({
                let cancel = handle.clone();
                move || cancel.update(|s| s.ui.conflict_resolver_open = false)
            })
            .child(body),
    )
    .into_element()
}

fn scope_row(handle: AppHandle, scope: BulkApplyScope) -> impl IntoElement {
    let t = theme();
    let mut row_view = view()
        .flex_row()
        .items_center()
        .gap(px(data::CELL_GAP))
        .child(
            text("Fill remaining with")
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground),
        );
    for (label, value) in [
        ("Active choice", BulkApplyScope::ActiveChoice),
        ("Ours", BulkApplyScope::Ours),
        ("Theirs", BulkApplyScope::Theirs),
    ] {
        let press = handle.clone();
        row_view = row_view.child(row(RowProps::new()
            .state(if scope == value {
                RowState::Selected
            } else {
                RowState::Default
            })
            .test_id(&test_ids::conflicts(format!("scope-{label}")))
            .on_press(move || press.update(move |s| s.ui.conflict_resolver_scope = value))
            .child(
                text(label)
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.foreground),
            )));
    }
    row_view
}

fn file_list(
    handle: AppHandle,
    conflicted: &[(RootId, PathBuf, Option<ConflictKind>)],
    selected: &str,
) -> impl IntoElement {
    let t = theme();
    let mut list = view()
        .flex_col()
        .w(px(230.0))
        .gap(px(data::DENSITY_LIST_GAP))
        .test_id(&test_ids::conflicts("files"))
        .child(
            text("CONFLICTED FILES")
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        )
        .child(
            text(format!("0 / {} resolved", conflicted.len()))
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground),
        );
    let mut current: Option<RootId> = None;
    for (root, path, kind) in conflicted {
        if current.as_ref() != Some(root) {
            current = Some(root.clone());
            list = list.child(
                text(root.name())
                    .text_size(px(data::TYPE_SECTION))
                    .text_color(t.muted_foreground),
            );
        }
        let label = match kind {
            Some(kind) => format!("{} — {}", path.display(), kind_word(*kind)),
            None => path.display().to_string(),
        };
        let is_selected = path.display().to_string() == selected;
        let press = handle.clone();
        let open_root = root.clone();
        let open_path = path.clone();
        list = list.child(row(RowProps::new()
            .state(if is_selected {
                RowState::Selected
            } else {
                RowState::Default
            })
            .test_id(&test_ids::conflicts(format!(
                "file:{}:{}",
                root.name(),
                path.display()
            )))
            .on_press(move || open_resolver(&press, &open_root, &open_path))
            .child(
                text(label)
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.foreground),
            )));
    }
    let auto = handle.peek(|s| s.ui.auto_merged_files.clone());
    list = list
        .child(
            text("AUTO-MERGED")
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        )
        .child(
            text(if auto.len() == 1 {
                "1 file".to_string()
            } else {
                format!("{} files", auto.len())
            })
            .text_size(px(data::TYPE_CHIP))
            .text_color(t.muted_foreground),
        );
    for path in auto {
        list = list.child(
            text(path.display().to_string())
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.foreground),
        );
    }
    list
}

fn block_controls(handle: AppHandle) -> impl IntoElement {
    let conflicts = handle.peek(|s| {
        s.ui.conflict_segs
            .iter()
            .filter(|(_, _, is_conflict)| *is_conflict)
            .count()
    });
    view().flex_1().h(px(BLOCK_ROWS)).child(
        virtual_list(conflicts, {
            let each = handle.clone();
            move |at| {
                let index = at.index();
                block_row(each.clone(), index).into_element()
            }
        })
        .estimated_item_size(px(24.0))
        .flex_1(),
    )
}

fn block_row(handle: AppHandle, index: usize) -> impl IntoElement {
    let t = theme();
    let number = index + 1;
    let chosen = handle.peek(|s| s.ui.conflict_res.get(index).copied().flatten());
    let active = handle.peek(|s| s.ui.conflict_resolver_active_idx) == index;
    let mut row_view = view()
        .flex_row()
        .items_center()
        .gap(px(data::DENSITY_LIST_GAP))
        .h(px(24.0))
        .px(px(data::DENSITY_COMPACT_BUTTON_PAD_X))
        .test_id(&test_ids::conflicts(format!("block:{number}")))
        .child(
            text(format!("Conflict {number}"))
                .text_size(px(data::TYPE_CONTROL))
                .text_color(if active {
                    t.foreground
                } else {
                    t.muted_foreground
                })
                .w(px(90.0)),
        );
    for (label, choice) in [
        (format!("Take ours {number}"), CHOICE_OURS),
        (format!("Take theirs {number}"), CHOICE_THEIRS),
        (format!("Take both {number}"), CHOICE_BOTH_THEIRS_FIRST),
    ] {
        let press = handle.clone();
        row_view = row_view.child(button(
            ButtonProps::new(label)
                .size(crate::ui::ControlSize::Compact)
                .variant(ButtonVariant::Ghost)
                .test_id(&test_ids::conflicts(format!("take:{choice}:{number}")))
                .on_press(move || take_side(&press, index, choice)),
        ));
    }
    let undo = handle.clone();
    row_view = row_view
        .child(button(
            ButtonProps::new(format!("Undo {number}"))
                .size(crate::ui::ControlSize::Compact)
                .variant(ButtonVariant::Ghost)
                .disabled(chosen.is_none())
                .test_id(&test_ids::conflicts(format!("undo:{number}")))
                .on_press(move || undo_block(&undo, index)),
        ))
        .child(
            text(match chosen {
                Some(CHOICE_OURS) => "yours",
                Some(CHOICE_THEIRS) => "theirs",
                Some(CHOICE_BOTH_THEIRS_FIRST) => "both, theirs first",
                Some(_) => "resolved",
                None => "unresolved",
            })
            .text_size(px(data::TYPE_CHIP))
            .text_color(if chosen.is_some() {
                data::STATE_SUCCESS
            } else {
                data::STATE_WARNING
            })
            .flex_1(),
        );
    row_view
}

fn merge_dialog(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let (path, unresolved, ours, theirs, result) = handle.peek(|s| {
        (
            s.ui.conflict_open
                .as_ref()
                .map(|(_, path)| path.display().to_string())
                .unwrap_or_default(),
            s.ui.conflict_res.iter().filter(|r| r.is_none()).count(),
            block_side(&s.ui.conflict_segs, 0, true),
            block_side(&s.ui.conflict_segs, 0, false),
            block_result(&s.ui.conflict_segs, &s.ui.conflict_res, 0),
        )
    });
    let body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .test_id(&test_ids::conflicts("merge-editor"))
        .child(
            text(format!(
                "{} · {}",
                path,
                if unresolved == 1 {
                    "1 conflict remaining".to_string()
                } else {
                    format!("{unresolved} conflicts remaining")
                }
            ))
            .text_size(px(data::TYPE_CONTROL))
            .text_color(t.foreground),
        )
        .child(
            nsh_base::h_resizable()
                .handle_class("w-px")
                .h(px(220.0))
                .child(
                    nsh_base::resizable_panel().child(
                        view()
                            .flex_col()
                            .flex_1()
                            .child(pane_header("Local (Yours)"))
                            .child(read_only_editor(ours, "Local (Yours)")),
                    ),
                )
                .child(
                    nsh_base::resizable_panel().child(
                        view()
                            .flex_col()
                            .flex_1()
                            .child(pane_header("Result"))
                            .child(read_only_editor(result, "Result")),
                    ),
                )
                .child(
                    nsh_base::resizable_panel().child(
                        view()
                            .flex_col()
                            .flex_1()
                            .child(pane_header("Incoming (Theirs)"))
                            .child(read_only_editor(theirs, "Incoming (Theirs)")),
                    ),
                ),
        )
        .child(merge_blocks(handle.clone()))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .child(button(
                    ButtonProps::new("Apply")
                        .variant(ButtonVariant::Primary)
                        .disabled(unresolved > 0)
                        .test_id(&test_ids::conflicts("merge-apply"))
                        .on_press({
                            let apply = handle.clone();
                            move || {
                                let (root, path, content) = apply.peek(|s| {
                                    (
                                        s.ui.conflict_open.as_ref().map(|(root, _)| root.clone()),
                                        s.ui.conflict_open.as_ref().map(|(_, path)| path.clone()),
                                        compose(&s.ui.conflict_segs, &s.ui.conflict_res),
                                    )
                                });
                                if let (Some(root), Some(path)) = (root, path) {
                                    apply.update(move |s| {
                                        s.dispatch(shell_writes::conflict::apply_merge_resolution(
                                            &root, &path, &content,
                                        ));
                                        s.ui.conflict_open = None;
                                    });
                                }
                            }
                        }),
                ))
                .child(button(
                    ButtonProps::new("Cancel")
                        .variant(ButtonVariant::Ghost)
                        .test_id(&test_ids::conflicts("merge-cancel"))
                        .on_press({
                            let cancel = handle.clone();
                            move || cancel.update(|s| s.ui.conflict_open = None)
                        }),
                )),
        );
    let _ = t;
    dialog(
        DialogProps::new("merge-editor", format!("Merge: {path}"))
            .width(MERGE_WIDTH)
            .on_cancel({
                let cancel = handle.clone();
                move || cancel.update(|s| s.ui.conflict_open = None)
            })
            .child(body),
    )
    .into_element()
}

fn merge_blocks(handle: AppHandle) -> impl IntoElement {
    let conflicts = handle.peek(|s| {
        s.ui.conflict_segs
            .iter()
            .filter(|(_, _, is_conflict)| *is_conflict)
            .count()
    });
    virtual_list(conflicts, {
        let each = handle.clone();
        move |at| merge_block_row(each.clone(), at.index()).into_element()
    })
    .estimated_item_size(px(24.0))
    .h(px(BLOCK_ROWS))
}

fn merge_block_row(handle: AppHandle, index: usize) -> impl IntoElement {
    let t = theme();
    let number = index + 1;
    let mut row_view = view()
        .flex_row()
        .items_center()
        .gap(px(data::DENSITY_LIST_GAP))
        .h(px(24.0))
        .test_id(&test_ids::conflicts(format!("merge-block:{number}")))
        .child(
            text(format!("Conflict {number}"))
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.foreground)
                .w(px(90.0)),
        );
    for (label, choice) in [
        (format!("Accept Yours {number}"), CHOICE_OURS),
        (format!("Accept Theirs {number}"), CHOICE_THEIRS),
        (format!("Ignore {number}"), 2u8),
    ] {
        let press = handle.clone();
        row_view = row_view.child(button(
            ButtonProps::new(label)
                .size(crate::ui::ControlSize::Compact)
                .variant(ButtonVariant::Ghost)
                .test_id(&test_ids::conflicts(format!(
                    "merge-take:{choice}:{number}"
                )))
                .on_press(move || {
                    press.update(move |s| {
                        if let Some(slot) = s.ui.conflict_res.get_mut(index) {
                            *slot = Some(choice);
                        }
                        s.ui.conflict_text = compose(&s.ui.conflict_segs, &s.ui.conflict_res);
                    });
                }),
        ));
    }
    row_view
}

/// The conflicts route: each conflicted file with its four routes, and the
/// per-root simple resolve.
pub fn conflicts_screen(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let paint = handle.clone();
    view()
        .flex_1()
        .flex_col()
        .gap(px(data::PANEL_PADDING))
        .bg(t.background)
        .p(px(data::PANEL_PADDING))
        .test_id(&test_ids::route("conflicts"))
        .child(
            text("Conflicts")
                .text_size(px(data::TYPE_PANE_TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.foreground),
        )
        .child(dynamic(move || {
            paint.read(|_| ());
            let conflicted = paint.peek(conflicted_files);
            if conflicted.is_empty() {
                return view()
                    .child(
                        text("No conflicted files.")
                            .text_size(px(data::TYPE_CONTROL))
                            .text_color(t.muted_foreground),
                    )
                    .into_element();
            }
            let mut list = view()
                .flex_col()
                .gap(px(data::DENSITY_LIST_GAP))
                .test_id(&test_ids::conflicts("list"));
            let mut current: Option<RootId> = None;
            for (root, path, kind) in &conflicted {
                if current.as_ref() != Some(root) {
                    current = Some(root.clone());
                    let status = paint.peek(|s| s.multi.by_id(root).map(|r| r.status.clone()));
                    let simple = paint.clone();
                    let simple_root = root.clone();
                    list = list
                        .child(
                            text(root.name())
                                .text_size(px(data::TYPE_SECTION))
                                .text_color(t.muted_foreground),
                        )
                        .child(button(
                            ButtonProps::new(format!("Resolve all simple · {}", root.name()))
                                .variant(ButtonVariant::Secondary)
                                .test_id(&test_ids::conflicts(format!(
                                    "resolve-simple:{}",
                                    root.name()
                                )))
                                .on_press(move || {
                                    if let Some(status) = status.clone() {
                                        let root = simple_root.clone();
                                        simple.update(move |s| {
                                            s.dispatch(shell_writes::conflict::resolve_simple(
                                                &root, &status,
                                            ))
                                        });
                                    }
                                }),
                        ));
                }
                let label = match kind {
                    Some(kind) => format!("{} — {}", path.display(), kind_word(*kind)),
                    None => path.display().to_string(),
                };
                let ours = paint.clone();
                let theirs = paint.clone();
                let merge = paint.clone();
                let resolve = paint.clone();
                let row_root = root.clone();
                let ours_path = path.clone();
                let theirs_root = root.clone();
                let theirs_path = path.clone();
                let merge_root = root.clone();
                let merge_path = path.clone();
                let resolve_root = root.clone();
                let resolve_path = path.clone();
                list = list.child(row(RowProps::new()
                    .state(RowState::Default)
                    .test_id(&test_ids::conflicts(format!(
                        "row:{}:{}",
                        root.name(),
                        path.display()
                    )))
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap(px(data::CELL_GAP))
                            .child(
                                text(label)
                                    .text_size(px(data::TYPE_CONTROL))
                                    .text_color(t.foreground)
                                    .flex_1(),
                            )
                            .child(button(
                                ButtonProps::new("Ours")
                                    .size(crate::ui::ControlSize::Compact)
                                    .variant(ButtonVariant::Ghost)
                                    .test_id(&test_ids::conflicts(format!(
                                        "ours:{}",
                                        path.display()
                                    )))
                                    .on_press(move || {
                                        let root = row_root.clone();
                                        let path = ours_path.clone();
                                        ours.update(move |s| {
                                            s.dispatch(shell_writes::conflict::accept_ours(
                                                &root, &path,
                                            ))
                                        })
                                    }),
                            ))
                            .child(button(
                                ButtonProps::new("Theirs")
                                    .size(crate::ui::ControlSize::Compact)
                                    .variant(ButtonVariant::Ghost)
                                    .test_id(&test_ids::conflicts(format!(
                                        "theirs:{}",
                                        path.display()
                                    )))
                                    .on_press(move || {
                                        let root = theirs_root.clone();
                                        let path = theirs_path.clone();
                                        theirs.update(move |s| {
                                            s.dispatch(shell_writes::conflict::accept_theirs(
                                                &root, &path,
                                            ))
                                        })
                                    }),
                            ))
                            .child(button(
                                ButtonProps::new("Merge…")
                                    .size(crate::ui::ControlSize::Compact)
                                    .variant(ButtonVariant::Secondary)
                                    .test_id(&test_ids::conflicts(format!(
                                        "merge:{}",
                                        path.display()
                                    )))
                                    .on_press(move || {
                                        open_merge_editor(&merge, &merge_root, &merge_path)
                                    }),
                            ))
                            .child(button(
                                ButtonProps::new("Resolve…")
                                    .size(crate::ui::ControlSize::Compact)
                                    .variant(ButtonVariant::Primary)
                                    .test_id(&test_ids::conflicts(format!(
                                        "resolve:{}",
                                        path.display()
                                    )))
                                    .on_press(move || {
                                        open_resolver(&resolve, &resolve_root, &resolve_path)
                                    }),
                            )),
                    )));
            }
            list.into_element()
        }))
}
