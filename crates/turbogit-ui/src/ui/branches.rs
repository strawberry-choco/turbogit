//! Branches tab (issues 03+): the three-zone screen — a 44px toolbar, the
//! grouped branch list (Local / Remote / Tags with live counts), and the 280px
//! right-hand detail panel. Behavior follows `docs/branches-screen-behavior.md`
//! (§12–§16): branch data is warm from repo open, the current branch sits
//! first (visible without scrolling), slow reads show a muted "reading
//! branches…" line, a repo with no branches gets one sentence + one create
//! action, names truncate in the middle, and the detail panel never blocks
//! the list. All geometry maps onto the §12 constants in
//! [`crate::ui::components`]; every git mutation crosses the
//! [`turbogit_engine_api::GitExecutor`] seam via [`AppState::run_git`].
//!
//! Since the branch-tree-view extraction the grouped list itself is the shared
//! [`branch_tree_view::branch_tree`] component: this surface builds the props,
//! applies the returned events, and keeps the toolbar, the detail panel, the
//! keyboard path and the actions.

use egui::{Align, CornerRadius, Layout, Pos2, Rect, RichText, ScrollArea, Ui, UiBuilder};

use turbogit_app::root_caches::Affected;
use turbogit_app::state::{AppState, Dialog, PendingConfirm};
use turbogit_domain::model::{Branch, BranchKind, Root, RootId};

use crate::theme::{Palette, TYPE_BODY, TYPE_CONTROL, TYPE_DETAIL_TITLE, chrome_font, data_font};
use crate::ui::branch_tree_view::{self, LocalRow, TreeEvent, TreeGroup, TreeProps};
use crate::ui::branch_widget::stale_badge;
use crate::ui::branches_tree::{self, BranchNode, BranchView};
use crate::ui::components::{
    DETAIL_W, KitButton, PAD_LIST, PAD_STRIP, SyncKind, TOOLBAR_H, kit_button,
};
use crate::ui::widgets;

/// One branch action, shared by the detail panel and the ⋯ overflow menu
/// (issue 05): the same items, same order, same wording.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BranchAction {
    Checkout,
    Merge,
    Rebase,
    Compare,
    Rename,
    Delete,
}

/// What the list area shows instead of a blank panel (issue 03, §2/§15).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ListState {
    /// The focused repo has no branch data yet and a scan is in flight.
    Reading,
    /// The repo genuinely has no branches: one sentence + one create action.
    Empty,
    /// The grouped list renders.
    Populated,
}

/// Decide what the list area shows for a focused root (issue 03). Branch data
/// is warm from repo open. A root with no branch rows and no HEAD commit is
/// either still being read (busy) or genuinely empty — an unborn current
/// branch (fresh `git init`) does not count as data.
pub fn list_state(root: &Root, busy: bool) -> ListState {
    if !root.branches.is_empty() || root.head.is_some() {
        ListState::Populated
    } else if busy {
        ListState::Reading
    } else {
        ListState::Empty
    }
}

/// Local-row ordering (design doc §3): the current branch first, then
/// everything else by most recent tip, ties broken alphabetically.
pub fn ordered_locals(locals: &[Branch], current: Option<&str>) -> Vec<Branch> {
    let mut rest: Vec<Branch> = locals
        .iter()
        .filter(|b| Some(b.name.as_str()) != current)
        .cloned()
        .collect();
    rest.sort_by(|a, b| {
        b.last_touched
            .cmp(&a.last_touched)
            .then_with(|| a.name.cmp(&b.name))
    });
    if let Some(cur) = locals.iter().find(|b| Some(b.name.as_str()) == current) {
        rest.insert(0, cur.clone());
    }
    rest
}

/// Character budget for a branch name at `avail` pixels of row width: mono 12
/// runs ~7px/char; names truncate in the middle so both ends stay readable.
pub fn name_budget(avail: f32) -> usize {
    ((avail / 7.0).floor() as usize).clamp(8, 48)
}

// --- Issue 04: row state at a glance ------------------------------------------

/// Stale threshold — a branch untouched for ~4 weeks is dimmed (never
/// hidden). Configurable later (design doc §11 Q2).
pub const STALE_THRESHOLD: chrono::Duration = chrono::Duration::days(28);

/// Whether `branch` reads as stale at `now`.
pub fn is_stale(branch: &Branch, now: chrono::DateTime<chrono::Utc>) -> bool {
    branch
        .last_touched
        .is_some_and(|t| now.signed_duration_since(t) > STALE_THRESHOLD)
}

/// Everything a branch row paints beyond its name (issue 04): staleness, the
/// upstream it tracks, and the sync relationship.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowMeta {
    /// Branch untouched past the ~4-week threshold → dimmed.
    pub stale: bool,
    /// Upstream tracking branch (e.g. `origin/main`), if any.
    pub upstream: Option<String>,
    /// The row's sync pair (icon+count, or "gone") when the branch tracks
    /// an upstream and has something to report; `None` when it is in sync.
    pub badge: Option<(SyncKind, String)>,
}

