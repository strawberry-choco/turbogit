//! The reusable branch tree component (branch-tree-view extraction, plan
//! D1/D4/D5/D6/D10).
//!
//! One shared painter for the repo-grouped branch tree, consumed by the
//! Branches tool window and (from plan step 4) the Git Log window's branches
//! pane. Its public surface is one function and two types:
//!
//! ```text
//! fn branch_tree(ui, props: &TreeProps, tree: &mut TreeState) -> Vec<TreeEvent>
//! ```
//!
//! Everything the tree needs arrives in [`TreeProps`] (plain data, borrows and
//! scalars, each frame); everything the tree remembers lives in
//! [`turbogit_app::state::TreeState`], owned by the caller and the only thing
//! the function mutates; everything the user does comes back as a
//! [`TreeEvent`]. The component takes no application state, makes no git
//! calls, and decides no policy — the caller applies the returned events
//! between painting the tree and painting any floating surface that depends
//! on them (the ⋯ overflow menu still opens in the frame it is clicked).
//!
//! The inline-rename draft is the one deliberate exception to "events out":
//! egui's text input needs mutable access to the string while rendering, so
//! the draft lives in `TreeState` and only commit/cancel are events.
//!
//! The component owns the vertical scroll area and the three non-populated
//! list states (reading, genuinely empty, search matched nothing). The
//! "working…" line, the delete-undo banner, the toolbar, the detail panel, the
//! keyboard path and the actions stay on the surface.

use std::collections::HashMap;

use egui::{
    Align, Color32, CornerRadius, Layout, Pos2, Rect, RichText, ScrollArea, Sense, Ui, UiBuilder,
    Vec2, WidgetInfo, WidgetType,
};
use turbogit_app::state::TreeState;
use turbogit_domain::model::{Branch, BranchKind, RootId};

use crate::theme::{
    Palette, TYPE_BODY, TYPE_CHIP, TYPE_CONTROL, TYPE_SECTION, chrome_font, data_font,
};
use crate::ui::branch_widget::stale_badge;
use crate::ui::branches::{branch_matches, matches_query, name_budget, row_meta, sync_chips};
use crate::ui::branches_tree::{
    self, BranchNode, BranchView, RemoteGroup, RepoSection, RepoStatus,
};
use crate::ui::components::{
    BRANCH_ROW_H, KIT_ICON, KitButton, PAD_LIST, RowState, SECTION_H, kit_button, middle_truncate,
    overflow_button, row_fill, row_ink, section_header, sync_bg, sync_ink,
};
use crate::ui::icons::{self, Icon};

/// Height of one repo section header (status dot + repo name + current chip).
const REPO_HEADER_H: f32 = 30.0;

/// Width of two whitespace characters in the data face — the per-level indent
/// for directory subgroups under "Local" (branch names are monospace, so the
/// hierarchy hangs two spaces per depth).
fn two_space_indent(ui: &Ui) -> f32 {
    ui.painter()
        .layout_no_wrap("  ".to_owned(), data_font(TYPE_BODY), Color32::WHITE)
        .size()
        .x
}

/// Width of one whitespace character in the data face — the inline gap between
/// a branch name and the current-branch marker after it.
fn space_width(ui: &Ui) -> f32 {
    ui.painter()
        .layout_no_wrap(" ".to_owned(), data_font(TYPE_BODY), Color32::WHITE)
        .size()
        .x
}

/// What the user did to the tree (plan D4). Plain data — every action target
/// arrives as `(owning repository, branch)` so no surface re-derives it and no
/// flat row aggregate is needed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeEvent {
    /// A row was clicked: selection policy is the caller's (the Branches
    /// window selects and toggles the detail; the Log window scopes the graph).
    RowClicked { root: RootId, branch: String },
    /// A row was double-clicked: activation (checkout in Branches, graph
    /// scope in the Log window).
    RowActivated { root: RootId, branch: String },
    /// The row's ⋯ overflow button toggled the menu for this branch.
    OverflowToggled { root: RootId, branch: String },
    /// A Local/Tags group header was clicked.
    GroupToggled(TreeGroup),
    /// A remote group header was clicked: collapse/expand that remote.
    RemoteToggled { root: RootId, remote: String },
    /// The collapsed Remote rollup (or a header switch) set remotes visibility.
    RemotesVisibleChanged { visible: bool },
    /// A repository header's Fetch button was clicked.
    FetchRequested { root: RootId },
    /// The inline rename started from a row action.
    RenameStarted { root: RootId, branch: String },
    /// The inline rename editor confirmed; `new` is the trimmed draft.
    RenameCommitted {
        root: RootId,
        old: String,
        new: String,
    },
    /// The inline rename editor was cancelled.
    RenameCancelled,
    /// Create a branch — from the empty state (`name` empty: "the first
    /// branch") or the no-match state (`name` = the typed query).
    CreateBranchRequested { name: String },
    /// The no-match state asked to clear the search.
    ClearFilterRequested,
}

