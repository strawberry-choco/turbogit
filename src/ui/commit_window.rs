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
use crate::model::{Change, ChangeStatus, Root};
use crate::root_caches::Affected;
use crate::state::{AppState, CommitSubTab, Dialog, PendingConfirm, Toast};
use crate::theme::Palette;
use crate::ui::icons::{self, Icon};
use crate::ui::widgets;
use egui::{Color32, Key, RichText, Sense, Ui, Vec2, WidgetInfo, WidgetType};
use std::path::PathBuf;

/// Canonical bucket names (user-created changelists are backlog).
pub const DEFAULT_CHANGELIST: &str = "Default Changelist";
pub const UNVERSIONED_FILES: &str = "Unversioned Files";
pub const MERGE_CONFLICTS: &str = "Merge conflicts";

/// One canonical bucket: its name plus that root's changes. Borrows the
/// root's status snapshot — rows render against a shared [`AppState`] and
/// their clicks are deferred (plan §1.4), so no change is cloned per frame.
struct Bucket<'a> {
    name: &'static str,
    root: &'a Root,
    changes: Vec<&'a Change>,
}

/// Deferred row interaction collected while rendering borrowed buckets;
/// applied after the pane finishes (same pattern as the widget button seam).
enum RowAction {
    /// Include/exclude one change, keyed by absolute path so multi-root
    /// projects never conflate same-named files across roots.
    Toggle { key: PathBuf, include: bool },
    /// Select a file for the diff preview pane.
    Preview(PathBuf),
}

/// Split one root's status into the three canonical buckets (empty buckets
/// are dropped so the tree only shows groups that have content). Paths that
/// granular staging fully staged (spec R2 story 9) are not listed anymore.
fn canonical_buckets(state: &AppState) -> Vec<Bucket<'_>> {
    let mut out = Vec::new();
    for root in &state.multi.roots {
        let mut default = Vec::new();
        let mut unversioned = Vec::new();
        let mut conflicts = Vec::new();
        for c in &root.status.changes {
            if state
                .ui
                .granularly_completed
                .contains(&root.canonical_key(c))
            {
                continue;
            }
            match c.status {
                ChangeStatus::Conflicted => conflicts.push(c),
                ChangeStatus::Unversioned => unversioned.push(c),
                // Ignored files never belong in the commit window.
                ChangeStatus::Ignored => {}
                _ => default.push(c),
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
                    root,
                    changes,
                });
            }
        }
    }
    out
}

/// Apply the row interactions collected during one pane's render pass.
fn apply_actions(state: &mut AppState, actions: Vec<RowAction>) {
    for action in actions {
        match action {
            RowAction::Toggle { key, include } => {
                if include {
                    state.ui.selected.insert(key);
                } else {
                    state.ui.selected.remove(&key);
                }
            }
            RowAction::Preview(path) => state.ui.preview_change = Some(path),
        }
    }
}

/// Collect the `Change` objects of the selected root whose path is included.
fn selected_changes(state: &AppState) -> Vec<Change> {
    let mut out = Vec::new();
    if let Some(id) = &state.selected_root
        && let Some(root) = state.multi.by_id(id)
    {
        for c in &root.status.changes {
            if state.ui.selected.contains(&root.canonical_key(c)) {
                out.push(c.clone());
            }
        }
    }
    out
}

/// Whether the selected root has at least one included change — the Commit
/// buttons' gate, computed without building an owned change list (plan §1.4).
fn has_selected_changes(state: &AppState) -> bool {
    state.selected_root.as_ref().is_some_and(|id| {
        state.multi.by_id(id).is_some_and(|root| {
            root.status
                .changes
                .iter()
                .any(|c| state.ui.selected.contains(&root.canonical_key(c)))
        })
    })
}

pub fn show(ui: &mut Ui, state: &mut AppState) {
    sub_tab_strip(ui, state);
    file_filter_row(ui, state);
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

// ------------------------------------------------------------ file filter --

/// Inline file filter over the changed-file list (spec R7, CONTEXT.md "File
/// filter"): one header input shared by both active sub-tabs, matched
/// case-insensitively against file paths. `/` focuses it (via
/// `focus_file_filter`, armed by the shell or the Filter Files palette
/// action); Esc while focused clears focus and text; otherwise the text
/// persists across root switches and refreshes within the session.
fn file_filter_row(ui: &mut Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        let resp = widgets::search_input(ui, "Filter files", &mut state.ui.file_filter);
        if state.ui.focus_file_filter {
            resp.request_focus();
            state.ui.focus_file_filter = false;
        }
        // egui surrenders focus on bare Escape; pair that transition with
        // clearing the query so Esc means "filter off".
        if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Escape)) {
            state.ui.file_filter.clear();
        }
    });
}

