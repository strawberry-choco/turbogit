//! Git Log four-pane workspace (issue #12, spec §8.3): branches pane,
//! graph pane, changed-files pane and commit-details pane. Since issue #19
//! the legacy History tab is gone: file history lives here as a path-scoped
//! view ("Show history for file..." on a changed-file entry).
//!
//! Layout (spec §8.3):
//! 1. **Branches** (left, 210px): live search; LOCAL / REMOTE / TAGS groups
//!    fed by ref decorations; a bottom `ROOTS` filter for multi-root projects.
//! 2. **Graph** (center): live search toolbar, root-stripe legend, and the
//!    commit table (Graph | Hash | Author | Date | Message) with inline ref
//!    chips (branch=brand, remote=success, tag=warning) and a translucent
//!    `SELECTION_BG` row highlight that keeps lane colors readable.
//! 3. **Changed files** (right-top, 320px): the selected commit's files with
//!    status badges; clicking loads the diff.
//! 4. **Commit details** (right-bottom, ~300px SURFACE): key-value hash /
//!    author / date / parents, the Actions section (issue 15), and the full
//!    message below.

use crate::theme::Palette;
use crate::ui::branch_tree_view::{self, TreeEvent, TreeGroup, TreeProps};
use crate::ui::branches::fetch_scope;
use crate::ui::branches_tree::build_branch_view;
use crate::ui::widgets::{self, BadgeKind, RefKind};
use chrono::{DateTime, Local, TimeZone, Utc};
use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Frame, Layout, Margin, Panel, Pos2, Rect,
    Response, RichText, ScrollArea, Sense, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType,
};
use std::path::PathBuf;
use turbogit_app::state::{AppState, BlameTarget, Dialog, DiffTarget, PendingConfirm, Toast};
use turbogit_domain::model::{
    BranchKind, ChangeStatus, Commit, CommitId, DateFormat, GitRefKind, RefState, Root, RootId,
    SignatureState,
};
use turbogit_services::sync_service;

// --- Pane metrics (spec §8.3) -------------------------------------------------

/// Branches pane width.
const BRANCHES_WIDTH: f32 = 210.0;
/// Right column (changed files + details) width.
const FILES_WIDTH: f32 = 320.0;
/// Commit details pane height (grew from the §8.3 200px in issue 15 to fit
/// the Actions section, and to 340px in issue 17 for the committer/signature
/// row plus the Copy-hash header): cherry-pick / revert / create branch here,
/// plus the guardrail explanation when one is blocked.
const DETAILS_HEIGHT: f32 = 340.0;
/// Commit table row height.
const ROW_HEIGHT: f32 = 24.0;
/// Root stripe width on multi-root rows.
const STRIPE_WIDTH: f32 = 3.0;
/// Uppercase micro text (§3.3).
const MICRO_TEXT: f32 = 11.0;
/// Mono cell font size.
const MONO_TEXT: f32 = 12.0;
/// Chip metrics — mirrors `widgets::chip` (`.tg-label` pills).
const CHIP_HEIGHT: f32 = 18.0;
const CHIP_PAD_X: f32 = 6.0;
const CHIP_GAP: f32 = 4.0;

/// Distinct lane colors for the commit graph (Epic D1). Also reused as the
/// deterministic per-root stripe palette.
const GRAPH_COLORS: &[Color32] = &[
    Color32::from_rgb(80, 140, 230),
    Color32::from_rgb(220, 120, 140),
    Color32::from_rgb(120, 200, 130),
    Color32::from_rgb(200, 170, 90),
    Color32::from_rgb(170, 130, 220),
    Color32::from_rgb(90, 190, 200),
    Color32::from_rgb(230, 150, 90),
    Color32::from_rgb(150, 200, 220),
];

fn fmt_time(t: i64) -> String {
    match Utc.timestamp_opt(t, 0) {
        chrono::LocalResult::Single(dt) => {
            let local: DateTime<Local> = DateTime::from(dt);
            local.format("%Y-%m-%d %H:%M").to_string()
        }
        _ => String::new(),
    }
}

/// Render a timestamp according to the configured format (Epic D2).
fn fmt_date(t: i64, mode: DateFormat) -> String {
    match mode {
        DateFormat::Iso => fmt_time(t),
        DateFormat::Absolute => fmt_time(t),
        DateFormat::Relative => {
            let now = Local::now().timestamp();
            let d = now - t;
            if d < 60 {
                format!("{d}s ago")
            } else if d < 3600 {
                format!("{}m ago", d / 60)
            } else if d < 86400 {
                format!("{}h ago", d / 3600)
            } else if d < 2592000 {
                format!("{}d ago", d / 86400)
            } else {
                fmt_time(t)
            }
        }
    }
}

/// Truncate a string to at most `n` chars without panicking on multibyte input.
fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn short(id: &str) -> String {
    truncate(id, 7)
}

/// Deterministic color for the root at `idx` (stripes + legend).
fn root_color(idx: usize) -> Color32 {
    GRAPH_COLORS[idx % GRAPH_COLORS.len()]
}

/// Assign each commit a lane color using a lightweight DAG walk so the list
/// reads like a commit graph (Epic D1). Newest-first input assumed. Takes
/// borrowed commits — the union is never owned (plan §1.3).
fn assign_colors(commits: &[&Commit]) -> std::collections::HashMap<String, usize> {
    use std::collections::HashMap;
    let mut color_of: HashMap<String, usize> = HashMap::new();
    let mut lanes: Vec<Option<String>> = Vec::new();
    let mut next_color = 0usize;
    for c in commits {
        let idx = lanes
            .iter()
            .position(|l| l.as_deref() == Some(c.id.as_str()))
            .unwrap_or_else(|| {
                if let Some(e) = lanes.iter().position(|l| l.is_none()) {
                    e
                } else {
                    lanes.push(None);
                    lanes.len() - 1
                }
            });
        color_of.entry(c.id.clone()).or_insert_with(|| {
            // pick the lane's color, allocating a new one if needed
            if idx < lanes.len() && lanes[idx].is_none() {
                let c = next_color;
                next_color += 1;
                c
            } else {
                idx
            }
        });
        lanes[idx] = c.parents.first().cloned();
        for p in c.parents.iter().skip(1) {
            if let Some(e) = lanes.iter_mut().find(|l| l.is_none()) {
                e.replace(p.clone());
                color_of.entry(p.clone()).or_insert_with(|| {
                    let c = next_color;
                    next_color += 1;
                    c
                });
            } else {
                lanes.push(Some(p.clone()));
                color_of.entry(p.clone()).or_insert_with(|| {
                    let c = next_color;
                    next_color += 1;
                    c
                });
            }
        }
    }
    color_of
}