/// Which expandable group header was toggled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeGroup {
    Local,
    Tags,
}

/// Everything the tree is told, each frame (plan D3). Borrows and scalars —
/// no application state, no policy.
pub struct TreeProps<'a> {
    /// The repo-grouped view model (pure builder output).
    pub view: &'a BranchView,
    /// The live search text (owned by the surface).
    pub filter: &'a str,
    /// The repo scope narrowing, `None` = every in-scope repo.
    pub repo_filter: Option<&'a RootId>,
    /// The warm tag cache, keyed by repository (plan D10: one shared warm
    /// cache reaches the tree as a prop; each section already carries its
    /// list from the view model). Entries carry their decoration state.
    pub tags_by_root: &'a HashMap<RootId, Vec<(String, turbogit_domain::model::RefState)>>,
    /// More than one repository in scope (scope wording, identity chrome).
    pub multi_repo: bool,
    /// An operation is in flight (the reading state).
    pub busy: bool,
    /// Any in-scope root has branch data or a HEAD — `false` shows the
    /// reading/empty states instead of the list.
    pub has_any_data: bool,
    /// A merge is in progress (the current branch's row reads "merging…").
    pub merge_in_progress: bool,
    /// The last fetch timestamp (remote-group freshness hint).
    pub last_fetch: Option<chrono::DateTime<chrono::Utc>>,
    /// The caller-supplied "now" — painted staleness/freshness stays
    /// deterministic in tests.
    pub now: chrono::DateTime<chrono::Utc>,
    /// Capability: this instance allows inline rename.
    pub allows_rename: bool,
    /// Capability: rows reveal per-row action buttons on hover.
    pub shows_row_actions: bool,
    /// egui id salt for the scroll area — unique per surface.
    pub id_salt: &'a str,
    /// Whether the scroll area claims all remaining height (`true` for the
    /// Branches tool window's full-height list) or shrinks to its content
    /// (`false` for the Log pane, which paints its Roots filter below).
    pub full_height: bool,
    /// Per-remote collapse default: `false` starts every remote group expanded
    /// (the Branches window), `true` starts them collapsed (the Log pane). A
    /// user toggle still flips the effective state either way.
    pub collapse_remotes_by_default: bool,
}

/// Paint the branch tree and report what the user did.
///
/// Renders the repo-grouped view model: per-repo sections (header, Local
/// group with directory subgroups, Remote area — expanded groups or the
/// collapsed rollup — and a Tags group), filtered live by `props.filter`, or
/// the reading/empty/no-match states instead of a blank panel. Mutates only
/// `tree` (scroll, the rename draft, the filter-transition bookkeeping) and
/// returns every policy-level action as events.
pub fn branch_tree(ui: &mut Ui, props: &TreeProps<'_>, tree: &mut TreeState) -> Vec<TreeEvent> {
    let mut events = Vec::new();

    // No branch data anywhere and no HEAD: reading or the one-sentence empty
    // state (the tree never paints a blank panel).
    if !props.has_any_data {
        if props.busy {
            reading_state(ui);
        } else {
            empty_state(ui, &mut events);
        }
        return events;
    }

    let filter = props.filter.to_string();
    let filtering = !filter.trim().is_empty();

    // Search is for jumping, not browsing: save the scroll position the
    // moment filtering begins and restore it when it clears. The transition
    // is detected against the previous frame's filter (plan D3's
    // `previous_filter`), so the tree owns the save/restore itself.
    let was_filtering = !tree.previous_filter.trim().is_empty();
    if filtering && !was_filtering {
        tree.scroll_saved = Some(tree.scroll);
    }
    if !filtering && was_filtering {
        tree.scroll = tree.scroll_saved.take().unwrap_or(0.0);
    }
    tree.previous_filter = filter.clone();
    let scroll = tree.scroll;

    let scroll_target = tree.scroll_to.clone();
    let mut target_y: Option<f32> = None;
    let mut y_cursor = 0.0;

    // Only the in-scope repos paint: the scope chip narrows the tree to one
    // repo (issue 04), and a lone in-scope repo renders its section invisibly
    // (issue 02 — the grouping adds no chrome).
    let in_scope: Vec<&RepoSection> = props
        .view
        .repos
        .iter()
        .filter(|s| match props.repo_filter {
            Some(f) => &s.root_id == f,
            None => true,
        })
        .collect();
    let invisible = in_scope.len() == 1;

    let out = ScrollArea::vertical()
        .id_salt(props.id_salt)
        .auto_shrink([false, !props.full_height])
        .vertical_scroll_offset(scroll)
        .show(ui, |ui| {
            // A dead end turns into the likely next intent.
            if filtering
                && in_scope
                    .iter()
                    .all(|s| section_match_count(s, &filter, tree.show_remotes) == 0)
            {
                no_match_state(ui, filter.trim(), &mut events);
                return;
            }
            for section in &in_scope {
                paint_repo_section(
                    ui,
                    props,
                    tree,
                    section,
                    &filter,
                    invisible,
                    &scroll_target,
                    &mut y_cursor,
                    &mut target_y,
                    &mut events,
                );
            }
        });
    tree.scroll = out.state.offset.y;
    // Scroll the freshly created branch into view (issue 08) and consume the
    // intent.
    if tree.scroll_to.take().is_some()
        && let Some(ty) = target_y
    {
        tree.scroll = ty;
    }

    events
}

