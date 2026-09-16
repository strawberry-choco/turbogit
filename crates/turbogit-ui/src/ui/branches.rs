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

use egui::{
    Align, Color32, CornerRadius, Layout, Pos2, Rect, RichText, ScrollArea, Sense, Ui, UiBuilder,
    Vec2, WidgetInfo, WidgetType,
};

use turbogit_app::root_caches::Affected;
use turbogit_app::state::{AppState, Dialog, PendingConfirm};
use turbogit_domain::model::{Branch, BranchKind, Root, RootId};

use crate::theme::{
    Palette, TYPE_BODY, TYPE_CHIP, TYPE_CONTROL, TYPE_DETAIL_TITLE, chrome_font, data_font,
};
use crate::ui::branch_widget::stale_badge;
use crate::ui::components::{
    BRANCH_ROW_H, DETAIL_W, KIT_ICON, KitButton, PAD_LIST, PAD_STRIP, RowState, SyncKind,
    TOOLBAR_H, kit_button, middle_truncate, overflow_button, row_fill, row_ink, section_header,
    sync_badge, sync_bg, sync_ink,
};
use crate::ui::icons::{self, Icon};
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
    /// The row's sync pair (icon+count, quiet "in sync", or "gone") — only
    /// when the branch tracks an upstream.
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

/// Per-direction chips for a tracked row (issue 04): icon+count pairs, a quiet
/// "in sync" confirmation when there is nothing to say, or the gone marker —
/// never the bare arrow, never silence.
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
    if v.is_empty() {
        v.push((SyncKind::InSync, "in sync".to_string()));
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
    // Ensure tags are cached for every in-scope root (issue 14 aggregation).
    for r in &state.multi.roots {
        if state.ui.branches_tags_root.as_ref() != Some(&r.id) {
            state.ui.branches_tags = state.executor.tag_list(&r.id.0).unwrap_or_default();
            state.ui.branches_tags_root = Some(r.id.clone());
        }
    }
    let rows = aggregate_rows(state);
    // The keyboard path (design §9/§15, issue 15): read the raw events at the
    // very top of the frame, before the toolbar's search input renders and
    // could consume them, so focus → filter → arrows → Enter works with the
    // cursor still in the filter box.
    handle_keys(ui, state, &rows);

    let body = ui.available_rect_before_wrap();
    let toolbar_rect = Rect::from_min_max(body.min, Pos2::new(body.max.x, body.min.y + TOOLBAR_H));
    let mut toolbar_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(toolbar_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    toolbar(&mut toolbar_ui, state);
    ui.advance_cursor_after_rect(toolbar_rect);

    let content_rect = Rect::from_min_max(Pos2::new(body.min.x, toolbar_rect.max.y), body.max);
    let detail_rect = Rect::from_min_max(
        Pos2::new(content_rect.max.x - DETAIL_W, content_rect.min.y),
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
    list_area(&mut list_ui, state, &rows);
    ui.advance_cursor_after_rect(list_rect);
}

/// One aggregated row (issue 14): the owning repo id + display name, the
/// branch itself, and its own repo's current branch (for the marker). Two
/// `master`s in different repos are never identical anonymous rows.
struct Row {
    root: RootId,
    repo: String,
    branch: Branch,
    current: Option<String>,
}

/// Aggregate the in-scope roots' branches into rows. With a repo filter set
/// (issue 14) only that repo contributes; otherwise every repo contributes
/// and rows carry their owning repo's name.
fn aggregate_rows(state: &AppState) -> Vec<Row> {
    let multi = state.multi.roots.len() > 1;
    let mut out = Vec::new();
    for r in &state.multi.roots {
        if let Some(f) = &state.ui.branches_repo_filter
            && &r.id != f
        {
            continue;
        }
        let repo = if multi { r.id.name() } else { String::new() };
        let current = r.current_branch.clone();
        for b in &r.branches {
            out.push(Row {
                root: r.id.clone(),
                repo: repo.clone(),
                branch: b.clone(),
                current: current.clone(),
            });
        }
    }
    out
}

/// Local-branch ordering for the keyboard path: list order (current first,
/// then newest tip) under the active filter — the same rows the list shows.
fn keyboard_rows<'a>(state: &AppState, rows: &'a [Row]) -> Vec<&'a Row> {
    let visible: Vec<&Row> = rows
        .iter()
        .filter(|r| {
            r.branch.kind == BranchKind::Local
                && branch_matches(&r.branch, &state.ui.branches_filter)
        })
        .collect();
    ordered_rows(&visible)
}