// --- Data plumbing ------------------------------------------------------------

/// The roots whose history is displayed (roots-filter aware).
fn visible_root_ids(state: &AppState) -> Vec<RootId> {
    match &state.ui.log_root_filter {
        Some(id) => vec![id.clone()],
        None => state.multi.roots.iter().map(|r| r.id.clone()).collect(),
    }
}

/// Lazily load (through the engine seam, cached) everything the four panes
/// need beyond the log itself: ref decorations per visible root, the
/// changed-file list of the selected commit, and — when a path scope is
/// active (issue #19) — the path-scoped commit listing. All fills happen
/// behind the [`turbogit_app::root_caches::RootCaches`] interface.
fn ensure_log_data(state: &mut AppState) {
    for id in visible_root_ids(state) {
        state.caches.ensure_refs(state.executor.as_ref(), &id);
    }
    if let (Some(root), Some(cid)) = (
        state.selected_root.clone(),
        state.ui.selected_commit.clone(),
    ) {
        state
            .caches
            .ensure_files(state.executor.as_ref(), &root, &cid);
    }
    // Path-scoped history (issue #19): fill the scoped cache through the
    // engine seam's `LogOpts::path` support (`git log -- <path>`).
    if let (Some(root), Some(path)) = (state.selected_root.clone(), state.ui.log_path_scope.clone())
    {
        state
            .caches
            .ensure_path_log(state.executor.as_ref(), &root, &path);
    }
    // Ref-scoped history (branch-tree extraction, plan D9): filled like the
    // path scope, through the engine seam's `LogOpts::branch` support
    // (`git log <ref>`).
    if let Some((root, ref_name)) = state.ui.log_ref_scope.clone() {
        state
            .caches
            .ensure_ref_log(state.executor.as_ref(), &root, &ref_name);
    }
    // Code-change search (issue 17): a non-empty search box also fills the
    // pickaxe cache per visible root — `git log -S` covers commits whose
    // content changed the query string's count, which client-side filtering
    // of message/hash/author cannot see.
    let query = state.ui.log_filter.trim().to_string();
    if !query.is_empty() {
        for id in visible_root_ids(state) {
            state
                .caches
                .ensure_search_log(state.executor.as_ref(), &id, &query);
        }
    }
}

/// Commits for `root` honoring the active path scope (issue #19): when a
/// path scope is active and its scoped query is cached, that listing
/// replaces the root's full log everywhere in this window (graph rows,
/// details pane, changed-files parent lookup). Borrows the cache slices —
/// no per-frame commit clones (plan §1.3).
fn commits_for<'a>(state: &'a AppState, root: &RootId) -> Vec<&'a Commit> {
    if let Some((scope_root, ref_name)) = &state.ui.log_ref_scope
        && let Some(commits) = state.caches.ref_log(scope_root, ref_name)
    {
        return if scope_root == root {
            commits.iter().collect()
        } else {
            Vec::new()
        };
    }
    if let Some(path) = &state.ui.log_path_scope
        && let Some(commits) = state.caches.path_log(root, path)
    {
        return commits.iter().collect();
    }
    state
        .caches
        .log(root)
        .map(|c| c.iter().collect())
        .unwrap_or_default()
}

/// The cached commit `cid` of `root`, honoring the active path scope like
/// [`commits_for`] — borrowed over the cache slice instead of cloning the
/// whole listing for parent lookups (plan §1.3).
fn find_commit<'a>(state: &'a AppState, root: &RootId, cid: &str) -> Option<&'a Commit> {
    if let Some((scope_root, ref_name)) = &state.ui.log_ref_scope
        && let Some(commits) = state.caches.ref_log(scope_root, ref_name)
    {
        return commits.iter().find(|c| c.id == cid);
    }
    if let Some(path) = &state.ui.log_path_scope
        && let Some(commits) = state.caches.path_log(root, path)
    {
        return commits.iter().find(|c| c.id == cid);
    }
    state
        .caches
        .log(root)
        .and_then(|commits| commits.iter().find(|c| c.id == cid))
}

/// The commits currently displayed: union across visible roots (newest first,
/// live-filtered by the graph search box). With an active path scope (issue
/// #19) only the selected root's scoped listing is shown — never another
/// root's unscoped log. Yields borrowed commits sorted by `(time, id)`
/// instead of cloning the union per frame (plan §1.3).
fn visible_commits(state: &AppState) -> Vec<&Commit> {
    // Ref scope (plan D9): only the scoped ref's cached listing is shown —
    // same shape as the path scope below.
    let mut commits: Vec<&Commit> = if let Some((root, ref_name)) = &state.ui.log_ref_scope {
        state
            .caches
            .ref_log(root, ref_name)
            .map(|c| c.iter().collect())
            .unwrap_or_default()
    } else if state.ui.log_path_scope.is_some() {
        match &state.selected_root {
            Some(root) => commits_for(state, root),
            None => Vec::new(),
        }
    } else {
        let roots: Vec<&RootId> = match &state.ui.log_root_filter {
            Some(id) => vec![id],
            None => state.multi.roots.iter().map(|r| &r.id).collect(),
        };
        roots
            .into_iter()
            .filter_map(|id| state.caches.log(id))
            .flatten()
            .collect()
    };
    commits.sort_by(|a, b| b.time.cmp(&a.time).then(a.id.cmp(&b.id)));
    let filter = state.ui.log_filter.to_lowercase();
    if filter.is_empty() {
        return commits;
    }
    commits.retain(|c| {
        c.message.to_lowercase().contains(&filter)
            || c.id.to_lowercase().contains(&filter)
            || c.author.name.to_lowercase().contains(&filter)
    });
    // Code-change hits (issue 17): union the cached pickaxe listing per
    // visible root — a commit whose content changed the query's count shows
    // even when message/hash/author do not match. Skipped inside a path
    // scope: the scoped view must only ever list commits touching the
    // scoped path, and the pickaxe cache is not path-scoped.
    if state.ui.log_path_scope.is_none() {
        let roots: Vec<&RootId> = match &state.ui.log_root_filter {
            Some(id) => vec![id],
            None => state.multi.roots.iter().map(|r| &r.id).collect(),
        };
        for id in roots {
            // The cache is keyed by the trimmed raw query (what the engine
            // received) — not the lowercased live-filter text.
            if let Some(hits) = state.caches.search_log(id, state.ui.log_filter.trim()) {
                for c in hits {
                    if !commits.iter().any(|v| v.id == c.id) {
                        commits.push(c);
                    }
                }
            }
        }
        commits.sort_by(|a, b| b.time.cmp(&a.time).then(a.id.cmp(&b.id)));
    }
    commits
}