/// Total visible branches/tags in a section under the current filter — used to
/// decide the no-match state and to size group headers.
fn section_match_count(section: &RepoSection, filter: &str, show_remotes: bool) -> usize {
    let locals = leaf_count(&filter_nodes(&section.locals, filter));
    let remotes = if show_remotes {
        section
            .remote_groups
            .iter()
            .map(|g| leaf_count(&filter_nodes(&g.children, filter)))
            .sum()
    } else {
        0
    };
    let tags = section
        .tags
        .iter()
        .filter(|t| matches_query(&t.name, filter))
        .count();
    locals + remotes + tags
}

/// Recursively keep nodes whose branch (or descendant) matches the query,
/// preserving the directory structure. Counts on surviving directory nodes are
/// recomputed from the surviving children.
fn filter_nodes(nodes: &[BranchNode], filter: &str) -> Vec<BranchNode> {
    if filter.trim().is_empty() {
        return nodes.to_vec();
    }
    let mut out = Vec::new();
    for n in nodes {
        match n {
            BranchNode::Leaf(l) => {
                if branch_matches(&l.branch, filter) {
                    out.push(n.clone());
                }
            }
            BranchNode::Dir(d) => {
                let children = filter_nodes(&d.children, filter);
                if !children.is_empty() {
                    out.push(BranchNode::Dir(crate::ui::branches_tree::DirNode {
                        label: d.label.clone(),
                        count: branches_tree::leaf_count(&children),
                        children,
                    }));
                }
            }
        }
    }
    out
}

/// Count the leaf branches beneath a node forest.
fn leaf_count(nodes: &[BranchNode]) -> usize {
    branches_tree::leaf_count(nodes)
}

