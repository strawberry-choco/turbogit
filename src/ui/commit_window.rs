//! Commit tool window redesigned onto canonical changelist buckets (issue #11).
//!
//! Layout: left pane = collapsible canonical groups ("Default Changelist",
//! "Unversioned Files", "Merge conflicts") with count badges. For multi-root
//! projects each group nests per-root sub-groups with count badges and a
//! select-all checkbox; single-root projects list files flat. Right pane =
//! unified diff preview of the selected file above the message editor and the
//! Amend / Commit / Commit-and-Push action row. Commit stays disabled until a
//! non-empty message AND at least one included change exist. User-created
//! changelists remain backlog.

use crate::core::changes;
use crate::model::{Change, ChangeStatus, RootId};
use crate::state::{AppState, Dialog, PendingConfirm};
use crate::theme::Palette;
use crate::ui::icons::{self, Icon};
use egui::{Color32, Ui};

/// Canonical bucket names (user-created changelists are backlog).
pub const DEFAULT_CHANGELIST: &str = "Default Changelist";
pub const UNVERSIONED_FILES: &str = "Unversioned Files";
pub const MERGE_CONFLICTS: &str = "Merge conflicts";

/// One canonical bucket: its name plus that root's changes.
struct Bucket {
    name: &'static str,
    root_id: RootId,
    changes: Vec<Change>,
}

/// Split one root's status into the three canonical buckets (empty buckets
/// are dropped so the tree only shows groups that have content).
fn canonical_buckets(state: &AppState) -> Vec<Bucket> {
    let mut out = Vec::new();
    for root in &state.multi.roots {
        let mut default = Vec::new();
        let mut unversioned = Vec::new();
        let mut conflicts = Vec::new();
        for c in &root.status.changes {
            match c.status {
                ChangeStatus::Conflicted => conflicts.push(c.clone()),
                ChangeStatus::Unversioned => unversioned.push(c.clone()),
                // Ignored files never belong in the commit window.
                ChangeStatus::Ignored => {}
                _ => default.push(c.clone()),
            }
        }
        for (name, changes) in [
            (DEFAULT_CHANGELIST, default),
            (UNVERSIONED_FILES, unversioned),
            (MERGE_CONFLICTS, conflicts),
        ] {
            if !changes.is_empty() {
                out.push(Bucket {
                    name,
                    root_id: root.id.clone(),
                    changes,
                });
            }
        }
    }
    out
}

/// Collect the `Change` objects of the selected root whose path is included.
fn selected_changes(state: &AppState) -> Vec<Change> {
    let mut out = Vec::new();
    if let Some(id) = &state.selected_root {
        if let Some(root) = state.multi.by_id(id) {
            for c in &root.status.changes {
                if state.ui.selected.contains(&root.id.0.join(&c.path)) {
                    out.push(c.clone());
                }
            }
        }
    }
    out
}

/// Toggle inclusion of one change (keyed by absolute path so multi-root
/// projects never conflate same-named files across roots).
fn toggle_included(state: &mut AppState, root: &RootId, c: &Change, include: bool) {
    let key = root.0.join(&c.path);
    if include {
        state.ui.selected.insert(key);
    } else {
        state.ui.selected.remove(&key);
    }
}

pub fn show(ui: &mut Ui, state: &mut AppState) {
    // Two panes: changelists on the left, diff preview + message editor on
    // the right (issue #11 layout).
    ui.columns(2, |cols| {
        changelist_pane(&mut cols[0], state);
        preview_and_editor_pane(&mut cols[1], state);
    });

    // Conflict resolution tools (ours / theirs / 3-way merge editor).
    crate::ui::conflicts::render(ui, state);

    if let Some(err) = &state.last_error {
        ui.separator();
        ui.colored_label(Color32::RED, format!("⚠ {err}"));
    }
}

// ------------------------------------------------------- changelist pane --

fn changelist_pane(ui: &mut Ui, state: &mut AppState) {
    ui.heading("Commit");
    recent_messages_row(ui, state);
    staging_toolbar_row(ui, state);

    let Some(root_id) = state.selected_root.clone() else {
        ui.colored_label(Color32::GRAY, "Select a repository to see changes.");
        return;
    };
    if state.multi.by_id(&root_id).is_none() {
        return;
    }

    let multi_root = state.multi.roots.len() > 1;
    let buckets = canonical_buckets(state);

    egui::ScrollArea::vertical().show(ui, |ui| {
        if buckets.is_empty() {
            ui.colored_label(Color32::GRAY, "No local changes.");
            return;
        }
        for bucket in &buckets {
            let header = format!("{} ({})", bucket.name, bucket.changes.len());
            egui::CollapsingHeader::new(header)
                .default_open(true)
                .show(ui, |ui| {
                    if multi_root {
                        root_subgroup(ui, state, bucket);
                    } else {
                        for c in &bucket.changes {
                            change_row(ui, state, &bucket.root_id, c);
                        }
                    }
                });
        }
    });
}