fn ref_kind(kind: GitRefKind) -> RefKind {
    match kind {
        GitRefKind::Branch => RefKind::Branch,
        GitRefKind::Remote => RefKind::Remote,
        GitRefKind::Tag => RefKind::Tag,
    }
}

// --- Composition ----------------------------------------------------------------

pub fn show_log(ui: &mut Ui, state: &mut AppState) {
    // The ref scope clears when the selected repository changes (plan D9),
    // so the graph never becomes silently empty.
    if let Some((root, _)) = &state.ui.log_ref_scope
        && state.selected_root.as_ref() != Some(root)
    {
        state.ui.log_ref_scope = None;
    }
    ensure_log_data(state);

    // Fixed spec widths shrink proportionally on narrow windows so the graph
    // pane always keeps positive width and nothing clips irrecoverably
    // (issue #23: minimum sizes hold at small window sizes).
    let avail_w = ui.available_width();
    let branches_w = BRANCHES_WIDTH.min((avail_w * 0.25).max(140.0));
    let files_w = FILES_WIDTH.min((avail_w * 0.32).max(180.0));

    // Pane 1 — branches (left, 210px at full size).
    Panel::left("log_branches_pane")
        .exact_size(branches_w)
        .resizable(false)
        .frame(
            Frame::new()
                .fill(Palette::SURFACE)
                .inner_margin(Margin::same(8)),
        )
        .show(ui, |ui| branches_pane(ui, state));

    // Panes 3+4 — right column: changed files on top, details pinned below.
    Panel::right("log_right_column")
        .exact_size(files_w)
        .resizable(false)
        .frame(Frame::new().fill(Palette::BG))
        .show(ui, |ui| {
            // The 300px details pane yields to short windows so the changed-
            // files pane above it never collapses to zero height.
            let details_h = DETAILS_HEIGHT.min((ui.available_height() - 80.0).max(96.0));
            Panel::bottom("log_details_pane")
                .exact_size(details_h)
                .resizable(false)
                .frame(
                    Frame::new()
                        .fill(Palette::SURFACE)
                        .inner_margin(Margin::same(8)),
                )
                .show(ui, |ui| details_pane(ui, state));
            files_pane(ui, state);
        });

    // Pane 2 — graph fills the remainder; the blame view (issue 18) takes
    // its place while open, keeping the branches / files / details panes.
    if state.ui.blame.is_some() {
        super::blame_view::show_blame(ui, state);
    } else {
        graph_pane(ui, state);
    }
}

// --- Pane 1: branches -------------------------------------------------------------

fn branches_pane(ui: &mut Ui, state: &mut AppState) {
    widgets::toolwindow_header(ui, "Branches", |_ui| {});
    ui.add_space(2.0);
    widgets::search_input(ui, "Search branches", &mut state.ui.log_branch_filter);
    ui.add_space(4.0);

    // Warm the shared tag cache (plan D10): whichever tool window opens
    // first warms the tags for both.
    branch_tree_view::warm_tags(state);

    // This instance has remotes visible, fixed — no toggle lives in the pane.
    state.ui.log_tree.show_remotes = true;

    // Build the tree over the Roots-filter-visible roots. The decorations
    // upgrade the warmed tag states (plan D8) and mark remote-tracking refs
    // whose upstream was deleted as gone (issue 17, carried through the
    // branch snapshot).
    let ids = visible_root_ids(state);
    let mut roots: Vec<Root> = ids
        .iter()
        .filter_map(|id| state.multi.by_id(id).cloned())
        .collect();
    let mut tags_by_root = state.ui.branches_tags.clone();
    for root in &mut roots {
        let mut deco: std::collections::HashMap<String, RefState> =
            std::collections::HashMap::new();
        for group in state.caches.ref_groups(&root.id) {
            for r in group {
                deco.entry(r.name.clone()).or_insert(r.state);
            }
        }
        for b in &mut root.branches {
            if b.kind == BranchKind::Remote {
                // Decorations name remote refs `remote/branch`; the snapshot
                // carries the split form (`name` + `remote`).
                let full = match &b.remote {
                    Some(r) => format!("{r}/{}", b.name),
                    None => b.name.clone(),
                };
                b.gone = deco.get(&full).is_some_and(|st| *st == RefState::Gone);
            }
        }
        if let Some(tags) = tags_by_root.get_mut(&root.id) {
            for (name, st) in tags {
                if let Some(d) = deco.get(name) {
                    *st = *d;
                }
            }
        }
    }
    let view = build_branch_view(&roots, &tags_by_root, state.ui.log_tree.show_remotes);

    // The shared tree component with the pane's narrower capability set
    // (plan D7): no inline rename, no per-row actions, no repo-scope picker.
    let props = TreeProps {
        view: &view,
        filter: &state.ui.log_branch_filter,
        repo_filter: None,
        tags_by_root: &tags_by_root,
        multi_repo: state.multi.roots.len() > 1,
        busy: false,
        has_any_data: true,
        merge_in_progress: false,
        last_fetch: None,
        now: chrono::Utc::now(),
        allows_rename: false,
        shows_row_actions: false,
        id_salt: "log_branch_tree",
        full_height: false,
    };
    let events = branch_tree_view::branch_tree(ui, &props, &mut state.ui.log_tree);
    for event in events {
        apply_log_tree_event(state, event);
    }

    ui.add_space(8.0);
    roots_filter_section(ui, state);
}

