use crate::theme::Palette;
use crate::ui::components::{self, KitButton};
use crate::ui::conflicts::{
    CHOICE_BOTH_THEIRS_FIRST, CHOICE_OURS, CHOICE_THEIRS, compose, resolved_block_text,
    seed_conflict_state,
};
use crate::ui::kit::conflict_pane::{self, Side};
use egui::{Key, Modifiers, RichText, TextEdit, Ui};
use turbogit_app::OwnedOperation;
use turbogit_app::operation::Operation;
use turbogit_app::shell_writes::conflict as writes;
use turbogit_app::state::{AppState, BulkApplyScope, ConflictBulkReport};
use turbogit_domain::model::{
    CONFLICT_MARKER_OURS, CONFLICT_MARKER_SEPARATOR, CONFLICT_MARKER_THEIRS, ConflictKind, RootId,
};

use std::path::{Path, PathBuf};

fn result_text_edit(ui: &mut Ui, state: &mut AppState) {
    let mut text = state.ui.conflict_text.clone();
    let resp = TextEdit::multiline(&mut text)
        .desired_width(ui.available_width())
        .desired_rows(6)
        .font(egui::TextStyle::Monospace)
        .show(ui);
    if resp.response.changed() {
        state.ui.conflict_text = text;
        state.ui.conflict_resolver_edited = true;
    }
}

fn conflict_block(
    ui: &mut Ui,
    state: &mut AppState,
    ci: usize,
    block: usize,
    ours: &str,
    theirs: &str,
) {
    let res_i = ci;
    let active = state.ui.conflict_resolver_active_idx == res_i;
    let chosen = state.ui.conflict_res.get(res_i).copied().flatten();

    if chosen.is_none() {
        conflict_pane::equal_panes(ui, |cols| {
            conflict_pane::marker_strip(&mut cols[0], CONFLICT_MARKER_OURS);
            conflict_pane::marker_strip(&mut cols[1], CONFLICT_MARKER_SEPARATOR);
            conflict_pane::marker_strip(&mut cols[2], CONFLICT_MARKER_THEIRS);
        });
    }
    conflict_pane::equal_panes(ui, |cols| {
        if chosen != Some(CHOICE_THEIRS) {
            conflict_pane::side_section(&mut cols[0], ours, Side::Local);
        }
        if active {
            result_text_edit(&mut cols[1], state);
        } else {
            let result_text = chosen.and_then(|choice| resolved_block_text(ours, theirs, choice));
            conflict_pane::result_cell(&mut cols[1], result_text);
        }
        if chosen != Some(CHOICE_OURS) {
            conflict_pane::side_section(&mut cols[2], theirs, Side::Incoming);
        }
    });
    if active {
        ui.horizontal(|ui| {
            if ui.button(format!("Take ours {block}")).clicked() {
                accept_side(state, res_i, CHOICE_OURS);
            }
            if ui.button(format!("Take theirs {block}")).clicked() {
                accept_side(state, res_i, CHOICE_THEIRS);
            }
            if ui
                .button(format!("Take both {block}"))
                .on_hover_text("Theirs first")
                .clicked()
            {
                resolve(state, res_i, CHOICE_BOTH_THEIRS_FIRST);
            }
            let can_undo = chosen.is_some();
            if ui
                .add_enabled(can_undo, egui::Button::new(format!("Undo {block}")))
                .clicked()
            {
                undo_resolution(state, res_i);
            }
        });
    } else {
        ui.horizontal(|ui| {
            ui.label(format!("Conflict {block}"));
            if ui.button("Take ours").clicked() {
                accept_side(state, res_i, CHOICE_OURS);
            }
            if ui.button("Take theirs").clicked() {
                accept_side(state, res_i, CHOICE_THEIRS);
            }
            if ui.button("Take both").clicked() {
                resolve(state, res_i, CHOICE_BOTH_THEIRS_FIRST);
            }
        });
    }
    ui.add_space(4.0);
}