/// Assemble the row's metadata at `now`.
pub fn row_meta(branch: &Branch, now: chrono::DateTime<chrono::Utc>) -> RowMeta {
    RowMeta {
        stale: is_stale(branch, now),
        upstream: branch.tracking.clone(),
        badge: branch
            .tracking
            .as_ref()
            .and_then(|_| sync_badge(branch.ahead, branch.behind, branch.gone)),
    }
}

use crate::ui::components::sync_badge;

/// Per-direction chips for a tracked row (issue 04): icon+count pairs or the
/// gone marker — a branch with nothing to report is left unmarked, never
/// labelled "in sync".
pub fn sync_chips(ahead: usize, behind: usize, gone: bool) -> Vec<(SyncKind, String)> {
    if gone {
        return vec![(SyncKind::Gone, "gone".to_string())];
    }
    let mut v = Vec::new();
    if ahead > 0 {
        v.push((SyncKind::Ahead, ahead.to_string()));
    }
    if behind > 0 {
        v.push((SyncKind::Behind, behind.to_string()));
    }
    v
}

// --- Issue 06: search & jump ------------------------------------------------------

/// Case-insensitive subsequence match: `mre` surfaces `multi-root-executor`
/// (design doc §3 — fuzzy and forgiving).
pub fn fuzzy_word(haystack: &str, needle: &str) -> bool {
    let needle = needle.trim();
    if needle.is_empty() {
        return true;
    }
    let hay: Vec<char> = haystack.to_lowercase().chars().collect();
    let mut pos = 0;
    for c in needle.to_lowercase().chars() {
        match hay[pos..].iter().position(|h| *h == c) {
            Some(i) => pos += i + 1,
            None => return false,
        }
    }
    true
}

/// Every whitespace-separated query word must fuzzy-match the haystack.
pub fn matches_query(haystack: &str, query: &str) -> bool {
    let q = query.trim();
    if q.is_empty() {
        return true;
    }
    q.split_whitespace().all(|w| fuzzy_word(haystack, w))
}

/// A branch matches when its name — or the message of its tip commit — matches
/// every query word (people remember what a branch contains, not its name).
pub fn branch_matches(branch: &Branch, query: &str) -> bool {
    if matches_query(&branch.name, query) {
        return true;
    }
    branch
        .tip
        .as_ref()
        .is_some_and(|t| matches_query(&t.message, query))
}

/// Render the Branches tab. Returns early when no root is focused (the shell
/// shows Welcome instead).
pub fn show(ui: &mut Ui, state: &mut AppState) {
    if state.multi.roots.is_empty() {
        return;
    }
    // Warm the per-root tag cache for every in-scope root (plan D10: one
    // shared step, keyed by repository, so whichever surface renders first
    // warms the cache for both).
    branch_tree_view::warm_tags(state);
    // Build the repo-grouped view model once (pure; issue 01).
    let tags_by_root = state.ui.branches_tags.clone();
    let view = branches_tree::build_branch_view(
        &state.multi.roots,
        &tags_by_root,
        state.ui.branches_tree.show_remotes,
    );

    // Esc clears the filter first, then closes the detail area (§9). Read at
    // the very top of the frame, before any widget could consume it.
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        if !state.ui.branches_filter.trim().is_empty() {
            state.ui.branches_filter.clear();
        } else {
            state.ui.branches_tree.selected = None;
            state.ui.branches_tree.selected_root = None;
            state.ui.branches_tree.overflow = None;
        }
    }

    // The keyboard path (design §9/§15, issue 15): read the raw events at the
    // very top of the frame, before the toolbar's search input renders and
    // could consume them, so focus → filter → arrows → Enter works with the
    // cursor still in the filter box.
    handle_keys(ui, state, &view);

    let body = ui.available_rect_before_wrap();
    let toolbar_rect = Rect::from_min_max(body.min, Pos2::new(body.max.x, body.min.y + TOOLBAR_H));
    let mut toolbar_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(toolbar_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    toolbar(&mut toolbar_ui, state);
    ui.advance_cursor_after_rect(toolbar_rect);

    let content_rect = Rect::from_min_max(Pos2::new(body.min.x, toolbar_rect.max.y), body.max)
        .intersect(ui.clip_rect());
    let detail_width = DETAIL_W.min(content_rect.width().max(0.0));
    let detail_rect = Rect::from_min_max(
        Pos2::new(content_rect.max.x - detail_width, content_rect.min.y),
        content_rect.max,
    );
    let list_rect = Rect::from_min_max(
        content_rect.min,
        Pos2::new(detail_rect.min.x, content_rect.max.y),
    );

    // Detail panel paints first so the list never overlaps its divider.
    let mut detail_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(detail_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    detail_panel(&mut detail_ui, state);
    ui.advance_cursor_after_rect(detail_rect);

    let mut list_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(list_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    list_area(&mut list_ui, state, &view);
    ui.advance_cursor_after_rect(list_rect);
}

/// The local rows the keyboard path walks (plan D6): every in-scope repo's
/// local leaves under the active filter, ordered by the shared pure function —
/// the same rows the list shows.
fn keyboard_locals<'a>(state: &AppState, view: &'a BranchView) -> Vec<LocalRow<'a>> {
    let multi = state.multi.roots.len() > 1;
    let mut out: Vec<LocalRow<'a>> = Vec::new();
    for section in &view.repos {
        if let Some(f) = &state.ui.branches_repo_filter
            && &section.root_id != f
        {
            continue;
        }
        let repo = if multi {
            section.repo_name.as_str()
        } else {
            ""
        };
        let current = section.current_branch.as_deref();
        let mut leaves: Vec<&'a Branch> = Vec::new();
        fn walk<'a>(nodes: &'a [BranchNode], out: &mut Vec<&'a Branch>) {
            for n in nodes {
                match n {
                    BranchNode::Leaf(l) => out.push(&l.branch),
                    BranchNode::Dir(d) => walk(&d.children, out),
                }
            }
        }
        walk(&section.locals, &mut leaves);
        for b in leaves {
            if branch_matches(b, &state.ui.branches_filter) {
                out.push(LocalRow {
                    root: &section.root_id,
                    branch: b,
                    current,
                    repo,
                });
            }
        }
    }
    branch_tree_view::keyboard_order(&mut out);
    out
}