/// Full keyboard path (issue 15, design §9/§15): arrows move the selection
/// through the visible branches, Enter checks the selected row out, Delete
/// asks to delete it (never the current branch), and Escape clears the
/// filter then closes the detail area. Nothing on this screen needs the
/// mouse to discover. Events are read before any widget consumes them — an
/// focused search box never eats the path.
fn handle_keys(ui: &mut Ui, state: &mut AppState, rows: &[Row]) {
    // Never hijack keys while a modal surface owns the keyboard.
    if state.ui.dialog.is_some()
        || state.ui.confirm.is_some()
        || state.ui.branches_renaming.is_some()
        || state.ui.branches_overflow.is_some()
        || state.ui.branches_scope_picker_open
    {
        return;
    }
    let ordered = keyboard_rows(state, rows);
    let sel = state.ui.branches_selected_root.as_ref().and_then(|rid| {
        ordered.iter().position(|r| {
            &r.root == rid && state.ui.branches_selected.as_deref() == Some(&r.branch.name)
        })
    });
    // Enter/Delete act on the selected row whatever its kind — the local
    // navigation set is only what the arrows move through (remote rows are
    // reference material, issue 13).
    let selected_row = state.ui.branches_selected_root.as_ref().and_then(|rid| {
        let name = state.ui.branches_selected.as_deref()?;
        rows.iter()
            .find(|r| &r.root == rid && r.branch.name == name)
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
        let row = ordered[idx];
        state.ui.branches_selected = Some(row.branch.name.clone());
        state.ui.branches_selected_root = Some(row.root.clone());
    } else if up && !ordered.is_empty() {
        let idx = sel.map_or(0, |i| i.saturating_sub(1));
        let row = ordered[idx];
        state.ui.branches_selected = Some(row.branch.name.clone());
        state.ui.branches_selected_root = Some(row.root.clone());
    } else if enter {
        if let Some(row) = selected_row {
            checkout_branch(state, &row.root, &row.branch);
        }
    } else if delete && let Some(row) = selected_row {
        // The current branch can never be deleted (issue 12); everything
        // else gets the same "what will be lost" ask as the click path.
        if row.current.as_deref() != Some(row.branch.name.as_str()) {
            apply_action(state, &row.root, &row.branch, BranchAction::Delete);
        }
    }
}

/// Toolbar (44px, §12): search input + the primary New Branch action, plus —
/// in multi-repo projects — a repo filter that narrows the aggregated list
/// and a visible "filtered" indicator (issue 14).
fn toolbar(ui: &mut Ui, state: &mut AppState) {
    ui.add_space(PAD_STRIP);
    // Right-aligned actions first (New Branch, then the repo scope control,
    // then the search input taking the rest of the strip). The scope control
    // is neighbours with the actions so its "all N repos" / "filtered to X"
    // header always paints — never starved for width by the search box.
    let _ = ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.add_space(PAD_STRIP);
        if kit_button(ui, KitButton::Primary, "New Branch").clicked() {
            state.ui.dlg.new_branch_name.clear();
            state.ui.dlg.new_branch_start.clear();
            state.ui.dlg.new_branch_base.clear();
            state.ui.dlg.new_branch_base_picker_open = false;
            state.ui.dlg.new_branch_checkout = true;
            state.ui.dialog = Some(Dialog::NewBranch);
        }
        // Repo scope control (issue 14): only shown when several repos are in
        // scope. The header names the scope so nobody acts on a partial list
        // thinking it is everything.
        if state.multi.roots.len() > 1 {
            ui.add_space(PAD_STRIP);
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
                                .selectable_label(
                                    state.ui.branches_repo_filter.is_none(),
                                    "All repos",
                                )
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
    });
    let search = widgets::search_input(ui, "Search branches", &mut state.ui.branches_filter);
    if state.ui.branches_focus_search {
        search.request_focus();
        state.ui.branches_focus_search = false;
    }
    ui.add_space(PAD_STRIP);
}

