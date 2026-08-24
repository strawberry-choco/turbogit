//! Commit tool window redesigned onto canonical changelist buckets (issue #11).
//!
//! Layout: a sub-tab strip (Local Changes / Unversioned Files / Shelf / Stash,
//! issue #18) above two panes — collapsible canonical groups with count
//! badges on the left, unified diff preview of the selected file above the
//! message editor and the Amend / Commit / Commit-and-Push action row on the
//! right. Local Changes shows the "Default Changelist" and "Merge conflicts"
//! groups; Unversioned Files lists untracked files includable in commits.
//! For multi-root projects each group nests per-root sub-groups with count
//! badges and a select-all checkbox; single-root projects list files flat.
//! Commit stays disabled until a non-empty message AND at least one included
//! change exist. Shelf / Stash are clickable tabs whose panes are labeled
//! placeholders until Phase J (ADR-0008); the "Advanced options..." control
//! renders per the mockup but is deliberately inert in v1 (ADR-0010).
//! User-created changelists remain backlog.

use crate::core::changes;
use crate::model::{Change, ChangeStatus, RootId};
use crate::root_caches::Affected;
use crate::state::{AppState, CommitSubTab, Dialog, PendingConfirm, Toast};
use crate::theme::Palette;
use crate::ui::icons::{self, Icon};
use egui::{Color32, RichText, Ui};

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
    if let Some(id) = &state.selected_root
        && let Some(root) = state.multi.by_id(id)
    {
        for c in &root.status.changes {
            if state.ui.selected.contains(&root.id.0.join(&c.path)) {
                out.push(c.clone());
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
    sub_tab_strip(ui, state);
    match state.ui.commit_subtab {
        CommitSubTab::LocalChanges => local_changes_body(ui, state),
        CommitSubTab::UnversionedFiles => unversioned_body(ui, state),
        // Phase-J scope: clickable tabs with labeled placeholder panes
        // (ADR-0008) instead of hidden or disabled-looking tabs.
        CommitSubTab::Shelf => placeholder_pane(ui, "Shelf"),
        CommitSubTab::Stash => placeholder_pane(ui, "Stash"),
    }

    // Conflict resolution tools (ours / theirs / 3-way merge editor); the
    // renderer no-ops while nothing is conflicted.
    crate::ui::conflicts::render(ui, state);

    if let Some(err) = &state.last_error {
        ui.separator();
        ui.colored_label(Color32::RED, format!("⚠ {err}"));
    }
}

// --------------------------------------------------------- sub-tab strip --

/// Sub-tabs in strip order. Shelf / Stash are Phase-J placeholders (ADR-0008).
const SUB_TABS: [(CommitSubTab, &str); 4] = [
    (CommitSubTab::LocalChanges, "Local Changes"),
    (CommitSubTab::UnversionedFiles, "Unversioned Files"),
    (CommitSubTab::Shelf, "Shelf"),
    (CommitSubTab::Stash, "Stash"),
];

fn sub_tab_strip(ui: &mut Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        for (tab, label) in SUB_TABS {
            if ui
                .selectable_label(state.ui.commit_subtab == tab, label)
                .clicked()
            {
                state.ui.commit_subtab = tab;
            }
        }
    });
    ui.separator();
}

// ------------------------------------------------------------ tab bodies --

fn local_changes_body(ui: &mut Ui, state: &mut AppState) {
    // Two panes: changelists on the left, diff preview + message editor on
    // the right (issue #11 layout).
    ui.columns(2, |cols| {
        changelist_pane(&mut cols[0], state);
        preview_and_editor_pane(&mut cols[1], state);
    });
}

fn unversioned_body(ui: &mut Ui, state: &mut AppState) {
    // Same two-pane layout; the left pane lists only untracked files so they
    // can be reviewed and included in commits on their own sub-tab.
    ui.columns(2, |cols| {
        unversioned_pane(&mut cols[0], state);
        preview_and_editor_pane(&mut cols[1], state);
    });
}

