use crate::theme::Palette;
use crate::ui::icons::{self, Icon};
use crate::ui::widgets;
use egui::{Align, Color32, Key, Layout, RichText, ScrollArea, Ui, vec2};
use turbogit_app::root_caches::Affected;
use turbogit_app::shell_writes;
use turbogit_app::state::{AppState, Dialog, NewBranchBase, PendingConfirm};
use turbogit_app::sync_service;
use turbogit_domain::model::{Branch, BranchKind, RootId};

pub const STAR_COLOR: Color32 = Palette::STATE_WARNING;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PopupEntry {
    Recent {
        name: String,
        favorite: bool,
        ahead: usize,
        behind: usize,
        gone: bool,
        last_touched: Option<chrono::DateTime<chrono::Utc>>,
    },
    Local {
        name: String,
        favorite: bool,
        ahead: usize,
        behind: usize,
        gone: bool,
        last_touched: Option<chrono::DateTime<chrono::Utc>>,
    },
    Remote {
        name: String,
        ahead: usize,
        behind: usize,
        gone: bool,
        last_touched: Option<chrono::DateTime<chrono::Utc>>,
    },
    Tag {
        name: String,
    },
}

impl PopupEntry {
    pub fn label(&self) -> String {
        match self {
            Self::Recent { name, .. } | Self::Local { name, .. } | Self::Tag { name } => {
                name.clone()
            }
            Self::Remote { name, .. } => format!("origin/{name}"),
        }
    }

    fn branch_name(&self) -> &str {
        match self {
            Self::Recent { name, .. } | Self::Local { name, .. } | Self::Remote { name, .. } => {
                name
            }
            Self::Tag { name } => name,
        }
    }

    fn last_touched(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        match self {
            Self::Recent { last_touched, .. }
            | Self::Local { last_touched, .. }
            | Self::Remote { last_touched, .. } => *last_touched,
            Self::Tag { .. } => None,
        }
    }

    pub fn matches(&self, filter_lower: &str) -> bool {
        filter_lower.is_empty() || self.label().to_lowercase().contains(filter_lower)
    }

    fn starred(&self) -> bool {
        matches!(
            self,
            Self::Recent { favorite: true, .. } | Self::Local { favorite: true, .. }
        )
    }

    fn section(&self) -> &'static str {
        match self {
            Self::Recent { .. } => "RECENT",
            Self::Local { .. } => "LOCAL",
            Self::Remote { .. } => "REMOTE",
            Self::Tag { .. } => "TAGS",
        }
    }
}

/// Flattened, ordered popup rows: RECENT (starred first), LOCAL (favorites
/// first, alphabetical, current excluded, it is pinned separately), REMOTE,
/// TAGS, every row filtered against `filter`.
pub fn popup_entries(
    locals: &[Branch],
    remotes: &[Branch],
    tags: &[String],
    recents: &[String],
    current: Option<&str>,
    filter: &str,
) -> Vec<PopupEntry> {
    let f = filter.to_lowercase();
    let fav_of = |name: &str| {
        locals
            .iter()
            .find(|b| b.name == name)
            .map(|b| b.favorite)
            .unwrap_or(false)
    };

    let mut out = Vec::new();

    let mut recent: Vec<PopupEntry> = recents
        .iter()
        .filter(|n| Some(n.as_str()) != current)
        .map(|n| {
            let b = locals.iter().find(|b| &b.name == n);
            PopupEntry::Recent {
                name: n.clone(),
                favorite: fav_of(n),
                ahead: b.map_or(0, |b| b.ahead),
                behind: b.map_or(0, |b| b.behind),
                gone: b.is_some_and(|b| b.gone),
                last_touched: b.and_then(|b| b.last_touched),
            }
        })
        .collect();
    recent.sort_by_key(|e| std::cmp::Reverse(e.starred()));
    out.extend(recent.into_iter().filter(|e| e.matches(&f)));

    let mut locs: Vec<&Branch> = locals
        .iter()
        .filter(|b| Some(b.name.as_str()) != current)
        .collect();
    locs.sort_by(|a, b| b.favorite.cmp(&a.favorite).then(a.name.cmp(&b.name)));
    out.extend(
        locs.into_iter()
            .map(|b| PopupEntry::Local {
                name: b.name.clone(),
                favorite: b.favorite,
                ahead: b.ahead,
                behind: b.behind,
                gone: b.gone,
                last_touched: b.last_touched,
            })
            .filter(|e| e.matches(&f)),
    );

    let mut seen: Vec<String> = remotes.iter().map(|b| b.name.clone()).collect();
    let mut rem_rows: Vec<PopupEntry> = remotes
        .iter()
        .map(|b| {
            let local = locals.iter().find(|l| {
                l.tracking.as_ref().is_some_and(|u| {
                    u.branch == b.name && b.remote.as_ref().is_some_and(|r| *r == u.remote)
                })
            });
            PopupEntry::Remote {
                name: b.name.clone(),
                ahead: local.map_or(0, |l| l.ahead),
                behind: local.map_or(0, |l| l.behind),
                gone: local.is_some_and(|l| l.gone),
                last_touched: local.and_then(|l| l.last_touched).or(b.last_touched),
            }
        })
        .filter(|e| e.matches(&f))
        .collect();
    for local in locals {
        let Some(track) = &local.tracking else {
            continue;
        };
        // A branch tracking another *local* branch (git's `.` remote) is not a
        // remote row.
        if track.remote == "." {
            continue;
        }
        let name = &track.branch;
        if seen.iter().any(|s| s == name) {
            continue;
        }
        seen.push(name.to_string());
        let entry = PopupEntry::Remote {
            name: name.to_string(),
            ahead: local.ahead,
            behind: local.behind,
            gone: local.gone,
            last_touched: local.last_touched,
        };
        if entry.matches(&f) {
            rem_rows.push(entry);
        }
    }
    rem_rows.sort_by(|a, b| a.branch_name().cmp(b.branch_name()));
    out.extend(rem_rows);

    let mut t: Vec<String> = tags.to_vec();
    t.sort();
    out.extend(
        t.into_iter()
            .map(|name| PopupEntry::Tag { name })
            .filter(|e| e.matches(&f)),
    );

    out
}