fn file_has_merged_changes(state: &AppState) -> bool {
    state.ui.conflict_resolver_edited || state.ui.conflict_res.iter().any(|r| r.is_some())
}

fn accept_side(state: &mut AppState, seg_idx: usize, choice: u8) {
    if file_has_merged_changes(state) {
        state.ui.conflict_overwrite_pending = Some((seg_idx, choice));
    } else {
        resolve(state, seg_idx, choice);
    }
}

fn resolve(state: &mut AppState, seg_idx: usize, choice: u8) {
    while state.ui.conflict_resolver_undo.len() <= seg_idx {
        state.ui.conflict_resolver_undo.push(None);
    }
    let prev = state.ui.conflict_res.get(seg_idx).copied().flatten();
    state.ui.conflict_resolver_undo[seg_idx] = prev;

    state.ui.conflict_res[seg_idx] = Some(choice);
    if !state.ui.conflict_resolver_edited {
        state.ui.conflict_text = compose(&state.ui.conflict_segs, &state.ui.conflict_res);
    }

    if state.ui.conflict_resolver_auto_advance {
        advance_active(state);
    }
}

fn undo_resolution(state: &mut AppState, seg_idx: usize) {
    state.ui.conflict_res[seg_idx] = None;
    if !state.ui.conflict_resolver_edited {
        state.ui.conflict_text = compose(&state.ui.conflict_segs, &state.ui.conflict_res);
    }
}
fn advance_active(state: &mut AppState) {
    let n = state.ui.conflict_res.len();
    if n == 0 {
        return;
    }
    let cur = state.ui.conflict_resolver_active_idx.min(n - 1);
    let mut i = (cur + 1) % n;
    let start = i;
    loop {
        if state.ui.conflict_res[i].is_none() {
            state.ui.conflict_resolver_active_idx = i;
            return;
        }
        i = (i + 1) % n;
        if i == start {
            return;
        }
    }
}

pub(crate) fn open_resolver(state: &mut AppState, root: &Path, path: &Path) {
    let Some(res) = seed_conflict_state(state, root, path) else {
        return;
    };
    state.ui.conflict_resolver_undo = res.iter().map(|_| None).collect();
    state.ui.conflict_text = compose(&state.ui.conflict_segs, &res);
    state.ui.conflict_resolver_edited = false;
    state.ui.conflict_resolver_active_idx = 0;
    state.ui.conflict_resolver_selected = Some(path.to_path_buf());
    state.ui.conflict_overwrite_pending = None;
    state.ui.conflict_bulk_report = None;
    state.ui.conflict_resolver_open = true;
}

pub fn render(ui: &mut Ui, state: &mut AppState) {
    let id = match &state.selected_root {
        Some(id) => id.clone(),
        None => return,
    };
    let conflicted: Vec<(PathBuf, Option<ConflictKind>)> = state
        .multi
        .by_id(&id)
        .map(|r| {
            r.status
                .conflicted
                .iter()
                .map(|p| (p.clone(), r.status.conflict_kind(p)))
                .collect()
        })
        .unwrap_or_default();
    if conflicted.is_empty() && !state.ui.conflict_resolver_open {
        return;
    }
    let root = state.selected_path();
    if let Some(r) = &root {
        if matches!(state.holding_operation(r), Some(OwnedOperation::Merge)) {
            state.ui.merge_in_progress = true;
            let root_id = RootId(r.clone().into());
            let conflicted_paths: Vec<PathBuf> =
                conflicted.iter().map(|(p, _)| p.clone()).collect();
            if let Some(list) = state.auto_merged_files(&root_id, &conflicted_paths) {
                state.ui.auto_merged_files = list;
            }
        } else {
            state.ui.merge_in_progress = false;
            state.ui.auto_merged_files.clear();
        }
    }

    if !state.ui.conflict_resolver_open {
        return;
    }
    let mut keep_open = true;

    egui::Window::new("Resolve conflicts")
        .open(&mut keep_open)
        .default_width(1100.0)
        .show(ui.ctx(), |ui| {
            render_resolver_body(ui, state, &conflicted);
        });
    if !keep_open {
        state.ui.conflict_resolver_open = false;
    }
    render_overwrite_confirm(ui, state);
}