/// Labeled placeholder for a sub-tab whose feature has not landed yet
/// (ADR-0008): explicit on-screen copy instead of a hidden or disabled tab.
fn placeholder_pane(ui: &mut Ui, name: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(48.0);
        ui.colored_label(Color32::GRAY, format!("{name} arrives in a later phase."));
        ui.add_space(4.0);
        ui.colored_label(
            Color32::GRAY,
            "This pane is a deliberate placeholder (Phase J).",
        );
    });
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

    bucket_groups(ui, state, &canonical_buckets(state), "No local changes.");
}

/// Untracked files of every root as one canonical bucket per root (issue #18:
/// the Unversioned Files sub-tab's active data).
fn unversioned_buckets(state: &AppState) -> Vec<Bucket> {
    let mut out = Vec::new();
    for root in &state.multi.roots {
        let untracked: Vec<Change> = root
            .status
            .changes
            .iter()
            .filter(|c| matches!(c.status, ChangeStatus::Unversioned))
            .cloned()
            .collect();
        if !untracked.is_empty() {
            out.push(Bucket {
                name: UNVERSIONED_FILES,
                root_id: root.id.clone(),
                changes: untracked,
            });
        }
    }
    out
}

fn unversioned_pane(ui: &mut Ui, state: &mut AppState) {
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

    bucket_groups(
        ui,
        state,
        &unversioned_buckets(state),
        "No unversioned files.",
    );
}

/// Collapsible count-badged groups; per-root sub-groups with select-all for
/// multi-root projects, flat file rows otherwise.
fn bucket_groups(ui: &mut Ui, state: &mut AppState, buckets: &[Bucket], empty_text: &str) {
    let multi_root = state.multi.roots.len() > 1;

    egui::ScrollArea::vertical().show(ui, |ui| {
        if buckets.is_empty() {
            ui.colored_label(Color32::GRAY, empty_text);
            return;
        }
        for bucket in buckets {
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
            state.run_git(
                "Stage".into(),
                Affected::from_optional_root(root.as_deref()),
                move |v| {
                    if let Some(r) = &root {
                        changes::stage_selected(v, r, &ch)
                    } else {
                        Ok(())
                    }
                },
            );
        }
        if ui
            .button("Unstage selected")
            .on_hover_text("Remove selected files from the staging area")
            .clicked()
        {
            let ch = selected_changes(state);
            let root = state.selected_path();
            state.run_git(
                "Unstage".into(),
                Affected::from_optional_root(root.as_deref()),
                move |v| {
                    if let Some(r) = &root {
                        changes::unstage_selected(v, r, &ch)
                    } else {
                        Ok(())
                    }
                },
            );
        }
        if ui
            .button("Discard")
            .on_hover_text("Discard working-tree changes (irreversible)")
            .clicked()
        {
            let ch = selected_changes(state);
            if ch.is_empty() {
                state.ui.toast = Some(Toast::warning("Select files to discard."));
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

    // Rendered per the mockup, but deliberately inert in v1: the advanced
    // commit surface (C10) has no backing feature yet (ADR-0010).
    if ui
        .button(RichText::new("Advanced options...").color(Palette::BRAND))
        .clicked()
    {
        // Intentionally inert — activating must change no state.
    }

    ui.horizontal(|ui| {
        ui.checkbox(&mut state.ui.amend, "Amend");
        if ui.button("Template").clicked() {
            let tpl = state.settings.commit_template.clone();
            if tpl.is_empty() {
                state.ui.toast = Some(Toast::warning("No commit template configured."));
            } else if let Ok(content) = std::fs::read_to_string(&tpl) {
                state.ui.commit_message = content;
            } else {
                state.ui.toast = Some(Toast::error(format!("Could not read template: {tpl}")));
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
    state.run_git(
        "Commit".into(),
        Affected::from_optional_root(root.as_deref()),
        move |v| {
            if let Some(r) = &root {
                let _ = changes::commit_selected(v, r, &msg, &changes, amend)?;
                Ok(())
            } else {
                Ok(())
            }
        },
    );
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
