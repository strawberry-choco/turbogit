//! Git Log four-pane workspace (issue #12, spec §8.3): branches pane,
//! graph pane, changed-files pane and commit-details pane. Since issue #19
//! the legacy History tab is gone: file history lives here as a path-scoped
//! view ("Show history for file…" on a changed-file entry).
//!
//! Layout (spec §8.3):
//! 1. **Branches** (left, 210px): live search; LOCAL / REMOTE / TAGS groups
//!    fed by ref decorations; a bottom `ROOTS` filter for multi-root projects.
//! 2. **Graph** (center): live search toolbar, root-stripe legend, and the
//!    commit table (Graph | Hash | Author | Message | Date) with one collapsed
//!    `.tg-label` pill per decorated commit — the ref names (branch=brand,
//!    remote=success, tag=warning) are revealed in its hover tooltip — and a
//!    translucent `SELECTION_BG` row highlight that keeps lane colors readable.
//! 3. **Changed files** (right-top, 320px): the selected commit's files with
//!    status badges; clicking loads the diff.
//! 4. **Commit details** (right-bottom, 440px SURFACE): the subject, the hash
//!    chip, the author card, the committer / date / parents grid, the churn
//!    summary, and the full message below. The pane says things about a commit
//!    and does nothing to one (ADR-0024): every action lives in the row's
//!    context menu, and the parent hashes stay the one link it owns.

use crate::theme::Palette;
use crate::ui::branch_tree_view::{self, TreeEvent, TreeGroup, TreeProps};
use crate::ui::branches::fetch_scope;
use crate::ui::branches_tree::build_branch_view;
use crate::ui::commit_menu::{CommitFacts, CommitMenuAction, commit_menu};
use crate::ui::components;
use crate::ui::icons::{self, Icon};
use crate::ui::widgets::{self, BadgeKind, MenuItemKind, MenuItemProps, RefKind, menu_item};
use chrono::{DateTime, Local, TimeZone, Utc};
use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Frame, Galley, Grid, Layout, Margin, Panel,
    Popup, PopupKind, Pos2, Rect, Response, RichText, ScrollArea, Sense, Ui, UiBuilder, Vec2,
    WidgetInfo, WidgetType,
};
use std::path::PathBuf;
use turbogit_app::root_caches::{LogScope, file_stat};
use turbogit_app::state::{
    AppState, BlameTarget, Dialog, DiffTarget, NewBranchBase, PendingConfirm, Toast,
};
use turbogit_domain::model::{
    BranchKind, ChangeStatus, Commit, CommitId, DateFormat, GitRefKind, RefState, Root, RootId,
    SignatureState,
};
use turbogit_services::sync_service;

// --- Pane metrics (spec §8.3) -------------------------------------------------

/// Branches pane width.
const BRANCHES_WIDTH: f32 = 210.0;
/// Right column (changed files + details) width. Widened from the §8.3 320px
/// for the redesigned two-line file rows (decision D3): name, directory and
/// the `+N −M` column need the room.
const FILES_WIDTH: f32 = 344.0;
/// Commit details pane height. Grew from the §8.3 200px in issue 15 for the
/// Actions section, to 340px in issue 17 for the committer row and the
/// Copy-hash header, and to 440px for the redesigned blocks (decision D4):
/// subject, hash chip, author card, the committer / date / parents grid, the
/// churn summary, and the message body.
///
/// The actions, the header's copy button and the guardrail alert that D4 was
/// sized around are gone (ADR-0024), so the pane now has slack at its foot. The
/// height is left where it is rather than re-fitted: it is a design value with
/// pixel snapshots standing behind it, not a consequence of the removal, and
/// re-fitting it is a decision of its own. The short-window yield below still
/// applies.
const DETAILS_HEIGHT: f32 = 440.0;
/// Changed-file row height (redesign issue 04): a name line with its directory
/// underneath, so two lines where [`crate::theme::FILE_ROW_HEIGHT`] is one.
/// Deliberately local rather than a third ramp step — this is the log table's
/// own two-line row, and two-line rows must not retarget the graph's geometry.
const LOG_FILE_ROW_HEIGHT: f32 = 40.0;
/// Gap between the two churn numbers at the right edge of a file row.
const STAT_GAP: f32 = 6.0;
/// Root stripe width on multi-root rows.
const STRIPE_WIDTH: f32 = 3.0;
/// Uppercase micro text (§3.3) — shared control role (T2).
const MICRO_TEXT: f32 = crate::theme::TYPE_CONTROL;
/// Mono cell font size — shared body role (T2).
const MONO_TEXT: f32 = crate::theme::TYPE_BODY;

/// The log file-row status pill: the shared chip's height and text inset, at
/// the compact control radius rather than the full pill radius. This site has
/// always rounded at CONTROL_RADIUS; changing that is a design change, not a
/// refactor, so the consolidation must preserve it. Naming the geometry here
/// (rather than switching to `CHIP_GEOMETRY`) is what lets the row drop its
/// hand-rolled `rect_filled` + two-axis centring for
/// [`widgets::ChipGeometry::paint`] without moving a pixel.
const STATUS_PILL: widgets::ChipGeometry = widgets::ChipGeometry {
    height: widgets::CHIP_HEIGHT,
    pad_x: widgets::CHIP_PAD_X,
    radius: crate::theme::CONTROL_RADIUS as f32,
};