fn render_overwrite_confirm(ui: &mut Ui, state: &mut AppState) {
    let Some((seg, choice)) = state.ui.conflict_overwrite_pending else {
        return;
    };
    let side = if choice == CHOICE_THEIRS {
        "theirs"
    } else {
        "yours"
    };
    let mut open = true;
    egui::Window::new("Overwrite Changes")
        .open(&mut open)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .resizable(false)
        .show(ui.ctx(), |ui| {
            ui.label("This file already has merged changes.");
            ui.label(format!(
                "Accepting the {side} side will discard and overwrite them."
            ));
            ui.horizontal(|ui| {
                if components::kit_button(ui, KitButton::Primary, "Discard and Accept").clicked() {
                    state.ui.conflict_overwrite_pending = None;
                    resolve(state, seg, choice);
                }
                if overwrite_safe_button(ui, "Keep Merged Changes").clicked() {
                    state.ui.conflict_overwrite_pending = None;
                }
            });
        });
    if !open {
        state.ui.conflict_overwrite_pending = None;
    }
}

fn overwrite_safe_button(ui: &mut Ui, label: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(label)).fill(Palette::RAISED_ON_CARD))
}
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

fn render_file_list(
    ui: &mut Ui,
    state: &mut AppState,
    conflicted: &[(PathBuf, Option<ConflictKind>)],
) {
    ui.label(
        RichText::new("CONFLICTED FILES")
            .size(crate::theme::TYPE_CONTROL)
            .color(Palette::INK_2),
    );
    ui.label(RichText::new(format!("{} / {} resolved", 0, conflicted.len())).color(Palette::INK_2));
    for (path, kind) in conflicted {
        let label = path.display().to_string();
        let is_selected = state.ui.conflict_resolver_selected.as_ref() == Some(path);
        let row = match kind {
            Some(kind) => format!("{label} — {}", kind_word(*kind)),
            None => label,
        };
        let resp = ui.selectable_label(is_selected, row);
        if resp.clicked()
            && let Some(root) = state.selected_path()
        {
            let path = path.clone();
            open_resolver(state, &root, &path);
        }
    }

    ui.add_space(8.0);
    let n_auto = state.ui.auto_merged_files.len();
    ui.label(
        RichText::new("AUTO-MERGED")
            .size(crate::theme::TYPE_CONTROL)
            .color(Palette::INK_2),
    );
    ui.label(RichText::new(components::counted(n_auto, "file", "files")).color(Palette::INK_2));
    for path in &state.ui.auto_merged_files {
        ui.label(path.display().to_string());
    }
}

/// The bulk control: `Apply to all N remaining` with its scope option, the
/// deliberate fold of the reference's `Apply Non-Conflicting Changes` /
/// `Resolve All Simple Conflicts` pair into one control, the side scope being
/// an option on it rather than two more buttons. It
/// sits in the file column under the lists because the footer row is the
/// window's tightest fit: an added footer row lays out past the window's
/// clip rect, where it still exists but can be neither seen nor clicked.
///
/// The button stays visible at zero remaining: a click
/// with nothing left to resolve REPORTS the negative result under the
/// control instead of staying silent, which a control that vanished could
/// never do. The scope option, meaningless with nothing remaining, is
/// what hides.
fn render_bulk_control(ui: &mut Ui, state: &mut AppState) {
    let n_remaining = state.ui.conflict_res.iter().filter(|r| r.is_none()).count();
    ui.add_space(8.0);
    ui.separator();
    if n_remaining > 0 {
        ui.label(RichText::new("Fill remaining with").color(Palette::INK_2));
        ui.radio_value(
            &mut state.ui.conflict_resolver_scope,
            BulkApplyScope::ActiveChoice,
            "Active choice",
        );
        ui.radio_value(
            &mut state.ui.conflict_resolver_scope,
            BulkApplyScope::Ours,
            "Ours",
        );
        ui.radio_value(
            &mut state.ui.conflict_resolver_scope,
            BulkApplyScope::Theirs,
            "Theirs",
        );
    }
    if ui
        .button(format!("Apply to all {n_remaining} remaining"))
        .clicked()
    {
        apply_to_all_remaining(state);
    }
    match state.ui.conflict_bulk_report {
        Some(ConflictBulkReport::AllResolved) => {
            ui.label(
                RichText::new("All conflicts were resolved automatically").color(Palette::INK_2),
            );
        }
        Some(ConflictBulkReport::NoneResolved) => {
            ui.label(
                RichText::new("No conflicts were resolved automatically").color(Palette::INK_2),
            );
        }
        None => {}
    }
}