/// The grouped list (issue 03): Local expanded, Remote expanded, Tags
/// collapsed — filtered live by the search (issue 06) — or the reading/empty
/// states instead of a blank panel. Rows come from every in-scope root
/// (issue 14) unless the repo filter narrows them.
fn list_area(ui: &mut Ui, state: &mut AppState, rows: &[Row]) {
    let locals_all: Vec<&Row> = rows
        .iter()
        .filter(|r| r.branch.kind == BranchKind::Local)
        .collect();
    let remotes_all: Vec<&Row> = rows
        .iter()
        .filter(|r| r.branch.kind == BranchKind::Remote)
        .collect();
    let tags_all: Vec<String> = state.ui.branches_tags.clone();

    // No rows and no HEAD anywhere: reading or the one-sentence empty state.
    if rows.is_empty() && !state.multi.roots.iter().any(|r| r.head.is_some()) {
        if state.ui.busy {
            reading_state(ui);
        } else {
            empty_state(ui, state);
        }
        return;
    }

    // Live filter from the toolbar: the match set narrows the rows, while
    // group headers stay visible with match-set counts.
    let filter = state.ui.branches_filter.clone();
    let filtering = !filter.trim().is_empty();
    let locals: Vec<&Row> = locals_all
        .into_iter()
        .filter(|r| branch_matches(&r.branch, &filter))
        .collect();
    let remotes: Vec<&Row> = remotes_all
        .into_iter()
        .filter(|r| branch_matches(&r.branch, &filter))
        .collect();
    let tags: Vec<String> = tags_all
        .iter()
        .filter(|t| matches_query(t, &filter))
        .cloned()
        .collect();
    let total = locals.len() + remotes.len() + tags.len();

    // Search is for jumping, not browsing: save the scroll position the
    // moment filtering begins and restore it when it clears.
    if filtering && state.ui.branches_scroll_saved.is_none() {
        state.ui.branches_scroll_saved = Some(state.ui.branches_scroll);
    }
    if !filtering && state.ui.branches_scroll_saved.is_some() {
        state.ui.branches_scroll = state.ui.branches_scroll_saved.take().unwrap_or(0.0);
    }
    let scroll = state.ui.branches_scroll;

    // Esc clears the filter first, then closes the detail area (§9).
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        if !filter.trim().is_empty() {
            state.ui.branches_filter.clear();
        } else {
            state.ui.branches_selected = None;
            state.ui.branches_selected_root = None;
            state.ui.branches_overflow = None;
        }
    }

    let scroll_target = state.ui.branches_scroll_to.clone();
    let mut target_y: Option<f32> = None;
    let mut y_cursor = 0.0;
    // Issue 15: an operation in flight shows in place — a muted "working…"
    // line above the list, never a silent wait. The busy flag is shell-global,
    // so the mid-operation state survives switching tabs.
    if state.ui.busy && !rows.is_empty() {
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
    let out = ScrollArea::vertical()
        .id_salt("branches_list")
        .auto_shrink([false, false])
        .vertical_scroll_offset(scroll)
        .show(ui, |ui| {
            if total == 0 && filtering {
                // A dead end turns into the likely next intent.
                no_match_state(ui, state, filter.trim());
                return;
            }
            let ordered = ordered_rows(&locals);
            let local_header = section_header(
                ui,
                "Local",
                ordered.len(),
                state.ui.branches_groups.local,
                |_| {},
            );
            if local_header.clicked() {
                state.ui.branches_groups.local = !state.ui.branches_groups.local;
            }
            y_cursor += crate::ui::components::SECTION_H;
            if state.ui.branches_groups.local {
                for r in &ordered {
                    if Some(r.branch.name.as_str()) == scroll_target.as_deref() {
                        target_y = Some(y_cursor);
                    }
                    y_cursor += BRANCH_ROW_H;
                    branch_row(ui, state, r);
                }
            }

            let remote_header = section_header(
                ui,
                "Remote",
                remotes.len(),
                state.ui.branches_groups.remote,
                |ui| {
                    // Fetch lives with the remote group (issue 13); after it
                    // the report says what changed. With several repos in
                    // scope every repo is fetched and reports per repo
                    // (issue 14) — never one flat "done".
                    ui.add_space(4.0);
                    if kit_button(ui, KitButton::Quiet, "Fetch").clicked() {
                        let targets: Vec<RootId> = match &state.ui.branches_repo_filter {
                            Some(id) => vec![id.clone()],
                            None => state.multi.roots.iter().map(|r| r.id.clone()).collect(),
                        };
                        for rid in targets {
                            let before: Vec<String> = rows
                                .iter()
                                .filter(|r| r.root == rid && r.branch.kind == BranchKind::Remote)
                                .map(|r| r.branch.name.clone())
                                .collect();
                            state.ui.branches_fetch_before.push((rid.clone(), before));
                            let exec_rid = rid.clone();
                            state.run_git("Fetch".to_string(), Affected::Root(rid), move |v| {
                                v.fetch(&exec_rid.0, None)
                            });
                        }
                    }
                },
            );
            if remote_header.clicked() {
                state.ui.branches_groups.remote = !state.ui.branches_groups.remote;
            }
            y_cursor += crate::ui::components::SECTION_H;
            if state.ui.branches_groups.remote {
                let mut sorted = remotes.clone();
                sorted.sort_by(|a, b| {
                    a.branch
                        .remote
                        .cmp(&b.branch.remote)
                        .then_with(|| a.branch.name.cmp(&b.branch.name))
                        .then_with(|| a.repo.cmp(&b.repo))
                });
                for r in &sorted {
                    if Some(r.branch.name.as_str()) == scroll_target.as_deref() {
                        target_y = Some(y_cursor);
                    }
                    y_cursor += BRANCH_ROW_H;
                    branch_row(ui, state, r);
                }
                // The app is honest about when the last fetch happened
                // (issue 13) — silent success and a no-op must not be
                // indistinguishable.
                if let Some(last) = state.ui.branches_last_fetch {
                    let age = chrono::Utc::now()
                        .signed_duration_since(last)
                        .to_std()
                        .unwrap_or_default();
                    ui.horizontal(|ui| {
                        ui.add_space(PAD_LIST + KIT_ICON + 8.0);
                        ui.label(
                            RichText::new(format!("last fetched {}", stale_badge(age)))
                                .font(data_font(TYPE_CONTROL))
                                .color(Palette::T_MUTED),
                        );
                    });
                    y_cursor += BRANCH_ROW_H;
                }
            }

            let tags_header = section_header(
                ui,
                "Tags",
                tags.len(),
                state.ui.branches_groups.tags,
                |_| {},
            );
            if tags_header.clicked() {
                state.ui.branches_groups.tags = !state.ui.branches_groups.tags;
            }
            y_cursor += crate::ui::components::SECTION_H;
            if state.ui.branches_groups.tags {
                let mut sorted = tags.clone();
                sorted.sort();
                for t in &sorted {
                    if Some(t.as_str()) == scroll_target.as_deref() {
                        target_y = Some(y_cursor);
                    }
                    y_cursor += BRANCH_ROW_H;
                    tag_row(ui, t);
                }
            }
        });
    state.ui.branches_scroll = out.state.offset.y;
    // Scroll the freshly created branch into view (issue 08) and consume the
    // intent.
    if state.ui.branches_scroll_to.take().is_some()
        && let Some(ty) = target_y
    {
        state.ui.branches_scroll = ty;
    }

    // Enter-on-selected-row checkout moved to `handle_keys` (issue 15) so the
    // key path also works while the filter box owns the keyboard.

    // The ⋯ overflow menu floats over the list with the same actions as the
    // detail panel (issue 05).
    if let Some((root_id, name)) = state.ui.branches_overflow.clone() {
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
                        if let Some(row) = rows
                            .iter()
                            .find(|r| r.root == root_id && r.branch.name == name)
                            && let Some(root) = state.multi.by_id(&row.root).cloned()
                            && let Some(a) = detail_actions(ui, state, &root, &row.branch)
                        {
                            apply_action(state, &row.root, &row.branch, a);
                        }
                    });
                });
        }
    }
}

