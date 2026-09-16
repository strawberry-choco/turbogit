//! Branch widget (status bar) + redesigned Branches popup (spec §8.5).
//!
//! The popup is a searchable floating list grouped RECENT / LOCAL / REMOTE /
//! TAGS with the current branch pinned top and check-marked. Row actions are
//! live (issue 32): Checkout, Rename, Delete (through the rich confirmation
//! from issue 02) and Compare… (a commit-list dialog, spec E9), with a
//! "+ New branch" / "Manage remotes…" / "Compare…" footer. Protected
//! branches carry a lock and gated destructive actions plus pull / merge
//! hover quick-actions; stale rows show the tip's age ("3w") and remote rows
//! show ↑n ↓m or `gone`. New Worktree… stays visibly inert until its flow
//! exists (ADR-0012). Keyboard: typing filters live, ↑/↓ move the highlight,
//! Enter checks the highlighted row out, Esc closes.
//!
//! All git mutations cross the [`turbogit_engine_api::GitExecutor`] seam via
//! [`AppState::run_git`]; the pure row-model helpers are unit-testable.

use crate::theme::Palette;
use crate::ui::icons::{self, Icon};
use crate::ui::widgets;
use egui::{Align, Color32, Key, Layout, RichText, ScrollArea, Ui, vec2};
use turbogit_app::root_caches::Affected;
use turbogit_app::state::{AppState, Dialog, PendingConfirm};
use turbogit_domain::model::{Branch, BranchKind, RootId};
use turbogit_services::{branch_service, sync_service};

/// Favorite-star ink: the central warning token (spec §8.5 row anatomy).
pub const STAR_COLOR: Color32 = Palette::STATE_WARNING;