/// Narrow `buckets` to changes whose path contains the file filter
/// (case-insensitive substring, log-search style). Returns the filtered
/// buckets plus the zero-match message to paint — empty when no filter is
/// active and the caller's own empty text applies.
fn filter_buckets<'a>(state: &AppState, mut buckets: Vec<Bucket<'a>>) -> (Vec<Bucket<'a>>, String) {
    let query = state.ui.file_filter.trim().to_lowercase();
    if query.is_empty() {
        return (buckets, String::new());
    }
    for bucket in &mut buckets {
        bucket
            .changes
            .retain(|c| c.path.display().to_string().to_lowercase().contains(&query));
    }
    buckets.retain(|b| !b.changes.is_empty());
    let shown = state.ui.file_filter.trim();
    (buckets, format!("No files match '{shown}'."))
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

    let (buckets, no_match) = filter_buckets(state, canonical_buckets(state));
    let empty_text = if no_match.is_empty() {
        "No local changes."
    } else {
        &no_match
    };
    let mut actions = Vec::new();
    bucket_groups(ui, state, &buckets, empty_text, &mut actions);
    apply_actions(state, actions);
}

/// Untracked files of every root as one canonical bucket per root (issue #18:
/// the Unversioned Files sub-tab's active data). Granularly completed paths
/// are skipped (spec R2 story 9). Borrows the status snapshots (plan §1.4).
fn unversioned_buckets(state: &AppState) -> Vec<Bucket<'_>> {
    let mut out = Vec::new();
    for root in &state.multi.roots {
        let untracked: Vec<&Change> = root
            .status
            .changes
            .iter()
            .filter(|c| matches!(c.status, ChangeStatus::Unversioned))
            .filter(|c| {
                !state
                    .ui
                    .granularly_completed
                    .contains(&root.canonical_key(c))
            })
            .collect();
        if !untracked.is_empty() {
            out.push(Bucket {
                name: UNVERSIONED_FILES,
                root,
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

    let (buckets, no_match) = filter_buckets(state, unversioned_buckets(state));
    let empty_text = if no_match.is_empty() {
        "No unversioned files."
    } else {
        &no_match
    };
    let mut actions = Vec::new();
    bucket_groups(ui, state, &buckets, empty_text, &mut actions);
    apply_actions(state, actions);
}

/// Flat changed-file paths of the active Commit sub-tab in display order —
/// the F7/Shift+F7 cross-file traversal list (spec R7). Unfiltered by the
/// file filter: navigation walks the real changelist. The Phase-J
/// placeholder tabs contribute nothing.
pub(crate) fn active_subtab_files(state: &AppState) -> Vec<PathBuf> {
    let buckets = match state.ui.commit_subtab {
        CommitSubTab::LocalChanges => canonical_buckets(state),
        CommitSubTab::UnversionedFiles => unversioned_buckets(state),
        CommitSubTab::Shelf | CommitSubTab::Stash => return Vec::new(),
    };
    buckets
        .iter()
        .flat_map(|b| b.changes.iter().map(|c| c.path.clone()))
        .collect()
}

/// Collapsible count-badged groups; per-root sub-groups with select-all for
/// multi-root projects, flat file rows otherwise. Renders against a shared
/// [`AppState`] (buckets borrow the status snapshots) and collects row
/// interactions into `actions` for the caller to apply (plan §1.4).
fn bucket_groups(
    ui: &mut Ui,
    state: &AppState,
    buckets: &[Bucket],
    empty_text: &str,
    actions: &mut Vec<RowAction>,
) {
    let multi_root = state.multi.roots.len() > 1;

    // Named id salt: `ui.columns` gives both panes the same stable child id,
    // and egui's default ScrollArea salt is constant — two unnamed areas
    // would share one persisted state and flip-flop scrollbar visibility
    // (a zero-delay repaint loop).
    egui::ScrollArea::vertical()
        .id_salt("changelist_scroll")
        .show(ui, |ui| {
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
                            root_subgroup(ui, state, bucket, actions);
                        } else {
                            for c in &bucket.changes {
                                change_row(ui, state, bucket.root, c, actions);
                            }
                        }
                    });
            }
        });
}

