//! Commit tool window redesigned onto canonical changelist buckets (issue #11).
//!
//! Layout: a sub-tab strip (Local Changes / Unversioned Files / Shelf / Stash,
//! issue #18) above two zones (redesign 03) — a fixed-width Commit panel of
//! collapsible canonical groups with count badges on the left, and a
//! permanent diff-preview pane (above the message editor and the Amend /
//! Commit / Commit-and-Push action row) on the right. Local Changes shows
//! the "Default Changelist" and "Merge conflicts" groups; Unversioned Files
//! lists untracked files includable in commits.
//! For multi-root projects each group nests per-root sub-groups with count
//! badges and a select-all checkbox; single-root projects list files flat.
//! Commit stays disabled until a non-empty message AND at least one included
//! change exist. Shelf / Stash are clickable tabs whose panes are labeled
//! placeholders until Phase J (ADR-0008); the "Advanced options..." control
//! renders per the mockup but is deliberately inert in v1 (ADR-0010).
//! User-created changelists remain backlog.

use crate::theme::Palette;
use crate::ui::icons::{self, Icon};
use crate::ui::widgets;
use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Key, Layout, Pos2, Rect, RichText, Sense,
    Stroke, StrokeKind, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType,
};
use std::path::{Path, PathBuf};
use turbogit_app::root_caches::{Affected, StatsView};
use turbogit_app::state::{AppState, CommitSubTab, Dialog, PendingConfirm, Toast};
use turbogit_domain::model::{Change, ChangeStatus, Root};
use turbogit_services::changes;

/// Fixed width of the Commit panel (redesign 03, screen 01): the Commit
/// window is exactly two zones — this fixed-width panel on the left and
/// a flexible diff preview on the right.
pub const COMMIT_PANEL_WIDTH: f32 = 340.0;

/// Height of the bottom action row (issue 07): the single primary `Commit ▾`
/// split button plus the grey `Shelve…` / `Stash…` beside it. Pinned to the
/// panel bottom via a [`egui::Panel`] so the tree scrolls above it.
pub const COMMIT_ACTION_ROW_HEIGHT: f32 = 44.0;

/// Canonical bucket names. The staging sections (issue 20, screen 06) mirror
/// Git's index: files with unstaged content under UNSTAGED, fully staged
/// files under STAGED; conflicts keep their own group. The Unversioned Files
/// sub-tab keeps its canonical bucket name.
pub const UNSTAGED: &str = "UNSTAGED";
pub const STAGED: &str = "STAGED";
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
    /// Expand / collapse one repo group of the one-tree (issue 04), keyed
    /// by root id. The focused repo is never the target — its group is
    /// always expanded ("focus = expand").
    ToggleGroup(turbogit_domain::model::RootId),
    /// Select one file for both the commit selection and the diff preview
    /// (issue 05): the row click gesture — "what I'm committing and what
    /// I'm looking at are the same thing". Keyed by canonical root-scoped
    /// path.
    Select { key: PathBuf, path: PathBuf },
    /// Select a file for the diff preview pane.
    Preview(PathBuf),
}