/// The Log pane's tree-event policy (plan D7): row activation scopes the
/// graph to that ref; group/remote toggles keep the pane's own tree state;
/// the repo header's Fetch covers the same scope as the Branches window's.
fn apply_log_tree_event(state: &mut AppState, event: TreeEvent) {
    // A modal surface owns the keyboard and pointer: tree events are ignored
    // while a dialog or confirmation is up.
    if state.ui.dialog.is_some() || state.ui.confirm.is_some() {
        return;
    }
    match event {
        TreeEvent::RowClicked { root, branch } | TreeEvent::RowActivated { root, branch } => {
            state.ui.log_ref_scope = Some((root.clone(), branch));
            state.selected_root = Some(root);
            state.ui.selected_commit = None;
            state.ui.log_selected_file = None;
        }
        TreeEvent::GroupToggled(group) => match group {
            TreeGroup::Local => {
                let on = state.ui.log_tree.groups.local;
                state.ui.log_tree.groups.local = !on;
            }
            TreeGroup::Tags => {
                let on = state.ui.log_tree.groups.tags;
                state.ui.log_tree.groups.tags = !on;
            }
        },
        TreeEvent::RemoteToggled { root, remote } => {
            let key = (root.clone(), remote.clone());
            if !state.ui.log_tree.collapsed_remotes.remove(&key) {
                state.ui.log_tree.collapsed_remotes.insert(key);
            }
        }
        TreeEvent::RemotesVisibleChanged { visible } => {
            state.ui.log_tree.show_remotes = visible;
        }
        TreeEvent::FetchRequested { .. } => fetch_scope(state),
        _ => {}
    }
}

/// Bottom-of-pane ROOTS filter (multi-root): All roots / per-root rows.
fn roots_filter_section(ui: &mut Ui, state: &mut AppState) {
    ui.separator();
    widgets::group_title(ui, "Roots");

    let all_active = state.ui.log_root_filter.is_none();
    let (rect, response) = allocate_row(ui);
    paint_row_fill(ui, &rect, all_active, response.hovered());
    // Painted via galley (not `ui.label`) so the row's widget label stays the
    // only accessibility node carrying "All roots".
    let galley =
        ui.painter()
            .layout_no_wrap("All roots".to_owned(), body_font(), row_ink(all_active));
    ui.painter().galley(
        Pos2::new(rect.left() + 4.0, rect.center().y - galley.size().y / 2.0),
        galley,
        row_ink(all_active),
    );
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "All roots"));
    widgets::focus_ring(ui, &response);
    if response.clicked() {
        state.ui.log_root_filter = None;
    }

    for (idx, root) in state.multi.roots.iter().enumerate() {
        let active = state.ui.log_root_filter.as_ref() == Some(&root.id);
        let label = format!("Root {}", root.id.name());
        let (rect, response) = allocate_row(ui);
        paint_row_fill(ui, &rect, active, response.hovered());
        if std::env::var("TG_PROBE_TRACE").as_deref() == Ok("1") {
            eprintln!(
                "[root {label}] pointer={:?} rect={rect:?} hovered={}",
                ui.input(|i| i.pointer.hover_pos()),
                response.hovered()
            );
        }
        let child = ui.new_child(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        // Root stripe dot in the root's color.
        let cy = rect.center().y;
        child
            .painter()
            .circle_filled(Pos2::new(rect.left() + 7.0, cy), 3.5, root_color(idx));
        // Galley text again: keep the widget label unique in the tree.
        let text_galley =
            child
                .painter()
                .layout_no_wrap(label.clone(), body_font(), row_ink(active));
        child.painter().galley(
            Pos2::new(rect.left() + 16.0, cy - text_galley.size().y / 2.0),
            text_galley,
            row_ink(active),
        );
        response.widget_info(|| {
            WidgetInfo::labeled(WidgetType::Button, true, format!("Root {}", root.id.name()))
        });
        widgets::focus_ring(ui, &response);
        if response.clicked() {
            state.ui.log_root_filter = Some(root.id.clone());
        }
    }
}

// --- Pane 2: graph ------------------------------------------------------------------