/// One selectable row of the branches popup.
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
    /// Painted row label (remote rows carry the `origin/` prefix, §8.5).
    pub fn label(&self) -> String {
        match self {
            Self::Recent { name, .. } | Self::Local { name, .. } | Self::Tag { name } => {
                name.clone()
            }
            Self::Remote { name, .. } => format!("origin/{name}"),
        }
    }

    /// The underlying branch name (no `origin/` prefix for remote rows).
    fn branch_name(&self) -> &str {
        match self {
            Self::Recent { name, .. } | Self::Local { name, .. } | Self::Remote { name, .. } => {
                name
            }
            Self::Tag { name } => name,
        }
    }

    /// Branch-tip committer time for the stale badge (`None` for tags).
    fn last_touched(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        match self {
            Self::Recent { last_touched, .. }
            | Self::Local { last_touched, .. }
            | Self::Remote { last_touched, .. } => *last_touched,
            Self::Tag { .. } => None,
        }
    }

    /// Live-filter match against the display label.
    pub fn matches(&self, filter_lower: &str) -> bool {
        filter_lower.is_empty() || self.label().to_lowercase().contains(filter_lower)
    }

    fn starred(&self) -> bool {
        matches!(
            self,
            Self::Recent { favorite: true, .. } | Self::Local { favorite: true, .. }
        )
    }

    /// Section tag driving group-title emission (`RECENT`, …).
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
/// first, alphabetical, current excluded — it is pinned separately), REMOTE,
/// TAGS — every row filtered against `filter`.
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

    // RECENT — starred rows first (§8.5), otherwise most-recent-first order.
    // Sync markers come from the still-existing local branch when there is
    // one (a recent may reference a branch deleted since).
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

    // LOCAL — favorites first, alphabetical; the current branch is pinned
    // above the groups and never repeats here.
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

    // REMOTE — the raw remote-tracking refs plus every upstream a local
    // branch tracks. The synthesized rows are what render "gone" after the
    // remote branch is deleted and its ref pruned: the local branch still
    // carries the tracking config, so the popup shows `origin/x  gone`.
    let mut seen: Vec<String> = remotes.iter().map(|b| b.name.clone()).collect();
    let mut rem_rows: Vec<PopupEntry> = remotes
        .iter()
        .map(|b| {
            let local = locals
                .iter()
                .find(|l| l.tracking.as_deref() == Some(&format!("origin/{}", b.name)));
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
        // A tracking ref is `remote/name`; a bare local branch name is not a
        // remote row.
        let Some((_remote, name)) = track.split_once('/') else {
            continue;
        };
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

    // TAGS — alphabetical.
    let mut t: Vec<String> = tags.to_vec();
    t.sort();
    out.extend(
        t.into_iter()
            .map(|name| PopupEntry::Tag { name })
            .filter(|e| e.matches(&f)),
    );

    out
}

/// Record a checked-out branch as most-recent (deduped, capped at 5).
pub fn push_recent(recents: &mut Vec<String>, name: &str) {
    recents.retain(|n| n != name);
    recents.insert(0, name.to_string());
    recents.truncate(5);
}

/// Bottom info strip for multi-root projects (§8.5).
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

/// Sync marker for a remote row (issue 32): `gone` when the tracked
/// upstream was deleted, `↑a ↓b` when the local counterpart diverged, and
/// nothing when in sync.
pub fn remote_marker(ahead: usize, behind: usize, gone: bool) -> Option<String> {
    if gone {
        Some("gone".into())
    } else if ahead > 0 || behind > 0 {
        Some(format!("↑{ahead} ↓{behind}"))
    } else {
        None
    }
}

/// Which row actions are offered and enabled for one popup entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowActions {
    /// Delete is offered and enabled (gated on the current branch, protected
    /// branches, and tag rows).
    pub delete: bool,
    /// Rename is offered and enabled (local branches only).
    pub rename: bool,
    /// The row is a protected branch: renders the lock and the hover
    /// quick-actions (pull / merge).
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
/// actions stay rendered (dimmed) with this explanation on hover — the
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

/// Why Rename is gated on this row, or `None` when it is enabled — same
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

// ------------------------------------------------------------------ widget --

/// Compact branch indicator in the status bar; opens the popup on click.
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

// ------------------------------------------------------------------ popup --

/// The Branches popup (spec §8.5).
pub fn branches_popup(ui: &mut Ui, state: &mut AppState) {
    if !state.ui.branches_popup {
        return;
    }

    // Esc closes (keyboard contract, §8.5).
    if ui.input(|i| i.key_pressed(Key::Escape)) {
        state.ui.branches_popup = false;
        return;
    }

    let ctx = ui.ctx().clone();
    let mut open = true;

    // Snapshots so the window body never borrows `state` while mutating it.
    let root_id = state.selected_root.clone();
    let root = root_id
        .as_ref()
        .and_then(|id| state.multi.by_id(id))
        .cloned();

    egui::Window::new("Branches")
        .open(&mut open)
        .default_width(420.0)
        .show(&ctx, |ui| {
            // Header: search input (flex fill) + close X (§8.5.1).
            ui.horizontal(|ui| {
                let search =
                    widgets::search_input(ui, "Search branches", &mut state.ui.branch_filter);
                search.request_focus();
                if widgets::icon_button(ui, Icon::X).clicked() {
                    state.ui.branches_popup = false;
                }
            });
            ui.separator();

            // Wired top action (ADR-0012): opens the New Branch flow preset to
            // create + check out, closing this popup so focus follows cleanly.
            if widgets::compact_button(ui, "New Branch…").clicked() {
                state.ui.dlg.new_branch_name.clear();
                state.ui.dlg.new_branch_start.clear();
                state.ui.dlg.new_branch_base.clear();
                state.ui.dlg.new_branch_base_picker_open = false;
                state.ui.dlg.new_branch_checkout = true;
                state.ui.dialog = Some(Dialog::NewBranch);
                state.ui.branches_popup = false;
            }

            let Some(root) = root else {
                ui.label("No repository selected");
                return;
            };
            let id = root.id.clone();
            let current = root.current_branch.clone();

            // Current branch pinned top, check-marked and emphasized (§8.5.2).
            if let Some(cur) = &current {
                ui.add_space(2.0);
                ui.label(
                    RichText::new(format!("✓ {cur}"))
                        .strong()
                        .color(Palette::BRAND),
                );
                ui.add_space(2.0);
            }

            let tags = state.executor.tag_list(&id.0).unwrap_or_default();
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

            // Keyboard: ↑/↓ move the highlight, Enter checks the row out.
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
            // Floor lowered from 240 so short viewports can still fit the
            // popup without clipping the checkout actions irrecoverably.
            let body_h = (view_h - 48.0).clamp(160.0, 620.0);
            ui.allocate_ui_with_layout(
                vec2(ui.available_width(), body_h),
                Layout::top_down(Align::Min),
                |ui| {
                    ScrollArea::vertical().max_height(body_h).show(ui, |ui| {
                        let mut clicked: Option<PopupEntry> = None;
                        let mut toggled_star: Option<String> = None;
                        let mut row_intent: Option<RowIntent> = None;
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
                            // Issue 32 row anatomy: the lock marks protected
                            // branches (settings patterns), the stale badge
                            // shows the tip's age, and the sync markers ride
                            // the remote rows.
                            let protected =
                                sync_service::is_protected(&state.settings, e.branch_name());
                            // Remote and tag rows are refs that exist by
                            // definition; a recent/local row exists when its
                            // branch is still listed locally.
                            let exists = match e {
                                PopupEntry::Remote { .. } | PopupEntry::Tag { .. } => true,
                                _ => locals.iter().any(|b| b.name == e.branch_name()),
                            };
                            let actions = row_actions(e, current.as_deref(), protected, exists);
                            // Hover band for the protected row's quick actions
                            // (issue 32): the row's full rect from the previous
                            // frame. Testing the pointer against the rect laid
                            // out so far would collapse the band the moment the
                            // pointer travels onto the revealed buttons, so the
                            // last frame's rect is kept in egui memory — the
                            // classic two-frame hover toolbar.
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
                                // Stale badge (issue 32): the branch tip's age
                                // in compact units, e.g. "3w".
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
                                // Remote rows carry the upstream sync state:
                                // `gone` when the tracked upstream was deleted,
                                // else `↑a ↓b` when diverged.
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

                                // Wired row action (ADR-0012).
                                if widgets::compact_button(ui, "Checkout").clicked() {
                                    clicked = Some(e.clone());
                                }
                                // Issue 32: rename / delete / compare dispatch real flows. Gated
                                // actions stay rendered but disabled with the
                                // reason on hover, so the mockup's action set
                                // is intact and the gate is discoverable.
                                let rename =
                                    widgets::compact_button_enabled(ui, "Rename", actions.rename);
                                if let Some(reason) =
                                    rename_gate(e, current.as_deref(), protected, exists)
                                {
                                    rename.on_disabled_hover_text(reason);
                                } else if rename.clicked() {
                                    row_intent = Some(RowIntent::Rename(e.clone()));
                                }
                                let delete =
                                    widgets::compact_button_enabled(ui, "Delete", actions.delete);
                                if let Some(reason) =
                                    delete_gate(e, current.as_deref(), protected, exists)
                                {
                                    delete.on_disabled_hover_text(reason);
                                } else if delete.clicked() {
                                    row_intent = Some(RowIntent::Delete(e.clone()));
                                }
                                if widgets::compact_button(ui, "Compare…").clicked() {
                                    row_intent = Some(RowIntent::Compare(e.clone()));
                                }
                                // New Worktree… stays visibly inert until the
                                // worktree-from-popup flow exists (ADR-0012).
                                ui.scope(|ui| {
                                    ui.disable();
                                    let _ = widgets::compact_button(ui, "New Worktree…");
                                });
                                // Protected branches: pull / merge quick actions appear on hover
                                // (issue 32, screen 13).
                                if protected && hinted {
                                    if widgets::compact_button(ui, "Pull").clicked() {
                                        row_intent =
                                            Some(RowIntent::Pull(e.branch_name().to_string()));
                                    }
                                    if widgets::compact_button(ui, "Merge").clicked() {
                                        row_intent =
                                            Some(RowIntent::Merge(e.branch_name().to_string()));
                                    }
                                }
                            });
                            // Remember this frame's full row rect as the next
                            // frame's hover band (includes the revealed
                            // quick actions once they appear).
                            let band = row.response.rect;
                            ui.ctx().memory_mut(|m| m.data.insert_temp(band_id, band));
                        }

                        if let Some(name) = toggled_star {
                            branch_service::toggle_favorite(&mut state.multi, &id, &name);
                        }
                        if enter && !entries.is_empty() {
                            clicked = Some(entries[state.ui.branches_cursor].clone());
                        }
                        match (clicked, row_intent) {
                            (Some(e), _) => checkout_entry(state, &id, &e),
                            (None, Some(intent)) => apply_row_intent(state, &id, intent),
                            _ => {}
                        }
                    });
                },
            );

            // Multi-root sync notice (§8.5 bottom info strip).
            let roots = state.multi.roots.len();
            if roots > 1 && state.settings.synchronous_branches {
                ui.separator();
                ui.horizontal(|ui| {
                    icons::icon(ui, Icon::LAYERS, 14.0, Palette::STATE_INFO);
                    ui.label(RichText::new(sync_notice(roots)).color(Palette::STATE_INFO));
                });
            }

            // Footer (issue 32, screen 13): "+ New branch" opens the create
            // flow, "Manage remotes…" stays visibly inert until ticket 33's
            // surface exists (ADR-0012), and "Compare…" compares the
            // highlighted row against the current branch.
            ui.separator();
            ui.horizontal(|ui| {
                if widgets::compact_button(ui, "+ New branch").clicked() {
                    state.ui.dlg.new_branch_name.clear();
                    state.ui.dlg.new_branch_start.clear();
                    state.ui.dlg.new_branch_base.clear();
                    state.ui.dlg.new_branch_base_picker_open = false;
                    state.ui.dlg.new_branch_checkout = true;
                    state.ui.dialog = Some(Dialog::NewBranch);
                    state.ui.branches_popup = false;
                }
                ui.scope(|ui| {
                    if widgets::compact_button(ui, "Manage remotes…").clicked() {
                        // Seed the apply scope with the focused root and
                        // reset the manager's transient editing state.
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
                    apply_row_intent(state, &id, RowIntent::Compare(e.clone()));
                }
            });
        });

    // Propagate an X-button close unless an internal action already closed us.
    if state.ui.branches_popup {
        state.ui.branches_popup = open;
    }
}