/// Per-root sub-group with count badge + select-all (multi-root projects).
fn root_subgroup(ui: &mut Ui, state: &AppState, bucket: &Bucket, actions: &mut Vec<RowAction>) {
    let root_name = bucket.root.id.name();
    let all_included = bucket
        .changes
        .iter()
        .all(|c| state.ui.selected.contains(&bucket.root.canonical_key(c)));
    let any_included = bucket
        .changes
        .iter()
        .any(|c| state.ui.selected.contains(&bucket.root.canonical_key(c)));

    ui.horizontal(|ui| {
        let mut select_all = all_included;
        if ui
            .checkbox(&mut select_all, format!("Select all {root_name}"))
            .changed()
        {
            for c in &bucket.changes {
                actions.push(RowAction::Toggle {
                    key: bucket.root.canonical_key(c),
                    include: select_all,
                });
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
        ui.id().with(("subgroup", &bucket.root.id, bucket.name)),
        |ui| {
            for c in &bucket.changes {
                change_row(ui, state, bucket.root, c, actions);
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

/// Coarse partially-staged marker (spec R2 story 12): a small warning-tinted
/// dot beside the status glyph, accessibility-labeled so tooling and screen
/// readers can spot partially staged rows without opening the diff.
fn partially_staged_dot(ui: &mut Ui) {
    const CELL: f32 = 14.0;
    const DOT_R: f32 = 2.5;
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(CELL), Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), DOT_R, Palette::STATE_WARNING);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, "Partially staged"));
    resp.on_hover_text("Partially staged");
}

/// One file row: inclusion checkbox ("M base.txt"), status icon, and a
/// path button that selects the file for the diff preview pane. Conflicted
/// files render as a single review row ("C conf.txt") instead — an unmerged
/// path cannot be staged or committed until resolved, so there is nothing
/// to include; clicking it opens the diff preview. Partially staged files
/// (staged AND unstaged, spec R2 story 12) carry the coarse warning dot.
/// Interactions are pushed onto `actions` and applied by the caller after
/// rendering (plan §1.4 defer pattern).
fn change_row(
    ui: &mut Ui,
    state: &AppState,
    root: &Root,
    c: &Change,
    actions: &mut Vec<RowAction>,
) {
    let key = root.canonical_key(c);
    let path_text = c.path.display().to_string();
    if c.status == ChangeStatus::Conflicted {
        let previewing = state.ui.preview_change.as_ref() == Some(&c.path);
        let (glyph, tint) = status_glyph(c.status);
        let label = format!("{} {}", badge_letter(c.status), path_text);
        ui.horizontal(|ui| {
            if ui.selectable_label(previewing, label).clicked() {
                actions.push(RowAction::Preview(c.path.clone()));
            }
            icons::icon(ui, glyph, 14.0, tint);
        });
        return;
    }
    ui.horizontal(|ui| {
        let mut included = state.ui.selected.contains(&key);
        let label = format!("{} {}", badge_letter(c.status), path_text);
        if ui.checkbox(&mut included, label).changed() {
            actions.push(RowAction::Toggle {
                key,
                include: included,
            });
        }
        let (glyph, tint) = status_glyph(c.status);
        icons::icon(ui, glyph, 14.0, tint);
        if c.staged && c.unstaged {
            partially_staged_dot(ui);
        }
        let previewing = state.ui.preview_change.as_ref() == Some(&c.path);
        if ui.selectable_label(previewing, path_text).clicked() {
            actions.push(RowAction::Preview(c.path.clone()));
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
        let can_commit = !state.ui.commit_message.trim().is_empty() && has_selected_changes(state);
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
    // Files whose index already diverges from HEAD carry a — possibly
    // granular — staged selection (spec R2); re-staging them whole would
    // blow it away (ADR-0013). They commit as-is from the index; untouched
    // files keep the stage-then-commit flow.
    let (untouched, partial): (Vec<Change>, Vec<Change>) =
        selected_changes(state).into_iter().partition(|c| !c.staged);
    let msg = state.ui.commit_message.clone();
    let amend = state.ui.amend;
    // Record the recent message before `msg` moves into the 'static closure
    // (plan Phase 3): no extra String clone per commit.
    if !state.ui.recent_messages.contains(&msg) {
        state.ui.recent_messages.insert(0, msg.clone());
        state.ui.recent_messages.truncate(12);
    }
    state.run_git(
        "Commit".into(),
        Affected::from_optional_root(root.as_deref()),
        move |v| {
            if let Some(r) = &root {
                let _ = changes::commit_selected(v, r, &msg, &untouched, &partial, amend)?;
                Ok(())
            } else {
                Ok(())
            }
        },
    );
    // Reset fields (the recent message was recorded above).
    state.ui.commit_message.clear();
    state.ui.selected.clear();
    state.persist_ui();
    if and_push {
        state.ui.dialog = Some(Dialog::Push);
    }
}