/// Split one root's status into the staging sections (issue 20, screen 06):
/// files with unstaged content under UNSTAGED, fully staged files under
/// STAGED, and conflicts in their own group (empty buckets are dropped).
/// Untracked files are excluded here — the one-tree redesign (issue 04)
/// renders them in the bottom `Unversioned Files` group instead. The staging
/// view mirrors the index itself, so granularly-completed paths still show
/// under STAGED — unlike the old changelist view they replaced, where they
/// left the list.
fn staging_buckets(state: &AppState) -> Vec<Bucket<'_>> {
    let mut out = Vec::new();
    for root in &state.multi.roots {
        let mut unstaged = Vec::new();
        let mut staged = Vec::new();
        let mut conflicts = Vec::new();
        for c in &root.status.changes {
            match c.status {
                ChangeStatus::Conflicted => conflicts.push(c),
                // Fully staged content only: a partially staged file still
                // has unstaged work and belongs in UNSTAGED.
                _ if c.staged && !c.unstaged => staged.push(c),
                ChangeStatus::Ignored | ChangeStatus::Unversioned => {}
                _ => unstaged.push(c),
            }
        }
        for (name, changes) in [
            (UNSTAGED, unstaged),
            (STAGED, staged),
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
            RowAction::Select { key, path } => {
                // Row click gesture: check the box (idempotent — the checkbox
                // glyph alone toggles it off) and drive the preview together.
                state.ui.selected.insert(key);
                state.ui.preview_change = Some(path);
            }
            RowAction::Preview(path) => state.ui.preview_change = Some(path),
            RowAction::ToggleGroup(id) => {
                if state.ui.changes_expanded.contains(&id) {
                    state.ui.changes_expanded.remove(&id);
                } else {
                    state.ui.changes_expanded.insert(id);
                }
            }
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
    // Hunk-span statistics (issue 20) feed the per-file hunk badges and the
    // staged-hunk rail; computed on miss through the engine seam, keyed per
    // root and invalidated with the other root caches.
    if let Some(root_id) = state.selected_root.clone() {
        let exec = state.executor.clone();
        state.caches.ensure_hunk_stats(exec.as_ref(), &root_id);
    }
    sub_tab_strip(ui, state);
    file_filter_row(ui, state);
    match state.ui.commit_subtab {
        CommitSubTab::LocalChanges => local_changes_body(ui, state),
        // Phase-J scope: clickable tabs with labeled placeholder panes
        // (ADR-0008) instead of hidden or disabled-looking tabs. The
        // Unversioned Files sub-tab was removed in the one-tree redesign
        // (issue 04); its data now lives in the Local Changes tree's bottom
        // group.
        CommitSubTab::Shelf => placeholder_pane(ui, "Shelf"),
        CommitSubTab::Stash => placeholder_pane(ui, "Stash"),
    }

    // Conflict resolution tools (ours / theirs / 3-way merge editor); the
    // renderer no-ops while nothing is conflicted.
    crate::ui::conflicts::render(ui, state);

    if let Some(err) = &state.last_error {
        ui.separator();
        ui.colored_label(Palette::STATE_ERROR, format!("⚠ {err}"));
    }
}

// --------------------------------------------------------- sub-tab strip --

/// Sub-tabs in strip order. Shelf / Stash are Phase-J placeholders (ADR-0008);
/// Unversioned Files was removed in the one-tree redesign (issue 04).
const SUB_TABS: [(CommitSubTab, &str); 3] = [
    (CommitSubTab::LocalChanges, "Local Changes"),
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
    // Two zones (redesign 03): a fixed-width Commit panel on the left, a
    // flexible diff preview on the right. Both always render — the preview
    // is a permanent pane, not a toggleable tab. The left pane is the one
    // tree (repo groups + the bottom `Unversioned Files` group, issue 04);
    // there is no separate Unversioned Files sub-tab body anymore.
    two_zone(ui, state, changelist_pane, preview_and_editor_pane);
}

/// Layout helper for the two-zone Commit window (redesign 03): the left
/// pane gets the fixed [`COMMIT_PANEL_WIDTH`], the right pane (diff
/// preview + message editor) takes the flexible remainder.
fn two_zone(
    ui: &mut Ui,
    state: &mut AppState,
    left: impl FnOnce(&mut Ui, &mut AppState),
    right: impl FnOnce(&mut Ui, &mut AppState),
) {
    let avail = ui.available_rect_before_wrap();
    let left_rect = Rect::from_min_max(
        avail.min,
        Pos2::new(avail.min.x + COMMIT_PANEL_WIDTH, avail.max.y),
    );
    let right_rect = Rect::from_min_max(Pos2::new(left_rect.max.x, avail.min.y), avail.max);
    let mut left_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(left_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    left(&mut left_ui, state);
    let mut right_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(right_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    right(&mut right_ui, state);
    // Advance the caller's cursor by the panes' *used* height, matching
    // `ui.columns`: sections rendered after the body (e.g. the conflict
    // resolution tools) must flow beneath the content, not below the full
    // reserved rect (which would clip them past the window's bottom edge).
    let max_height = left_ui.min_size().y.max(right_ui.min_size().y);
    ui.advance_cursor_after_rect(Rect::from_min_size(
        avail.min,
        Vec2::new(avail.width(), max_height),
    ));
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
    // Issue 07: one set of commit controls. The message box + Amend sit at the
    // TOP (one box for the whole view), and the single action row — primary
    // `Commit ▾` split button plus grey `Shelve…` / `Stash…` — sits at the
    // bottom. The tree's ScrollArea is height-capped so it never grows onto
    // the action row below it; this pane keeps reporting its real content
    // height, so sections the two-zone body renders after it (e.g. conflict
    // resolution tools) keep their layout.
    ui.heading("Commit");
    // Issue 07: one set of commit controls at the top of the panel — the
    // single commit message box + Amend were moved out of the preview pane's
    // bottom margin into this panel's header area.
    commit_message_box(ui, state);
    recent_messages_row(ui, state);
    tree_toolbar_row(ui, state);

    let Some(root_id) = state.selected_root.clone() else {
        ui.colored_label(Color32::GRAY, "Select a repository to see changes.");
        commit_action_row(ui, state);
        return;
    };
    if state.multi.by_id(&root_id).is_none() {
        commit_action_row(ui, state);
        return;
    }

    // Focus = expand (issue 04): whenever the selected root moves, manual
    // expansions reset so only the newly focused repo's files are visible.
    // Normalized before the buckets borrow the status snapshots (plan §1.4).
    if state.ui.changes_focus != state.selected_root {
        state.ui.changes_focus = state.selected_root.clone();
        state.ui.changes_expanded.clear();
    }

    let (buckets, no_match) = filter_buckets(state, staging_buckets(state));
    let (unversioned, _) = filter_buckets(state, unversioned_buckets(state));
    let empty_text = if no_match.is_empty() {
        "No local changes."
    } else {
        &no_match
    };
    let mut actions = Vec::new();
    changes_tree(ui, state, &buckets, &unversioned, empty_text, &mut actions);
    apply_actions(state, actions);

    commit_action_row(ui, state);
}

/// The one tree (issue 04, visual doc §3): every repository renders as a
/// collapsible group — chevron + repo icon + name + dim branch + file-count
/// badge. Non-focused groups collapse by default so the selected repo's
/// files are the visual focus ("focus = expand"); the user can expand any
/// other group manually ([`AppState::ui`] `changes_expanded`). Each expanded
/// group lists that repo's staging sections flat — the group itself is the
/// per-repo scoping, so the old per-root sub-groups are gone. A single
/// `Unversioned Files` group sits at the bottom of the same tree.
fn changes_tree(
    ui: &mut Ui,
    state: &AppState,
    buckets: &[Bucket<'_>],
    unversioned_buckets: &[Bucket<'_>],
    empty_text: &str,
    actions: &mut Vec<RowAction>,
) {
    // Height-capped (issue 07): the pane's action row sits below the tree, so
    // the scroll area must never grow onto it. Uses the available rect
    // instead of the capped pane height so sections rendered after the
    // two-zone body (e.g. conflict resolution tools) keep real geometry.
    let remaining = ui.available_rect_before_wrap();
    let max_h = (remaining.height() - COMMIT_ACTION_ROW_HEIGHT).max(COMMIT_ACTION_ROW_HEIGHT);
    egui::ScrollArea::vertical()
        .id_salt("changes_tree")
        .max_height(max_h)
        .show(ui, |ui| {
            let mut painted = false;
            for root in &state.multi.roots {
                let root_buckets: Vec<&Bucket> =
                    buckets.iter().filter(|b| b.root.id == root.id).collect();
                if root_buckets.is_empty() {
                    continue;
                }
                painted = true;
                repo_group(ui, state, root, &root_buckets, actions);
            }
            if !unversioned_buckets.is_empty() {
                painted = true;
                unversioned_group(ui, state, unversioned_buckets, actions);
            }
            if !painted {
                ui.colored_label(Color32::GRAY, empty_text);
            }
        });
}

/// The bottom `Unversioned Files` group of the one tree (issue 04): one
/// collapsible group listing every root's untracked files. Multi-root
/// projects keep a small per-root sub-label so same-named paths stay
/// attributable (rows still key by canonical root-scoped path).
fn unversioned_group(
    ui: &mut Ui,
    state: &AppState,
    buckets: &[Bucket<'_>],
    actions: &mut Vec<RowAction>,
) {
    let total: usize = buckets.iter().map(|b| b.changes.len()).sum();
    egui::CollapsingHeader::new(format!("{UNVERSIONED_FILES} ({total})"))
        .id_salt("unversioned_group")
        .default_open(true)
        .show(ui, |ui| {
            let multi_root = state.multi.roots.len() > 1;
            for bucket in buckets {
                if multi_root {
                    ui.label(
                        RichText::new(bucket.root.id.name())
                            .font(FontId::new(11.0, FontFamily::Proportional))
                            .color(Palette::INK_3),
                    );
                }
                for c in &bucket.changes {
                    change_row(ui, state, bucket.root, c, actions);
                }
            }
        });
}

/// One collapsible repo group of the one-tree (issue 04). The focused repo
/// is always expanded ("focus = expand"); every other group is collapsed
/// unless the user toggled it open. Clicking a non-focused group header
/// records a [`RowAction::ToggleGroup`]; clicking the focused group is a
/// no-op (its files are the visual focus).
fn repo_group(
    ui: &mut Ui,
    state: &AppState,
    root: &Root,
    buckets: &[&Bucket<'_>],
    actions: &mut Vec<RowAction>,
) {
    let focused = state.selected_root.as_ref() == Some(&root.id);
    let expanded = focused || state.ui.changes_expanded.contains(&root.id);
    let name = root.id.name();

    let height = crate::theme::GROUP_ROW_HEIGHT;
    let row = Rect::from_min_size(
        ui.cursor().left_top(),
        Vec2::new(ui.available_width(), height),
    );
    ui.allocate_exact_size(Vec2::new(row.width(), height), Sense::hover());
    let response = ui.interact(
        row,
        ui.auto_id_with(("repo_group", &root.id)),
        Sense::click(),
    );
    response
        .widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, format!("Toggle {name}")));
    if response.clicked() && !focused {
        actions.push(RowAction::ToggleGroup(root.id.clone()));
    }

    let painter = ui.painter().clone();
    if focused || response.hovered() {
        let fill = if focused {
            Palette::selection_bg()
        } else {
            Palette::SURFACE_2
        };
        painter.rect_filled(row, egui::CornerRadius::same(4), fill);
    }
    let cy = row.center().y;
    icon_at(
        ui,
        if expanded {
            Icon::CHEVRON_DOWN
        } else {
            Icon::CHEVRON_RIGHT
        },
        Pos2::new(row.left() + 14.0, cy),
        12.0,
        Palette::INK_3,
    );
    icon_at(
        ui,
        Icon::FOLDER_GIT,
        Pos2::new(row.left() + 32.0, cy),
        14.0,
        if focused {
            Palette::BRAND
        } else {
            Palette::INK_2
        },
    );
    let name_galley = painter.layout_no_wrap(
        name.clone(),
        FontId::new(12.5, FontFamily::Proportional),
        Palette::INK,
    );
    let name_w = name_galley.size().x;
    painter.galley_with_override_text_color(
        Pos2::new(row.left() + 46.0, cy - name_galley.size().y / 2.0),
        name_galley,
        Palette::INK,
    );
    // Dim branch label; detached repos fall back to the sidebar's marker.
    let branch = root
        .current_branch
        .clone()
        .unwrap_or_else(|| "<detached>".to_owned());
    let branch_galley = painter.layout_no_wrap(
        branch,
        FontId::new(11.0, FontFamily::Proportional),
        Palette::INK_3,
    );
    painter.galley_with_override_text_color(
        Pos2::new(
            row.left() + 46.0 + name_w + 8.0,
            cy - branch_galley.size().y / 2.0,
        ),
        branch_galley,
        Palette::INK_3,
    );
    // File-count badge, right-aligned to the pane edge.
    let count: usize = buckets.iter().map(|b| b.changes.len()).sum();
    let count_galley = painter.layout_no_wrap(
        count.to_string(),
        FontId::new(11.0, FontFamily::Proportional),
        Palette::INK_2,
    );
    painter.galley_with_override_text_color(
        Pos2::new(
            row.right() - 12.0 - count_galley.size().x,
            cy - count_galley.size().y / 2.0,
        ),
        count_galley,
        Palette::INK_2,
    );

    if expanded {
        ui.indent(ui.id().with(("repo_group_body", &root.id)), |ui| {
            for bucket in buckets {
                group_section(ui, state, bucket, actions);
            }
        });
    }
}

/// Paint one icon primitive centered at `origin` without disturbing layout
/// (mirrors the sidebar's helper of the same name).
fn icon_at(ui: &mut Ui, icon: Icon, center: Pos2, size: f32, color: Color32) {
    let mut child =
        ui.new_child(UiBuilder::new().max_rect(Rect::from_center_size(center, Vec2::splat(size))));
    icons::icon(&mut child, icon, size, color);
}

/// One bucket's file rows inside an expanded repo group. Issue 07 removes the
/// per-repo `UNSTAGED (n)` / `STAGED (n)` header rows and their repeated
/// `Stage all` / `Unstage all` actions — staging now lives in the per-file
/// checkboxes (issue 05), so those sections paint flat. The `Merge conflicts`
/// group keeps its header: conflicts are a distinct review surface with no
/// checkbox, so they stay visibly grouped.
fn group_section(ui: &mut Ui, state: &AppState, bucket: &Bucket<'_>, actions: &mut Vec<RowAction>) {
    if bucket.name == MERGE_CONFLICTS {
        ui.strong(format!("{} ({})", bucket.name, bucket.changes.len()));
    }
    ui.indent(
        ui.id()
            .with(("group_section", &bucket.root.id, bucket.name)),
        |ui| {
            for c in &bucket.changes {
                change_row(ui, state, bucket.root, c, actions);
            }
        },
    );
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

/// Flat changed-file paths of the active Commit sub-tab in display order —
/// the F7/Shift+F7 cross-file traversal list (spec R7). Unfiltered by the
/// file filter: navigation walks the real staging sections. Local Changes
/// spans both the per-repo staging sections and the bottom `Unversioned
/// Files` group (issue 04). The Phase-J placeholder tabs contribute nothing.
pub(crate) fn active_subtab_files(state: &AppState) -> Vec<PathBuf> {
    let buckets = match state.ui.commit_subtab {
        CommitSubTab::LocalChanges => {
            let mut b = staging_buckets(state);
            b.extend(unversioned_buckets(state));
            b
        }
        CommitSubTab::Shelf | CommitSubTab::Stash => return Vec::new(),
    };
    buckets
        .iter()
        .flat_map(|b| b.changes.iter().map(|c| c.path.clone()))
        .collect()
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

/// Status-letter / filename colour for a file row (issue 05, design doc §4):
/// M modified blue, A added green, U unversioned olive — the redesign tokens;
/// every remaining state maps onto a real semantic hue so no row ever paints
/// a fallback (deleted keeps error red, renames the brand, copies share the
/// added green, ignored dims to INK_3, conflicts stay error red).
fn status_color(status: ChangeStatus) -> Color32 {
    match status {
        ChangeStatus::Modified => Palette::STATUS_MODIFIED,
        ChangeStatus::Added => Palette::STATUS_ADDED,
        ChangeStatus::Unversioned => Palette::STATUS_UNVERSIONED,
        ChangeStatus::Deleted => Palette::STATE_ERROR,
        ChangeStatus::Renamed => Palette::BRAND,
        ChangeStatus::Copied => Palette::STATUS_ADDED,
        ChangeStatus::Ignored => Palette::INK_3,
        ChangeStatus::Conflicted => Palette::STATE_ERROR,
    }
}

/// Row text split for the `[filename] … [dim location]` layout (issue 05):
/// `("app.rs", Some("src/"))` for `src/app.rs`; files at the repo root have
/// no location to paint (`None`). The location renders dim grey after the
/// coloured filename.
fn row_display(path: &Path) -> (String, Option<String>) {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let location = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| format!("{}/", p.display()));
    (name, location)
}

/// Coarse partially-staged marker (spec R2 story 12): a small warning-tinted
/// dot at `center`, accessibility-labeled so tooling and screen readers can
/// spot partially staged rows without opening the diff.
fn partially_staged_dot_at(ui: &mut Ui, center: Pos2, salt: &PathBuf) {
    const DOT_R: f32 = 2.5;
    let rect = Rect::from_center_size(center, Vec2::splat(14.0));
    ui.painter()
        .circle_filled(center, DOT_R, Palette::STATE_WARNING);
    let resp = ui.interact(rect, ui.auto_id_with(("partial_dot", salt)), Sense::hover());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, "Partially staged"));
    resp.on_hover_text("Partially staged");
}

/// Paint the per-file checkbox glyph at `rect` (issue 05): a rounded square
/// filled BRAND with an INK check when checked, else an INK_3 border over the
/// row's transparent background.
fn paint_checkbox(ui: &Ui, rect: Rect, checked: bool, resp: &egui::Response) {
    let radius = CornerRadius::same(3);
    let border = if checked || resp.hovered() {
        Palette::BRAND
    } else {
        Palette::INK_3
    };
    ui.painter().rect_filled(
        rect,
        radius,
        if checked {
            Palette::BRAND
        } else {
            Palette::SURFACE_2
        },
    );
    ui.painter()
        .rect_stroke(rect, radius, Stroke::new(1.0, border), StrokeKind::Inside);
    if checked {
        let p1 = Pos2::new(rect.left() + 3.5, rect.center().y - 1.0);
        let p2 = Pos2::new(rect.left() + 6.0, rect.center().y + 2.0);
        let p3 = Pos2::new(rect.left() + 11.5, rect.center().y - 3.5);
        let check = Stroke::new(1.6, Palette::BRAND_INK);
        ui.painter().line_segment([p1, p2], check);
        ui.painter().line_segment([p2, p3], check);
    }
}

// Fixed columns of one file row, expressed from the row's left edge. They are
// named constants because a row's text has to be measured — and its height
// settled — before the row rect is allocated.

/// Left padding before the checkbox glyph.
const ROW_PAD_X: f32 = 6.0;
/// Width of the per-file include checkbox.
const ROW_CHECKBOX: f32 = 16.0;
/// Gap between two row columns.
const ROW_GAP: f32 = 6.0;
/// Right padding of the row: the dim location column ends here.
const ROW_EDGE: f32 = 12.0;
/// Space the partially-staged dot reserves after the status letter.
const PARTIAL_DOT_COLUMN: f32 = 18.0;
/// Lead of the rename arrow (plus its gap) before the renamed-from path.
const RENAME_ARROW_W: f32 = 24.0;
/// Narrowest name column a row accepts before the dim location gives up its
/// first-line seat and moves onto its own line under the name.
const MIN_NAME_COLUMN: f32 = 96.0;

/// One file row (issue 05, design doc §4): `[checkbox] [status letter]
/// [coloured filename] … [dim location]` on the 24 px file-row height — or
/// taller: a filename too long for its column **wraps onto continuation
/// lines** and the row grows one text line per extra row of text, so a long
/// name is never clipped at the pane edge or painted over the dim location.
/// The status letter colour-codes the row — M blue / A green / U olive — the
/// filename takes the status colour, and the parent directory renders dim at
/// the pane edge; hunk counts moved out of rows into the diff header
/// (issue 06). Clicking the row body checks the box AND previews the diff, so
/// "what I'm committing and what I'm looking at are the same thing"; the
/// checkbox glyph alone toggles commit inclusion without moving the preview.
/// The selected row paints the solid design-blue selection background
/// (design doc §7). Renames/copies (spec R8) keep a muted arrow plus the old
/// path; partially staged files (staged AND unstaged) keep the coarse warning
/// dot. Conflicts render as a single review row — an unmerged path cannot be
/// staged or included until resolved, so there is no checkbox; clicking it
/// opens the diff preview. Interactions are pushed onto `actions` and applied
/// by the caller after rendering (plan §1.4 defer pattern).
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

    let included = state.ui.selected.contains(&key);
    let tint = status_color(c.status);
    let (name, location) = row_display(&c.path);
    // The file-row name rides the shared body type (T2) — one size role for
    // filename/body text across tool windows, not a local fractional copy.
    let font = FontId::new(crate::theme::TYPE_BODY, FontFamily::Proportional);
    let painter = ui.painter().clone();

    // ---- measure, then allocate --------------------------------------
    // The row's height is a function of its text, so everything is laid out
    // first: the name's wrap width depends on the fixed prefix columns and
    // on whether the dim location keeps its first-line seat.
    let letter_galley =
        painter.layout_no_wrap(badge_letter(c.status).to_owned(), font.clone(), tint);
    let (letter_w, letter_h) = letter_galley.size().into();
    let left = ui.cursor().left_top();
    let content_right = left.x + ui.available_width() - ROW_EDGE;
    // x of the name column: the row's left edge plus the fixed prefix — the
    // checkbox, the status letter, and the partially-staged dot when the
    // file is partly staged.
    let name_x = left.x
        + ROW_PAD_X
        + ROW_CHECKBOX
        + ROW_GAP
        + letter_w
        + ROW_GAP
        + if c.staged && c.unstaged {
            PARTIAL_DOT_COLUMN
        } else {
            0.0
        };
    // The dim location keeps its own right-aligned column on the first line
    // as long as the name keeps a readable width; on deep paths in a narrow
    // pane it drops onto its own line beneath the name instead of squeezing
    // the name into a sliver.
    let loc_galley = location
        .as_ref()
        .map(|loc| painter.layout_no_wrap(loc.clone(), font.clone(), Palette::INK_3));
    let loc_w = loc_galley.as_ref().map_or(0.0, |g| g.size().x);
    let loc_h = loc_galley.as_ref().map_or(0.0, |g| g.size().y);
    let loc_gap = if loc_galley.is_some() { ROW_GAP } else { 0.0 };
    let full_column = content_right - name_x;
    let loc_below = loc_galley.is_some() && full_column - loc_w - loc_gap < MIN_NAME_COLUMN;
    let name_column = if loc_below {
        full_column
    } else {
        full_column - loc_w - loc_gap
    }
    .max(1.0);
    // A filename that does not fit its column wraps onto continuation lines
    // (breaking mid-word — file names rarely contain a space) rather than
    // overrunning the pane or the location column.
    let name_galley = painter.layout(name.clone(), font.clone(), tint, name_column);

    // Renames/copies (spec R8) keep the muted arrow + old path: inline after
    // a single-line name that leaves room for them, else on their own line.
    let orig_galley = c
        .orig_path
        .as_ref()
        .map(|p| painter.layout_no_wrap(p.display().to_string(), font.clone(), Palette::INK_3));
    let rename_below = match (&orig_galley, name_galley.rows.len()) {
        (Some(orig), 1) => {
            name_x + name_galley.size().x + ROW_GAP + RENAME_ARROW_W + orig.size().x
                > name_x + name_column
        }
        (Some(_), _) => true,
        (None, _) => false,
    };

    // The row keeps the design's 24 px single-line height (design doc §8)
    // and grows exactly one text line per extra line of text.
    let line_h = name_galley
        .rows
        .first()
        .map_or(name_galley.size().y, |row| row.size.y);
    let name_h = name_galley.size().y;
    let name_lines = name_galley.rows.len().max(1);
    let below_lines = usize::from(rename_below) + usize::from(loc_below);
    let content_h = name_h + below_lines as f32 * line_h;
    let row_h = crate::theme::FILE_ROW_HEIGHT + (name_lines - 1 + below_lines) as f32 * line_h;
    let row = Rect::from_min_size(left, Vec2::new(ui.available_width(), row_h));
    ui.allocate_exact_size(row.size(), Sense::hover());

    // Selected row: solid design-blue fill under the content — the checkbox
    // and the background derive from the same selection, which also drives
    // the diff preview.
    if included {
        painter.rect_filled(row, CornerRadius::same(4), Palette::SELECTION_BG);
    }

    // The text block is centred in the row; the checkbox, the status letter
    // and the right-aligned location all ride the block's first line.
    let block_top = row.top() + (row_h - content_h) / 2.0;
    let cy = block_top + line_h / 2.0;

    // Checkbox: toggles commit inclusion only, never the preview.
    let cb = ROW_CHECKBOX;
    let cb_rect = Rect::from_min_max(
        Pos2::new(row.left() + ROW_PAD_X, cy - cb / 2.0),
        Pos2::new(row.left() + ROW_PAD_X + cb, cy + cb / 2.0),
    );
    let mut is_included = included;
    let cb_resp = ui.interact(
        cb_rect,
        ui.auto_id_with(("row_checkbox", &key)),
        Sense::click(),
    );
    cb_resp
        .widget_info(|| WidgetInfo::labeled(WidgetType::Checkbox, true, format!("Select {name}")));
    if cb_resp.clicked() {
        is_included = !is_included;
        actions.push(RowAction::Toggle {
            key: key.clone(),
            include: is_included,
        });
    }
    paint_checkbox(ui, cb_rect, is_included, &cb_resp);

    // Status letter, then the coloured filename, then the dim location.
    let letter_x = row.left() + ROW_PAD_X + ROW_CHECKBOX + ROW_GAP;
    painter.galley_with_override_text_color(
        Pos2::new(letter_x, cy - letter_h / 2.0),
        letter_galley,
        tint,
    );

    if c.staged && c.unstaged {
        // The dot's 14 px hit rect is centred in its own slot, just left of
        // the name column.
        partially_staged_dot_at(ui, Pos2::new(name_x - PARTIAL_DOT_COLUMN + 7.0, cy), &key);
    }

    painter.galley_with_override_text_color(
        Pos2::new(name_x, block_top),
        name_galley.clone(),
        tint,
    );

    // Lines below the name: the rename marker first, the dim location last.
    let rename_top = block_top + name_h;
    let loc_top = rename_top + if rename_below { line_h } else { 0.0 };
    if let Some(orig) = &orig_galley {
        let orig_h = orig.size().y;
        let (arrow_cy, orig_x, orig_y) = if rename_below {
            (
                rename_top + line_h / 2.0,
                name_x + RENAME_ARROW_W,
                rename_top + (line_h - orig_h) / 2.0,
            )
        } else {
            (
                cy,
                name_x + name_galley.size().x + ROW_GAP + RENAME_ARROW_W,
                cy - orig_h / 2.0,
            )
        };
        // Muted arrow then the renamed-from path, both dim (R8).
        icon_at(
            ui,
            Icon::ARROW_RIGHT,
            Pos2::new(orig_x - RENAME_ARROW_W / 2.0, arrow_cy),
            12.0,
            Palette::INK_3,
        );
        painter.galley_with_override_text_color(
            Pos2::new(orig_x, orig_y),
            orig.clone(),
            Palette::INK_3,
        );
    }

    // Dim location, right-aligned to the pane edge — on the first line while
    // the name keeps its column, otherwise under the name.
    if let Some(loc) = &loc_galley {
        let loc_y = if loc_below {
            loc_top + (line_h - loc_h) / 2.0
        } else {
            cy - loc_h / 2.0
        };
        painter.galley_with_override_text_color(
            Pos2::new(content_right - loc_w, loc_y),
            loc.clone(),
            Palette::INK_3,
        );
    }

    // Row body: one select gesture — checks the box AND previews. Registered
    // after the checkbox so the checkbox owns its own clicks.
    let body_rect = Rect::from_min_max(
        Pos2::new(row.left() + ROW_PAD_X + ROW_CHECKBOX + ROW_GAP, row.top()),
        row.max,
    );
    let body = ui.interact(
        body_rect,
        ui.auto_id_with(("row_body", &key)),
        Sense::click(),
    );
    body.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name.clone()));
    if body.clicked() {
        actions.push(RowAction::Select {
            key,
            path: c.path.clone(),
        });
    }
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

/// The one-tree toolbar row (issue 04, visual doc §3): the `Changes` label
/// with the total change count, then small icon-only controls — expand /
/// collapse all groups, group-by (inert, ADR-0010), rollback (discard, via
/// the destructive confirm), and refresh. The old `Stage selected` /
/// `Unstage selected` / `Discard` text buttons are gone (issue 05 completes
/// the per-file checkbox staging; this ticket already ships the icon row).
fn tree_toolbar_row(ui: &mut Ui, state: &mut AppState) {
    let total = state
        .multi
        .roots
        .iter()
        .flat_map(|r| &r.status.changes)
        .filter(|c| !matches!(c.status, ChangeStatus::Ignored))
        .count();
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("Changes ({total})"))
                .strong()
                .font(FontId::new(12.0, FontFamily::Proportional))
                .color(Palette::INK),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // Commit options gear (issue 07): the `advanced options…` link
            // collapsed into this panel icon. Deliberately inert in v1 — the
            // options surface has no backing feature yet (ADR-0010).
            let gear = widgets::icon_button(ui, Icon::SETTINGS);
            gear.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Commit options"));
            // Refresh: the full scoped refresh (same as the shell button).
            let refresh = widgets::icon_button(ui, Icon::REFRESH_CW);
            refresh
                .widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Refresh changes"));
            if refresh.clicked() {
                state.refresh(Affected::All);
            }
            // Rollback (discard): destructive, confirmation-gated.
            let rollback = widgets::icon_button(ui, Icon::UNDO);
            rollback.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Rollback"));
            if rollback.clicked() {
                let ch = selected_changes(state);
                if ch.is_empty() {
                    state.ui.toast = Some(Toast::warning("Select files to discard."));
                } else {
                    state.ui.confirm = Some(PendingConfirm::Discard { changes: ch });
                }
            }
            // Group-by: rendered per the mockup, deliberately inert in v1
            // (the same ADR-0010 pattern as the Commit options gear).
            let group_by = widgets::icon_button(ui, Icon::LAYERS);
            group_by.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Group by"));
            // Expand / collapse all non-focused groups; the focused repo
            // stays expanded either way ("focus = expand").
            let all_expanded = state
                .multi
                .roots
                .iter()
                .filter(|r| state.selected_root.as_ref() != Some(&r.id))
                .all(|r| state.ui.changes_expanded.contains(&r.id));
            let (icon, label) = if all_expanded {
                (Icon::CHEVRON_UP, "Collapse all groups")
            } else {
                (Icon::CHEVRON_DOWN, "Expand all groups")
            };
            let expand_all = widgets::icon_button(ui, icon);
            expand_all.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
            if expand_all.clicked() {
                if all_expanded {
                    state.ui.changes_expanded.clear();
                } else {
                    for r in &state.multi.roots {
                        if state.selected_root.as_ref() != Some(&r.id) {
                            state.ui.changes_expanded.insert(r.id.clone());
                        }
                    }
                }
            }
        });
    });
    ui.separator();
}