/// Full keyboard path (issue 15, design §9/§15): arrows move the selection
/// through the visible branches, Enter checks the selected row out, Delete
/// asks to delete it (never the current branch). Nothing on this screen needs
/// the mouse to discover. Events are read before any widget consumes them — a
/// focused search box never eats the path.
fn handle_keys(ui: &mut Ui, state: &mut AppState, view: &BranchView) {
    // Never hijack keys while a modal surface owns the keyboard.
    if state.ui.dialog.is_some()
        || state.ui.confirm.is_some()
        || state.ui.branches_tree.renaming.is_some()
        || state.ui.branches_tree.overflow.is_some()
        || state.ui.branches_scope_picker_open
    {
        return;
    }
    let ordered = keyboard_locals(state, view);
    let sel = state
        .ui
        .branches_tree
        .selected_root
        .as_ref()
        .and_then(|rid| {
            ordered.iter().position(|r| {
                r.root == rid
                    && state.ui.branches_tree.selected.as_deref() == Some(r.branch.name.as_str())
            })
        });
    // Enter/Delete act on the selected row whatever its kind — the local
    // navigation set is only what the arrows move through (remote rows are
    // reference material, issue 13). The branch is looked up in its own repo.
    let selected_branch: Option<(RootId, Branch, Option<String>)> = state
        .ui
        .branches_tree
        .selected_root
        .clone()
        .and_then(|rid| {
            let name = state.ui.branches_tree.selected.clone()?;
            let root = state.multi.by_id(&rid)?.clone();
            let branch = root.branches.iter().find(|b| b.name == name)?.clone();
            Some((rid, branch, root.current_branch.clone()))
        });
    let down = ui.input(|i| i.key_pressed(egui::Key::ArrowDown));
    let up = ui.input(|i| i.key_pressed(egui::Key::ArrowUp));
    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
    let delete = ui.input(|i| i.key_pressed(egui::Key::Delete));

    if down && !ordered.is_empty() {
        // Nothing selected → the first row; a selection → one further.
        let idx = match sel {
            Some(i) => (i + 1).min(ordered.len() - 1),
            None => 0,
        };
        let row = &ordered[idx];
        state.ui.branches_tree.selected = Some(row.branch.name.clone());
        state.ui.branches_tree.selected_root = Some(row.root.clone());
    } else if up && !ordered.is_empty() {
        let idx = sel.map_or(0, |i| i.saturating_sub(1));
        let row = &ordered[idx];
        state.ui.branches_tree.selected = Some(row.branch.name.clone());
        state.ui.branches_tree.selected_root = Some(row.root.clone());
    } else if enter && let Some((rid, branch, _)) = &selected_branch {
        checkout_branch(state, rid, branch);
    } else if delete && let Some((rid, branch, current)) = &selected_branch {
        // The current branch can never be deleted (issue 12); everything
        // else gets the same "what will be lost" ask as the click path.
        if current.as_deref() != Some(branch.name.as_str()) {
            apply_action(state, rid, branch, BranchAction::Delete);
        }
    }
}

/// Toolbar (44px, §12): scope chip and New Branch sit right-aligned.
/// The search input takes the remaining left space; remote visibility is
/// controlled by the tree rollups.
fn toolbar(ui: &mut Ui, state: &mut AppState) {
    ui.add_space(PAD_STRIP);
    // Right-aligned cluster, painted right→left: New Branch (rightmost), then
    // the scope chip (multi-repo only).
    let _ = ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.add_space(PAD_STRIP);
        // Primary action: New Branch (issue 04 — blue).
        if kit_button(ui, KitButton::Primary, "New Branch").clicked() {
            state.ui.dlg.new_branch_name.clear();
            state.ui.dlg.new_branch_start.clear();
            state.ui.dlg.new_branch_base.clear();
            state.ui.dlg.new_branch_base_picker_open = false;
            state.ui.dlg.new_branch_checkout = true;
            state.ui.dialog = Some(Dialog::NewBranch);
        }
        // Scope chip (issue 04): only when several repos are in scope.
        if state.multi.roots.len() > 1 {
            ui.add_space(PAD_STRIP);
            scope_chip(ui, state);
        }
    });
    let search = widgets::search_input(ui, "Search branches", &mut state.ui.branches_filter);
    if state.ui.branches_focus_search {
        search.request_focus();
        state.ui.branches_focus_search = false;
    }
    ui.add_space(PAD_STRIP);
}

