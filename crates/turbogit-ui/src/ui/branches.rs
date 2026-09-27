//! Branches tab (issues 03+): a one-row toolbar over the grouped branch list
//! (Local / Remote / Tags with live counts). Every branch action lives in the
//! row's right-click context menu — the screen has no side panel and no
//! per-row button. Behavior: branch data is warm from repo open, the current
//! branch sits first (visible without scrolling), slow reads show a muted
//! "reading branches…" line, a repo with no branches gets one sentence + one
//! create action, and names truncate in the middle. The geometry comes from
//! the §12 constants in [`crate::ui::components`]; every git mutation crosses
//! the Git engine only through [`AppState::dispatch`] and cached reads.
//!
//! Since the branch-tree-view extraction the grouped list itself is the shared
//! [`branch_tree_view::branch_tree`] component: this surface builds the props,
//! applies the returned events, and keeps the toolbar, the keyboard path, and
//! the one action dispatcher.

use egui::{Align, Layout, Pos2, Rect, RichText, Ui, UiBuilder};

use turbogit_app::operation::Operation;
use turbogit_app::root_caches::Affected;
use turbogit_app::state::{AppState, Dialog, NewBranchBase, PendingConfirm};
use turbogit_domain::model::{Branch, BranchKind, Root, RootId, Upstream};
use turbogit_services::sync_service::PushScope;

use crate::theme::{Palette, TYPE_CONTROL, chrome_font, data_font};
use crate::ui::branch_menu::{BranchMenuAction, BranchMenuProps, branch_menu};
use crate::ui::branch_tree_view::{self, LocalRow, TreeEvent, TreeGroup, TreeProps};
use crate::ui::branches_tree::{self, BranchNode, BranchView};
use crate::ui::components::{
    KIT_BUTTON_H, KitButton, PAD_LIST, PAD_STRIP, SyncKind, TOOLBAR_H, kit_button,
};
use crate::ui::widgets;

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

// --- Issue 04: row state at a glance ------------------------------------------

/// Height of the lane above the list that carries "working…" and the delete-undo
/// banner. Allocated every frame, so a quiet read never shifts the list down.
const QUIET_LANE_H: f32 = 24.0;

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
    pub upstream: Option<Upstream>,
    /// The row's sync badges — one per direction, so a diverged row carries
    /// both — when the branch tracks an upstream and has something to report.
    /// Empty when there is nothing to say.
    pub badge: Vec<(SyncKind, String)>,
}

/// Assemble the row's metadata at `now`.
pub fn row_meta(branch: &Branch, now: chrono::DateTime<chrono::Utc>) -> RowMeta {
    RowMeta {
        stale: is_stale(branch, now),
        upstream: branch.tracking.clone(),
        badge: branch
            .tracking
            .as_ref()
            .map(|_| sync_badge(branch.ahead, branch.behind, branch.gone))
            .unwrap_or_default(),
    }
}

use crate::ui::components::sync_badge;

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
    let branches_tree = &state.ui.branches_tree;
    let view = branches_tree::build_branch_view(&state.multi.roots, &tags_by_root, &|root| {
        branch_tree_view::remotes_revealed(branches_tree, root)
    });

    // Esc clears the filter first, then closes the context menu, then drops the
    // selection (§9). Read at the very top of the frame, before any widget could
    // consume it: one press closes the menu and disturbs nothing else.
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        if !state.ui.branches_filter.trim().is_empty() {
            state.ui.branches_filter.clear();
        } else if state.ui.branches_tree.context_menu.is_some() {
            state.ui.branches_tree.context_menu = None;
        } else {
            state.ui.branches_tree.selected = None;
            state.ui.branches_tree.selected_root = None;
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
    // The list owns everything below the toolbar: no side panel splits the
    // width, so there is no divider to paint under the list.
    let list_rect = content_rect;
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
            apply_branch_action(state, rid, branch, BranchMenuAction::Delete);
        }
    }
}