/// Local-row ordering across the aggregate (issue 14): each repo's current
/// branch first, then most recent tip, ties alphabetical.
fn ordered_rows<'a>(rows: &[&'a Row]) -> Vec<&'a Row> {
    let mut v = rows.to_vec();
    v.sort_by(|a, b| {
        let ac = Some(a.branch.name.as_str()) == a.current.as_deref();
        let bc = Some(b.branch.name.as_str()) == b.current.as_deref();
        bc.cmp(&ac)
            .then_with(|| b.branch.last_touched.cmp(&a.branch.last_touched))
            .then_with(|| a.repo.cmp(&b.repo))
            .then_with(|| a.branch.name.cmp(&b.branch.name))
    });
    v
}

/// Muted "reading branches…" line in place of the list — never a blank panel.
fn reading_state(ui: &mut Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(40.0);
        ui.label(
            RichText::new("reading branches…")
                .font(chrome_font(TYPE_CONTROL))
                .color(Palette::T_MUTED),
        );
    });
}

/// No search results: the list itself says so and turns the dead end into the
/// likely next intent — creating the branch (design doc §3/§15).
fn no_match_state(ui: &mut Ui, state: &mut AppState, query: &str) {
    ui.add_space(24.0);
    ui.horizontal(|ui| {
        ui.add_space(PAD_LIST);
        ui.label(
            RichText::new(format!("no branch called {query}. Create it?"))
                .font(chrome_font(TYPE_CONTROL))
                .color(Palette::T_MUTED),
        );
    });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(PAD_LIST);
        if kit_button(ui, KitButton::Secondary, &format!("Create \"{query}\"")).clicked() {
            state.ui.dlg.new_branch_name = query.to_string();
            state.ui.dlg.new_branch_start.clear();
            state.ui.dlg.new_branch_base.clear();
            state.ui.dlg.new_branch_base_picker_open = false;
            state.ui.dlg.new_branch_checkout = true;
            state.ui.dialog = Some(Dialog::NewBranch);
        }
    });
}