/// The scope chip (issue 04): "all N repos" when nothing is narrowed, or
/// "filtered to X" when a single repo is selected. The chip opens a picker that
/// narrows the list to one repo using today's filter semantics.
fn scope_chip(ui: &mut Ui, state: &mut AppState) {
    let filtered = state.ui.branches_repo_filter.is_some();
    let label = match &state.ui.branches_repo_filter {
        Some(id) => format!("filtered to {}", id.name()),
        None => format!("all {} repos", state.multi.roots.len()),
    };
    ui.label(
        RichText::new(label)
            .font(chrome_font(TYPE_CONTROL))
            .color(if filtered {
                Palette::STATE_WARNING
            } else {
                Palette::T_MUTED
            }),
    );
    if kit_button(ui, KitButton::Quiet, "Scope…").clicked() {
        state.ui.branches_scope_picker_open = !state.ui.branches_scope_picker_open;
    }
    if state.ui.branches_scope_picker_open {
        let roots: Vec<(RootId, String)> = state
            .multi
            .roots
            .iter()
            .map(|r| (r.id.clone(), r.id.name()))
            .collect();
        egui::Area::new(ui.id().with("repo_scope"))
            .current_pos(ui.min_rect().left_bottom())
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    if ui
                        .selectable_label(state.ui.branches_repo_filter.is_none(), "All repos")
                        .clicked()
                    {
                        state.ui.branches_repo_filter = None;
                        state.ui.branches_scope_picker_open = false;
                    }
                    for (id, name) in &roots {
                        let on = state.ui.branches_repo_filter.as_ref() == Some(id);
                        if ui.selectable_label(on, format!("Show {name}")).clicked() {
                            state.ui.branches_repo_filter = Some(id.clone());
                            state.ui.branches_scope_picker_open = false;
                        }
                    }
                });
            });
    }
}

/// The Branches list area: a thin caller over the shared tree component
/// (branch-tree-view extraction, plan D5). The "working…" line and the
/// delete-undo banner stay above the scroll area, on the surface; the
/// component paints the list states and the rows; the surface applies the
/// returned events and then paints the ⋯ overflow menu that depends on them.
fn list_area(ui: &mut Ui, state: &mut AppState, view: &BranchView) {
    let has_any_data = state
        .multi
        .roots
        .iter()
        .any(|r| !r.branches.is_empty() || r.head.is_some());
    let tags_by_root = state.ui.branches_tags.clone();
    // Owned copies of the surface-owned string props: the component borrows
    // them while the surface keeps &mut AppState for the banner and events.
    let filter = state.ui.branches_filter.clone();
    let repo_filter = state.ui.branches_repo_filter.clone();
    let props = TreeProps {
        view,
        filter: &filter,
        repo_filter: repo_filter.as_ref(),
        tags_by_root: &tags_by_root,
        multi_repo: state.multi.roots.len() > 1,
        busy: state.ui.busy,
        has_any_data,
        merge_in_progress: state.ui.merge_in_progress,
        last_fetch: state.ui.branches_last_fetch,
        now: chrono::Utc::now(),
        allows_rename: true,
        shows_row_actions: true,
        id_salt: "branches_list",
        full_height: true,
        collapse_remotes_by_default: false,
    };

    // Issue 15: an operation in flight shows in place — a muted "working…"
    // line above the list, never a silent wait. The busy flag is shell-global,
    // so the mid-operation state survives switching tabs.
    if has_any_data && state.ui.busy {
        ui.horizontal(|ui| {
            ui.add_space(PAD_LIST);
            ui.label(
                RichText::new("working…")
                    .font(data_font(TYPE_CONTROL))
                    .color(Palette::T_MUTED),
            );
        });
    }
    // Deleted-branch undo lives at the top of the list for a short window
    // (issue 12).
    undo_banner(ui, state);

    let events = branch_tree_view::branch_tree(ui, &props, &mut state.ui.branches_tree);
    for event in events {
        apply_tree_event(state, event);
    }

    // The ⋯ overflow menu floats over the list with the same actions as the
    // detail panel (issue 05). Events were applied before this paint, so the
    // menu still opens in the frame it was clicked (plan D4). The target row
    // is resolved from its owning repository's snapshot.
    if let Some((root_id, name)) = state.ui.branches_tree.overflow.clone() {
        let anchor = ui.ctx().memory(|m| {
            m.data
                .get_temp::<Rect>(egui::Id::new(("branches_overflow_anchor", &root_id, &name)))
        });
        if let Some(anchor) = anchor {
            egui::Area::new(egui::Id::new(("branches_overflow", &name)))
                .current_pos(anchor.left_bottom())
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(160.0);
                        if let Some(root) = state.multi.by_id(&root_id).cloned()
                            && let Some(branch) =
                                root.branches.iter().find(|b| b.name == name).cloned()
                            && let Some(a) = detail_actions(ui, state, &root, &branch)
                        {
                            apply_action(state, &root.id, &branch, a);
                        }
                    });
                });
        }
    }
}