/// Toolbar (one row, §12): the search input takes the left space and the
/// action cluster (New Branch, then the scope chip in multi-repo) the right
/// edge. Each half is painted into an explicit rect of the shared row: a
/// `with_layout` cluster spans everything the parent has left, so anything
/// added after it lands at the band's right edge — off-screen. Remote
/// visibility is controlled by the tree rollups.
fn toolbar(ui: &mut Ui, state: &mut AppState) {
    let band = ui.available_rect_before_wrap();
    let row = Rect::from_center_size(band.center(), egui::vec2(band.width(), KIT_BUTTON_H));

    // The cluster is measured by painting it: its used left edge is where the
    // search input stops.
    let mut actions_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(row)
            .layout(Layout::right_to_left(Align::Center)),
    );
    actions_ui.add_space(PAD_STRIP);
    // Primary action: New Branch (issue 04 — blue).
    if kit_button(&mut actions_ui, KitButton::Primary, "New Branch").clicked() {
        state.open_new_branch(NewBranchBase::Unset, true, None);
    }
    // Scope label (issue 04): only when several repos are in scope.
    if state.multi.roots.len() > 1 {
        actions_ui.add_space(PAD_STRIP);
        scope_label(&mut actions_ui, state);
    }
    let actions_left = actions_ui.min_rect().min.x;

    let search_right = (actions_left - PAD_STRIP).max(row.min.x + PAD_STRIP);
    let mut search_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_max(
                Pos2::new(row.min.x + PAD_STRIP, row.min.y),
                Pos2::new(search_right, row.max.y),
            ))
            .layout(Layout::left_to_right(Align::Center)),
    );
    let search = widgets::search_input(
        &mut search_ui,
        "Search branches",
        &mut state.ui.branches_filter,
    );
    if state.ui.branches_focus_search {
        search.request_focus();
        state.ui.branches_focus_search = false;
    }
}