fn render_resolver_body(
    ui: &mut Ui,
    state: &mut AppState,
    conflicted: &[(PathBuf, Option<ConflictKind>)],
) {
    let path_disp = state
        .ui
        .conflict_resolver_selected
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(no file)".to_string());
    let unresolved = state.ui.conflict_res.iter().filter(|r| r.is_none()).count();
    let total = state.ui.conflict_res.len();
    let counter = if total == 0 {
        "0 conflicts".to_string()
    } else {
        format!(
            "{} remaining",
            components::counted(unresolved, "conflict", "conflicts")
        )
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(path_disp).strong().color(Palette::INK));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if state.ui.merge_in_progress && ui.button("Abort merge").clicked() {
                abort_merge(state);
            }
            if state.ui.merge_in_progress
                && ui
                    .add_enabled(unresolved == 0, egui::Button::new("Continue merge"))
                    .clicked()
            {
                continue_merge(state);
            }
        });
    });
    ui.label(RichText::new(counter).color(Palette::INK_2));
    ui.separator();

    // Left file list (auto-sized) + right panes. The column layout is
    // spelled out: `allocate_ui` alone inherits the caller's layout, so
    // inside this horizontal row the file list would lay out left-to-right
    // and overflow its 240pt strip, starving the panes of width.
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(240.0, ui.available_height()),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                render_file_list(ui, state, conflicted);
                render_bulk_control(ui, state);
            },
        );
        ui.vertical(|ui| {
            render_panes(ui, state);
        });
    });

    ui.separator();
    let total = state.ui.conflict_res.len();
    let active = state.ui.conflict_resolver_active_idx;
    ui.horizontal(|ui| {
        let label = format!("Conflict {}", active + 1);
        ui.label(RichText::new(label).color(Palette::INK_2));
        if ui
            .add_enabled(active > 0, egui::Button::new("< Prev"))
            .clicked()
        {
            state.ui.conflict_resolver_active_idx -= 1;
        }
        if ui
            .add_enabled(active + 1 < total, egui::Button::new("Next >"))
            .clicked()
        {
            state.ui.conflict_resolver_active_idx += 1;
        }
        if ui.button("Edit manually").clicked() {
            state.ui.conflict_resolver_edited = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.scope(|ui| {
                if unresolved > 0 {
                    ui.disable();
                }
                let resp = components::kit_button(
                    ui,
                    if unresolved == 0 {
                        KitButton::Primary
                    } else {
                        KitButton::Quiet
                    },
                    "Apply",
                );
                if unresolved == 0 && resp.clicked() {
                    apply_resolution(state);
                }
            });
            ui.checkbox(
                &mut state.ui.conflict_resolver_auto_advance,
                "Auto-advance to next conflict",
            );
        });
    });
}