/// Apply one tree event (plan D4): the surface owns the policy — selection,
/// checkout, fetch scope, rename dispatch, dialog opening — while the
/// component decides none of it.
fn apply_tree_event(state: &mut AppState, event: TreeEvent) {
    match event {
        TreeEvent::RowClicked { root, branch } => {
            // Clicking selects; clicking the selected row again closes the
            // detail (it closes cleanly without disturbing the list — issue 05).
            if state.ui.branches_tree.selected_root.as_ref() == Some(&root)
                && state.ui.branches_tree.selected.as_deref() == Some(branch.as_str())
            {
                state.ui.branches_tree.selected = None;
                state.ui.branches_tree.selected_root = None;
            } else {
                state.ui.branches_tree.selected = Some(branch);
                state.ui.branches_tree.selected_root = Some(root);
            }
            state.ui.branches_tree.overflow = None;
        }
        TreeEvent::RowActivated { root, branch } => {
            // Double-click (and the row's hover Checkout button) checks out —
            // selecting and switching stay separate intents.
            if let Some(b) = state
                .multi
                .by_id(&root)
                .and_then(|r| r.branches.iter().find(|b| b.name == branch))
                .cloned()
            {
                checkout_branch(state, &root, &b);
            }
        }
        TreeEvent::OverflowToggled { root, branch } => {
            let key = (root, branch);
            if state.ui.branches_tree.overflow.as_ref() == Some(&key) {
                state.ui.branches_tree.overflow = None;
            } else {
                state.ui.branches_tree.overflow = Some(key);
            }
        }
        TreeEvent::GroupToggled(group) => match group {
            TreeGroup::Local => {
                let on = state.ui.branches_tree.groups.local;
                state.ui.branches_tree.groups.local = !on;
            }
            TreeGroup::Tags => {
                let on = state.ui.branches_tree.groups.tags;
                state.ui.branches_tree.groups.tags = !on;
            }
        },
        TreeEvent::RemoteToggled { root, remote } => {
            let key = (root.clone(), remote.clone());
            if !state.ui.branches_tree.collapsed_remotes.remove(&key) {
                state.ui.branches_tree.collapsed_remotes.insert(key);
            }
        }
        TreeEvent::RemotesVisibleChanged { visible } => {
            state.ui.branches_tree.show_remotes = visible;
        }
        TreeEvent::FetchRequested { .. } => {
            // The repo-level Fetch covers the narrowed repo's scope — or all
            // in-scope repos when nothing is narrowed (issue 03).
            fetch_scope(state);
        }
        TreeEvent::RenameStarted { root, branch } => {
            state.ui.branches_tree.overflow = None;
            state.ui.branches_tree.renaming = Some(branch.clone());
            state.ui.branches_tree.rename_draft = branch;
            state.ui.branches_tree.selected_root = Some(root);
        }
        TreeEvent::RenameCommitted { root, old, new } => {
            state.ui.branches_tree.renaming = None;
            if new.is_empty() || new == old {
                return;
            }
            // The selection follows the new name so the detail panel stays
            // coherent.
            if state.ui.branches_tree.selected.as_deref() == Some(old.as_str()) {
                state.ui.branches_tree.selected = Some(new.clone());
            }
            state.rename_branch(&root, &old, &new);
        }
        TreeEvent::RenameCancelled => {
            state.ui.branches_tree.renaming = None;
        }
        TreeEvent::CreateBranchRequested { name } => {
            // The no-match state carries the typed query; the empty state
            // starts from a blank name.
            if name.is_empty() {
                state.ui.dlg.new_branch_name.clear();
            } else {
                state.ui.dlg.new_branch_name = name;
            }
            state.ui.dlg.new_branch_start.clear();
            state.ui.dlg.new_branch_base.clear();
            state.ui.dlg.new_branch_base_picker_open = false;
            state.ui.dlg.new_branch_checkout = true;
            state.ui.dialog = Some(Dialog::NewBranch);
        }
        TreeEvent::ClearFilterRequested => {
            state.ui.branches_filter.clear();
        }
    }
}

/// The view-wide Fetch (issue 03). Reachable from every repo header, it covers
/// the narrowed repo's scope — or all in-scope repos when nothing is narrowed —
/// so primary actions never leave reach. Each target records its pre-fetch
/// remote snapshot so the report can say what *that* repo changed.
pub fn fetch_scope(state: &mut AppState) {
    let targets: Vec<RootId> = match &state.ui.branches_repo_filter {
        Some(id) => vec![id.clone()],
        None => state.multi.roots.iter().map(|r| r.id.clone()).collect(),
    };
    for rid in targets {
        let before: Vec<String> = state
            .multi
            .by_id(&rid)
            .map(|r| {
                r.branches
                    .iter()
                    .filter(|b| b.kind == BranchKind::Remote)
                    .map(|b| b.name.clone())
                    .collect()
            })
            .unwrap_or_default();
        state.ui.branches_fetch_before.push((rid.clone(), before));
        let exec_rid = rid.clone();
        state.run_git("Fetch".to_string(), Affected::Root(rid), move |v| {
            v.fetch(&exec_rid.0, None)
        });
    }
}