/// A repo with no branches at all: one sentence + one create action, and no
/// empty group headers (design doc §2/§15).
fn empty_state(ui: &mut Ui, state: &mut AppState) {
    ui.vertical_centered(|ui| {
        ui.add_space(56.0);
        ui.label(
            RichText::new("This repo has no branches yet")
                .font(data_font(TYPE_BODY))
                .color(Palette::T_PRIMARY),
        );
        ui.add_space(8.0);
        if kit_button(ui, KitButton::Primary, "Create the first branch").clicked() {
            state.ui.dlg.new_branch_name.clear();
            state.ui.dlg.new_branch_start.clear();
            state.ui.dlg.new_branch_base.clear();
            state.ui.dlg.new_branch_base_picker_open = false;
            state.ui.dlg.new_branch_checkout = true;
            state.ui.dialog = Some(Dialog::NewBranch);
        }
    });
}

/// One 30px branch row: current marker, owning-repo prefix (multi-repo),
/// middle-truncated mono name, upstream, sync chips (icon+count / in-sync /
/// gone), relative last-activity time. Hover and selection fills come from
/// the §14.1 row states; clicking selects (never checks out — issue 05
/// wires the detail).
fn branch_row(ui: &mut Ui, state: &mut AppState, row: &Row) {
    let branch = &row.branch;
    let id = &row.root;
    // Issue 11: the branch being renamed edits inline on its own row. Rename
    // is a local-branch verb, scoped to the owning repo.
    if branch.kind == BranchKind::Local
        && state.ui.branches_selected_root.as_ref() == Some(id)
        && state.ui.branches_renaming.as_deref() == Some(branch.name.as_str())
    {
        rename_editor(ui, state, id, branch);
        return;
    }

    let now = chrono::Utc::now();
    let meta = row_meta(branch, now);
    let is_current = row.current.as_deref() == Some(branch.name.as_str());
    let selected = state.ui.branches_selected_root.as_ref() == Some(id)
        && state.ui.branches_selected.as_deref() == Some(branch.name.as_str());
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, BRANCH_ROW_H), Sense::hover());
    let hovered = ui.rect_contains_pointer(rect);

    // Interact after contents so child widgets (later actions) win clicks.
    let response = ui.interact(
        rect,
        ui.auto_id_with(("branch_row", id, &branch.name)),
        Sense::click(),
    );

    let row_state = if selected {
        RowState::Selected
    } else if hovered {
        RowState::Hover
    } else {
        RowState::Default
    };
    let fill = row_fill(row_state);
    if fill != Color32::TRANSPARENT {
        let mut bg = ui.painter().clone();
        bg.set_layer_id(egui::LayerId::new(egui::Order::Background, response.id));
        bg.rect_filled(rect, CornerRadius::same(3), fill);
    }

    // Right cluster: on hover the row reveals its actions (Checkout + the ⋯
    // overflow) — design doc §4 "Actions live on the row"; otherwise the sync
    // chips and relative time. Rendered first so name+upstream take the rest.
    let chips = branch
        .tracking
        .as_ref()
        .map(|_| sync_chips(branch.ahead, branch.behind, branch.gone));
    let overflow_id = egui::Id::new(("branches_overflow_anchor", id, &branch.name));
    // The action names its repo's scope in multi-repo projects so a bare
    // "Checkout" never applies to an ambiguous repo (issue 14).
    let scope = if state.multi.roots.len() > 1 {
        format!(" in {}", row.repo)
    } else {
        String::new()
    };
    let mut action: Option<BranchAction> = None;
    if hovered {
        ui.new_child(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::right_to_left(Align::Center)),
        )
        .with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(8.0);
            let more = overflow_button(ui, "More actions");
            ui.ctx()
                .memory_mut(|m| m.data.insert_temp(overflow_id, more.rect));
            if more.clicked() {
                let key = (row.root.clone(), branch.name.clone());
                if state.ui.branches_overflow.as_ref() == Some(&key) {
                    state.ui.branches_overflow = None;
                } else {
                    state.ui.branches_overflow = Some(key);
                }
            }
            if kit_button(ui, KitButton::Quiet, &format!("Checkout{scope}")).clicked() {
                action = Some(BranchAction::Checkout);
            }
        });
    } else {
        ui.new_child(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::right_to_left(Align::Center)),
        )
        .with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(12.0);
            if let Some(ts) = branch.last_touched {
                let age = now.signed_duration_since(ts).to_std().unwrap_or_default();
                ui.label(
                    RichText::new(stale_badge(age))
                        .font(data_font(TYPE_CONTROL))
                        .color(Palette::T_MUTED),
                );
            }
            // Mid-operation state is first-class on the row (issue 09): a
            // merge in progress reads "merging…" and survives tab switches.
            if is_current && state.ui.merge_in_progress {
                ui.label(
                    RichText::new(crate::ui::components::mid_op_label("merging"))
                        .font(data_font(TYPE_CONTROL))
                        .color(Palette::STATE_WARNING),
                );
            }
            if let Some(chips) = &chips {
                for (kind, label) in chips.iter().rev() {
                    sync_chip(ui, *kind, label);
                    ui.add_space(4.0);
                }
            }
        });
    }

    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.add_space(PAD_LIST);
    // Remote rows are reference material (issue 13): quieter, labelled with
    // their remote's name, and never carrying the current marker.
    let is_remote = branch.kind == BranchKind::Remote;
    if is_current && !is_remote {
        icons::icon(&mut child, Icon::GIT_BRANCH, KIT_ICON, Palette::AHEAD);
        child.add_space(6.0);
    } else {
        child.add_space(KIT_ICON + 6.0);
    }
    let budget = name_budget(rect.width() - PAD_LIST - KIT_ICON - 6.0 - 130.0);
    let label = if is_remote {
        middle_truncate(
            &format!(
                "{}/{}",
                branch.remote.as_deref().unwrap_or("origin"),
                branch.name
            ),
            budget,
        )
    } else {
        middle_truncate(&branch.name, budget)
    };
    let ink = if is_remote {
        Palette::T_SECONDARY
    } else {
        row_ink(meta.stale)
    };
    // Issue 14: every row names its owning repo in multi-repo projects — two
    // `main`s are never identical anonymous rows.
    if !row.repo.is_empty() {
        child.add(
            egui::Label::new(
                RichText::new(format!("{} · ", row.repo))
                    .font(data_font(TYPE_CONTROL))
                    .color(Palette::T_MUTED),
            )
            .truncate(),
        );
    }
    child.add(
        egui::Label::new(RichText::new(label).font(data_font(TYPE_BODY)).color(ink)).truncate(),
    );
    if let Some(up) = &meta.upstream {
        child.add_space(6.0);
        child.add(
            egui::Label::new(
                RichText::new(up.clone())
                    .font(data_font(TYPE_CONTROL))
                    .color(Palette::T_MUTED),
            )
            .truncate(),
        );
    }

    if let Some(a) = action {
        apply_action(state, &row.root, branch, a);
    } else if response.clicked() {
        // Clicking selects; clicking the selected row again closes the detail
        // (it closes cleanly without disturbing the list — issue 05).
        if state.ui.branches_selected_root.as_ref() == Some(&row.root)
            && state.ui.branches_selected.as_deref() == Some(branch.name.as_str())
        {
            state.ui.branches_selected = None;
            state.ui.branches_selected_root = None;
        } else {
            state.ui.branches_selected = Some(branch.name.clone());
            state.ui.branches_selected_root = Some(row.root.clone());
        }
        state.ui.branches_overflow = None;
    }
    if response.double_clicked() {
        // Double-click checks out — selecting and switching stay separate
        // intents, and double-click is the explicit switch gesture.
        checkout_branch(state, &row.root, branch);
    }
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &branch.name));
}