pub fn push_recent(recents: &mut Vec<String>, name: &str) {
    recents.retain(|n| n != name);
    recents.insert(0, name.to_string());
    recents.truncate(crate::ui::branches_tree::RECENT_CAP);
}

pub fn sync_notice(roots: usize) -> String {
    format!("Synchronous branch operations across {roots} repositories")
}

/// Compact last-touched age for the popup's stale badge (issue 32, screen
/// 13: "3w"): under a minute `now`, then minutes, hours, days, weeks
/// (< 5 weeks), months (< 12 months), and years beyond that. Floor division
/// so the label reads the largest whole unit.
pub fn stale_badge(age: std::time::Duration) -> String {
    let secs = age.as_secs();
    if secs < 60 {
        return "now".into();
    }
    if secs < 60 * 60 {
        return format!("{}m", secs / 60);
    }
    if secs < 24 * 60 * 60 {
        return format!("{}h", secs / (60 * 60));
    }
    if secs < 7 * 24 * 60 * 60 {
        return format!("{}d", secs / (24 * 60 * 60));
    }
    if secs < 35 * 24 * 60 * 60 {
        return format!("{}w", secs / (7 * 24 * 60 * 60));
    }
    if secs < 365 * 24 * 60 * 60 {
        return format!("{}mo", secs / (30 * 24 * 60 * 60));
    }
    format!("{}y", secs / (365 * 24 * 60 * 60))
}

pub fn remote_marker(ahead: usize, behind: usize, gone: bool) -> Option<String> {
    if gone {
        Some("gone".into())
    } else if crate::theme::DivergenceMeaning::AnyDrift.is_diverged(ahead, behind) {
        Some(format!("↑{ahead} ↓{behind}"))
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowActions {
    pub delete: bool,
    pub rename: bool,
    pub protected: bool,
}

/// Gating rules for one popup row (issue 32): destructive actions are gated
/// on protected branches; the current branch can never be deleted; tags are
/// neither deletable nor renameable. `exists` is false for a recent entry
/// whose local branch has since been deleted. Compare… is offered for every
/// row, so it has no gate.
pub fn row_actions(
    entry: &PopupEntry,
    current: Option<&str>,
    protected: bool,
    exists: bool,
) -> RowActions {
    let is_current = Some(entry.branch_name()) == current;
    let deletable = exists
        && matches!(
            entry,
            PopupEntry::Recent { .. } | PopupEntry::Local { .. } | PopupEntry::Remote { .. }
        );
    let renameable =
        exists && matches!(entry, PopupEntry::Recent { .. } | PopupEntry::Local { .. });
    RowActions {
        delete: deletable && !is_current && !protected,
        rename: renameable && !is_current,
        protected,
    }
}

/// Why Delete is gated on this row, or `None` when it is enabled. Gated
/// actions stay rendered (dimmed) with this explanation on hover, the
/// repo's disabled-with-reason convention, so the reason is always
/// discoverable rather than the control silently vanishing.
pub fn delete_gate(
    entry: &PopupEntry,
    current: Option<&str>,
    protected: bool,
    exists: bool,
) -> Option<&'static str> {
    if row_actions(entry, current, protected, exists).delete {
        return None;
    }
    Some(if !exists {
        "this branch no longer exists"
    } else if Some(entry.branch_name()) == current {
        "check out another branch first"
    } else if protected {
        "protected branches cannot be deleted"
    } else {
        "tags are not deleted from here"
    })
}

/// Why Rename is gated on this row, or `None` when it is enabled, same
/// contract as [`delete_gate`].
pub fn rename_gate(
    entry: &PopupEntry,
    current: Option<&str>,
    protected: bool,
    exists: bool,
) -> Option<&'static str> {
    if row_actions(entry, current, protected, exists).rename {
        return None;
    }
    Some(match entry {
        PopupEntry::Remote { .. } => "remote branches cannot be renamed",
        PopupEntry::Tag { .. } => "tags cannot be renamed",
        _ if !exists => "this branch no longer exists",
        _ => "the current branch cannot be renamed from here",
    })
}