/// Paint one repo's section: header (unless invisible for single-repo), Local
/// group, Remote area, and Tags group.
#[allow(clippy::too_many_arguments)]
fn paint_repo_section(
    ui: &mut Ui,
    props: &TreeProps<'_>,
    tree: &mut TreeState,
    section: &RepoSection,
    filter: &str,
    invisible: bool,
    scroll_target: &Option<String>,
    y_cursor: &mut f32,
    target_y: &mut Option<f32>,
    events: &mut Vec<TreeEvent>,
) {
    // The header always paints so the repo-level Fetch stays one click away
    // (issue 03); its identity chrome is suppressed for a single-repo project
    // whose grouping must stay invisible (issue 02).
    repo_header(ui, section, !invisible, events);
    *y_cursor += REPO_HEADER_H;

    // --- Local group ---
    let locals = filter_nodes(&section.locals, filter);
    let local_count = leaf_count(&locals);
    let local_header = section_header(ui, "Local", local_count, tree.groups.local, |_| {});
    if local_header.clicked() {
        events.push(TreeEvent::GroupToggled(TreeGroup::Local));
    }
    *y_cursor += SECTION_H;
    if tree.groups.local {
        paint_nodes(
            ui,
            props,
            tree,
            section,
            &locals,
            0,
            scroll_target,
            y_cursor,
            target_y,
            events,
        );
    }

    // --- Remote area ---
    if tree.show_remotes {
        for rg in &section.remote_groups {
            let children = filter_nodes(&rg.children, filter);
            if leaf_count(&children) == 0 && !filter.trim().is_empty() {
                continue;
            }
            remote_group_header(ui, props, tree, section, rg, events);
            *y_cursor += SECTION_H;
            if !remote_collapsed(tree, props, &section.root_id, &rg.remote) {
                paint_nodes(
                    ui,
                    props,
                    tree,
                    section,
                    &children,
                    1,
                    scroll_target,
                    y_cursor,
                    target_y,
                    events,
                );
            }
        }
    } else {
        // Collapsed rollup row — one per repo, toggles remotes on when clicked.
        remote_rollup_row(ui, tree, section, events);
        *y_cursor += BRANCH_ROW_H;
    }

    // --- Tags group ---
    let mut tags: Vec<crate::ui::branches_tree::TagLeaf> = section
        .tags
        .iter()
        .filter(|t| matches_query(&t.name, filter))
        .cloned()
        .collect();
    tags.sort_by(|a, b| a.name.cmp(&b.name));
    let tags_header = section_header(ui, "Tags", tags.len(), tree.groups.tags, |_| {});
    if tags_header.clicked() {
        events.push(TreeEvent::GroupToggled(TreeGroup::Tags));
    }
    *y_cursor += SECTION_H;
    if tree.groups.tags {
        for t in &tags {
            if Some(t.name.as_str()) == scroll_target.as_deref() {
                *target_y = Some(*y_cursor);
            }
            *y_cursor += BRANCH_ROW_H;
            tag_row(ui, t);
        }
    }
}

/// Paint a forest of branch nodes (local or remote) with directory headers at
/// `depth` indentation.
#[allow(clippy::too_many_arguments)]
fn paint_nodes(
    ui: &mut Ui,
    props: &TreeProps<'_>,
    tree: &mut TreeState,
    section: &RepoSection,
    nodes: &[BranchNode],
    depth: usize,
    scroll_target: &Option<String>,
    y_cursor: &mut f32,
    target_y: &mut Option<f32>,
    events: &mut Vec<TreeEvent>,
) {
    let step = two_space_indent(ui);
    for n in nodes {
        match n {
            BranchNode::Leaf(l) => {
                if Some(l.branch.name.as_str()) == scroll_target.as_deref() {
                    *target_y = Some(*y_cursor);
                }
                *y_cursor += BRANCH_ROW_H;
                branch_row(
                    ui,
                    props,
                    tree,
                    section,
                    &l.label,
                    &l.branch,
                    depth as f32 * step,
                    events,
                );
            }
            BranchNode::Dir(d) => {
                dir_header(ui, &d.label, d.count, depth, step);
                *y_cursor += BRANCH_ROW_H;
                paint_nodes(
                    ui,
                    props,
                    tree,
                    section,
                    &d.children,
                    depth + 1,
                    scroll_target,
                    y_cursor,
                    target_y,
                    events,
                );
            }
        }
    }
}

/// One repo section header: a status dot, the repo name (sans), a repo-level
/// Fetch (issue 03 — one click away even with remotes hidden), and a chip
/// showing the repo's current branch (mono). `show_identity` is false for a
/// single-repo project, where the grouping is invisible: the identity chrome
/// (dot, name, chip) is suppressed so no redundant repo header appears, but the
/// Fetch control stays so primary actions remain one click away.
fn repo_header(
    ui: &mut Ui,
    section: &RepoSection,
    show_identity: bool,
    events: &mut Vec<TreeEvent>,
) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, REPO_HEADER_H), Sense::hover());
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.add_space(PAD_LIST);
    if show_identity {
        // Status dot.
        let (dot_rect, _) = child.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
        child
            .painter()
            .circle_filled(dot_rect.center(), 4.0, repo_status_color(section.status));
        child.add_space(8.0);
        // Repo name (UI sans, section-ish weight).
        child.add(
            egui::Label::new(
                RichText::new(section.repo_name.clone())
                    .font(chrome_font(TYPE_CONTROL))
                    .color(Palette::T_PRIMARY),
            )
            .truncate(),
        );
    }
    // Right-aligned cluster: current-branch chip (rightmost), then Fetch.
    child.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.add_space(PAD_LIST);
        if show_identity && let Some(cur) = &section.current_branch {
            let _ = egui::Frame::new()
                .fill(Palette::selection_bg())
                .corner_radius(Palette::RADIUS_CHIP)
                .inner_margin(egui::Margin::symmetric(6, 2))
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(cur.clone())
                                .font(data_font(TYPE_CONTROL))
                                .color(Palette::STATE_INFO),
                        )
                        .truncate(),
                    );
                });
        }
        ui.add_space(PAD_LIST);
        // Repo-level Fetch (issue 03): one click away even with remotes hidden,
        // because the old Remote group header is gone.
        if kit_button(ui, KitButton::Quiet, "Fetch").clicked() {
            events.push(TreeEvent::FetchRequested {
                root: section.root_id.clone(),
            });
        }
    });
}