/// Inline rename editor on the row (issue 11): a draft input plus the
/// upstream-follows disclosure, with confirm/cancel. Renaming the current
/// branch does not touch the working tree (git's `branch -m` moves the ref
/// and HEAD follows); the renamed branch re-sorts under the current ordering
/// after the refresh.
fn rename_editor(ui: &mut Ui, state: &mut AppState, id: &RootId, branch: &Branch) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 56.0), Sense::hover());
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::top_down(Align::Min)),
    );
    child.add_space(6.0);
    let enter = child.input(|i| i.key_pressed(egui::Key::Enter));
    child.horizontal(|ui| {
        ui.add_space(PAD_LIST);
        let input = widgets::text_input(ui, &branch.name, &mut state.ui.branches_rename_draft);
        input.request_focus();
        if kit_button(ui, KitButton::Secondary, "Apply rename").clicked() || enter {
            confirm_rename(state, id, branch);
        }
        if kit_button(ui, KitButton::Quiet, "Cancel").clicked() {
            state.ui.branches_renaming = None;
        }
    });
    // The one thing the person must know before confirming: the tracking
    // relationship does NOT follow the rename (issue 11, design §6.5).
    if let Some(up) = &branch.tracking {
        child.add_space(2.0);
        child.horizontal(|ui| {
            ui.add_space(PAD_LIST);
            ui.label(
                RichText::new(format!(
                    "tracking {up} does not follow the new name — set it again after"
                ))
                .font(chrome_font(TYPE_CONTROL))
                .color(Palette::T_MUTED),
            );
        });
    }
}