fn graph_pane(ui: &mut Ui, state: &mut AppState) {
    // Filter toolbar: live search over message / hash / author / code
    // change, with the path scope (issue #19) rendered as a removable chip
    // on the right (issue 17).
    ui.horizontal(|ui| {
        widgets::search_input(ui, "Search commits", &mut state.ui.log_filter);
        if let Some(path) = state.ui.log_path_scope.clone() {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // Painted as a compact ×; the accessibility label carries
                // the full verb (kittest drives it by that label).
                let remove = ui
                    .small_button(RichText::new("×").size(13.0))
                    .on_hover_text("Remove the path filter");
                remove.widget_info(|| {
                    WidgetInfo::labeled(WidgetType::Button, true, "Remove path filter")
                });
                if remove.clicked() {
                    state.ui.log_path_scope = None;
                }
                ui.label(
                    RichText::new(format!("Path filter: {}", path.display()))
                        .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                        .color(Palette::BRAND),
                );
            });
        }
        if let Some((_, ref_name)) = state.ui.log_ref_scope.clone() {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // The ref scope's chip — the same removable-chip gesture as
                // the path filter (plan D7/D9).
                let remove = ui
                    .small_button(RichText::new("×").size(13.0))
                    .on_hover_text("Remove the ref filter");
                remove.widget_info(|| {
                    WidgetInfo::labeled(WidgetType::Button, true, "Remove ref filter")
                });
                if remove.clicked() {
                    state.ui.log_ref_scope = None;
                }
                ui.label(
                    RichText::new(format!("Ref filter: {ref_name}"))
                        .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                        .color(Palette::BRAND),
                );
            });
        }
    });

    let commits = visible_commits(state);
    let colors = assign_colors(&commits);
    let date_mode = state.settings.date_format;
    // Scoped views are single-root by definition — no root stripes/legend.
    let multi_root = state.multi.roots.len() > 1
        && state.ui.log_root_filter.is_none()
        && state.ui.log_path_scope.is_none()
        && state.ui.log_ref_scope.is_none();

    // Pagination (issue 17): "Load more" is offered while a visible root's
    // cached log fills its whole fetch window — and never in a scoped view,
    // whose listing is fetched uncapped. The click is deferred like the row
    // selections: `commits` borrows the caches until rendering ends.
    let page_limit = (state.ui.log_page + 1) * turbogit_app::state::LOG_PAGE_SIZE;
    let may_have_more = state.ui.log_path_scope.is_none()
        && state.ui.log_ref_scope.is_none()
        && visible_root_ids(state)
            .iter()
            .any(|id| state.caches.log(id).is_some_and(|c| c.len() >= page_limit));
    let mut load_more = false;

    // Root-stripe legend chip row (11px INK_3) for multi-root setups.
    if multi_root {
        ui.horizontal_wrapped(|ui| {
            for (idx, root) in state.multi.roots.iter().enumerate() {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(10.0, 10.0), Sense::hover());
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(2), root_color(idx));
                ui.label(
                    RichText::new(root.id.name())
                        .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                        .color(Palette::INK_3),
                );
            }
        });
    }

    // Column micro-headers aligned with the cells below.
    let left = ui.cursor().left() + STRIPE_WIDTH;
    header_cells(ui, left);

    // Row clicks are deferred (plan §1.3): the displayed union borrows the
    // cache slices, so rows render against a shared AppState and the
    // selection lands after the scroll pass ends.
    let mut clicked: Option<String> = None;
    ScrollArea::vertical().show(ui, |ui| {
        for c in &commits {
            if commit_row(ui, state, c, &colors, date_mode, multi_root) {
                clicked = Some(c.id.clone());
            }
        }
        if commits.is_empty() {
            ui.label(
                RichText::new("No commits match.")
                    .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                    .color(Palette::INK_3),
            );
        }
    });

    // Pagination status line (issue 17): what is shown + the Load more
    // affordance while the fetched window may not cover the whole history.
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if may_have_more && ui.small_button("Load more").clicked() {
                load_more = true;
            }
            ui.label(
                RichText::new(format!("{} shown", commits.len()))
                    .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                    .color(Palette::INK_3),
            );
        });
    });

    if load_more {
        state.load_more_log();
    }
    if let Some(id) = clicked {
        state.ui.selected_commit = Some(id);
        state.ui.log_selected_file = None;
    }
}

fn body_font() -> FontId {
    FontId::new(12.0, FontFamily::Proportional)
}

fn mono_font() -> FontId {
    FontId::new(MONO_TEXT, FontFamily::Monospace)
}

fn row_ink(active: bool) -> Color32 {
    if active { Palette::INK } else { Palette::INK_2 }
}

fn allocate_row(ui: &mut Ui) -> (Rect, Response) {
    let width = ui.available_width();
    ui.allocate_exact_size(Vec2::new(width, ROW_HEIGHT), Sense::click())
}

/// Row fill decision: active rows keep the translucent selection token,
/// hovered rows take SURFACE_2, idle rows stay transparent.
fn paint_row_fill(ui: &Ui, rect: &Rect, active: bool, hovered: bool) {
    let fill = if active {
        Palette::selection_bg()
    } else if hovered {
        Palette::SURFACE_2
    } else {
        return;
    };
    ui.painter().rect_filled(*rect, CornerRadius::same(4), fill);
}

/// Micro column headers above the commit table.
fn header_cells(ui: &mut Ui, left: f32) {
    let top = ui.cursor().top();
    ui.add_space(16.0);
    let headers = [
        ("GRAPH", 16.0),
        ("HASH", 68.0),
        ("AUTHOR", 148.0),
        ("DATE", 228.0),
        ("MESSAGE", 308.0),
    ];
    for (title, dx) in headers {
        let galley = ui.painter().layout_no_wrap(
            title.to_owned(),
            FontId::new(MICRO_TEXT, FontFamily::Proportional),
            Palette::INK_3,
        );
        ui.painter()
            .galley(Pos2::new(left + dx, top + 2.0), galley, Palette::INK_3);
    }
}

/// One commit-table row: stripe | node | hash | author | date | message(+chips).
/// Renders against a shared [`AppState`] (the displayed union borrows the
/// caches) and reports whether the row was clicked; the caller applies the
/// selection after rendering (plan §1.3 defer pattern).
fn commit_row(
    ui: &mut Ui,
    state: &AppState,
    c: &Commit,
    colors: &std::collections::HashMap<String, usize>,
    date_mode: DateFormat,
    multi_root: bool,
) -> bool {
    let selected = state.ui.selected_commit.as_deref() == Some(c.id.as_str());
    let (rect, response) = allocate_row(ui);

    // Translucent selection (SELECTION_BG) keeps lane colors readable (§7.2).
    if selected {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(4), Palette::selection_bg());
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(4), Palette::SURFACE_2);
    }

    // Root stripe (multi-root only).
    let mut content_left = rect.left();
    if multi_root {
        let idx = state
            .multi
            .roots
            .iter()
            .position(|r| r.id == c.root)
            .unwrap_or(0);
        ui.painter().rect_filled(
            Rect::from_min_size(
                Pos2::new(rect.left(), rect.top()),
                Vec2::new(STRIPE_WIDTH, rect.height()),
            ),
            CornerRadius::ZERO,
            root_color(idx),
        );
        content_left += STRIPE_WIDTH + 2.0;
    }

    // Graph cell: colored lane node (ring for merges).
    let lane = colors
        .get(&c.id)
        .map(|i| root_color(*i))
        .unwrap_or(Color32::GRAY);
    let center = Pos2::new(content_left + 10.0, rect.center().y);
    if c.parents.len() > 1 {
        ui.painter()
            .circle_stroke(center, 4.0, egui::Stroke::new(1.5, lane));
    } else {
        ui.painter().circle_filled(center, 4.0, lane);
    }

    // Hash | Author | Date cells.
    let painter = ui.painter().clone();
    let cy = rect.center().y;
    let hash_galley = painter.layout_no_wrap(short(&c.id), mono_font(), Palette::BRAND);
    painter.galley(
        Pos2::new(content_left + 26.0, cy - hash_galley.size().y / 2.0),
        hash_galley,
        Palette::BRAND,
    );
    let author_galley =
        painter.layout_no_wrap(truncate(&c.author.name, 10), body_font(), Palette::INK_2);
    painter.galley(
        Pos2::new(content_left + 84.0, cy - author_galley.size().y / 2.0),
        author_galley,
        Palette::INK_2,
    );
    let date_galley =
        painter.layout_no_wrap(fmt_date(c.time, date_mode), body_font(), Palette::INK_3);
    painter.galley(
        Pos2::new(content_left + 164.0, cy - date_galley.size().y / 2.0),
        date_galley,
        Palette::INK_3,
    );

    // Message cell with inline ref chips. Everything is painted directly
    // (galleys + pills, no child widgets) so the row itself stays the only
    // interactive surface — a child `ui.label` here would sit on top of the
    // row in hit-testing and swallow its clicks.
    let painter = ui.painter().clone();
    let mut mx = content_left + 236.0;
    let subject = c.message.lines().next().unwrap_or("");
    let subject_galley = painter.layout_no_wrap(truncate(subject, 44), body_font(), Palette::INK);
    let subject_w = subject_galley.size().x;
    painter.galley(
        Pos2::new(mx, cy - subject_galley.size().y / 2.0),
        subject_galley,
        Palette::INK,
    );
    mx += subject_w + 6.0;
    for r in state.caches.refs_for(&c.root, &c.id) {
        mx = paint_ref_chip(&painter, r.name.clone(), ref_kind(r.kind), mx, cy);
    }

    response.widget_info(|| {
        WidgetInfo::labeled(
            WidgetType::Button,
            true,
            format!("{} {}", short(&c.id), subject),
        )
    });
    widgets::focus_ring(ui, &response);
    response.clicked()
}