/// The scope label (issue 04): "all N repos" when nothing is narrowed, or
/// "filtered to X" when a single repo is selected. The label opens a picker that
/// narrows the list to one repo using today's filter semantics.
///
/// Not a chip, and deliberately not one: it paints **coloured text with no
/// background at all**, so it shares no geometry, no radius and no fill with the
/// shared chip vocabulary. Turning it into a real chip would introduce a
/// background where none exists and would newly register an accessibility node
/// for a piece of status text — a design change that needs its own ticket, not a
/// consolidation. See `docs/design-system-roles.md`.
fn scope_label(ui: &mut Ui, state: &mut AppState) {
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
/// returned events and then paints the context menu that depends on them.
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
        allows_context_menu: true,
        id_salt: "branches_list",
        full_height: true,
        collapse_remotes_by_default: false,
    };

    // Issue 15: an operation in flight and the delete-undo window both speak
    // above the list. They share a lane that is allocated every frame, with or
    // without content, so neither one shifts the list it reports on. The banner
    // wins when both would speak: it is actionable and self-expiring, while the
    // shell's activity strip already carries the in-flight state.
    let (lane, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), QUIET_LANE_H),
        egui::Sense::hover(),
    );
    let mut lane_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(lane)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    if state.ui.branches_undo.is_some() {
        undo_banner(&mut lane_ui, state);
    } else if has_any_data && state.ui.busy {
        lane_ui.horizontal(|ui| {
            ui.add_space(PAD_LIST);
            ui.label(
                RichText::new("working…")
                    .font(data_font(TYPE_CONTROL))
                    .color(Palette::T_MUTED),
            );
        });
    }

    let events = branch_tree_view::branch_tree(ui, &props, &mut state.ui.branches_tree);
    for event in events {
        apply_tree_event(state, event);
    }

    // The right-click context menu floats over the list, anchored at the
    // pointer position where the row was right-clicked. Events were applied
    // before this paint, so it opens in the frame it was clicked. The host
    // owns that lifecycle; this surface owns only which row's menu is open.
    if let Some((root_id, name)) = state.ui.branches_tree.context_menu.clone() {
        let target = widgets::menu_host::target_of(&root_id, &name);
        let root_name = root_id.name();
        let mut dismiss = false;
        let picked = widgets::menu_host::host_menu(
            ui,
            widgets::menu_host::MenuId::new("branches", &target),
            true,
            &mut dismiss,
            |ui| {
                let mut picked = None;
                if let Some(root) = state.multi.by_id(&root_id).cloned()
                    && let Some(branch) = root.branches.iter().find(|b| b.name == name).cloned()
                {
                    let props = BranchMenuProps {
                        repo_name: &root_name,
                        multi_repo: state.multi.roots.len() > 1,
                        current_branch: root.current_branch.as_deref(),
                    };
                    picked = branch_menu(ui, &props, &branch);
                }
                picked
            },
        );
        if dismiss {
            state.ui.branches_tree.context_menu = None;
        }
        // The dispatcher closes the menu before its action runs, so an item
        // click and a dismissal never race over the same field.
        if let Some(action) = picked.flatten() {
            let branch = state
                .multi
                .by_id(&root_id)
                .and_then(|r| r.branches.iter().find(|b| b.name == name).cloned());
            if let Some(branch) = branch {
                apply_branch_action(state, &root_id, &branch, action);
            }
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
            state.ui.branches_tree.context_menu = None;
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
        TreeEvent::ContextMenuRequested { root, branch } => {
            state.ui.branches_tree.context_menu = Some((root, branch));
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
        TreeEvent::RemoteRevealToggled { root, revealed } => {
            // One repository's own reveal. `show_remotes` stays the view-wide
            // switch the Git Log pane forces.
            if revealed {
                state.ui.branches_tree.remotes_revealed.insert(root);
            } else {
                state.ui.branches_tree.remotes_revealed.remove(&root);
            }
        }
        TreeEvent::FetchRequested { root } => {
            // The header belongs to one repository, and Fetch means that one.
            fetch_root(state, &root);
        }
        TreeEvent::RenameStarted { root, branch } => {
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
            // The no-match state carries the typed query; the empty state
            // starts from a blank name.
            let name = if name.is_empty() { None } else { Some(name) };
            state.open_new_branch(NewBranchBase::Unset, true, name);
        }
        TreeEvent::ClearFilterRequested => {
            state.ui.branches_filter.clear();
        }
    }
}

/// The view-wide Fetch (issue 03): the narrowed repo's scope, or all in-scope
/// repos when nothing is narrowed. Reachable from the command palette and
/// the Git Log pane; a repo header uses [`fetch_root`] for just its own
/// repository.
pub fn fetch_scope(state: &mut AppState) {
    let targets: Vec<RootId> = match &state.ui.branches_repo_filter {
        Some(id) => vec![id.clone()],
        None => state.multi.roots.iter().map(|r| r.id.clone()).collect(),
    };
    for rid in targets {
        fetch_root(state, &rid);
    }
}

/// Fetch one repository. `dispatch` records its pre-fetch remote snapshot, so
/// the report can say what *that* repo changed.
pub fn fetch_root(state: &mut AppState, rid: &RootId) {
    state.dispatch(Operation::Fetch {
        roots: vec![rid.clone()],
    });
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
            state.dispatch(Operation::custom(
                format!("Restore branch {n}"),
                Affected::Root(root.clone()),
                move |v| v.branch_create(root.as_path(), &n, false, Some(&sha)),
            ));
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
            detach: false,
        });
        return;
    }

    crate::ui::branch_widget::push_recent(&mut state.ui.recent_branches, &branch.name);
    state.checkout_branch_op(root, branch.kind, &branch.name);
}