/// Commit-table column x-offsets, measured from `content_left` (the row left
/// edge, plus the root stripe in multi-root views). The micro column headers
/// and the row cells share these offsets so they stay vertically aligned.
/// `COL_GRAPH` is the graph node's center x (no header is drawn for it); the
/// remaining columns left-align with their cell text, except the date, which
/// trails the row right-aligned to `rect.right() - DATE_RIGHT_PAD`.
const COL_GRAPH: f32 = 10.0;
const COL_HASH: f32 = 26.0;
const COL_AUTHOR: f32 = 84.0;
/// Message column left edge — the wide column, sitting where the date used to.
const COL_MESSAGE: f32 = 164.0;
/// Gap between the right-aligned date and the row's trailing edge.
const DATE_RIGHT_PAD: f32 = 8.0;
/// Space held back from the message for a label pill (icon + padding + gap)
/// so it never runs under the right-aligned date on decorated rows.
const PILL_RESERVE: f32 = 30.0;

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
    // Ref decorations load off the render thread (log-open perf, D1): kick
    // the worker per root while its decorations aren't in yet; the filled
    // cache arrives with the `RefsLoaded` event and the view renders the
    // empty-first log meanwhile. The one-per-root in-flight guard inside
    // `fetch_refs` keeps this from stacking workers frame after frame.
    for id in visible_root_ids(state) {
        if !state.caches.refs_loaded(&id) {
            state.fetch_refs(id);
        }
    }
    if let (Some(root), Some(cid)) = (
        state.selected_root.clone(),
        state.ui.selected_commit.clone(),
    ) {
        state.ensure_files(&root, &cid);
        // Per-file line counts (redesign issue 02) load off the render thread:
        // the in-flight guard makes a repeat ask a no-op, so rows render
        // without numbers for the frames a request is open rather than the
        // pane waiting on git.
        state.fetch_file_stats(root, cid);
    }
    // Path-scoped history (issue #19): the scoped listing fills through the
    // same cache read as the other two scopes.
    if let (Some(root), Some(path)) = (state.selected_root.clone(), state.ui.log_path_scope.clone())
    {
        state.ensure_log(&root, LogScope::Path(path));
    }
    // Ref-scoped history (branch-tree extraction, plan D9).
    if let Some((root, ref_name)) = state.ui.log_ref_scope.clone() {
        state.ensure_log(&root, LogScope::Ref(ref_name));
    }
    // Code-change search (issue 17): a non-empty search box also fills the
    // pickaxe cache per visible root — `git log -S` covers commits whose
    // content changed the query string's count, which client-side filtering
    // of message/hash/author cannot see.
    let query = state.ui.log_filter.trim().to_string();
    if !query.is_empty() {
        for id in visible_root_ids(state) {
            state.ensure_log(&id, LogScope::Search(query.clone()));
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
    // ... and they start collapsed: the remote-header rows show, their
    // branches hide until a group is expanded.

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
    // This pane shows remotes for every repository alike.
    let view = build_branch_view(&roots, &tags_by_root, &|_| state.ui.log_tree.show_remotes);

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
        allows_context_menu: false,
        id_salt: "log_branch_tree",
        full_height: false,
        collapse_remotes_by_default: true,
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
        TreeEvent::RemoteRevealToggled { .. } => {
            // The pane forces remotes on every frame, so no repository here is
            // ever rolled up and nothing reveals.
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
                    .small_button(RichText::new("×").size(crate::theme::TYPE_DETAIL_TITLE))
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
                    .small_button(RichText::new("×").size(crate::theme::TYPE_DETAIL_TITLE))
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
    // cached window says history continues past it — and never in a scoped
    // view, whose listing is fetched uncapped. The click is deferred like the row
    // selections: `commits` borrows the caches until rendering ends.
    let may_have_more = state.ui.log_path_scope.is_none()
        && state.ui.log_ref_scope.is_none()
        && visible_root_ids(state)
            .iter()
            .any(|id| state.caches.log_has_more(id));
    let mut load_more = false;

    // Root-stripe legend chip row (11px INK_3) for multi-root setups.
    if multi_root {
        ui.horizontal_wrapped(|ui| {
            for (idx, root) in state.multi.roots.iter().enumerate() {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(10.0, 10.0), Sense::hover());
                ui.painter().rect_filled(
                    rect,
                    CornerRadius::same(crate::theme::MARK_RADIUS),
                    root_color(idx),
                );
                ui.label(
                    RichText::new(root.id.name())
                        .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                        .color(Palette::INK_3),
                );
            }
        });
    }

    // Column micro-headers aligned with the cells below.
    header_cells(ui, multi_root);

    // Row clicks are deferred (plan §1.3): the displayed union borrows the
    // cache slices, so rows render against a shared AppState and the
    // selection lands after the scroll pass ends. A press carries its own
    // root, because the listing is a union across every visible root and a
    // commit id alone does not say which repository it belongs to.
    let mut clicked: Option<(RootId, String)> = None;
    let mut opened: Option<(RootId, String)> = None;
    // The height the list is given, captured before it takes it: egui 0.36
    // reports only the content's own rect back out, and an overflowing list
    // fills exactly this.
    let viewport_height = ui.available_rect_before_wrap().height();
    let scrolled = ScrollArea::vertical().show(ui, |ui| {
        for c in &commits {
            match commit_row(ui, state, c, &colors, date_mode, multi_root) {
                RowIntent::None => {}
                RowIntent::Select => {
                    if clicked.is_none() {
                        clicked = Some((c.root.clone(), c.id.clone()));
                    }
                }
                RowIntent::SelectAndOpenMenu => {
                    if clicked.is_none() {
                        clicked = Some((c.root.clone(), c.id.clone()));
                        opened = Some((c.root.clone(), c.id.clone()));
                    }
                }
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

    // Auto-load-more (P6): the trigger is a settled bottom on an overflowing
    // list, not a scroll gesture — so the wheel, the scrollbar and the
    // keyboard all arrive here, and a list that fits its viewport never fires.
    // `state.offset.y` counts down the list, so it is the scrolled distance.
    if may_have_more
        && settled_at_bottom(
            scrolled.state.offset.y,
            viewport_height,
            scrolled.content_size.y,
            crate::theme::FILE_ROW_HEIGHT,
        )
    {
        load_more = true;
    }

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
    if let Some((root, id)) = clicked {
        // A press means this row's repository as well as this row's commit.
        select_commit_row(state, root, id);
        // A left click selects and closes any open menu: the press always does
        // what it looks like it does.
        state.ui.log_commit_menu = None;
    }
    if let Some(target) = opened {
        state.ui.log_commit_menu = Some(target);
    }

    commit_context_menu(ui, state);
}

/// What a press on a commit row means for the app: this row's repository, this
/// row's commit, and no changed file carried over from the commit before it.
///
/// The same shape as [`apply_log_tree_event`]'s row activation — set the
/// repository, clear the changed file — with one deliberate difference, and the
/// difference is the whole point of a commit row: a *branch* row is a scope
/// control, so it sets `log_ref_scope` and clears `selected_commit` because no
/// commit is what the developer asked to look at. A commit row names a commit,
/// so it sets `selected_commit` and leaves the scope alone — the developer asked
/// to see this commit, not to browse a different history. Both end with the pane
/// describing what was pressed.
///
/// `selected_root` is what every dialog the menu opens reads, and the listing is
/// a union across roots, so this is also what makes a dialog act on the
/// repository the row came from rather than on whichever one happened to be
/// selected when the row was painted.
fn select_commit_row(state: &mut AppState, root: RootId, id: String) {
    state.selected_root = Some(root);
    state.ui.selected_commit = Some(id);
    state.ui.log_selected_file = None;
}

/// What a press on a commit row asks for. Right-click selects **and** opens the
/// menu, so the menu and the details pane cannot describe different commits
/// (ADR-0024); there is deliberately no "menu without selection" intent. Both
/// intents are applied by [`select_commit_row`], so neither can end up naming a
/// different repository from the other.
enum RowIntent {
    None,
    Select,
    SelectAndOpenMenu,
}

/// The gates are evaluated per repository root, so a dirty root does not
/// disable an action on a clean one. Everything here is state the log already
/// holds; none of it costs a git call.
fn commit_facts<'a>(state: &'a AppState, root: &'a RootId, cid: &str) -> CommitFacts<'a> {
    let snapshot = state.multi.by_id(root);
    CommitFacts {
        dirty: snapshot.is_some_and(|r| !r.status.changes.is_empty()),
        protected_branch: snapshot
            .and_then(|r| r.current_branch.clone())
            .is_some_and(|b| sync_service::is_protected(&state.settings, &b)),
        // A plan is built from the commit to the current branch's tip, so a
        // commit outside this root's listing cannot be rewritten from it. A root
        // with no named branch checked out has no such history either, and the
        // item states that rather than looking broken.
        on_current_branch: snapshot.is_some_and(|r| r.current_branch.is_some())
            && state
                .caches
                .log(root)
                .is_some_and(|cs| cs.iter().any(|c| c.id == cid)),
        multi_root: state.multi.roots.len() > 1,
        repo_name: root_display_name(root),
    }
}

/// The repository's own name — its directory's, which is what the branches
/// pane's ROOTS filter and the shell already call it. Borrowed off the root id
/// so the menu can name the repository a commit belongs to without copying a
/// `String` per frame per row.
fn root_display_name(root: &RootId) -> &str {
    let path = root.as_path();
    path.file_name()
        .and_then(|name| name.to_str())
        .or_else(|| path.to_str())
        .unwrap_or_default()
}

/// The Git Log's commit context menu: the one place a commit action lives. The
/// host owns the lifecycle; this surface owns which row's menu is open and what
/// each pick means.
fn commit_context_menu(ui: &mut Ui, state: &mut AppState) {
    let Some((root_id, cid)) = state.ui.log_commit_menu.clone() else {
        return;
    };
    let target = widgets::menu_host::target_of(&root_id, &cid);
    let ctx = ui.ctx().clone();
    let mut dismiss = false;
    let picked = widgets::menu_host::host_menu(
        ui,
        widgets::menu_host::MenuId::new("log_commit", &target),
        true,
        &mut dismiss,
        |ui| {
            find_commit(state, &root_id, &cid)
                .cloned()
                .and_then(|commit| commit_menu(ui, &commit, &commit_facts(state, &root_id, &cid)))
        },
    );
    if dismiss {
        state.ui.log_commit_menu = None;
    }
    if let Some(action) = picked.flatten() {
        apply_commit_action(state, &ctx, &root_id, &cid, action);
    }
}

/// One dispatcher for all eleven items — the ruling ADR-0023 made for branches
/// applied to commits. It closes the menu first, so no action can be taken
/// against a menu the developer can no longer see.
fn apply_commit_action(
    state: &mut AppState,
    ctx: &egui::Context,
    root: &RootId,
    cid: &str,
    action: CommitMenuAction,
) {
    state.ui.log_commit_menu = None;
    match action {
        // Clipboard writes are not git work and never were an `Operation`; this
        // is the app's single clipboard call site and its toast convention, with
        // the text naming WHICH thing was copied so the two are distinguishable.
        CommitMenuAction::CopyHash => {
            ctx.copy_text(cid.to_owned());
            state.ui.toast_shown_at = None;
            state.ui.toast = Some(Toast::success(format!(
                "Copied {}",
                widgets::short_commit_ref(cid)
            )));
        }
        CommitMenuAction::CopyMessage => {
            let message = find_commit(state, root, cid)
                .map(|c| c.message.clone())
                .unwrap_or_default();
            ctx.copy_text(message);
            state.ui.toast_shown_at = None;
            state.ui.toast = Some(Toast::success("Copied commit message"));
        }
        CommitMenuAction::CherryPickTo => {
            state.ui.dlg.cherry_pick_commit = Some(cid.to_owned());
            state.ui.dialog = Some(Dialog::CherryPickTarget);
        }
        CommitMenuAction::CherryPickAcross => {
            state.open_cherry_across();
            state.ui.dialog = Some(Dialog::CherryPickAcross);
        }
        CommitMenuAction::RevertCommit => {
            state.ui.confirm = Some(PendingConfirm::RevertCommit {
                commit: cid.to_owned(),
            });
        }
        CommitMenuAction::NewBranch => {
            // The base is the commit that was right-clicked, not the tip — the
            // whole point of right-clicking a row in the middle of history — and
            // creating the branch does not move the checkout, which is why this
            // arm passes `false` where every other open path passes `true`.
            state.open_new_branch(NewBranchBase::Commit(cid.to_owned()), false, None);
        }
        CommitMenuAction::Checkout => {
            // The menu's gate reads the cached worktree; this is the last look
            // before git is touched. A tree that turned dirty in between goes to
            // the SAME bring-along / set-aside / cancel confirmation a branch
            // checkout raises, so the two checkouts behave identically.
            let dirty = state
                .multi
                .by_id(root)
                .is_some_and(|r| !r.status.changes.is_empty());
            if dirty {
                state.ui.confirm = Some(PendingConfirm::CheckoutDirty {
                    root: root.clone(),
                    target: cid.to_owned(),
                    kind: turbogit_domain::model::BranchKind::Local,
                    detach: true,
                });
            } else {
                state.checkout_detached_op(root, cid);
            }
        }
        CommitMenuAction::NewTag => {
            // One open path, shared with the VCS palette's Tag: the dialog is
            // seeded with the commit that was right-clicked, and everything the
            // last visit worked out for itself is cleared.
            state.open_tag_dialog(Some(cid));
        }
        CommitMenuAction::DropCommit => {
            // A rewrite is never one click: the preflight is built from the same
            // repository the menu's own root names, and carries the plan the
            // confirm dispatches.
            state.open_rewrite_preflight(root, cid, turbogit_app::state::HistoryVerb::Drop);
        }
        CommitMenuAction::CreatePatch => {
            // A file write, not git work: no Operation, no activity entry.
            state.create_patch(root, cid);
        }
        CommitMenuAction::RewordCommit => {
            // Two steps, deliberately: the message is written here, and the cost
            // of writing it is shown by the preflight this editor's confirm
            // opens. The editor is seeded from the commit's own message, so a
            // correction starts from what the commit actually says.
            state.open_reword_editor(root, cid);
        }
    }
}

fn body_font() -> FontId {
    crate::theme::chrome_font(crate::theme::TYPE_BODY)
}

fn mono_font() -> FontId {
    FontId::new(MONO_TEXT, FontFamily::Monospace)
}

fn row_ink(active: bool) -> Color32 {
    if active { Palette::INK } else { Palette::INK_2 }
}

fn allocate_row(ui: &mut Ui) -> (Rect, Response) {
    let width = ui.available_width();
    ui.allocate_exact_size(
        Vec2::new(width, crate::theme::FILE_ROW_HEIGHT),
        Sense::click(),
    )
}

/// One two-line changed-file row (redesign issue 04).
fn allocate_file_row(ui: &mut Ui) -> (Rect, Response) {
    let width = ui.available_width();
    ui.allocate_exact_size(Vec2::new(width, LOG_FILE_ROW_HEIGHT), Sense::click())
}

/// Lay `text` out no wider than `max_width`, dropping characters from the tail
/// and marking the cut with an ellipsis — a name is never trimmed in the
/// middle (redesign issue 04).
fn elide(
    painter: &egui::Painter,
    text: &str,
    font: FontId,
    color: Color32,
    max_width: f32,
) -> std::sync::Arc<Galley> {
    let full = painter.layout_no_wrap(text.to_owned(), font.clone(), color);
    if text.is_empty() || full.size().x <= max_width {
        return full;
    }
    // Start from a proportional guess and step down: this paints for every
    // row on every frame, so one layout per character is not affordable.
    let chars = text.chars().count();
    let guess = (chars as f32 * (max_width / full.size().x)) as usize;
    let mut keep = guess.saturating_sub(1).min(chars).max(1);
    loop {
        let cut: String = text.chars().take(keep).collect();
        let galley = painter.layout_no_wrap(format!("{cut}…"), font.clone(), color);
        if galley.size().x <= max_width || keep <= 1 {
            return galley;
        }
        keep -= 1;
    }
}

/// Row fill decision for the commit table: active rows keep the translucent
/// focus band, hovered rows take SURFACE_2, idle rows stay transparent. This is
/// the shared row-state API at the log's own full-row rect (conformance
/// issue 12); [`widgets::paint_row`] supplies the control radius, which is what
/// this row has always rounded at.
fn paint_row_fill(ui: &Ui, rect: &Rect, active: bool, hovered: bool) {
    widgets::paint_row(
        ui,
        *rect,
        if active {
            components::RowState::FocusSelected
        } else {
            components::RowState::from_flags(false, hovered)
        },
    );
}

/// Micro column headers above the commit table, aligned with the row cells.
/// Rows measure their content from `content_left` (the row left edge, plus
/// the root stripe when `multi_root`); the headers replicate that origin and
/// the shared [`COL_*`] offsets so header text sits directly over the cells.
fn header_cells(ui: &mut Ui, multi_root: bool) {
    let top = ui.cursor().top();
    ui.add_space(16.0);
    let left = ui.cursor().left();
    let content_left = left + if multi_root { STRIPE_WIDTH + 2.0 } else { 0.0 };
    let micro = FontId::new(MICRO_TEXT, FontFamily::Proportional);
    for (title, dx) in [
        ("HASH", COL_HASH),
        ("AUTHOR", COL_AUTHOR),
        ("MESSAGE", COL_MESSAGE),
    ] {
        let galley = ui
            .painter()
            .layout_no_wrap(title.to_owned(), micro.clone(), Palette::INK_3);
        ui.painter().galley(
            Pos2::new(content_left + dx, top + 2.0),
            galley,
            Palette::INK_3,
        );
    }
    // The date trails the row, so its header right-aligns to the same edge.
    let date = ui
        .painter()
        .layout_no_wrap("DATE".to_owned(), micro, Palette::INK_3);
    let right = left + ui.available_width();
    ui.painter().galley(
        Pos2::new(right - DATE_RIGHT_PAD - date.size().x, top + 2.0),
        date,
        Palette::INK_3,
    );
}

/// One commit-table row: stripe | node | hash | author | message(+chips) | date.
/// Renders against a shared [`AppState`] (the displayed union borrows the
/// caches) and reports what a press on it asked for; the caller applies the
/// selection after rendering (plan §1.3 defer pattern).
fn commit_row(
    ui: &mut Ui,
    state: &AppState,
    c: &Commit,
    colors: &std::collections::HashMap<String, usize>,
    date_mode: DateFormat,
    multi_root: bool,
) -> RowIntent {
    let selected = state.ui.selected_commit.as_deref() == Some(c.id.as_str());
    let (rect, response) = allocate_row(ui);

    // Translucent selection (SELECTION_BG) keeps lane colors readable (§7.2).
    if selected {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(crate::theme::CONTROL_RADIUS),
            Palette::selection_bg(),
        );
    } else if response.hovered() {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(crate::theme::CONTROL_RADIUS),
            Palette::SURFACE_2,
        );
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
    let center = Pos2::new(content_left + COL_GRAPH, rect.center().y);
    if c.parents.len() > 1 {
        ui.painter()
            .circle_stroke(center, 4.0, egui::Stroke::new(1.5, lane));
    } else {
        ui.painter().circle_filled(center, 4.0, lane);
    }

    // Hash | Author cells.
    let painter = ui.painter().clone();
    let cy = rect.center().y;
    let hash_galley = painter.layout_no_wrap(
        widgets::short_commit_ref(&c.id),
        mono_font(),
        Palette::BRAND,
    );
    painter.galley(
        Pos2::new(content_left + COL_HASH, cy - hash_galley.size().y / 2.0),
        hash_galley,
        Palette::BRAND,
    );
    let author_galley =
        painter.layout_no_wrap(truncate(&c.author.name, 10), body_font(), Palette::INK_2);
    painter.galley(
        Pos2::new(content_left + COL_AUTHOR, cy - author_galley.size().y / 2.0),
        author_galley,
        Palette::INK_2,
    );

    // Date cell: the trailing column, right-aligned to the row's edge.
    let date_galley =
        painter.layout_no_wrap(fmt_date(c.time, date_mode), body_font(), Palette::INK_3);
    let date_x = rect.right() - DATE_RIGHT_PAD - date_galley.size().x;
    painter.galley(
        Pos2::new(date_x, cy - date_galley.size().y / 2.0),
        date_galley,
        Palette::INK_3,
    );

    // Message cell with one collapsed label pill — the wide column, sitting
    // left of the date. Everything is painted directly (galleys + the pill,
    // no child widgets) so the row itself stays the only interactive surface;
    // a child `ui.label` here would sit on top of the row in hit-testing and
    // swallow its clicks.
    let labels = state.caches.refs_for(&c.root, &c.id);
    let message_left = content_left + COL_MESSAGE;
    let budget = (date_x - 8.0 - if labels.is_empty() { 0.0 } else { PILL_RESERVE } - message_left)
        .max(24.0);
    let subject = c.message.lines().next().unwrap_or("");
    let mut fit = truncate(subject, 44);
    let mut subject_galley = painter.layout_no_wrap(fit.clone(), body_font(), Palette::INK);
    while subject_galley.size().x > budget && fit.chars().count() > 1 {
        fit.pop();
        subject_galley = painter.layout_no_wrap(fit.clone(), body_font(), Palette::INK);
    }
    let subject_w = subject_galley.size().x;
    painter.galley(
        Pos2::new(message_left, cy - subject_galley.size().y / 2.0),
        subject_galley,
        Palette::INK,
    );
    let mx = message_left + subject_w + 6.0;

    // Refs collapse into one ".tg-label" pill, shown only when the commit
    // actually carries labels (branch / remote / tag); the individual names
    // are revealed in a hover tooltip so decorated rows stay scannable.
    if !labels.is_empty() {
        let pill = paint_label_pill(&painter, mx, cy);
        if ui.rect_contains_pointer(pill) {
            // The popup anchors in global space; the row lives inside the
            // graph's scroll area, so map the pill rect to screen first.
            let screen_pill = ui
                .ctx()
                .layer_transform_to_global(ui.layer_id())
                .map(|t| t * pill)
                .unwrap_or(pill);
            Popup::new(
                ui.id().with(("log_label_tooltip", &c.id)),
                ui.ctx().clone(),
                screen_pill,
                ui.layer_id(),
            )
            .kind(PopupKind::Tooltip)
            .gap(4.0)
            .interactable(false)
            .show(|ui| {
                for r in labels {
                    ui.label(
                        RichText::new(&r.name)
                            .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                            .color(ref_kind(r.kind).accent()),
                    );
                }
            });
        }
    }

    response.widget_info(|| {
        WidgetInfo::labeled(
            WidgetType::Button,
            true,
            format!("{} {}", widgets::short_commit_ref(&c.id), subject),
        )
    });
    widgets::focus_ring(ui, &response);
    if response.clicked() {
        RowIntent::Select
    } else if response.secondary_clicked() {
        // The anchor is the pointer position at the right-click, stashed in egui
        // memory for the host to read back at the end of this pane.
        let pos = ui
            .input(|i| i.pointer.interact_pos())
            .unwrap_or_else(|| rect.left_bottom());
        widgets::menu_host::note_anchor(
            ui,
            widgets::menu_host::MenuId::new(
                "log_commit",
                &widgets::menu_host::target_of(&c.root, &c.id),
            ),
            pos,
        );
        RowIntent::SelectAndOpenMenu
    } else {
        RowIntent::None
    }
}

/// Paint one `.tg-label` pill (18px, neutral token colors) holding a label
/// (tag) icon, and return its rect. The ref names live in the hover tooltip
/// (see `commit_row`). Painter-only: registers no widget.
///
/// Deliberately *not* on [`widgets::ChipGeometry::paint`], unlike the file-row
/// status pill: this pill carries no galley at all — it is a layout-level
/// `ui.label` drawn straight to the painter — so there is no laid-out text for
/// `paint` to place. It already sits on the shared chip tokens (height,
/// padding, [`widgets::chip_radius`], [`BadgeKind::Neutral`] colors), and its
/// centring is the icon rectangle arithmetic, which is a different calculation
/// from the two-axis text centring in [`widgets::paint_centered_text`].
fn paint_label_pill(painter: &egui::Painter, x: f32, cy: f32) -> Rect {
    const ICON_SIZE: f32 = 12.0;
    let colors = BadgeKind::Neutral.colors();
    let rect = Rect::from_min_size(
        Pos2::new(x, cy - widgets::CHIP_HEIGHT / 2.0),
        Vec2::new(ICON_SIZE + widgets::CHIP_PAD_X * 2.0, widgets::CHIP_HEIGHT),
    );
    painter.rect_filled(rect, widgets::chip_radius(), colors.bg);
    icons::paint_icon(
        painter,
        Pos2::new(
            rect.center().x - ICON_SIZE / 2.0,
            rect.center().y - ICON_SIZE / 2.0,
        ),
        ICON_SIZE,
        Icon::TAG,
        colors.fg,
    );
    rect
}

// --- Pane 3: changed files -----------------------------------------------------------

/// Deferred interaction from one changed-file row (plan §1.3): rows render
/// against a shared [`AppState`] — the selection and the cached file list are
/// borrowed — so mutations land after the pane finishes rendering.
enum FileAction {
    None,
    /// Row clicked: open its diff; the caller resolves root / commit / parent.
    OpenDiff(PathBuf),
    /// Footer link / menu item: blame the file at the selected commit.
    OpenBlame(PathBuf),
    /// Menu item: scope the whole workspace to this file's history.
    ScopeHistory(PathBuf),
    /// Row right-clicked: open the shared menu on this row.
    OpenMenu(PathBuf),
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
    // Line counts arrive one worker round-trip behind the file list (redesign
    // issue 02); until they land, the rows simply carry no numbers.
    let stats = selection
        .and_then(|(root, cid)| state.caches.file_stats_for(root, cid))
        .unwrap_or(&[]);

    // Header: the count is a chip of its own (redesign issue 04), not a suffix
    // on the title. Without a selection there is no count to state.
    ui.horizontal(|ui| {
        widgets::group_title(ui, "Changed files");
        if selection.is_some() {
            widgets::badge(ui, &files.len().to_string(), BadgeKind::Neutral);
        }
    });

    let Some((root_id, cid)) = selection else {
        ui.label(
            RichText::new("Select a commit to see its changed files.")
                .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                .color(Palette::INK_3),
        );
        return;
    };

    let parent = find_commit(state, root_id, cid).and_then(|c| c.parents.first().cloned());

    // Filter row (redesign issue 04): the pane's own narrowing of a long
    // change list, matched case-insensitively against the path.
    widgets::search_input(ui, "Filter changed files", &mut state.ui.log_file_filter);
    let filter = state.ui.log_file_filter.trim().to_lowercase();
    let row_shown = |change: &turbogit_domain::model::Change| {
        filter.is_empty()
            || change
                .path
                .to_string_lossy()
                .to_lowercase()
                .contains(&filter)
    };
    let shown = files.iter().filter(|c| row_shown(c)).count();

    let mut action = FileAction::None;
    ScrollArea::vertical().show(ui, |ui| {
        for ch in files {
            if !row_shown(ch) {
                continue;
            }
            let row_action = file_row(ui, state, root_id, ch, file_stat(stats, &ch.path));
            // The first row that reports an intent owns the frame: a later
            // non-clicked row must not clear it (only one row can be clicked
            // per frame, but the loop keeps painting after it).
            if matches!(action, FileAction::None) {
                action = row_action;
            }
        }
        if files.is_empty() {
            ui.label(
                RichText::new("No changed files.")
                    .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                    .color(Palette::INK_3),
            );
        } else if shown == 0 {
            // A filter that hides every row is its own state — never the
            // "this commit changed nothing" message (issue 02).
            ui.label(
                RichText::new("No file matches the filter.")
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
        FileAction::OpenMenu(path) => {
            state.ui.log_file_menu = Some((root_id.clone(), cid.to_owned(), path));
        }
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

    file_row_context_menu(ui, state);
}

/// The changed-file row's context menu, on the same host the commit menu and the
/// branches list use. It used to be the window's third menu dialect — stock egui
/// with raw buttons, no shared surface, and no way to show a blocked item with a
/// reason. Both verbs do exactly what they did before; the surface is shared now,
/// so the whole window answers "why can't I do this" the same way.
fn file_row_context_menu(ui: &mut Ui, state: &mut AppState) {
    let Some((root_id, cid, path)) = state.ui.log_file_menu.clone() else {
        return;
    };
    let target = widgets::menu_host::target_of(&root_id, &path.to_string_lossy());
    let mut dismiss = false;
    let picked = widgets::menu_host::host_menu(
        ui,
        widgets::menu_host::MenuId::new("log_file_row", &target),
        true,
        &mut dismiss,
        |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.set_min_width(240.0);
            // Scoping to a file whose history is already on screen is a no-op, so
            // the item stays rendered and says why rather than vanishing.
            let already_scoped = state.ui.log_path_scope.as_deref() == Some(path.as_path());
            let mut picked = None;
            if menu_item(
                ui,
                MenuItemProps {
                    icon: Icon::FILE_CODE,
                    label: "Show blame",
                    data: None,
                    shortcut: None,
                    enabled: true,
                    disabled_reason: None,
                    kind: MenuItemKind::Default,
                },
            )
            .clicked()
            {
                picked = Some(FileAction::OpenBlame(path.clone()));
            }
            let history = menu_item(
                ui,
                MenuItemProps {
                    icon: Icon::CLOCK,
                    label: "Show history for file…",
                    data: None,
                    shortcut: None,
                    enabled: !already_scoped,
                    disabled_reason: already_scoped
                        .then_some("already showing this file's history"),
                    kind: MenuItemKind::Default,
                },
            );
            if history.clicked() {
                picked = Some(FileAction::ScopeHistory(path.clone()));
            }
            picked
        },
    );
    if dismiss {
        state.ui.log_file_menu = None;
    }
    if let Some(action) = picked.flatten() {
        // The menu closes before its verb runs, exactly as the commit menu's does.
        state.ui.log_file_menu = None;
        apply_file_action(state, &root_id, &cid, action);
    }
}

/// Run one changed-file row's verb against the root and commit the menu was
/// opened on — not whatever is selected now.
fn apply_file_action(state: &mut AppState, root: &RootId, cid: &CommitId, action: FileAction) {
    match action {
        FileAction::OpenBlame(path) => {
            state.ui.blame = Some(BlameTarget {
                root: root.clone(),
                path,
                rev: cid.to_owned(),
            });
        }
        FileAction::ScopeHistory(path) => {
            state.ui.log_path_scope = Some(path);
            state.ui.selected_commit = None;
            state.ui.log_selected_file = None;
        }
        _ => {}
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

/// One changed-file row (redesign issue 04): status badge, file name on the
/// first line, its directory underneath, and the commit's `+N −M` for this
/// path at the right. Click opens the diff preview, context menu scopes the
/// workspace to the file's history (issue #19).
/// Renders against a shared [`AppState`] and reports its intent; the caller
/// applies it after rendering (plan §1.3 defer pattern).
fn file_row(
    ui: &mut Ui,
    state: &AppState,
    root: &RootId,
    change: &turbogit_domain::model::Change,
    stat: Option<(usize, usize)>,
) -> FileAction {
    let selected = state.ui.log_selected_file.as_ref() == Some(&change.path);
    let (rect, response) = allocate_file_row(ui);
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
        Pos2::new(mx, cy - STATUS_PILL.height / 2.0),
        STATUS_PILL.size(&badge_galley),
    );
    STATUS_PILL.paint(&painter, badge_rect, badge_galley, colors.bg, colors.fg);
    mx = badge_rect.right() + 6.0;

    // Right-aligned churn (redesign issue 04): each number paints only when
    // git counted it, so a pure addition shows no `−0`, and a row measured
    // nothing at all — a binary file, or counts still in flight — shows
    // neither.
    let added = stat.filter(|(ins, _)| *ins > 0).map(|(ins, _)| {
        painter.layout_no_wrap(format!("+{ins}"), mono_font(), Palette::STATE_SUCCESS)
    });
    let removed = stat.filter(|(_, dels)| *dels > 0).map(|(_, dels)| {
        painter.layout_no_wrap(format!("−{dels}"), mono_font(), Palette::STATE_ERROR)
    });
    let mut text_right = rect.right() - 4.0;
    for galley in added.iter().chain(removed.iter()) {
        text_right -= galley.size().x + STAT_GAP;
    }

    // Line 1: the file name, tail-elided only when the pane cannot fit it.
    let ink = row_ink(selected);
    let name = change
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| change.path.to_string_lossy().into_owned());
    let name_galley = elide(&painter, &name, body_font(), ink, text_right - mx);
    painter.galley(Pos2::new(mx, rect.top() + 7.0), name_galley, ink);

    // Line 2: the directory it lives in, de-emphasized. A file at the repo
    // root has none, so that row carries the name line alone.
    if let Some(dir) = change.path.parent().filter(|p| !p.as_os_str().is_empty()) {
        let dir_font = FontId::new(MICRO_TEXT, FontFamily::Proportional);
        let dir_text = dir.to_string_lossy().replace('\\', "/");
        let dir_galley = elide(
            &painter,
            &dir_text,
            dir_font,
            Palette::INK_3,
            text_right - mx,
        );
        painter.galley(Pos2::new(mx, rect.top() + 22.0), dir_galley, Palette::INK_3);
    }

    let mut churn_right = rect.right() - 4.0;
    if let Some(galley) = removed {
        churn_right -= galley.size().x;
        painter.galley(
            Pos2::new(churn_right, rect.top() + 7.0),
            galley,
            Palette::STATE_ERROR,
        );
        churn_right -= STAT_GAP;
    }
    if let Some(galley) = added {
        churn_right -= galley.size().x;
        painter.galley(
            Pos2::new(churn_right, rect.top() + 7.0),
            galley,
            Palette::STATE_SUCCESS,
        );
    }
    response.widget_info(|| {
        WidgetInfo::labeled(WidgetType::Button, true, change.path.display().to_string())
    });
    widgets::focus_ring(ui, &response);
    if response.clicked() {
        return FileAction::OpenDiff(change.path.clone());
    }
    // Right-click opens the shared menu (ticket 09): path-scoped file history
    // (issue #19) and blame (issue 18) are its two items.
    if response.secondary_clicked() {
        let pos = ui
            .input(|i| i.pointer.interact_pos())
            .unwrap_or_else(|| rect.left_bottom());
        let target = widgets::menu_host::target_of(root, &change.path.to_string_lossy());
        widgets::menu_host::note_anchor(
            ui,
            widgets::menu_host::MenuId::new("log_file_row", &target),
            pos,
        );
        return FileAction::OpenMenu(change.path.clone());
    }
    FileAction::None
}

// --- Pane 4: commit details -----------------------------------------------------------

/// The one interaction left in the details pane (plan §1.3: it renders against
/// the borrowed cached commit, so the mutation lands after rendering).
///
/// It used to carry `CherryPick`, `CherryPickAcross`, `Revert`, `NewBranchHere`
/// and `CopyHash`. Those are the actions, and ADR-0024 moved every one of them
/// into the commit's context menu, leaving the pane to say things about a commit
/// and do nothing to one. `SelectParent` stays: a parent hash link is navigation
/// inside the metadata, and no menu item replaces it.
enum DetailAction {
    None,
    /// Jump to a parent commit via its hash link (issue 17).
    SelectParent(CommitId),
}

fn details_pane(ui: &mut Ui, state: &mut AppState) {
    // Every interaction defers (plan §1.3): the pane borrows the cached
    // commit below, so clicks set `action` and mutations land at the end.
    let mut action = DetailAction::None;
    widgets::group_title(ui, "Commit details");
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

    // Subject first (redesign issue 05): the commit's identity, taking over
    // the top of the key-value wall it replaces.
    ui.label(
        RichText::new(commit.message.lines().next().unwrap_or_default())
            .font(widgets::bold_font_if_available(ui))
            .color(Palette::INK),
    );

    // Hash: still shown, no longer clickable. Copying it lives in the commit's
    // context menu, which states the same short reference on the item itself.
    ui.horizontal(|ui| {
        widgets::hash_chip(ui, &widgets::short_commit_ref(&commit.id), "");
    });

    // Author card: initials, name, email, and the day on the trailing edge.
    ui.horizontal(|ui| {
        widgets::avatar_initials(ui, &commit.author.name);
        ui.vertical(|ui| {
            ui.label(
                RichText::new(&commit.author.name)
                    .font(body_font())
                    .color(Palette::INK),
            );
            ui.label(micro_text(&commit.author.email));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(micro_text(
                fmt_time(commit.time)
                    .split(' ')
                    .next()
                    .unwrap_or_default()
                    .to_string(),
            ));
        });
    });

    // Meta grid on a quiet container: one shared label column keeps the values
    // aligned (issue 05). The committer keeps its signature suffix (issue 17)
    // and the parents stay links that jump the selection.
    let sig_suffix = match commit.signature {
        SignatureState::Unsigned => String::new(),
        SignatureState::Good => " · signed ✓".to_string(),
        SignatureState::Bad => " · signature BAD".to_string(),
        SignatureState::Unverified => " · signed (unverified)".to_string(),
    };
    ui.add_space(4.0);
    Frame::new()
        .fill(Palette::SURFACE_3)
        .corner_radius(CornerRadius::same(crate::theme::CONTROL_RADIUS))
        .inner_margin(Margin::same(8))
        .show(ui, |ui| {
            Grid::new("log_commit_meta")
                .num_columns(2)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    ui.label(micro_text("Committer"));
                    ui.label(
                        RichText::new(format!(
                            "{} <{}>{}",
                            commit.committer.name, commit.committer.email, sig_suffix
                        ))
                        .font(body_font())
                        .color(Palette::INK),
                    );
                    ui.end_row();
                    ui.label(micro_text("Date"));
                    ui.label(
                        RichText::new(fmt_time(commit.time))
                            .font(body_font())
                            .color(Palette::INK),
                    );
                    ui.end_row();
                    ui.label(micro_text("Parents"));
                    if commit.parents.is_empty() {
                        ui.label(RichText::new("—").font(body_font()).color(Palette::INK_3));
                    } else {
                        ui.horizontal(|ui| {
                            for p in &commit.parents {
                                if ui
                                    .link(
                                        RichText::new(widgets::short_commit_ref(p))
                                            .font(mono_font())
                                            .color(Palette::INK),
                                    )
                                    .clicked()
                                {
                                    action = DetailAction::SelectParent(p.clone());
                                }
                            }
                        });
                    }
                    ui.end_row();
                });
        });

    // Churn: the bar git measured, then the file count and the `+X −Y` totals.
    // Before the stats land only the file count is known, so that is all that
    // shows (issue 02's "no stat" rule, applied to the aggregate too).
    let files = state.caches.files_for(root_id, &commit.id).unwrap_or(&[]);
    let stats = state
        .caches
        .file_stats_for(root_id, &commit.id)
        .unwrap_or(&[]);
    let added: usize = stats.iter().filter_map(|f| f.insertions).sum();
    let removed: usize = stats.iter().filter_map(|f| f.deletions).sum();
    ui.add_space(6.0);
    widgets::churn_bar(ui, added, removed);
    ui.horizontal(|ui| {
        ui.label(micro_text(format!(
            "{} {} changed",
            files.len(),
            if files.len() == 1 { "file" } else { "files" }
        )));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if removed > 0 {
                ui.label(
                    RichText::new(format!("−{removed}"))
                        .font(mono_font())
                        .color(Palette::STATE_ERROR),
                );
            }
            if added > 0 {
                ui.label(
                    RichText::new(format!("+{added}"))
                        .font(mono_font())
                        .color(Palette::STATE_SUCCESS),
                );
            }
        });
    });

    // The message body: everything under the subject, which now leads the
    // pane (issue 05) — repeating it here would spend the pane's last inches
    // on a line the user has already read. This stays the pane's only
    // scrolling region, with the viewport capped to what is left so the
    // `ScrollArea` cannot claim the whole pane and clip the metadata above.
    let body: Vec<&str> = commit
        .message
        .lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .collect();
    if !body.is_empty() {
        let message_height = ui.available_height().max(40.0);
        ScrollArea::vertical()
            .max_height(message_height)
            .show(ui, |ui| {
                for line in body {
                    ui.label(RichText::new(line).font(body_font()).color(Palette::INK));
                }
            });
    }

    // Deferred action application (plan §1.3): the borrow of the cached
    // commit ended above, so `state.ui` is free to mutate here.
    if let DetailAction::SelectParent(parent) = action {
        state.ui.selected_commit = Some(parent);
        state.ui.log_selected_file = None;
    }
}

/// Muted micro label — the details pane's secondary text role (issue 05).
fn micro_text(text: impl Into<String>) -> RichText {
    RichText::new(text)
        .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
        .color(Palette::INK_3)
}

/// Whether a scrolled list sits settled at its bottom: `offset` pixels scrolled
/// from the top of a `content`-tall list inside a `viewport`-tall window, with
/// the last `tolerance` pixels counting as arrived.
///
/// Two rules make this the whole automatic trigger (plan P6): a list that does
/// not overflow its viewport is NEVER at the bottom — there is nothing to
/// scroll to, so a one-screen log never auto-pages — and the test is on where
/// the view rests, not on a scroll gesture, which is what lets the wheel, the
/// scrollbar and the keyboard all fire it.
pub fn settled_at_bottom(offset: f32, viewport: f32, content: f32, tolerance: f32) -> bool {
    content > viewport && offset + tolerance >= content - viewport
}

#[cfg(test)]
mod tests {
    use super::settled_at_bottom;

    const VIEWPORT: f32 = 300.0;
    const CONTENT: f32 = 1_000.0;
    /// The offset that puts the list's last pixel at the window's last pixel.
    const BOTTOM: f32 = CONTENT - VIEWPORT;
    const ROW: f32 = 24.0;

    /// A log that fits its pane has nothing left to scroll to, so it must not
    /// page itself — however far its rows sit from the bottom edge.
    #[test]
    fn a_list_that_fits_its_viewport_is_never_at_the_bottom() {
        assert!(!settled_at_bottom(0.0, VIEWPORT, 120.0, ROW));
        assert!(
            !settled_at_bottom(0.0, VIEWPORT, VIEWPORT, ROW),
            "an exact fit is still not an overflowing list"
        );
    }

    #[test]
    fn an_overflowing_list_arrives_within_one_row_of_its_end() {
        assert!(!settled_at_bottom(0.0, VIEWPORT, CONTENT, ROW));
        assert!(!settled_at_bottom(BOTTOM / 2.0, VIEWPORT, CONTENT, ROW));
        assert!(
            settled_at_bottom(BOTTOM - ROW, VIEWPORT, CONTENT, ROW),
            "one row short still counts as arrived"
        );
        assert!(settled_at_bottom(BOTTOM, VIEWPORT, CONTENT, ROW));
        assert!(
            !settled_at_bottom(BOTTOM - ROW - 1.0, VIEWPORT, CONTENT, ROW),
            "a row and a pixel short does not"
        );
    }
}