/// Dispatch one row's checkout through the engine seam and close the popup.
fn checkout_entry(state: &mut AppState, id: &RootId, e: &PopupEntry) {
    let path = id.0.clone();
    let affected = Affected::Root(id.clone());
    match e {
        PopupEntry::Recent { name, .. } | PopupEntry::Local { name, .. } => {
            push_recent(&mut state.ui.recent_branches, name);
            let nm = name.clone();
            state.run_git(format!("Checkout {nm}"), affected.clone(), move |v| {
                v.branch_checkout(&path, &nm)
            });
        }
        PopupEntry::Remote { name, .. } => {
            push_recent(&mut state.ui.recent_branches, name);
            let nm = name.clone();
            let start = format!("origin/{name}");
            state.run_git(
                format!("Checkout {nm} (new local)"),
                affected.clone(),
                move |v| v.branch_create(&path, &nm, true, Some(&start)),
            );
        }
        PopupEntry::Tag { name } => {
            let nm = name.clone();
            state.run_git(format!("Checkout {nm}"), affected, move |v| {
                v.tag_checkout(&path, &nm)
            });
        }
    }
    state.ui.branches_popup = false;
}

/// A non-checkout row action, deferred to after the row loop (issue 32).
#[derive(Clone, Debug, PartialEq, Eq)]
enum RowIntent {
    Rename(PopupEntry),
    Delete(PopupEntry),
    Compare(PopupEntry),
    /// Protected-row hover quick-action: check the branch out and pull it.
    Pull(String),
    /// Protected-row hover quick-action: merge the branch into the current
    /// one via the existing merge dialog.
    Merge(String),
}