pub fn widget(ui: &mut Ui, state: &mut AppState) {
    let branch = state
        .selected_root
        .as_ref()
        .and_then(|id| state.multi.by_id(id))
        .and_then(|r| r.current_branch.clone())
        .unwrap_or_else(|| "<detached>".to_string());
    if ui.button(format!("⎇ {branch}")).clicked() {
        state.ui.branches_popup = true;
    }
}

pub fn branches_popup(ui: &mut Ui, state: &mut AppState) {
    if !state.ui.branches_popup {
        return;
    }

    if ui.input(|i| i.key_pressed(Key::Escape)) {
        state.ui.branches_popup = false;
        return;
    }

    let ctx = ui.ctx().clone();
    let mut open = true;

    let root_id = state.selected_root.clone();
    let root = root_id
        .as_ref()
        .and_then(|id| state.multi.by_id(id))
        .cloned();

    egui::Window::new("Branches")
        .open(&mut open)
        .default_width(420.0)
        .show(&ctx, |ui| {
            ui.horizontal(|ui| {
                let search =
                    widgets::search_input(ui, "Search branches", &mut state.ui.branch_filter);
                search.request_focus();
                if widgets::icon_button(ui, Icon::X).clicked() {
                    state.ui.branches_popup = false;
                }
            });
            ui.separator();

            if widgets::compact_button(ui, "New Branch…").clicked() {
                state.open_new_branch(NewBranchBase::Head, true, None);
                state.ui.branches_popup = false;
            }

            let Some(root) = root else {
                widgets::empty_state(ui, "No repository selected");
                return;
            };
            let id = root.id.clone();
            let current = root.current_branch.clone();

            if let Some(cur) = &current {
                ui.add_space(2.0);
                ui.label(
                    RichText::new(format!("✓ {cur}"))
                        .strong()
                        .color(Palette::BRAND),
                );
                ui.add_space(2.0);
            }

            let tags = state.tag_names(&id).unwrap_or_default();
            let locals: Vec<Branch> = root
                .branches
                .iter()
                .filter(|b| b.kind == BranchKind::Local)
                .cloned()
                .collect();
            let remotes: Vec<Branch> = root
                .branches
                .iter()
                .filter(|b| b.kind == BranchKind::Remote)
                .cloned()
                .collect();

            let entries = popup_entries(
                &locals,
                &remotes,
                &tags,
                &state.ui.recent_branches,
                current.as_deref(),
                &state.ui.branch_filter,
            );

            let n = entries.len();
            let (up, down, enter) = ui.input(|i| {
                (
                    i.key_pressed(Key::ArrowUp),
                    i.key_pressed(Key::ArrowDown),
                    i.key_pressed(Key::Enter),
                )
            });
            if n > 0 {
                if down {
                    state.ui.branches_cursor = (state.ui.branches_cursor + 1).min(n - 1);
                }
                if up {
                    state.ui.branches_cursor = state.ui.branches_cursor.saturating_sub(1);
                }
            }
            state.ui.branches_cursor = state.ui.branches_cursor.min(n.saturating_sub(1));

            // Reserve an explicit body height (spec §8.5: max-height
            // viewport − 48). A bare `max_height` here lets the egui Window's
            // remembered size clamp the visible area on every later frame,
            // permanently culling lower rows; demanding the region up front
            // keeps the list scrollable at a stable size instead.
            let view_h = ctx.input(|i| i.raw.screen_rect.map_or(568.0, |r| r.height()));
            let body_h = (view_h - 48.0).clamp(160.0, 620.0);
            ui.allocate_ui_with_layout(
                vec2(ui.available_width(), body_h),
                Layout::top_down(Align::Min),
                |ui| {
                    ScrollArea::vertical()
                        .id_salt("branch_popup_list")
                        .max_height(body_h)
                        .show(ui, |ui| {
                            let mut clicked: Option<PopupEntry> = None;
                            let mut toggled_star: Option<String> = None;
                            let mut row_intent: Option<BranchRowAction> = None;
                            let mut last_section: Option<&'static str> = None;

                            for (idx, e) in entries.iter().enumerate() {
                                if last_section != Some(e.section()) {
                                    if last_section.is_some() {
                                        ui.separator();
                                    }
                                    widgets::group_title(ui, e.section());
                                    last_section = Some(e.section());
                                }

                                let selected = idx == state.ui.branches_cursor;
                                let label = e.label();
                                let protected =
                                    sync_service::is_protected(&state.settings, e.branch_name());
                                let exists = match e {
                                    PopupEntry::Remote { .. } | PopupEntry::Tag { .. } => true,
                                    _ => locals.iter().any(|b| b.name == e.branch_name()),
                                };
                                let actions = row_actions(e, current.as_deref(), protected, exists);
                                let band_id = egui::Id::new(("branch-quick-actions", e.label()));
                                let hinted = ui
                                    .ctx()
                                    .memory(|m| m.data.get_temp::<egui::Rect>(band_id))
                                    .is_some_and(|r| ui.rect_contains_pointer(r));
                                let row = ui.horizontal(|ui| {
                                    match e {
                                        PopupEntry::Recent { name, favorite, .. }
                                        | PopupEntry::Local { name, favorite, .. } => {
                                            let glyph = if *favorite { "★" } else { "☆" };
                                            let star = egui::Button::new(
                                                RichText::new(glyph).color(STAR_COLOR),
                                            )
                                            .frame(false);
                                            if ui.add(star).clicked() {
                                                toggled_star = Some(name.clone());
                                            }
                                        }
                                        _ => {}
                                    }
                                    if protected {
                                        icons::icon(ui, Icon::LOCK, 13.0, Palette::STATE_WARNING);
                                    }
                                    if ui.selectable_label(selected, label.clone()).clicked() {
                                        clicked = Some(e.clone());
                                    }
                                    if let Some(ts) = e.last_touched() {
                                        let age = chrono::Utc::now()
                                            .signed_duration_since(ts)
                                            .to_std()
                                            .unwrap_or_default();
                                        ui.label(
                                            RichText::new(stale_badge(age))
                                                .small()
                                                .color(Palette::INK_3),
                                        );
                                    }
                                    if let PopupEntry::Remote {
                                        ahead,
                                        behind,
                                        gone,
                                        ..
                                    } = e
                                        && let Some(marker) = remote_marker(*ahead, *behind, *gone)
                                    {
                                        ui.label(RichText::new(marker).small().color(if *gone {
                                            Palette::STATE_WARNING
                                        } else {
                                            Palette::INK_3
                                        }));
                                    }

                                    if widgets::compact_button(ui, "Checkout").clicked() {
                                        clicked = Some(e.clone());
                                    }
                                    let rename = widgets::compact_button_enabled(
                                        ui,
                                        "Rename",
                                        actions.rename,
                                    );
                                    if let Some(reason) =
                                        rename_gate(e, current.as_deref(), protected, exists)
                                    {
                                        rename.on_disabled_hover_text(reason);
                                    } else if rename.clicked() {
                                        row_intent = Some(BranchRowAction::Rename(e.clone()));
                                    }
                                    let delete = widgets::compact_button_enabled(
                                        ui,
                                        "Delete",
                                        actions.delete,
                                    );
                                    if let Some(reason) =
                                        delete_gate(e, current.as_deref(), protected, exists)
                                    {
                                        delete.on_disabled_hover_text(reason);
                                    } else if delete.clicked() {
                                        row_intent = Some(BranchRowAction::Delete(e.clone()));
                                    }
                                    if widgets::compact_button(ui, "Compare…").clicked() {
                                        row_intent = Some(BranchRowAction::Compare(e.clone()));
                                    }
                                    ui.scope(|ui| {
                                        ui.disable();
                                        let _ = widgets::compact_button(ui, "New Worktree…");
                                    });
                                    if protected && hinted {
                                        if widgets::compact_button(ui, "Pull").clicked() {
                                            row_intent = Some(BranchRowAction::Pull(
                                                e.branch_name().to_string(),
                                            ));
                                        }
                                        if widgets::compact_button(ui, "Merge").clicked() {
                                            row_intent = Some(BranchRowAction::Merge(
                                                e.branch_name().to_string(),
                                            ));
                                        }
                                    }
                                });
                                let band = row.response.rect;
                                ui.ctx().memory_mut(|m| m.data.insert_temp(band_id, band));
                            }

                            if let Some(name) = toggled_star {
                                shell_writes::branch::toggle_favorite(state, &id, &name);
                            }
                            if enter && !entries.is_empty() {
                                clicked = Some(entries[state.ui.branches_cursor].clone());
                            }
                            match (clicked, row_intent) {
                                (Some(e), _) => checkout_entry(state, &id, &e),
                                (None, Some(intent)) => apply_branch_row_action(state, &id, intent),
                                _ => {}
                            }
                        });
                },
            );

            let roots = state.multi.roots.len();
            if roots > 1 && state.settings.synchronous_branches {
                ui.separator();
                ui.horizontal(|ui| {
                    icons::icon(ui, Icon::LAYERS, 14.0, Palette::STATE_INFO);
                    ui.label(RichText::new(sync_notice(roots)).color(Palette::STATE_INFO));
                });
            }

            ui.separator();
            ui.horizontal(|ui| {
                if widgets::compact_button(ui, "+ New branch").clicked() {
                    state.open_new_branch(NewBranchBase::Head, true, None);
                    state.ui.branches_popup = false;
                }
                ui.scope(|ui| {
                    if widgets::compact_button(ui, "Manage remotes…").clicked() {
                        state.ui.dlg.remotes_row_action = None;
                        state.ui.dlg.remotes_apply_results = None;
                        state.ui.dlg.remotes_apply_scope.clear();
                        if let Some(rid) = state.selected_root.clone() {
                            state.ui.dlg.remotes_apply_scope.insert(rid);
                        }
                        state.ui.dialog = Some(Dialog::ManageRemotes);
                        state.ui.branches_popup = false;
                    }
                });
                if widgets::compact_button(ui, "Compare…").clicked()
                    && let Some(e) = entries.get(state.ui.branches_cursor)
                {
                    apply_branch_row_action(state, &id, BranchRowAction::Compare(e.clone()));
                }
            });
        });

    if state.ui.branches_popup {
        state.ui.branches_popup = open;
    }
}