// --------------------------------------------- preview + editor pane ------

/// The staged-hunk summary rail (issue 20, screen 06): "N hunks staged"
/// plus one chip per staged hunk of the selected root's files —
/// "file @@start", with " (part)" when an unstaged remainder overlaps the
/// staged hunk's region. A chip click previews that file. Rendered only
/// when at least one hunk is staged.
fn staged_hunks_rail(ui: &mut Ui, state: &mut AppState) {
    use turbogit_services::hunk_stats;
    let Some(root_id) = state.selected_root.clone() else {
        return;
    };
    let Some(stats) = state.caches.hunk_stats(&root_id) else {
        return;
    };
    // Collect the chips first — rendering the clicks needs `&mut state`.
    let mut total = 0usize;
    let mut chips: Vec<(String, PathBuf)> = Vec::new();
    for f in &stats.staged {
        if f.hunks.is_empty() {
            continue;
        }
        let local = stats
            .file(StatsView::Local, Path::new(&f.path))
            .map(|lf| lf.hunks.as_slice())
            .unwrap_or(&[]);
        let name = Path::new(&f.path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| f.path.clone());
        for s in &f.hunks {
            total += 1;
            let part = if hunk_stats::staged_hunk_partial(s, local) {
                " (part)"
            } else {
                ""
            };
            chips.push((
                format!("{name} @@{}{part}", s.old_start),
                PathBuf::from(&f.path),
            ));
        }
    }
    if total == 0 {
        return;
    }
    ui.colored_label(
        Palette::BRAND,
        format!(
            "{total} {} staged",
            if total == 1 { "hunk" } else { "hunks" }
        ),
    );
    ui.horizontal_wrapped(|ui| {
        for (label, path) in chips {
            if crate::ui::diff::chip_button(ui, &label, false).clicked() {
                state.ui.preview_change = Some(path);
            }
        }
    });
    ui.add_space(4.0);
}