/// Per-root sub-group with count badge + select-all (multi-root projects).
fn root_subgroup(ui: &mut Ui, state: &mut AppState, bucket: &Bucket) {
    let root_name = bucket.root_id.name();
    let all_included = bucket
        .changes
        .iter()
        .all(|c| state.ui.selected.contains(&bucket.root_id.0.join(&c.path)));
    let any_included = bucket
        .changes
        .iter()
        .any(|c| state.ui.selected.contains(&bucket.root_id.0.join(&c.path)));

    ui.horizontal(|ui| {
        let mut select_all = all_included;
        if ui
            .checkbox(&mut select_all, format!("Select all {root_name}"))
            .changed()
        {
            for c in &bucket.changes {
                toggle_included(state, &bucket.root_id, c, select_all);
            }
        }
        // Count badge (shaded when partially selected).
        let badge = format!("{root_name} ({})", bucket.changes.len());
        let tint = if any_included && !all_included {
            Color32::from_rgb(230, 170, 80)
        } else {
            Color32::from_gray(150)
        };
        ui.colored_label(tint, badge);
    });
    ui.indent(
        ui.id().with(("subgroup", &bucket.root_id, bucket.name)),
        |ui| {
            for c in &bucket.changes {
                change_row(ui, state, &bucket.root_id, c);
            }
        },
    );
}

/// Status glyph + semantic tint for a change row (Lucide tokens, issue #7).
fn status_glyph(status: ChangeStatus) -> (Icon, Color32) {
    match status {
        ChangeStatus::Modified => (Icon::FILE, Palette::STATE_WARNING),
        ChangeStatus::Added => (Icon::FILE_PLUS, Palette::STATE_SUCCESS),
        ChangeStatus::Deleted => (Icon::FILE_MINUS, Palette::STATE_ERROR),
        ChangeStatus::Renamed => (Icon::ARROW_RIGHT_LEFT, Palette::BRAND),
        ChangeStatus::Copied => (Icon::FILES, Palette::STATE_SUCCESS),
        ChangeStatus::Unversioned => (Icon::PLUS_CIRCLE, Palette::INK_2),
        ChangeStatus::Ignored => (Icon::EYE_OFF, Palette::INK_3),
        ChangeStatus::Conflicted => (Icon::FILE_WARNING, Palette::STATE_ERROR),
    }
}

/// Badge letter per the canonical M/A/C scheme (spec §commit): conflicts
/// show `C` rather than git porcelain's `U`.
fn badge_letter(status: ChangeStatus) -> &'static str {
    match status {
        ChangeStatus::Conflicted => "C",
        other => other.short(),
    }
}

/// One file row: inclusion checkbox ("M base.txt"), status icon, and a
/// path button that selects the file for the diff preview pane.
fn change_row(ui: &mut Ui, state: &mut AppState, root_id: &RootId, c: &Change) {
    let key = root_id.0.join(&c.path);
    let path_text = c.path.display().to_string();
    ui.horizontal(|ui| {
        let mut included = state.ui.selected.contains(&key);
        let label = format!("{} {}", badge_letter(c.status), path_text);
        if ui.checkbox(&mut included, label).changed() {
            toggle_included(state, root_id, c, included);
        }
        let (glyph, tint) = status_glyph(c.status);
        icons::icon(ui, glyph, 14.0, tint);
        let previewing = state.ui.preview_change.as_ref() == Some(&c.path);
        if ui.selectable_label(previewing, path_text).clicked() {
            state.ui.preview_change = Some(c.path.clone());
        }
    });
}

fn recent_messages_row(ui: &mut Ui, state: &mut AppState) {
    if state.ui.recent_messages.is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.label("Recent:");
        for m in state.ui.recent_messages.iter().take(6) {
            let short = if m.chars().count() > 22 {
                m.chars().take(22).collect::<String>()
            } else {
                m.clone()
            };
            if ui.button(short).clicked() {
                state.ui.commit_message = m.clone();
            }
        }
    });
    ui.separator();
}