/// Apply an inline rename through the engine seam and close the editor.
fn confirm_rename(state: &mut AppState, id: &RootId, branch: &Branch) {
    let new = state.ui.branches_rename_draft.trim().to_string();
    let old = branch.name.clone();
    state.ui.branches_renaming = None;
    if new.is_empty() || new == old {
        return;
    }
    // The selection follows the new name so the detail panel stays coherent.
    if state.ui.branches_selected.as_deref() == Some(old.as_str()) {
        state.ui.branches_selected = Some(new.clone());
    }
    state.rename_branch(id, &old, &new);
}

/// Dispatch one branch action from the detail panel or the ⋯ overflow
/// (issue 05). The owning root rides every action so its scope is stated in
/// the wording (issue 14) — "Checkout in alpha".
fn apply_action(state: &mut AppState, root: &RootId, branch: &Branch, action: BranchAction) {
    state.ui.branches_overflow = None;
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
            state.ui.branches_renaming = Some(branch.name.clone());
            state.ui.branches_rename_draft = branch.name.clone();
            state.ui.branches_selected_root = Some(root.clone());
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

/// One tinted sync chip: 10px icon + count (or the quiet "in sync" / "gone"
/// label) on the §13 meaning-color tint. 18px tall, radius 3, mono data type.
fn sync_chip(ui: &mut Ui, kind: SyncKind, label: &str) {
    let fg = sync_ink(kind);
    let bg = sync_bg(kind);
    let font = data_font(TYPE_CHIP);
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, fg);
    let icon_s = 10.0;
    let pad = 5.0;
    let w = pad * 2.0 + icon_s + 2.0 + galley.size().x;
    let h = 18.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(Palette::RADIUS_CHIP), bg);

    let icon = match kind {
        SyncKind::Ahead => Some(Icon::ARROW_UP),
        SyncKind::Behind => Some(Icon::ARROW_DOWN),
        SyncKind::InSync => Some(Icon::CHECK),
        SyncKind::Gone => Some(Icon::ALERT_TRIANGLE),
        SyncKind::Diverged => None,
    };
    let mut x = rect.left() + pad;
    if let Some(ic) = icon {
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(Rect::from_min_size(
                    Pos2::new(x, rect.center().y - icon_s / 2.0),
                    Vec2::splat(icon_s),
                ))
                .layout(egui::Layout::centered_and_justified(
                    egui::Direction::LeftToRight,
                )),
        );
        icons::icon(&mut child, ic, icon_s, fg);
        x += icon_s + 2.0;
    }
    ui.painter().galley_with_override_text_color(
        Pos2::new(x, rect.center().y - galley.size().y / 2.0),
        galley,
        fg,
    );
}

