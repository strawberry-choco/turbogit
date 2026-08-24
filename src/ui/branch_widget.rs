//! Branch widget (status bar) + redesigned Branches popup (spec §8.5).
//!
//! The popup is a searchable floating list grouped RECENT / LOCAL / REMOTE /
//! TAGS with the current branch pinned top and check-marked. Per-row actions
//! render for every branch row; only New Branch… and Checkout are wired —
//! Rename / Delete / Compare… / New Worktree… stay visibly inert until their
//! flows exist (ADR-0012). Keyboard: typing filters live, ↑/↓ move the
//! highlight, Enter checks the highlighted row out, Esc closes.
//!
//! All git mutations cross the [`crate::engine::GitExecutor`] seam via
//! [`AppState::run_git`]; the pure row-model helpers are unit-testable.

use crate::core::branch_service;
use crate::model::{Branch, BranchKind, RootId};
use crate::root_caches::Affected;
use crate::state::{AppState, Dialog};
use crate::theme::Palette;
use crate::ui::icons::{self, Icon};
use crate::ui::widgets;
use egui::{vec2, Align, Color32, Key, Layout, RichText, ScrollArea, Ui};

/// Favorite-star ink: the central warning token (spec §8.5 row anatomy).
pub const STAR_COLOR: Color32 = Palette::STATE_WARNING;

/// One selectable row of the branches popup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PopupEntry {
    Recent { name: String, favorite: bool },
    Local { name: String, favorite: bool },
    Remote { name: String },
    Tag { name: String },
}

impl PopupEntry {
    /// Painted row label (remote rows carry the `origin/` prefix, §8.5).
    pub fn label(&self) -> String {
        match self {
            Self::Recent { name, .. } | Self::Local { name, .. } | Self::Tag { name } => {
                name.clone()
            }
            Self::Remote { name } => format!("origin/{name}"),
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
    let mut recent: Vec<PopupEntry> = recents
        .iter()
        .filter(|n| Some(n.as_str()) != current)
        .map(|n| PopupEntry::Recent {
            name: n.clone(),
            favorite: fav_of(n),
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
            })
            .filter(|e| e.matches(&f)),
    );

    // REMOTE — muted rows, alphabetical.
    let mut rems: Vec<&Branch> = remotes.iter().collect();
    rems.sort_by(|a, b| a.name.cmp(&b.name));
    out.extend(
        rems.into_iter()
            .map(|b| PopupEntry::Remote {
                name: b.name.clone(),
            })
            .filter(|e| e.matches(&f)),
    );

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
                            ui.horizontal(|ui| {
                                match e {
                                    PopupEntry::Recent { name, favorite }
                                    | PopupEntry::Local { name, favorite } => {
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
                                if ui.selectable_label(selected, label.clone()).clicked() {
                                    clicked = Some(e.clone());
                                }
                                // Wired row action (ADR-0012).
                                if widgets::compact_button(ui, "Checkout").clicked() {
                                    clicked = Some(e.clone());
                                }
                                // Visibly inert row actions (ADR-0012): rendered,
                                // disabled, no-op. Each runs in its own scope so the
                                // disabled state cannot leak into later rows.
                                for label in ["Rename", "Delete", "Compare…", "New Worktree…"] {
                                    ui.scope(|ui| {
                                        ui.disable();
                                        let _ = widgets::compact_button(ui, label);
                                    });
                                }
                            });
                        }

                        if let Some(name) = toggled_star {
                            branch_service::toggle_favorite(&mut state.multi, &id, &name);
                        }
                        if enter && !entries.is_empty() {
                            clicked = Some(entries[state.ui.branches_cursor].clone());
                        }
                        if let Some(e) = clicked {
                            checkout_entry(state, &id, &e);
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
        PopupEntry::Remote { name } => {
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