fn preview_and_editor_pane(ui: &mut Ui, state: &mut AppState) {
    staged_hunks_rail(ui, state);
    ui.heading("Preview");
    match state.ui.preview_change.clone() {
        Some(path) => {
            diff_header(ui, state, &path);
            crate::ui::diff::render_diff(ui, state, &None, &None, &Some(path));
        }
        None => {
            ui.colored_label(
                Color32::GRAY,
                "Select a changed file to preview its unified diff.",
            );
        }
    }
}

/// The diff preview header (issue 06, design doc §5): the previewed file's
/// path, a status chip (Modified / Added / …), the `+N −M` change-size
/// stats, and prev/next change navigation over the active sub-tab's
/// changed-file list. Renders from `ui.preview_change` every frame, so
/// selecting a row updates the header immediately. The nav buttons retarget
/// the preview to the adjacent changed file — the same path a file-row click
/// uses, so `ensure_diff` loads the new diff. The diff modes, hunk nav, and
/// whitespace switches below are untouched (issue 06 checklist).
fn diff_header(ui: &mut Ui, state: &mut AppState, path: &Path) {
    ui.horizontal(|ui| {
        let path_text = path.display().to_string();
        let path_resp = ui.label(
            RichText::new(&path_text)
                .font(FontId::new(13.0, FontFamily::Proportional))
                .color(Palette::INK),
        );
        path_resp.widget_info(|| {
            WidgetInfo::labeled(WidgetType::Label, true, format!("Previewing {path_text}"))
        });

        // Status chip, tinted with the row's status colour.
        let status = crate::ui::diff::preview_status(state, Some(path));
        status_chip(ui, status);

        // `+N −M` change-size stats: counts land once the async diff text
        // is cached, so they read 0 while loading.
        let (added, removed) = crate::ui::diff::preview_line_counts(state, path);
        let stat_font = FontId::new(12.0, FontFamily::Proportional);
        ui.label(
            RichText::new(format!("+{added}"))
                .font(stat_font.clone())
                .color(Palette::DIFF_ADD_ACCENT),
        );
        ui.label(
            RichText::new(format!("\u{2212}{removed}"))
                .font(stat_font)
                .color(Palette::DIFF_DEL_ACCENT),
        );

        // Prev/next change navigation, right-aligned to the pane edge.
        let files = active_subtab_files(state);
        let current = files.iter().position(|p| p.as_path() == path);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let prev_enabled = current.is_some_and(|i| i > 0);
            let next_enabled = current.is_some_and(|i| i + 1 < files.len());
            // Right-to-left: the Next chevron is allocated first so it sits
            // at the pane edge, with Previous to its left.
            let next = icon_button_enabled(ui, Icon::CHEVRON_RIGHT, next_enabled);
            next.widget_info(|| {
                WidgetInfo::labeled(WidgetType::Button, next_enabled, "Next change")
            });
            if next_enabled && next.clicked() {
                state.ui.preview_change = current.and_then(|i| files.get(i + 1)).cloned();
            }
            let prev = icon_button_enabled(ui, Icon::CHEVRON_LEFT, prev_enabled);
            prev.widget_info(|| {
                WidgetInfo::labeled(WidgetType::Button, prev_enabled, "Previous change")
            });
            if prev_enabled && prev.clicked() {
                state.ui.preview_change = current.and_then(|i| files.get(i - 1)).cloned();
            }
        });
    });
}

