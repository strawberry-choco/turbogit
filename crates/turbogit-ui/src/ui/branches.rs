use egui::{Align, Layout, Pos2, Rect, RichText, Ui, UiBuilder};

use turbogit_app::operation::Operation;
use turbogit_app::persistence::BranchesViewOptions;
use turbogit_app::shell_writes;
use turbogit_app::state::{AppState, Dialog, NewBranchBase, PendingConfirm, Toast};
use turbogit_app::sync_service::PushScope;
use turbogit_domain::model::{Branch, BranchKind, RootId, Upstream};

use crate::theme::{Palette, TYPE_CONTROL, chrome_font, data_font};
use crate::ui::branch_menu::{BranchMenuAction, BranchMenuProps, branch_menu};
use crate::ui::branch_tree_view::{self, LocalRow, TreeEvent, TreeGroup, TreeProps};
use crate::ui::branches_tree::{self, BranchNode, BranchView};
use crate::ui::components::{
    self, KIT_BUTTON_H, KitButton, PAD_LIST, PAD_STRIP, SyncKind, TOOLBAR_H, kit_button,
};
use crate::ui::icons::Icon;
use crate::ui::widgets;

const QUIET_LANE_H: f32 = 24.0;

pub const STALE_THRESHOLD: chrono::Duration = chrono::Duration::days(28);

pub fn is_stale(branch: &Branch, now: chrono::DateTime<chrono::Utc>) -> bool {
    branch
        .last_touched
        .is_some_and(|t| now.signed_duration_since(t) > STALE_THRESHOLD)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowMeta {
    pub stale: bool,
    pub upstream: Option<Upstream>,
    pub badge: Vec<(SyncKind, String)>,
}

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

/// Case-insensitive subsequence match: `mre` surfaces `multi-root-executor`
/// (design doc §3, fuzzy and forgiving).
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

pub fn matches_query(haystack: &str, query: &str) -> bool {
    let q = query.trim();
    if q.is_empty() {
        return true;
    }
    q.split_whitespace().all(|w| fuzzy_word(haystack, w))
}

pub fn branch_matches(branch: &Branch, query: &str) -> bool {
    if matches_query(&branch.name, query) {
        return true;
    }
    branch
        .tip
        .as_ref()
        .is_some_and(|t| matches_query(&t.message, query))
}

pub fn show(ui: &mut Ui, state: &mut AppState) {
    if state.multi.roots.is_empty() {
        return;
    }
    branch_tree_view::warm_tags(state);
    state.warm_branch_recents();
    let view = branches_tree::build_branch_view_with(
        &state.multi.roots,
        state.caches.tags_by_root(),
        state.caches.recents_by_root(),
        &|root| branch_tree_view::remotes_revealed(&state.ui.branches_tree, root),
        &state.ui.branches_view,
    );

    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        if state.ui.branches_settings_open {
            state.ui.branches_settings_open = false;
        } else if !state.ui.branches_filter.trim().is_empty() {
            state.ui.branches_filter.clear();
        } else if state.ui.branches_tree.context_menu.is_some() {
            state.ui.branches_tree.context_menu = None;
        } else {
            state.ui.branches_tree.selected = None;
            state.ui.branches_tree.selected_root = None;
        }
    }

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
    let list_rect = content_rect;
    let mut list_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(list_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    list_area(&mut list_ui, state, &view);
    ui.advance_cursor_after_rect(list_rect);

    let mut dismiss = false;
    let changed = widgets::menu_host::host_menu(
        ui,
        widgets::menu_host::MenuId::new("branches_settings", ""),
        state.ui.branches_settings_open,
        &mut dismiss,
        |ui| settings_toggles(ui, &mut state.ui.branches_view),
    )
    .unwrap_or(false);
    if dismiss || changed {
        state.ui.branches_settings_open = false;
    }
    if changed {
        state.save_durable_ui();
    }
}

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

fn handle_keys(ui: &mut Ui, state: &mut AppState, view: &BranchView) {
    if state.ui.dialog.is_some()
        || state.ui.confirm.is_some()
        || state.ui.branches_tree.renaming.is_some()
        || state.ui.branches_scope_picker_open
        || state.ui.branches_settings_open
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
            apply_branch_action(state, ui.ctx(), rid, branch, BranchMenuAction::Delete);
        }
    }
}