/// Map a repo's derived status to its dot color (dark-only palette): clean =
/// success green, unpushed = soft blue, unpulled/dirty = amber.
fn repo_status_color(status: RepoStatus) -> Color32 {
    match status {
        RepoStatus::Clean => Palette::STATE_SUCCESS,
        RepoStatus::Unpushed => Palette::STATE_INFO,
        RepoStatus::Unpulled | RepoStatus::Dirty => Palette::STATE_WARNING,
    }
}

/// One directory group header inside a repo section: the directory segment
/// (sans) and its branch count, indented by nesting depth.
fn dir_header(ui: &mut Ui, label: &str, count: usize, depth: usize, step: f32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, BRANCH_ROW_H), Sense::hover());
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.add_space(PAD_LIST + depth as f32 * step);
    child.add(
        egui::Label::new(
            RichText::new(format!("{label}/"))
                .font(chrome_font(TYPE_CONTROL))
                .color(Palette::T_SECONDARY),
        )
        .truncate(),
    );
    child.add_space(6.0);
    child.add(egui::Label::new(
        RichText::new(count.to_string())
            .font(chrome_font(TYPE_CONTROL))
            .color(Palette::T_MUTED),
    ));
}

/// Whether a remote group is effectively collapsed. The per-surface default
/// (`collapse_remotes_by_default`) flips the meaning of
/// `TreeState::collapsed_remotes` membership: with the default expanded, a
/// member is collapsed; with the default collapsed, a member is expanded.
/// Either way the user toggle flips the effective state.
fn remote_collapsed(tree: &TreeState, props: &TreeProps<'_>, root: &RootId, remote: &str) -> bool {
    let user_collapsed = tree
        .collapsed_remotes
        .contains(&(root.clone(), remote.to_owned()));
    if props.collapse_remotes_by_default {
        !user_collapsed
    } else {
        user_collapsed
    }
}

/// One expanded remote group header: the remote name, its branch count, and a
/// "fetched Nm ago" freshness hint from the last-fetch timestamp. Clicking the
/// header collapses that remote's group.
fn remote_group_header(
    ui: &mut Ui,
    props: &TreeProps<'_>,
    tree: &mut TreeState,
    section: &RepoSection,
    rg: &RemoteGroup,
    events: &mut Vec<TreeEvent>,
) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, SECTION_H), Sense::hover());
    let id = ui.auto_id_with(("remote_group", &section.root_id, &rg.remote));
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &rg.remote));

    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.add_space(PAD_LIST);
    let collapsed = remote_collapsed(tree, props, &section.root_id, &rg.remote);
    let chevron = if collapsed {
        Icon::CHEVRON_RIGHT
    } else {
        Icon::CHEVRON_DOWN
    };
    icons::icon(&mut child, chevron, KIT_ICON, Palette::T_MUTED);
    child.add_space(6.0);
    // A remote's name is data, like a branch name (spec §19) — monospace, so
    // `origin` and `upstream-fork` line up the same way branch paths do.
    child.add(
        egui::Label::new(
            RichText::new(rg.remote.clone())
                .font(data_font(TYPE_SECTION))
                .color(Palette::T_SECONDARY),
        )
        .truncate(),
    );
    // Branch count.
    child.add_space(6.0);
    child.add(egui::Label::new(
        RichText::new(rg.count.to_string())
            .font(chrome_font(TYPE_CONTROL))
            .color(Palette::T_MUTED),
    ));
    if response.clicked() {
        events.push(TreeEvent::RemoteToggled {
            root: section.root_id.clone(),
            remote: rg.remote.clone(),
        });
    }

    // Freshness hint: the last fetch, reused from the single view-wide
    // timestamp (spec: "fetched Nm ago").
    if let Some(last) = props.last_fetch {
        let age = props
            .now
            .signed_duration_since(last)
            .to_std()
            .unwrap_or_default();
        child.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(PAD_LIST);
            ui.add(egui::Label::new(
                RichText::new(format!("fetched {}", stale_badge(age)))
                    .font(data_font(TYPE_CONTROL))
                    .color(Palette::T_MUTED),
            ));
        });
    }
}