/// The status chip of the diff header (issue 06, design doc §5): a small
/// pill carrying the full status word, tinted with the row's status colour
/// (the same mapping the file rows use, issue 05).
fn status_chip(ui: &mut Ui, status: ChangeStatus) {
    let label = status_chip_label(status);
    let tint = status_color(status);
    const CHIP_H: f32 = 18.0;
    const PAD_X: f32 = 8.0;
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::new(11.0, FontFamily::Proportional),
        tint,
    );
    let size = Vec2::new(galley.size().x + PAD_X * 2.0, CHIP_H);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(CHIP_H as u8 / 2),
        Palette::SURFACE_3,
    );
    ui.painter().galley(
        Pos2::new(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        tint,
    );
    let resp = ui.interact(
        rect,
        ui.auto_id_with(("status_chip", label)),
        Sense::hover(),
    );
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, label));
}

/// Full-word status label for the diff header chip.
fn status_chip_label(status: ChangeStatus) -> &'static str {
    match status {
        ChangeStatus::Modified => "Modified",
        ChangeStatus::Added => "Added",
        ChangeStatus::Deleted => "Deleted",
        ChangeStatus::Renamed => "Renamed",
        ChangeStatus::Copied => "Copied",
        ChangeStatus::Unversioned => "Unversioned",
        ChangeStatus::Ignored => "Ignored",
        ChangeStatus::Conflicted => "Conflicted",
    }
}