fn checkout_entry(state: &mut AppState, id: &RootId, e: &PopupEntry) {
    let affected = Affected::Root(id.clone());
    match e {
        PopupEntry::Recent { name, .. } | PopupEntry::Local { name, .. } => {
            push_recent(&mut state.ui.recent_branches, name);
            let op = shell_writes::branch::checkout(id, name, affected.clone());
            state.dispatch(op);
        }
        PopupEntry::Remote { name, .. } => {
            push_recent(&mut state.ui.recent_branches, name);
            let op = shell_writes::branch::checkout_new_local(id, name, affected.clone());
            state.dispatch(op);
        }
        PopupEntry::Tag { name } => {
            let op = shell_writes::branch::checkout_detached(id, name, affected);
            state.dispatch(op);
        }
    }
    state.ui.branches_popup = false;
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum BranchRowAction {
    Rename(PopupEntry),
    Delete(PopupEntry),
    Compare(PopupEntry),
    Pull(String),
    Merge(String),
}

fn apply_branch_row_action(state: &mut AppState, id: &RootId, intent: BranchRowAction) {
    match intent {
        BranchRowAction::Rename(e) => state.open_rename_branch(id, e.branch_name()),
        BranchRowAction::Delete(e) => match e {
            PopupEntry::Remote { name, .. } => {
                state.ui.confirm = Some(PendingConfirm::DeleteRemoteBranch {
                    root: id.clone(),
                    remote: "origin".into(),
                    name: name.clone(),
                });
            }
            PopupEntry::Recent { name, .. } | PopupEntry::Local { name, .. } => {
                state.ui.branches_delete_unmerged = state.unmerged_commits(id, name.as_str());
                state.ui.confirm = Some(PendingConfirm::DeleteLocalBranch {
                    root: id.clone(),
                    name: name.clone(),
                });
            }
            PopupEntry::Tag { .. } => {}
        },
        BranchRowAction::Compare(e) => state.open_compare(id, e.branch_name()),
        BranchRowAction::Pull(name) => {
            state.dispatch_pull(format!("Pull {name}"), id.clone(), Some(name));
            state.ui.branches_popup = false;
        }
        BranchRowAction::Merge(name) => state.open_merge_into(id, &name),
    }
}