/// Dispatch one deferred row action (issue 32). Delete routes through the
/// existing rich confirmation (issue 02); rename and compare open their
/// dialogs via the AppState seams.
fn apply_row_intent(state: &mut AppState, id: &RootId, intent: RowIntent) {
    match intent {
        RowIntent::Rename(e) => state.open_rename_branch(id, e.branch_name()),
        RowIntent::Delete(e) => match e {
            PopupEntry::Remote { name, .. } => {
                state.ui.confirm = Some(PendingConfirm::DeleteRemoteBranch {
                    remote: "origin".into(),
                    name: name.clone(),
                });
            }
            PopupEntry::Recent { name, .. } | PopupEntry::Local { name, .. } => {
                state.ui.confirm = Some(PendingConfirm::DeleteLocalBranch { name: name.clone() });
            }
            PopupEntry::Tag { .. } => {}
        },
        RowIntent::Compare(e) => state.open_compare(id, e.branch_name()),
        RowIntent::Pull(name) => {
            let path = id.0.clone();
            let affected = Affected::Root(id.clone());
            let rebase =
                state.settings.update_method == turbogit_domain::model::UpdateMethod::Rebase;
            state.run_git(format!("Pull {name}"), affected, move |v| {
                v.branch_checkout(&path, &name)?;
                v.pull(&path, rebase)
            });
            state.ui.branches_popup = false;
        }
        RowIntent::Merge(name) => state.open_merge_into(id, &name),
    }
}