/// [`widgets::icon_button`] with an explicit `enabled` flag: disabled dims
/// the button and turns clicks into no-ops, rendered in a child scope so the
/// disabled state never leaks into the remaining header widgets (the
/// [`widgets::compact_button_enabled`] pattern).
fn icon_button_enabled(ui: &mut Ui, icon: Icon, enabled: bool) -> egui::Response {
    if enabled {
        return widgets::icon_button(ui, icon);
    }
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(ui.available_rect_before_wrap())
            .layout(*ui.layout()),
    );
    child.disable();
    let response = widgets::icon_button(&mut child, icon);
    ui.advance_cursor_after_rect(child.min_rect());
    response
}

/// Issue 07: the single commit message box + Amend option at the TOP of the
/// commit panel (one set of commit controls for the whole view — the per-repo
/// controls are gone). Extra message table helpers (`Template` / `Clear`) stay
/// with it; the action row now lives in its own [`commit_action_row`].
fn commit_message_box(ui: &mut Ui, state: &mut AppState) {
    ui.label("Commit message:");
    ui.text_edit_multiline(&mut state.ui.commit_message);

    // Subject-length guidance.
    let subject = state.ui.commit_message.lines().next().unwrap_or("");
    let slen = subject.chars().count();
    ui.horizontal(|ui| {
        ui.label(format!("Subject: {slen}/50"));
        if slen > 50 {
            ui.colored_label(Palette::STATE_WARNING, "(keep ≤ 50)");
        }
    });

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
}