/// One tag row: quiet mono name (tags are reference material, issue 13 makes
/// them visually quieter; today they read at secondary).
fn tag_row(ui: &mut Ui, name: &str) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, BRANCH_ROW_H), Sense::hover());
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.add_space(PAD_LIST + KIT_ICON + 6.0);
    child.add(
        egui::Label::new(
            RichText::new(name.to_string())
                .font(data_font(TYPE_BODY))
                .color(Palette::T_SECONDARY),
        )
        .truncate(),
    );
}

/// Detail panel (280px): paints its background + left divider, then the
/// selected branch's block (issue 05) or the quiet selection prompt. It never
/// blocks the list — it is a side panel on the same frame.
fn detail_panel(ui: &mut Ui, state: &mut AppState) {
    let rect = ui.available_rect_before_wrap();
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

    let Some(name) = state.ui.branches_selected.clone() else {
        // Quiet prompt about what selecting a branch will give you — never an
        // error, never empty space (§2).
        ui.add_space(24.0);
        crate::ui::components::detail_panel_header(ui, "Branches");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(PAD_LIST);
            ui.label(
                RichText::new("Select a branch")
                    .font(data_font(TYPE_DETAIL_TITLE))
                    .color(Palette::T_PRIMARY),
            );
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(PAD_LIST);
            ui.label(
                RichText::new("See how it relates to the current branch, its latest commit, and what you can do with it.")
                    .font(chrome_font(TYPE_CONTROL))
                    .color(Palette::T_MUTED),
            );
        });
        return;
    };

    // The selected row is looked up in its own repo (issue 14); a vanished
    // selection closes the panel cleanly.
    let Some(root) = state
        .ui
        .branches_selected_root
        .as_ref()
        .and_then(|id| state.multi.by_id(id))
        .cloned()
    else {
        state.ui.branches_selected = None;
        state.ui.branches_selected_root = None;
        return;
    };
    let Some(branch) = root.branches.iter().find(|b| b.name == name).cloned() else {
        state.ui.branches_selected = None;
        state.ui.branches_selected_root = None;
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
                ui.horizontal(|ui| {
                    ui.add_space(PAD_LIST);
                    ui.label(
                        RichText::new(tip.message.clone())
                            .font(data_font(TYPE_BODY))
                            .color(Palette::T_PRIMARY),
                    );
                });
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
/// ("2 ahead · 1 behind · tracks origin/main", "in sync · tracks origin/main").
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
    ui.horizontal(|ui| {
        ui.add_space(PAD_LIST);
        ui.label(
            RichText::new(parts.join(" · "))
                .font(chrome_font(TYPE_CONTROL))
                .color(Palette::T_SECONDARY),
        );
    });
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