/// The collapsed per-repo "Remote" rollup row: a chevron, the word "Remote",
/// and a count like "3 remotes · 41 branches". Clicking it reveals the remote
/// groups for the whole view (issue 03).
fn remote_rollup_row(
    ui: &mut Ui,
    tree: &mut TreeState,
    section: &RepoSection,
    events: &mut Vec<TreeEvent>,
) {
    let width = ui.available_width();
    // Hover-only allocation; the `interact` below owns the click target. (An
    // extra `Sense::click` on the allocation would register a second, silently
    // discarded widget that wins the press and swallows the click.)
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, BRANCH_ROW_H), Sense::hover());
    let id = ui.auto_id_with(("remote_rollup", &section.root_id));
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Remote"));

    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.add_space(PAD_LIST);
    let chevron = if tree.show_remotes {
        Icon::CHEVRON_DOWN
    } else {
        Icon::CHEVRON_RIGHT
    };
    icons::icon(&mut child, chevron, KIT_ICON, Palette::T_MUTED);
    child.add_space(6.0);
    child.add(
        egui::Label::new(
            RichText::new("Remote")
                .font(chrome_font(TYPE_SECTION))
                .color(Palette::T_SECONDARY),
        )
        .truncate(),
    );
    // The derived rollup count — never a separate estimate. Labels and counts
    // are chrome, so it reads in the UI sans (spec §19): only the branch and
    // remote *names* are monospace.
    if let Some(rollup) = section.remote_rollup() {
        child.add_space(8.0);
        child.add(
            egui::Label::new(
                RichText::new(rollup)
                    .font(chrome_font(TYPE_CONTROL))
                    .color(Palette::T_MUTED),
            )
            .truncate(),
        );
    }
    if response.clicked() {
        events.push(TreeEvent::RemotesVisibleChanged { visible: true });
    }
}