/// One action row of commit controls (issue 07): the primary `Commit ▾` split
/// button plus the small grey `Shelve…` / `Stash…` buttons beside it — one
/// primary action only. The old `Commit` / `Commit and Push...` / cascade
/// button queue collapsed into the split button's dropdown (see
/// [`commit_split_menu`]).
fn commit_action_row(ui: &mut Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        let can_commit = !state.ui.commit_message.trim().is_empty() && has_selected_changes(state);

        // Primary `Commit ▾`: the main part commits; the chevron opens the
        // alternatives menu.
        let main = primary_button_enabled(ui, "Commit", can_commit);
        main.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Commit changes"));
        if can_commit && main.clicked() {
            do_commit(state, false);
        }
        let chevron = icon_button_enabled(ui, Icon::CHEVRON_DOWN, can_commit);
        chevron
            .widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Commit split options"));
        if can_commit {
            commit_split_menu(&chevron, state);
        }

        ui.add_space(10.0);

        // Small grey secondary buttons beside the single primary action.
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

/// The split button's dropdown menu (issue 07): `Commit and Push...` always,
/// plus the multi-repo cascade entry when more than one repo is selected.
fn commit_split_menu(chevron: &egui::Response, state: &mut AppState) {
    use egui::Popup;
    Popup::menu(chevron).show(|menu| {
        if menu
            .button("Commit and Push...")
            .on_hover_text("Commit then open the push dialog")
            .clicked()
        {
            do_commit(state, true);
            menu.close();
        }
        let selection_count = state.ui.repo_selection.len();
        if selection_count > 1 {
            menu.separator();
            let footer = state.commit_rail_footer(&state.ui.commit_message);
            menu.label(footer);
            let button_label = format!("Also commit on {selection_count} selected repos");
            if menu
                .button(button_label)
                .on_hover_text("Commit the same message on every selected repo")
                .clicked()
            {
                let message = state.ui.commit_message.clone();
                let amend = state.ui.amend;
                state.run_commit_across(&message, amend);
                state.ui.commit_message.clear();
                state.ui.selected.clear();
                state.persist_ui();
                menu.close();
            }
        }
    });
}

/// [`widgets::primary_button`] with an explicit `enabled` flag (the disabled
/// twin of the action row's split button): disabled dims the button and turns
/// clicks into no-ops, rendered in a child scope so the disabled state never
/// leaks into the remaining row widgets.
fn primary_button_enabled(ui: &mut Ui, label: &str, enabled: bool) -> egui::Response {
    if enabled {
        return widgets::primary_button(ui, None, label);
    }
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(ui.available_rect_before_wrap())
            .layout(*ui.layout()),
    );
    child.disable();
    let response = widgets::primary_button(&mut child, None, label);
    ui.advance_cursor_after_rect(child.min_rect());
    response
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_color_maps_the_redesign_letters_to_their_tokens() {
        // Issue 05 (design doc §4): the status letter and filename take the
        // status colour — M modified blue, A added green, U unversioned
        // olive; every remaining state maps onto a real semantic hue so no
        // row can ever paint a fallback.
        assert_eq!(
            status_color(ChangeStatus::Modified),
            Palette::STATUS_MODIFIED
        );
        assert_eq!(status_color(ChangeStatus::Added), Palette::STATUS_ADDED);
        assert_eq!(
            status_color(ChangeStatus::Unversioned),
            Palette::STATUS_UNVERSIONED
        );
        assert_eq!(status_color(ChangeStatus::Deleted), Palette::STATE_ERROR);
        assert_eq!(status_color(ChangeStatus::Renamed), Palette::BRAND);
        assert_eq!(status_color(ChangeStatus::Copied), Palette::STATUS_ADDED);
        assert_eq!(status_color(ChangeStatus::Ignored), Palette::INK_3);
        assert_eq!(status_color(ChangeStatus::Conflicted), Palette::STATE_ERROR);
    }

    #[test]
    fn row_display_splits_filename_from_dim_location() {
        // Issue 05 (design doc §4): `[filename] … [dim location]` — the
        // filename is the basename, the location is its parent directory.
        // Files at the repo root have no location to paint.
        assert_eq!(
            row_display(Path::new("base.txt")),
            ("base.txt".to_owned(), None)
        );
        assert_eq!(
            row_display(Path::new("src/app.rs")),
            ("app.rs".to_owned(), Some("src/".to_owned()))
        );
        assert_eq!(
            row_display(Path::new("a/b/c.txt")),
            ("c.txt".to_owned(), Some("a/b/".to_owned()))
        );
    }
}