/// Paint one `.tg-label` pill (18px, kind colors) at `(x, cy)` and return the
/// x position after it. Painter-only: registers no widget (see commit_row).
fn paint_ref_chip(painter: &egui::Painter, text: String, kind: RefKind, x: f32, cy: f32) -> f32 {
    let colors = kind.colors();
    let font = FontId::new(MICRO_TEXT, FontFamily::Proportional);
    let galley = painter.layout_no_wrap(text, font, colors.fg);
    let rect = Rect::from_min_size(
        Pos2::new(x, cy - CHIP_HEIGHT / 2.0),
        Vec2::new(galley.size().x + CHIP_PAD_X * 2.0, CHIP_HEIGHT),
    );
    painter.rect_filled(
        rect,
        CornerRadius::same((CHIP_HEIGHT / 2.0) as u8),
        colors.bg,
    );
    painter.galley(
        Pos2::new(
            rect.center().x - galley.size().x / 2.0,
            cy - galley.size().y / 2.0,
        ),
        galley,
        colors.fg,
    );
    rect.right() + CHIP_GAP
}

// --- Pane 3: changed files -----------------------------------------------------------

/// Deferred interaction from one changed-file row (plan §1.3): rows render
/// against a shared [`AppState`] — the selection and the cached file list are
/// borrowed — so mutations land after the pane finishes rendering.
enum FileAction {
    None,
    /// Row clicked: open its diff; the caller resolves root / commit / parent.
    OpenDiff(PathBuf),
    /// Footer link / context menu: blame the file at the selected commit.
    OpenBlame(PathBuf),
    /// Context menu: scope the whole workspace to this file's history.
    ScopeHistory(PathBuf),
}

fn files_pane(ui: &mut Ui, state: &mut AppState) {
    // Split-borrow the selection up front (plan §1.3): no owned RootId /
    // CommitId clones per frame, and the cached file list is iterated in
    // place instead of copied into a fresh Vec.
    let selection = state
        .selected_root
        .as_ref()
        .zip(state.ui.selected_commit.as_ref());
    let files = selection
        .and_then(|(root, cid)| state.caches.files_for(root, cid))
        .unwrap_or(&[]);

    widgets::toolwindow_header(ui, &format!("Changed files ({})", files.len()), |_ui| {});

    let Some((root_id, cid)) = selection else {
        ui.label(
            RichText::new("Select a commit to see its changed files.")
                .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                .color(Palette::INK_3),
        );
        return;
    };

    let parent = find_commit(state, root_id, cid).and_then(|c| c.parents.first().cloned());

    let mut action = FileAction::None;
    ScrollArea::vertical().show(ui, |ui| {
        for ch in files {
            action = file_row(ui, state, ch);
        }
        if files.is_empty() {
            ui.label(
                RichText::new("No changed files.")
                    .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                    .color(Palette::INK_3),
            );
        }
    });

    // Footer links (screen 09): Open diff · Blame · Full path history,
    // acting on the selected changed file. Rendered only with a selection —
    // without one none of the verbs has a subject.
    if let Some(selected) = state.ui.log_selected_file.clone() {
        ui.separator();
        ui.horizontal(|ui| {
            if ui.link("Open diff").clicked() {
                action = FileAction::OpenDiff(selected.clone());
            }
            ui.label(RichText::new("·").color(Palette::INK_3));
            if ui.link("Blame").clicked() {
                action = FileAction::OpenBlame(selected.clone());
            }
            ui.label(RichText::new("·").color(Palette::INK_3));
            if ui.link("Full path history").clicked() {
                action = FileAction::ScopeHistory(selected);
            }
        });
    }

    match action {
        FileAction::None => {}
        FileAction::OpenDiff(path) => {
            state.ui.log_selected_file = Some(path.clone());
            state.ui.diff = Some(DiffTarget {
                root: root_id.clone(),
                left: parent,
                right: Some(cid.to_owned()),
                path: Some(path),
            });
        }
        FileAction::OpenBlame(path) => {
            state.ui.blame = Some(BlameTarget {
                root: root_id.clone(),
                path,
                rev: cid.to_owned(),
            });
        }
        FileAction::ScopeHistory(path) => {
            state.ui.log_path_scope = Some(path);
            state.ui.selected_commit = None;
            state.ui.log_selected_file = None;
        }
    }
}