fn staging_toolbar_row(ui: &mut Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        if ui
            .button("Stage selected")
            .on_hover_text("Move selected files into the staging area")
            .clicked()
        {
            let ch = selected_changes(state);
            let root = state.selected_path();
            state.run_git("Stage".into(), move |v| {
                if let Some(r) = &root {
                    changes::stage_selected(v, r, &ch)
                } else {
                    Ok(())
                }
            });
        }
        if ui
            .button("Unstage selected")
            .on_hover_text("Remove selected files from the staging area")
            .clicked()
        {
            let ch = selected_changes(state);
            let root = state.selected_path();
            state.run_git("Unstage".into(), move |v| {
                if let Some(r) = &root {
                    changes::unstage_selected(v, r, &ch)
                } else {
                    Ok(())
                }
            });
        }
        if ui
            .button("Discard")
            .on_hover_text("Discard working-tree changes (irreversible)")
            .clicked()
        {
            let ch = selected_changes(state);
            if ch.is_empty() {
                state.ui.toast = Some("Select files to discard.".into());
            } else {
                // Gate behind confirmation — destructive, irreversible.
                state.ui.confirm = Some(PendingConfirm::Discard { changes: ch });
            }
        }
    });
}

// --------------------------------------------- preview + editor pane ------

fn preview_and_editor_pane(ui: &mut Ui, state: &mut AppState) {
    ui.heading("Preview");
    match state.ui.preview_change.clone() {
        Some(path) => {
            ui.label(format!("Preview: {}", path.display()));
            crate::ui::diff::render_diff(ui, state, &None, &None, &Some(path));
        }
        None => {
            ui.colored_label(
                Color32::GRAY,
                "Select a changed file to preview its unified diff.",
            );
        }
    }

    ui.separator();
    message_editor(ui, state);
}

fn message_editor(ui: &mut Ui, state: &mut AppState) {
    ui.label("Commit message:");
    ui.text_edit_multiline(&mut state.ui.commit_message);

    // Subject-length guidance.
    let subject = state.ui.commit_message.lines().next().unwrap_or("");
    let slen = subject.chars().count();
    ui.horizontal(|ui| {
        ui.label(format!("Subject: {slen}/50"));
        if slen > 50 {
            ui.colored_label(Color32::from_rgb(230, 120, 110), "(keep ≤ 50)");
        }
    });

    ui.horizontal(|ui| {
        ui.checkbox(&mut state.ui.amend, "Amend");
        if ui.button("Template").clicked() {
            let tpl = state.settings.commit_template.clone();
            if tpl.is_empty() {
                state.ui.toast = Some("No commit template configured.".into());
            } else if let Ok(content) = std::fs::read_to_string(&tpl) {
                state.ui.commit_message = content;
            } else {
                state.ui.toast = Some(format!("Could not read template: {tpl}"));
            }
        }
        if ui.button("Clear").clicked() {
            state.ui.commit_message.clear();
            state.ui.selected.clear();
        }
    });

    // Action row beneath the preview: primary Commit first.
    ui.separator();
    ui.horizontal(|ui| {
        let can_commit =
            !state.ui.commit_message.trim().is_empty() && !selected_changes(state).is_empty();
        if ui
            .add_enabled(can_commit, egui::Button::new("Commit"))
            .on_hover_text("Commit included changes")
            .clicked()
        {
            do_commit(state, false);
        }
        if ui
            .add_enabled(can_commit, egui::Button::new("Commit and Push..."))
            .on_hover_text("Commit then open the push dialog")
            .clicked()
        {
            do_commit(state, true);
        }
        if ui
            .button("Shelve…")
            .on_hover_text("Shelve selected changes")
            .clicked()
        {
            state.ui.dialog = Some(Dialog::Shelve);
        }
        if ui
            .button("Stash…")
            .on_hover_text("Stash all changes")
            .clicked()
        {
            state.ui.dialog = Some(Dialog::Stash);
        }
    });
}

fn do_commit(state: &mut AppState, and_push: bool) {
    let root = state.selected_path();
    let changes = selected_changes(state);
    let msg = state.ui.commit_message.clone();
    let amend = state.ui.amend;
    let recent = msg.clone();
    state.run_git("Commit".into(), move |v| {
        if let Some(r) = &root {
            let _ = changes::commit_selected(v, r, &msg, &changes, amend)?;
            Ok(())
        } else {
            Ok(())
        }
    });
    // Record recent message + reset fields.
    if !state.ui.recent_messages.contains(&recent) {
        state.ui.recent_messages.insert(0, recent);
        state.ui.recent_messages.truncate(12);
    }
    state.ui.commit_message.clear();
    state.ui.selected.clear();
    state.persist_ui();
    if and_push {
        state.ui.dialog = Some(Dialog::Push);
    }
}