/// Short-window undo affordance for a deleted branch (issue 12): rendered at
/// the top of the list, restores the branch at its captured tip, and
/// disappears on its own after the window.
fn undo_banner(ui: &mut Ui, state: &mut AppState) {
    let Some(undo) = state.ui.branches_undo.clone() else {
        return;
    };
    if undo.created_at.elapsed() > std::time::Duration::from_secs(10) {
        state.ui.branches_undo = None;
        return;
    }
    ui.horizontal(|ui| {
        ui.add_space(PAD_LIST);
        ui.label(
            RichText::new(format!("Deleted '{}'", undo.name))
                .font(chrome_font(TYPE_CONTROL))
                .color(Palette::T_SECONDARY),
        );
        if kit_button(ui, KitButton::Secondary, "Undo delete").clicked() {
            let root = undo.root.clone();
            let n = undo.name.clone();
            let sha = undo.tip_sha.clone();
            state.ui.branches_undo = None;
            state.run_git(
                format!("Restore branch {n}"),
                Affected::Root(root.clone()),
                move |v| v.branch_create(&root.0, &n, false, Some(&sha)),
            );
        }
    });
    ui.add_space(4.0);
}

/// Check the branch out through the engine seam (issue 07): a branch already
/// checked out in another worktree is refused up front (naming the worktree),
/// a dirty working tree opens the bring-along / set-aside / cancel dialog,
/// and a clean tree switches directly with a quiet activity note. A remote
/// branch becomes "I want to work on this": a matching local branch tracking
/// it is created and checked out (design doc §7; refined in issue 13).
/// The action names its repo scope in multi-repo projects (issue 14).
fn checkout_branch(state: &mut AppState, root: &RootId, branch: &Branch) {
    let Some(r) = state.multi.by_id(root).cloned() else {
        return;
    };

    // Already checked out in another worktree → refuse up front, naming it.
    if let Some(wt) = state
        .caches
        .worktrees(root)
        .into_iter()
        .flatten()
        .find(|w| w.branch == branch.name)
    {
        state.ui.confirm = Some(PendingConfirm::CheckoutInWorktree {
            branch: branch.name.clone(),
            worktree: wt.path.clone(),
        });
        return;
    }

    // Dirty working tree → plain-language care dialog, never a bare refusal.
    if r.status.modified() > 0 {
        state.ui.confirm = Some(PendingConfirm::CheckoutDirty {
            root: root.clone(),
            target: branch.name.clone(),
            kind: branch.kind,
        });
        return;
    }

    crate::ui::branch_widget::push_recent(&mut state.ui.recent_branches, &branch.name);
    state.checkout_branch_op(root, branch.kind, &branch.name);
}

/// Wrap prose within the visible panel, including both side insets.
fn detail_label(ui: &mut Ui, text: RichText) {
    let width = ui
        .available_width()
        .min(ui.clip_rect().right() - ui.cursor().left());
    ui.allocate_ui_with_layout(
        egui::Vec2::new(width.max(0.0), 0.0),
        Layout::left_to_right(Align::Min),
        |ui| {
            let inset = PAD_LIST.min(width.max(0.0) / 4.0);
            ui.add_space(inset);
            ui.add_sized(
                egui::Vec2::new((width - 2.0 * inset).max(0.0), 0.0),
                egui::Label::new(text).wrap(),
            );
        },
    );
}