fn badge_kind(status: ChangeStatus) -> BadgeKind {
    match status {
        ChangeStatus::Added => BadgeKind::Added,
        ChangeStatus::Deleted => BadgeKind::Deleted,
        ChangeStatus::Modified => BadgeKind::Modified,
        _ => BadgeKind::Neutral,
    }
}

/// One changed-file row: status badge + path, click opens the diff preview,
/// context menu scopes the workspace to the file's history (issue #19).
/// Renders against a shared [`AppState`] and reports its intent; the caller
/// applies it after rendering (plan §1.3 defer pattern).
fn file_row(ui: &mut Ui, state: &AppState, change: &turbogit_domain::model::Change) -> FileAction {
    let path_str = change.path.display().to_string();
    let selected = state.ui.log_selected_file.as_ref() == Some(&change.path);
    let (rect, response) = allocate_row(ui);
    paint_row_fill(ui, &rect, selected, response.hovered());

    // Painter-only contents so the row owns the pointer (see commit_row).
    let painter = ui.painter().clone();
    let cy = rect.center().y;
    let mut mx = rect.left() + 4.0;

    // Status badge pill (mirrors `widgets::badge` metrics).
    let kind = badge_kind(change.status);
    let colors = kind.colors();
    let badge_galley = painter.layout_no_wrap(
        change.status.short().to_owned(),
        FontId::new(MICRO_TEXT, FontFamily::Proportional),
        colors.fg,
    );
    let badge_rect = Rect::from_min_size(
        Pos2::new(mx, cy - CHIP_HEIGHT / 2.0),
        Vec2::new(badge_galley.size().x + CHIP_PAD_X * 2.0, CHIP_HEIGHT),
    );
    painter.rect_filled(badge_rect, CornerRadius::same(4), colors.bg);
    painter.galley(
        Pos2::new(
            badge_rect.center().x - badge_galley.size().x / 2.0,
            cy - badge_galley.size().y / 2.0,
        ),
        badge_galley,
        colors.fg,
    );
    mx = badge_rect.right() + 6.0;

    let path_galley =
        painter.layout_no_wrap(truncate(&path_str, 34), body_font(), row_ink(selected));
    painter.galley(
        Pos2::new(mx, cy - path_galley.size().y / 2.0),
        path_galley,
        row_ink(selected),
    );

    response.widget_info(|| {
        WidgetInfo::labeled(WidgetType::Button, true, change.path.display().to_string())
    });
    widgets::focus_ring(ui, &response);
    if response.clicked() {
        return FileAction::OpenDiff(change.path.clone());
    }
    // Path-scoped file history (issue #19): right-click a changed file to
    // narrow the whole Git Log workspace to the commits touching it.
    // Blame (issue 18): annotate the same file at the selected commit.
    let mut action = FileAction::None;
    response.context_menu(|ui| {
        if ui.button("Show blame").clicked() {
            action = FileAction::OpenBlame(change.path.clone());
            ui.close();
        }
        if ui.button("Show history for file...").clicked() {
            action = FileAction::ScopeHistory(change.path.clone());
            ui.close();
        }
    });
    action
}

// --- Pane 4: commit details -----------------------------------------------------------

/// Deferred interaction from one details-pane action button (plan §1.3):
/// the pane renders against the borrowed cached commit, and the mutation
/// lands after rendering (mirrors [`FileAction`]).
enum DetailAction {
    None,
    /// Open the cherry-pick target-branch picker for the selected commit.
    CherryPick,
    /// Open the cross-repo cherry-pick dialog (issue 16) for the source repo.
    CherryPickAcross,
    /// Ask to confirm reverting the selected commit.
    Revert,
    /// Open the new-branch dialog prefilled at the selected commit.
    NewBranchHere,
    /// Copy the selected commit's full hash to the clipboard (issue 17).
    CopyHash,
    /// Jump to a parent commit via its hash link (issue 17).
    SelectParent(CommitId),
}