/// One 30px branch row: current marker, middle-truncated mono name, upstream,
/// and sync chips (icon+count / in-sync / gone). Hover and selection fills
/// come from the §14.1 row states; clicking reports [`TreeEvent::RowClicked`]
/// (never checks out — the caller decides).
#[allow(clippy::too_many_arguments)]
fn branch_row(
    ui: &mut Ui,
    props: &TreeProps<'_>,
    tree: &mut TreeState,
    section: &RepoSection,
    label: &str,
    branch: &Branch,
    indent: f32,
    events: &mut Vec<TreeEvent>,
) {
    let id = &section.root_id;
    // Issue 11: the branch being renamed edits inline on its own row. Rename
    // is a local-branch verb, scoped to the owning repo.
    if props.allows_rename
        && branch.kind == BranchKind::Local
        && tree.selected_root.as_ref() == Some(id)
        && tree.renaming.as_deref() == Some(branch.name.as_str())
    {
        rename_editor(ui, tree, section, branch, events);
        return;
    }

    let now = props.now;
    let meta = row_meta(branch, now);
    let is_current = section.current_branch.as_deref() == Some(branch.name.as_str());
    let selected = tree.selected_root.as_ref() == Some(id)
        && tree.selected.as_deref() == Some(branch.name.as_str());
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
        // Paint on the row's own layer, before the row content: a dedicated
        // `Order::Background` layer renders *above* the scroll content in the
        // final pass, so a fill parked there would bury the branch name. Same
        // layer + insertion order keeps the fill behind the row's own text.
        ui.painter().rect_filled(rect, CornerRadius::same(3), fill);
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
    let scope = if props.multi_repo {
        format!(" in {}", section.repo_name)
    } else {
        String::new()
    };
    let mut activated = false;
    if hovered && props.shows_row_actions {
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
                events.push(TreeEvent::OverflowToggled {
                    root: section.root_id.clone(),
                    branch: branch.name.clone(),
                });
            }
            if kit_button(ui, KitButton::Quiet, &format!("Checkout{scope}")).clicked() {
                activated = true;
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
            // Mid-operation state is first-class on the row (issue 09): a
            // merge in progress reads "merging…" and survives tab switches.
            if is_current && props.merge_in_progress {
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
            // A remote-tracking ref deleted upstream reads "gone" (issue 17,
            // carried over from the Log pane's decoration states via the
            // branch snapshot the caller assembles).
            if branch.kind == BranchKind::Remote && branch.gone {
                ui.label(
                    RichText::new("gone")
                        .font(chrome_font(TYPE_CHIP))
                        .color(Palette::STATE_ERROR),
                );
            }
        });
    }

    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.add_space(PAD_LIST + indent);
    // Remote rows are reference material (issue 13): quieter, labelled with
    // their remote's name, and never carrying the current marker.
    let is_remote = branch.kind == BranchKind::Remote;
    let budget = name_budget(rect.width() - PAD_LIST - KIT_ICON - 6.0 - 130.0);
    // Display the leaf's own label: remote leaves carry the prefix-stripped
    // name because their group header already names the remote (issue 03), so
    // "origin/" is never re-printed here.
    let label = middle_truncate(label, budget);
    let ink = if is_remote {
        // Remote rows are reference material: quiet by design.
        Palette::T_SECONDARY
    } else if is_current {
        // Active/current branch reads in soft blue (issue 04).
        Palette::STATE_INFO
    } else if branch.ahead > 0 && branch.behind > 0 {
        // Diverged from upstream: red (issue 04).
        Palette::STATUS_DIVERGED
    } else if branch.ahead > 0 || branch.behind > 0 {
        // Ahead (unpushed) or behind (unpulled): amber (issue 04).
        Palette::STATE_WARNING
    } else {
        // Plain local branch: stale-aware primary (preserves §14.1 dimming).
        row_ink(meta.stale)
    };
    // No repo prefix on the row: the section header directly above already
    // names the repo, so repeating it on every row under that header is noise
    // (and it would push the branch name past its truncation budget). Rows stay
    // distinguishable because each one lives inside exactly one repo's section.
    child.add(
        egui::Label::new(RichText::new(label).font(data_font(TYPE_BODY)).color(ink)).truncate(),
    );
    // The current-branch marker reads inline right after the name, one
    // whitespace apart — not a leading column, and not right-aligned.
    if is_current && !is_remote {
        child.add_space(space_width(ui));
        icons::icon(&mut child, Icon::GIT_BRANCH, KIT_ICON, Palette::AHEAD);
    }
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

    if activated {
        events.push(TreeEvent::RowActivated {
            root: section.root_id.clone(),
            branch: branch.name.clone(),
        });
    } else if response.clicked() {
        events.push(TreeEvent::RowClicked {
            root: section.root_id.clone(),
            branch: branch.name.clone(),
        });
    }
    if response.double_clicked() {
        // Double-click activates — selecting and switching stay separate
        // intents, and double-click is the explicit switch gesture.
        events.push(TreeEvent::RowActivated {
            root: section.root_id.clone(),
            branch: branch.name.clone(),
        });
    }
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &branch.name));
}

/// Inline rename editor on the row (issue 11): a draft input plus the
/// upstream-follows disclosure, with confirm/cancel. Renaming the current
/// branch does not touch the working tree (git's `branch -m` moves the ref
/// and HEAD follows); the renamed branch re-sorts under the current ordering
/// after the refresh. The draft lives in `TreeState` — the one deliberate
/// exception to events-out — and only commit/cancel are reported.
fn rename_editor(
    ui: &mut Ui,
    tree: &mut TreeState,
    section: &RepoSection,
    branch: &Branch,
    events: &mut Vec<TreeEvent>,
) {
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
        let input = widgets::text_input(ui, &branch.name, &mut tree.rename_draft);
        input.request_focus();
        if kit_button(ui, KitButton::Secondary, "Apply rename").clicked() || enter {
            events.push(TreeEvent::RenameCommitted {
                root: section.root_id.clone(),
                old: branch.name.clone(),
                new: tree.rename_draft.trim().to_string(),
            });
        }
        if kit_button(ui, KitButton::Quiet, "Cancel").clicked() {
            events.push(TreeEvent::RenameCancelled);
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

/// One tinted sync chip: 10px icon + count (or the "gone" label) on the §13
/// meaning-color tint. 18px tall, radius 3, mono data type.
fn sync_chip(ui: &mut Ui, kind: crate::ui::components::SyncKind, label: &str) {
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
        crate::ui::components::SyncKind::Ahead => Some(Icon::ARROW_UP),
        crate::ui::components::SyncKind::Behind => Some(Icon::ARROW_DOWN),
        crate::ui::components::SyncKind::InSync => Some(Icon::CHECK),
        crate::ui::components::SyncKind::Gone => Some(Icon::ALERT_TRIANGLE),
        crate::ui::components::SyncKind::Diverged => None,
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
/// them visually quieter; today they read at secondary) plus the tag's
/// per-ref marker — `pushed` / `local only` / `gone` — when a decoration
/// state exists (plan D8). The warm Branches cache is all-Default, so its
/// rows paint unchanged.
fn tag_row(ui: &mut Ui, tag: &crate::ui::branches_tree::TagLeaf) {
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
            RichText::new(tag.name.clone())
                .font(data_font(TYPE_BODY))
                .color(Palette::T_SECONDARY),
        )
        .truncate(),
    );
    if let Some((text, color)) = ref_state_marker(tag.state) {
        child.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(PAD_LIST);
            ui.label(
                RichText::new(text)
                    .font(chrome_font(TYPE_CHIP))
                    .color(color),
            );
        });
    }
}