/// Detail panel (280px): paints its background + left divider, then the
/// selected branch's block (issue 05) or the quiet selection prompt. It never
/// blocks the list — it is a side panel on the same frame.
fn detail_panel(ui: &mut Ui, state: &mut AppState) {
    let rect = ui.available_rect_before_wrap().intersect(ui.clip_rect());
    ui.set_clip_rect(rect);
    ui.set_max_width(rect.width().max(0.0));
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(Palette::RADIUS_CONTROL),
        Palette::PANEL_BG,
    );
    ui.painter().rect_filled(
        Rect::from_min_max(rect.min, Pos2::new(rect.min.x + 1.0, rect.max.y)),
        CornerRadius::ZERO,
        Palette::DIVIDER,
    );

    let Some(name) = state.ui.branches_tree.selected.clone() else {
        // Quiet prompt about what selecting a branch will give you — never an
        // error, never empty space (§2).
        ui.add_space(24.0);
        crate::ui::components::detail_panel_header(ui, "Branches");
        ui.add_space(8.0);
        detail_label(
            ui,
            RichText::new("Select a branch")
                .font(data_font(TYPE_DETAIL_TITLE))
                .color(Palette::T_PRIMARY),
        );
        ui.add_space(6.0);
        detail_label(ui, RichText::new("See how it relates to the current branch, its latest commit, and what you can do with it.")
            .font(chrome_font(TYPE_CONTROL))
            .color(Palette::T_MUTED));
        return;
    };

    // The selected row is looked up in its own repo (issue 14); a vanished
    // selection closes the panel cleanly.
    let Some(root) = state
        .ui
        .branches_tree
        .selected_root
        .as_ref()
        .and_then(|id| state.multi.by_id(id))
        .cloned()
    else {
        state.ui.branches_tree.selected = None;
        state.ui.branches_tree.selected_root = None;
        return;
    };
    let Some(branch) = root.branches.iter().find(|b| b.name == name).cloned() else {
        state.ui.branches_tree.selected = None;
        state.ui.branches_tree.selected_root = None;
        return;
    };
    let now = chrono::Utc::now();
    let meta = row_meta(&branch, now);

    ScrollArea::vertical()
        .id_salt("branches_detail")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Full branch name (13px data type) — the detail title.
            crate::ui::components::detail_panel_header(ui, &branch.name);

            // Relationship to the current branch + tracked remote (§5).
            ui.add_space(6.0);
            relationship_line(ui, &branch, &meta);
            ui.add_space(8.0);

            // Latest-commit block: short hash, message, author, when.
            crate::ui::widgets::group_title(ui, "LATEST COMMIT");
            if let Some(tip) = &branch.tip {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.add_space(PAD_LIST);
                    ui.label(
                        RichText::new(tip.short_hash.clone())
                            .font(data_font(TYPE_BODY))
                            .color(Palette::LINK),
                    );
                });
                ui.add_space(2.0);
                detail_label(
                    ui,
                    RichText::new(tip.message.clone())
                        .font(data_font(TYPE_BODY))
                        .color(Palette::T_PRIMARY),
                );
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.add_space(PAD_LIST);
                    let age = now
                        .signed_duration_since(tip.time)
                        .to_std()
                        .unwrap_or_default();
                    ui.label(
                        RichText::new(format!("{} · {}", tip.author, stale_badge(age)))
                            .font(data_font(TYPE_CONTROL))
                            .color(Palette::T_MUTED),
                    );
                });
            }
            ui.add_space(8.0);

            // Actions: Checkout strongest, merge/rebase next, compare, then a
            // divider separating the quieter rename/delete (§5).
            ui.separator();
            ui.add_space(2.0);
            if let Some(action) = detail_actions(ui, state, &root, &branch) {
                apply_action(state, &root.id, &branch, action);
            }
        });
}

/// One quiet relationship line: the sync state vs its tracked remote
/// ("2 ahead · 1 behind · tracks origin/main", "tracks origin/main" when in
/// sync the row is unmarked).
fn relationship_line(ui: &mut Ui, _branch: &Branch, meta: &RowMeta) {
    let mut parts = Vec::new();
    if let Some((kind, label)) = &meta.badge {
        parts.push(match kind {
            SyncKind::Ahead => format!("{label} ahead"),
            SyncKind::Behind => format!("{label} behind"),
            SyncKind::Diverged | SyncKind::InSync | SyncKind::Gone => label.clone(),
        });
    }
    if let Some(up) = &meta.upstream {
        parts.push(format!("tracks {up}"));
    }
    detail_label(
        ui,
        RichText::new(parts.join(" · "))
            .font(chrome_font(TYPE_CONTROL))
            .color(Palette::T_SECONDARY),
    );
}

/// The action list in the spec's order and wording (issue 05): Checkout
/// strongest, Merge into / Rebase onto «current», Compare with «current», a
/// divider, then the quieter Rename and Delete. Shared verbatim by the ⋯
/// overflow menu. Returns the clicked action, or `None`.
fn detail_actions(
    ui: &mut Ui,
    state: &mut AppState,
    root: &Root,
    branch: &Branch,
) -> Option<BranchAction> {
    let current = root
        .current_branch
        .as_deref()
        .unwrap_or_default()
        .to_string();
    let is_current = root.current_branch.as_deref() == Some(branch.name.as_str());
    let is_local = branch.kind == BranchKind::Local;
    // The primary action names its repo's scope in multi-repo projects
    // (issue 14) — the same rules as the row hover action.
    let scope = if state.multi.roots.len() > 1 {
        format!(" in {}", root.id.name())
    } else {
        String::new()
    };
    let mut action = None;

    if kit_button(ui, KitButton::Primary, &format!("Checkout{scope}")).clicked() {
        action = Some(BranchAction::Checkout);
    }
    if !is_current {
        if is_local {
            if kit_button(ui, KitButton::Secondary, &format!("Merge into {current}")).clicked() {
                action = Some(BranchAction::Merge);
            }
            if kit_button(ui, KitButton::Secondary, &format!("Rebase onto {current}")).clicked() {
                action = Some(BranchAction::Rebase);
            }
        }
        if kit_button(ui, KitButton::Secondary, &format!("Compare with {current}")).clicked() {
            action = Some(BranchAction::Compare);
        }
    }
    ui.add_space(2.0);
    ui.separator();
    ui.add_space(2.0);
    if is_local && kit_button(ui, KitButton::Quiet, "Rename").clicked() {
        action = Some(BranchAction::Rename);
    }
    if !is_current && kit_button(ui, KitButton::Danger, "Delete").clicked() {
        action = Some(BranchAction::Delete);
    }
    action
}