fn toolbar(ui: &mut Ui, state: &mut AppState) {
    let band = ui.available_rect_before_wrap();
    let row = Rect::from_center_size(band.center(), egui::vec2(band.width(), KIT_BUTTON_H));

    let mut actions_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(row)
            .layout(Layout::right_to_left(Align::Center)),
    );
    actions_ui.add_space(PAD_STRIP);
    if kit_button(&mut actions_ui, KitButton::Primary, "New Branch").clicked() {
        state.open_new_branch(NewBranchBase::Unset, true, None);
    }
    if state.multi.roots.len() > 1 {
        actions_ui.add_space(PAD_STRIP);
        scope_chip(&mut actions_ui, state);
    }
    actions_ui.add_space(PAD_STRIP);
    let gear =
        widgets::icon_button(&mut actions_ui, Icon::SETTINGS).on_hover_text("Branch list settings");
    if gear.clicked() {
        state.ui.branches_settings_open = !state.ui.branches_settings_open;
        if state.ui.branches_settings_open {
            widgets::menu_host::note_anchor(
                &actions_ui,
                widgets::menu_host::MenuId::new("branches_settings", ""),
                gear.rect.left_bottom(),
            );
        }
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

fn settings_toggles(ui: &mut Ui, options: &mut BranchesViewOptions) -> bool {
    ui.spacing_mut().item_spacing = crate::theme::ITEM_SPACING;
    let mut changed = false;
    changed |= toggle_row(
        ui,
        &mut options.group_by_directory,
        "Group by Directory",
        "Nest branches under a header per `folder/` prefix",
    );
    changed |= toggle_row(
        ui,
        &mut options.group_by_repository,
        "Group by Repository",
        "One set of groups per repository in scope",
    );
    changed |= toggle_row(
        ui,
        &mut options.show_recent,
        "Show Recent Branches",
        "The branches HEAD moved onto most recently",
    );
    changed |= toggle_row(
        ui,
        &mut options.show_tags,
        "Show Tags",
        "Tags beside the branches, in their own group",
    );
    changed
}

fn toggle_row(ui: &mut Ui, flag: &mut bool, label: &str, help: &str) -> bool {
    ui.checkbox(flag, label).on_hover_text(help).changed()
}

fn scope_chip(ui: &mut Ui, state: &mut AppState) {
    let label = match &state.ui.branches_repo_filter {
        Some(id) => format!("filtered to {}", id.name()),
        None => format!("all {} repos", state.multi.roots.len()),
    };
    if components::scope_chip(ui, &label).clicked() {
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

fn list_area(ui: &mut Ui, state: &mut AppState, view: &BranchView) {
    let has_any_data = state
        .multi
        .roots
        .iter()
        .any(|r| !r.branches.is_empty() || r.head.is_some());
    let filter = state.ui.branches_filter.clone();
    let repo_filter = state.ui.branches_repo_filter.clone();

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

    let props = TreeProps {
        view,
        filter: &filter,
        repo_filter: repo_filter.as_ref(),
        tags_by_root: state.caches.tags_by_root(),
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

    let events = branch_tree_view::branch_tree(ui, &props, &mut state.ui.branches_tree);
    for event in events {
        apply_tree_event(state, event);
    }

    if let Some((root_id, name)) = state.ui.branches_tree.context_menu.clone() {
        let target = widgets::menu_host::target_of(&root_id, &name);
        let root_name = root_id.name();
        let ctx = ui.ctx().clone();
        widgets::menu_host::host_row_menu(
            ui,
            state,
            "branches",
            target,
            |ui, state| {
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
            |state| state.ui.branches_tree.context_menu = None,
            |state, action| {
                let branch = state
                    .multi
                    .by_id(&root_id)
                    .and_then(|r| r.branches.iter().find(|b| b.name == name).cloned());
                if let Some(branch) = branch {
                    apply_branch_action(state, &ctx, &root_id, &branch, action);
                }
            },
        );
    }
}

fn apply_tree_event(state: &mut AppState, event: TreeEvent) {
    match event {
        TreeEvent::RowClicked { root, branch } => {
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
            TreeGroup::Recent => {
                let on = state.ui.branches_tree.groups.recent;
                state.ui.branches_tree.groups.recent = !on;
            }
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
            if revealed {
                state.ui.branches_tree.remotes_revealed.insert(root);
            } else {
                state.ui.branches_tree.remotes_revealed.remove(&root);
            }
        }
        TreeEvent::FetchRequested { root } => {
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
            if state.ui.branches_tree.selected.as_deref() == Some(old.as_str()) {
                state.ui.branches_tree.selected = Some(new.clone());
            }
            state.rename_branch(&root, &old, &new, false);
        }
        TreeEvent::RenameCancelled => {
            state.ui.branches_tree.renaming = None;
        }
        TreeEvent::CreateBranchRequested { name } => {
            let name = if name.is_empty() { None } else { Some(name) };
            state.open_new_branch(NewBranchBase::Unset, true, name);
        }
        TreeEvent::ClearFilterRequested => {
            state.ui.branches_filter.clear();
        }
    }
}

pub fn fetch_scope(state: &mut AppState) {
    let targets: Vec<RootId> = match &state.ui.branches_repo_filter {
        Some(id) => vec![id.clone()],
        None => state.multi.roots.iter().map(|r| r.id.clone()).collect(),
    };
    for rid in targets {
        fetch_root(state, &rid);
    }
}

pub fn fetch_root(state: &mut AppState, rid: &RootId) {
    state.dispatch(Operation::Fetch {
        roots: vec![rid.clone()],
    });
}

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
        if let Some(unmerged) = &undo.unmerged
            && kit_button(ui, KitButton::Quiet, &unmerged.link_label()).clicked()
        {
            state.open_unmerged_commits(unmerged);
        }
        if let Some(upstream) = &undo.upstream {
            let label = format!("Remove the tracking remote branch {}", upstream.git_ref());
            if kit_button(ui, KitButton::Quiet, &label).clicked() {
                state.ui.confirm = Some(PendingConfirm::DeleteRemoteBranch {
                    remote: upstream.remote.clone(),
                    name: upstream.branch.clone(),
                });
            }
        }
        if kit_button(ui, KitButton::Secondary, "Undo delete").clicked() {
            state.ui.branches_undo = None;
            let op = shell_writes::branch::restore(&undo.root, &undo.name, &undo.tip_sha);
            state.dispatch(op);
        }
    });
    ui.add_space(4.0);
}

fn checkout_branch(state: &mut AppState, root: &RootId, branch: &Branch) {
    let Some(r) = state.multi.by_id(root).cloned() else {
        return;
    };

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

fn apply_branch_action(
    state: &mut AppState,
    ctx: &egui::Context,
    root: &RootId,
    branch: &Branch,
    action: BranchMenuAction,
) {
    match action {
        BranchMenuAction::CopyBranchName => {
            ctx.copy_text(branch.name.clone());
            state.ui.toast_shown_at = None;
            state.ui.toast = Some(Toast::success(format!("Copied {}", branch.name)));
        }
        BranchMenuAction::Checkout => checkout_branch(state, root, branch),
        BranchMenuAction::NewBranchFrom => {
            state.open_new_branch(NewBranchBase::Branch(branch.name.clone()), true, None);
        }
        BranchMenuAction::CheckoutAndPull => {
            state.dispatch_pull("Checkout and pull", root.clone(), Some(branch.name.clone()));
        }
        BranchMenuAction::Pull => {
            state.dispatch_pull("Pull", root.clone(), None);
        }
        BranchMenuAction::Push => {
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
            state.ui.branches_tree.renaming = Some(branch.name.clone());
            state.ui.branches_tree.rename_draft = branch.name.clone();
            state.ui.branches_tree.selected = Some(branch.name.clone());
            state.ui.branches_tree.selected_root = Some(root.clone());
        }
        BranchMenuAction::Merge => state.open_merge_into(root, &branch.name),
        BranchMenuAction::Rebase => {
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
        BranchMenuAction::DiffWithWorkingTree => {
            state.open_get_from_branch(root, &branch.name);
        }
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
                state.ui.branches_delete_consequence = delete_consequence(state, root, branch);
                state.ui.branches_delete_unmerged = state.unmerged_commits(root, &branch.name);
                state.ui.confirm = Some(PendingConfirm::DeleteLocalBranch {
                    name: branch.name.clone(),
                });
            }
        },
    }
}

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