fn details_pane(ui: &mut Ui, state: &mut AppState) {
    // Every interaction defers (plan §1.3): the pane borrows the cached
    // commit below, so clicks set `action` and mutations land at the end.
    let mut action = DetailAction::None;
    // Header row: the group title left, Copy hash (issue 17) right — the
    // one detail-pane affordance the mockup pins to the header.
    ui.horizontal(|ui| {
        widgets::group_title(ui, "Commit details");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .small_button(RichText::new("Copy hash").size(MICRO_TEXT))
                .clicked()
            {
                action = DetailAction::CopyHash;
            }
        });
    });
    // Compact vertical rhythm so the full message fits the pane.
    ui.style_mut().spacing.item_spacing.y = 3.0;

    // Split-borrow the selection (plan §1.3): the commit is looked up over
    // the cache slice and the file summary iterates the cached list in place.
    let Some((root_id, cid)) = state
        .selected_root
        .as_ref()
        .zip(state.ui.selected_commit.as_ref())
    else {
        ui.label(
            RichText::new("Select a commit…")
                .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                .color(Palette::INK_3),
        );
        return;
    };

    let Some(commit) = find_commit(state, root_id, cid) else {
        return;
    };

    // Key-value block (compact single rows — the pane is ~300px tall).
    ui.horizontal(|ui| {
        kv_label(ui, "Hash:");
        ui.label(
            RichText::new(short(&commit.id))
                .font(mono_font())
                .color(Palette::BRAND),
        );
    });
    kv_value(
        ui,
        "Author:",
        format!("{} <{}>", commit.author.name, commit.author.email),
    );
    // Committer + signature state (issue 17, screen 09).
    let sig_suffix = match commit.signature {
        SignatureState::Unsigned => String::new(),
        SignatureState::Good => " · signed ✓".to_string(),
        SignatureState::Bad => " · signature BAD".to_string(),
        SignatureState::Unverified => " · signed (unverified)".to_string(),
    };
    kv_value(
        ui,
        "Committer:",
        format!(
            "{} <{}>{}",
            commit.committer.name, commit.committer.email, sig_suffix
        ),
    );
    kv_value(ui, "Date:", fmt_time(commit.time));

    ui.horizontal(|ui| {
        kv_label(ui, "Parents:");
        if commit.parents.is_empty() {
            ui.label(RichText::new("—").font(body_font()).color(Palette::INK_3));
        } else {
            for p in &commit.parents {
                // Parent hashes are links (issue 17): clicking jumps to the
                // parent — deferred like every other pane action.
                if ui
                    .link(
                        RichText::new(short(p))
                            .font(mono_font())
                            .color(Palette::INK),
                    )
                    .clicked()
                {
                    action = DetailAction::SelectParent(p.clone());
                }
            }
        }
    });

    // File summary badges ("2 modified · 1 added").
    let files = state.caches.files_for(root_id, &commit.id).unwrap_or(&[]);
    let modified = files
        .iter()
        .filter(|f| f.status == ChangeStatus::Modified)
        .count();
    let added = files
        .iter()
        .filter(|f| f.status == ChangeStatus::Added)
        .count();
    let deleted = files
        .iter()
        .filter(|f| f.status == ChangeStatus::Deleted)
        .count();
    let summary = [modified, added, deleted]
        .into_iter()
        .zip(["modified", "added", "deleted"])
        .filter(|(n, _)| *n > 0)
        .map(|(n, w)| format!("{n} {w}"))
        .collect::<Vec<_>>()
        .join(" · ");
    if !summary.is_empty() {
        ui.label(
            RichText::new(summary)
                .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                .color(Palette::INK_3),
        );
    }

    // Actions section (issue 15, screen 09): stacked full-width buttons —
    // cherry-pick (primary), revert, create branch here. Guarded by the
    // protected-branch and dirty-worktree guardrails: a blocked action is
    // disabled and its reason is painted (visible without hover, so the
    // explanation is always discoverable).
    widgets::group_title(ui, "Actions");
    let snapshot = state.multi.by_id(root_id);
    let dirty = snapshot.is_some_and(|r| !r.status.changes.is_empty());
    let current_protected = snapshot
        .and_then(|r| r.current_branch.clone())
        .is_some_and(|b| sync_service::is_protected(&state.settings, &b));
    let mut reasons: Vec<String> = Vec::new();
    if let Some(r) = snapshot
        && let Some(branch) = &r.current_branch
        && sync_service::is_protected(&state.settings, branch)
    {
        reasons.push(format!("'{branch}' is a protected branch — revert blocked"));
    }
    if dirty {
        reasons.push("working tree is dirty — cherry-pick and revert blocked".into());
    }
    // Snapshot the id up front (plan §1.3): `commit` borrows the cache
    // slice, and the click handlers below mutate `state.ui` — so clicks are
    // reported and applied after the pane finishes rendering.
    let commit_id = commit.id.clone();
    let pick = widgets::action_button(ui, "Cherry-pick to…", true, !dirty)
        .on_disabled_hover_text("Resolve the uncommitted changes first");
    if pick.clicked() {
        action = DetailAction::CherryPick;
    }
    // The two secondary git actions share a row: the pane is 200px tall and
    // a fourth stacked button would push the full message below the fold.
    let (across, revert) = ui.columns(2, |columns| {
        let across = widgets::action_button(&mut columns[0], "Cherry-pick across…", false, !dirty);
        let revert = widgets::action_button(
            &mut columns[1],
            "Revert commit",
            false,
            !dirty && !current_protected,
        );
        (across, revert)
    });
    let revert = revert.on_disabled_hover_text("Resolve the guardrail below first");
    if across.clicked() {
        action = DetailAction::CherryPickAcross;
    }
    if revert.clicked() {
        action = DetailAction::Revert;
    }
    if widgets::action_button(ui, "Create branch here", false, true).clicked() {
        action = DetailAction::NewBranchHere;
    }
    if !reasons.is_empty() {
        ui.label(
            RichText::new(reasons.join(" · "))
                .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                .color(Palette::STATE_WARNING),
        );
    }

    // Full message below the key-value block. The viewport is capped to
    // the remaining pane height — a `ScrollArea` would otherwise claim all
    // of it and push past the fixed pane, clipping the message when the
    // metadata block grows (issue 17 added the committer row).
    let message_height = ui.available_height().max(40.0);
    ScrollArea::vertical()
        .max_height(message_height)
        .show(ui, |ui| {
            for line in commit.message.lines().filter(|l| !l.trim().is_empty()) {
                ui.label(RichText::new(line).font(body_font()).color(Palette::INK));
            }
        });

    // Deferred action application (plan §1.3): the borrow of the cached
    // commit ended above, so `state.ui` is free to mutate here.
    match action {
        DetailAction::None => {}
        DetailAction::CherryPick => {
            state.ui.dlg.cherry_pick_commit = Some(commit_id);
            state.ui.dialog = Some(Dialog::CherryPickTarget);
        }
        DetailAction::CherryPickAcross => {
            state.open_cherry_across();
            state.ui.dialog = Some(Dialog::CherryPickAcross);
        }
        DetailAction::Revert => {
            state.ui.confirm = Some(PendingConfirm::RevertCommit { commit: commit_id });
        }
        DetailAction::NewBranchHere => {
            state.ui.dlg.new_branch_name.clear();
            state.ui.dlg.new_branch_start = commit_id;
            state.ui.dlg.new_branch_checkout = false;
            state.ui.dialog = Some(Dialog::NewBranch);
        }
        DetailAction::CopyHash => {
            let id = state.ui.selected_commit.clone().unwrap_or_default();
            ui.ctx().copy_text(id.clone());
            state.ui.toast_shown_at = None;
            state.ui.toast = Some(Toast::success(format!("Copied {}", short(&id))));
        }
        DetailAction::SelectParent(parent) => {
            state.ui.selected_commit = Some(parent);
            state.ui.log_selected_file = None;
        }
    }
}

fn kv_label(ui: &mut Ui, key: &str) {
    ui.label(
        RichText::new(key)
            .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
            .color(Palette::INK_3),
    );
}

fn kv_value(ui: &mut Ui, key: &str, value: impl Into<String>) {
    ui.horizontal(|ui| {
        kv_label(ui, key);
        ui.label(
            RichText::new(value.into())
                .font(body_font())
                .color(Palette::INK),
        );
    });
}