/// Dispatch one branch action from the detail panel or the ⋯ overflow
/// (issue 05). The owning root rides every action so its scope is stated in
/// the wording (issue 14) — "Checkout in alpha".
fn apply_action(state: &mut AppState, root: &RootId, branch: &Branch, action: BranchAction) {
    state.ui.branches_tree.overflow = None;
    match action {
        BranchAction::Checkout => checkout_branch(state, root, branch),
        BranchAction::Merge => state.open_merge_into(root, &branch.name),
        BranchAction::Rebase => {
            // Issue 09: rebase the selected branch onto the current one — the
            // label states the direction before anything runs.
            let current = state
                .multi
                .by_id(root)
                .and_then(|r| r.current_branch.clone())
                .unwrap_or_default();
            if !current.is_empty() {
                state.rebase_branch_onto_current(root, &branch.name, &current);
            }
        }
        BranchAction::Compare => state.open_compare(root, &branch.name),
        BranchAction::Rename => {
            // Issue 11: renaming happens inline on the branch itself, never a
            // separate form screen.
            state.ui.branches_tree.renaming = Some(branch.name.clone());
            state.ui.branches_tree.rename_draft = branch.name.clone();
            state.ui.branches_tree.selected_root = Some(root.clone());
        }
        BranchAction::Delete => match branch.kind {
            BranchKind::Remote => {
                let (remote, name) = branch
                    .name
                    .split_once('/')
                    .map(|(r, n)| (r.to_string(), n.to_string()))
                    .unwrap_or(("origin".into(), branch.name.clone()));
                state.ui.confirm = Some(PendingConfirm::DeleteRemoteBranch { remote, name });
            }
            BranchKind::Local => {
                // A branch checked out in another worktree is refused up
                // front, with the worktree named (issue 12).
                if let Some(wt) = state
                    .caches
                    .worktrees(root)
                    .into_iter()
                    .flatten()
                    .find(|w| w.branch == branch.name)
                {
                    state.ui.confirm = Some(PendingConfirm::CheckoutInWorktree {
                        branch: branch.name.clone(),
                        worktree: wt.path.clone(),
                    });
                    return;
                }
                // The one piece of information that makes deletion feel safe:
                // what is lost, in human terms (issue 12, design §6.6).
                state.ui.branches_delete_consequence = delete_consequence(state, root, branch);
                state.ui.confirm = Some(PendingConfirm::DeleteLocalBranch {
                    name: branch.name.clone(),
                });
            }
        },
    }
}

/// The human-terms consequence of deleting a local branch (issue 12,
/// design §6.6): unreachable commits versus safe-to-delete, naming the
/// current branch the way the design's examples do ("not on any other
/// branch" → "not on <current>").
fn delete_consequence(state: &mut AppState, id: &RootId, branch: &Branch) -> Option<String> {
    let current = state
        .multi
        .by_id(id)
        .and_then(|r| r.current_branch.clone())?;
    let args = [
        "rev-list".to_string(),
        "--count".to_string(),
        format!("HEAD..{}", branch.name),
    ];
    let count = state
        .executor
        .run_raw(&id.0, &args)
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok());
    match count {
        Some(0) => Some(format!(
            "everything on this branch already exists on {current} — safe to delete"
        )),
        Some(n) => Some(format!(
            "this branch has {n} commit(s) not on {current} — they become unreachable after deleting"
        )),
        None => None,
    }
}

#[cfg(test)]
mod visual_tests {
    use super::*;

    #[test]
    fn detail_prompt_wraps_to_allocated_and_clipped_width() {
        let project = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let state = AppState::launch_in(Some(project.path().into()), Some(config.path().into()));
        let mut harness = egui_kittest::Harness::new_ui_state(
            |ui, state| {
                crate::theme::configure_style(ui.ctx());
                // Reproduce a nominal 280px allocation with only 208px visible.
                let rect = Rect::from_min_size(ui.cursor().min, egui::vec2(DETAIL_W, 400.0));
                let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
                child.set_clip_rect(rect.intersect(ui.clip_rect()));
                detail_panel(&mut child, state);
            },
            state,
        );
        crate::theme::install_fonts(&harness.ctx);
        for width in [280.0, 208.0, 120.0] {
            harness.set_size(egui::vec2(width, 500.0));
            harness.run();
            let shape = harness.output().shapes.iter().find(|shape| {
                matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text().starts_with("See how"))
            }).expect("detail prompt painted");
            let egui::Shape::Text(text) = &shape.shape else {
                unreachable!()
            };
            assert!(text.galley.rows.len() > 1, "prose must wrap");
            let bounds = text.galley.rect.translate(text.pos.to_vec2());
            assert!(
                bounds.right() <= shape.clip_rect.right(),
                "prose cut mid-word: {bounds:?} / {:?}",
                shape.clip_rect
            );
            assert!(shape.clip_rect.right() <= width);
            assert!(text.galley.text().ends_with("do with it."), "no lost prose");
        }
    }
}