/// Wrap prose within the visible panel, including both side insets.
/// Detail panel (280px): paints its background + left divider, then the
/// selected branch's block (issue 05) or the quiet selection prompt. It never
/// blocks the list — it is a side panel on the same frame.
/// One quiet relationship line: the sync state vs its tracked remote
/// ("2 ahead · 1 behind · tracks origin/main"). The status words come from
/// [`sync_badge`], the same source the row's chips read, so the panel and the
/// list cannot drift.
/// The action list in the spec's order and wording (issue 05), now a shim
/// over the context menu's union: the same six items, one width, one order,
/// dispatched by [`apply_branch_action`] like every other affordance.
/// Goes with the panel.
/// Returns the picked action, or `None`.
/// Turn one branch action into what the app already knows how to do — the
/// single dispatcher behind the context menu and the detail panel. No new
/// named `Operation`, no git call from the UI. The menu closes first, so
/// acting never leaves a stale one behind.
fn apply_branch_action(
    state: &mut AppState,
    root: &RootId,
    branch: &Branch,
    action: BranchMenuAction,
) {
    state.ui.branches_tree.context_menu = None;
    match action {
        BranchMenuAction::Checkout => checkout_branch(state, root, branch),
        BranchMenuAction::NewBranchFrom => {
            // The same prefill `CreateBranchRequested` performs, with the
            // clicked row as the base.
            state.open_new_branch(NewBranchBase::Branch(branch.name.clone()), true, None);
        }
        BranchMenuAction::CheckoutAndPull => {
            // One composite operation, modelled on the branches popup's
            // checkout-then-pull. It must not also fire the standalone
            // checkout confirmation — that guard belongs to `Checkout`.
            let root_id = root.clone();
            let name = branch.name.clone();
            let rebase =
                state.settings.update_method == turbogit_domain::model::UpdateMethod::Rebase;
            state.dispatch(Operation::custom(
                "Checkout and pull",
                Affected::Root(root_id.clone()),
                move |v| {
                    let path = root_id.as_path();
                    v.branch_checkout(path, &name)?;
                    v.pull(path, rebase)
                },
            ));
        }
        BranchMenuAction::Pull => {
            // The port's pull has no branch parameter, which is exactly why
            // this item is gated to the checked-out branch.
            let rebase =
                state.settings.update_method == turbogit_domain::model::UpdateMethod::Rebase;
            let r = root.clone();
            state.dispatch(Operation::custom(
                "Pull",
                Affected::Root(r.clone()),
                move |v| v.pull(r.as_path(), rebase),
            ));
        }
        BranchMenuAction::Push => {
            // Prefilled at that branch, consistent with Ctrl+Shift+K: the
            // engine's push takes an explicit branch, so a non-current
            // branch needs no checkout first.
            // `ensure_target_defaults` re-fills Remote/Branch while the
            // remote field is empty, so the prefill states both.
            let remote = branch
                .tracking
                .as_ref()
                .map(|t| t.remote.clone())
                .or_else(|| {
                    state
                        .multi
                        .by_id(root)
                        .and_then(|r| r.remotes.first())
                        .map(|r| r.name.clone())
                })
                .unwrap_or_else(|| "origin".to_string());
            state.ui.dlg.push_remote = remote;
            state.ui.dlg.push_branch = branch.name.clone();
            state.ui.dlg.push_scope = PushScope::ThisRepo;
            state.ui.dialog = Some(Dialog::Push);
        }
        BranchMenuAction::Rename => {
            // The tree's inline rename — one rename experience per surface.
            // The selection follows the row so the editor opens on the
            // branch that was right-clicked.
            state.ui.branches_tree.renaming = Some(branch.name.clone());
            state.ui.branches_tree.rename_draft = branch.name.clone();
            state.ui.branches_tree.selected = Some(branch.name.clone());
            state.ui.branches_tree.selected_root = Some(root.clone());
        }
        BranchMenuAction::Merge => state.open_merge_into(root, &branch.name),
        BranchMenuAction::Rebase => {
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
        BranchMenuAction::Compare => state.open_compare(root, &branch.name),
        BranchMenuAction::Delete => match branch.kind {
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
    let count = state.unmerged_commit_count(id, &branch.name);
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