fn render_panes(ui: &mut Ui, state: &mut AppState) {
    let segs = state.ui.conflict_segs.clone();
    conflict_pane::equal_panes(ui, |cols| {
        conflict_pane::pane_header(&mut cols[0], "Local (Yours)", false);
        conflict_pane::pane_header(&mut cols[1], "Result", true);
        conflict_pane::pane_header(&mut cols[2], "Incoming (Theirs)", false);
    });

    if ui.input(|i| i.key_pressed(Key::ArrowUp) && i.modifiers == Modifiers::ALT)
        && state.ui.conflict_resolver_active_idx > 0
    {
        state.ui.conflict_resolver_active_idx -= 1;
    }
    if ui.input(|i| i.key_pressed(Key::ArrowDown) && i.modifiers == Modifiers::ALT)
        && state.ui.conflict_resolver_active_idx + 1 < state.ui.conflict_res.len()
    {
        state.ui.conflict_resolver_active_idx += 1;
    }

    let mut ci = 0usize;
    for (ours, theirs, is_conf) in segs.iter() {
        if !*is_conf {
            let text = ours.as_str();
            conflict_pane::equal_panes(ui, |cols| {
                for col in cols.iter_mut() {
                    col.label(RichText::new(text).monospace().color(Palette::INK_2));
                }
            });
        } else {
            let block = ci + 1;
            conflict_block(ui, state, ci, block, ours, theirs);
            ci += 1;
        }
    }
}

fn apply_to_all_remaining(state: &mut AppState) {
    let active = state.ui.conflict_resolver_active_idx;
    let choice = match state.ui.conflict_resolver_scope {
        BulkApplyScope::Ours => CHOICE_OURS,
        BulkApplyScope::Theirs => CHOICE_THEIRS,
        BulkApplyScope::ActiveChoice => state
            .ui
            .conflict_resolver_undo
            .get(active)
            .copied()
            .flatten()
            .or_else(|| state.ui.conflict_res.get(active).copied().flatten())
            .unwrap_or(CHOICE_OURS),
    };
    let mut filled = 0usize;
    for i in 0..state.ui.conflict_res.len() {
        if state.ui.conflict_res[i].is_none() {
            state.ui.conflict_resolver_undo[i] = state.ui.conflict_res[i];
            state.ui.conflict_res[i] = Some(choice);
            filled += 1;
        }
    }
    if !state.ui.conflict_resolver_edited {
        state.ui.conflict_text = compose(&state.ui.conflict_segs, &state.ui.conflict_res);
    }
    state.ui.conflict_bulk_report = Some(if filled == 0 {
        ConflictBulkReport::NoneResolved
    } else {
        ConflictBulkReport::AllResolved
    });
}

fn apply_resolution(state: &mut AppState) {
    let content = state.ui.conflict_text.clone();
    let r = state.selected_root.clone();
    let p = state.ui.conflict_resolver_selected.clone();
    if let (Some(r), Some(p)) = (r, p) {
        state.dispatch(writes::apply_merge_resolution(&r, &p, &content));
    }
    // Keep the resolver open while a merge is still pending Continue,
    // the file is staged but the merge commit hasn't been written yet.
    if !state.ui.merge_in_progress {
        state.ui.conflict_resolver_open = false;
    }
}

/// The held state the resolver's two header verbs resolve.
///
/// Read from git rather than hard-wired to `merge`: the resolver opens on a
/// merge handoff today, but a paused rebase leaves the same unmerged paths, and
/// `git merge --abort` in a repository that is rebasing is a second failure on
/// top of the one being recovered. Falls back to `Merge` when nothing is held
/// anymore, which is what the header says and what the operation then reports.
fn held_operation(state: &turbogit_app::state::AppState) -> OwnedOperation {
    state
        .selected_root
        .as_ref()
        .and_then(|root| state.holding_operation(root.as_path()))
        .unwrap_or(OwnedOperation::Merge)
}

fn abort_merge(state: &mut AppState) {
    let owner = held_operation(state);
    if let Some(r) = state.selected_root.clone() {
        state.dispatch(Operation::Resolve {
            root: r,
            owner,
            verb: turbogit_app::operation::LifecycleVerb::Abort,
        });
    }
    state.ui.conflict_resolver_open = false;
}

fn continue_merge(state: &mut AppState) {
    let owner = held_operation(state);
    if let Some(r) = state.selected_root.clone() {
        state.dispatch(Operation::Resolve {
            root: r,
            owner,
            verb: turbogit_app::operation::LifecycleVerb::Continue,
        });
    }
    state.ui.conflict_resolver_open = false;
}