/// The painted `(text, ink)` for a decoration state, or `None` for the plain
/// default. Shared by tag rows and remote rows (the gone marker).
fn ref_state_marker(state: turbogit_domain::model::RefState) -> Option<(&'static str, Color32)> {
    use turbogit_domain::model::RefState;
    match state {
        RefState::Default => None,
        RefState::Gone => Some(("gone", Palette::STATE_ERROR)),
        RefState::Pushed => Some(("pushed", Palette::STATE_SUCCESS)),
        RefState::LocalOnly => Some(("local only", Palette::STATE_WARNING)),
    }
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
fn no_match_state(ui: &mut Ui, query: &str, events: &mut Vec<TreeEvent>) {
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
            events.push(TreeEvent::CreateBranchRequested {
                name: query.to_string(),
            });
        }
    });
}

/// A repo with no branches at all: one sentence + one create action, and no
/// empty group headers (design doc §2/§15).
fn empty_state(ui: &mut Ui, events: &mut Vec<TreeEvent>) {
    ui.vertical_centered(|ui| {
        ui.add_space(56.0);
        ui.label(
            RichText::new("This repo has no branches yet")
                .font(data_font(TYPE_BODY))
                .color(Palette::T_PRIMARY),
        );
        ui.add_space(8.0);
        if kit_button(ui, KitButton::Primary, "Create the first branch").clicked() {
            events.push(TreeEvent::CreateBranchRequested {
                name: String::new(),
            });
        }
    });
}

use crate::ui::widgets;

/// One local-branch reference for the keyboard path (plan D6): the owning
/// repository, the branch, its repo's current branch (for the marker), and the
/// repo's display name (deterministic tie-break across repos).
#[derive(Clone, Copy, Debug)]
pub struct LocalRow<'a> {
    pub root: &'a RootId,
    pub branch: &'a Branch,
    pub current: Option<&'a str>,
    pub repo: &'a str,
}

/// Local-branch ordering for the keyboard path, as a pure function over the
/// view model (plan D6): each repo's current branch first, then most recent
/// tip, ties alphabetical by repo then branch name. Both surfaces order rows
/// identically through this function.
pub fn keyboard_order<'a>(rows: &mut [LocalRow<'a>]) {
    rows.sort_by(|a, b| {
        let ac = Some(a.branch.name.as_str()) == a.current;
        let bc = Some(b.branch.name.as_str()) == b.current;
        bc.cmp(&ac)
            .then_with(|| b.branch.last_touched.cmp(&a.branch.last_touched))
            .then_with(|| a.repo.cmp(b.repo))
            .then_with(|| a.branch.name.cmp(&b.branch.name))
    });
}

/// Warm the per-root tag cache for every in-scope root (plan D10): one shared
/// step both tree surfaces invoke before rendering, keyed by repository, so
/// whichever tool window opens first warms the cache for both.
pub fn warm_tags(state: &mut turbogit_app::state::AppState) {
    for r in &state.multi.roots {
        if !state.ui.branches_tags.contains_key(&r.id) {
            // The bare tag read carries no decoration states; a surface that
            // holds real ref decorations (the Log window) upgrades them.
            let tags = state
                .executor
                .tag_list(&r.id.0)
                .unwrap_or_default()
                .into_iter()
                .map(|name| (name, turbogit_domain::model::RefState::Default))
                .collect();
            state.ui.branches_tags.insert(r.id.clone(), tags);
        }
    }
}
