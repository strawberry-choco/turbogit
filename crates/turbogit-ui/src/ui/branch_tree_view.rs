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
    Palette, TYPE_BODY, TYPE_CHIP, TYPE_CONTROL, TYPE_SECTION, chrome_font, data_font, indent_step,
};
use crate::ui::branch_widget::stale_badge;
use crate::ui::branches::{branch_matches, matches_query, row_meta};
use crate::ui::branches_tree::{
    self, BranchNode, BranchView, RemoteGroup, RepoSection, RepoStatus,
};
use crate::ui::components::{
    BRANCH_ROW_H, CLICK_TARGET_MIN, KIT_ICON, KitButton, PAD_LIST, PillKind, RowState, SECTION_H,
    current_row_fill, kit_button, middle_truncate_to_width, overflow_button, pill, pill_width,
    row_fill, row_ink, section_header, sync_badge, sync_bg, sync_ink,
};
use crate::ui::icons::{self, Icon};

/// Height of one repo section header (status dot + repo name + current chip).
const REPO_HEADER_H: f32 = 30.0;

/// Share of a branch row's content width held by the icon + name zone; the
/// tracking branch starts on the other side of that line. A fraction of the
/// row — not a pixels-per-character reserve — so the tracking column keeps one
/// left edge whatever the names do.
const NAME_ZONE_SHARE: f32 = 0.5;

/// Share of a repo header held by its identity cluster (dot, name, current
/// branch); the status summary and Fetch take the rest.
const HEADER_IDENTITY_SHARE: f32 = 0.45;

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
    /// One repository's collapsed REMOTE rollup was clicked, or its REMOTE
    /// header used to hide them again. Root-bearing: revealing one repo's
    /// remotes never reveals another's.
    RemoteRevealToggled { root: RootId, revealed: bool },
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
                && in_scope.iter().all(|s| {
                    section_match_count(s, &filter, remotes_revealed(tree, &s.root_id)) == 0
                })
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
/// decide the no-match state and to size group headers. `remotes` is this
/// section's own reveal, so a hidden remote branch cannot suppress the
/// no-match state.
fn section_match_count(section: &RepoSection, filter: &str, remotes: bool) -> usize {
    let locals = leaf_count(&filter_nodes(&section.locals, filter));
    let remotes = if remotes {
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
    if remotes_revealed(tree, &section.root_id) {
        // A repo revealed from its own rollup gets one header that puts it back.
        // The view-wide surface (`show_remotes`) has no such control and paints
        // no such row, so its rendering is unchanged.
        if !tree.show_remotes {
            let header = section_header(ui, "Remote", section.remote_branch_count, true, |_| {});
            if header.clicked() {
                events.push(TreeEvent::RemoteRevealToggled {
                    root: section.root_id.clone(),
                    revealed: false,
                });
            }
            *y_cursor += SECTION_H;
        }
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
    let step = indent_step(ui);
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
    // A repository reads as a block, and its header is the one strip that says
    // so. SURFACE, so the band is neither a row's hover fill nor a current row's
    // brand tint.
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, Palette::SURFACE);
    // Identity on the left, summary and Fetch on the right. Without an explicit
    // split a truncating repo name consumes the whole strip.
    let split_x = rect.left() + rect.width() * HEADER_IDENTITY_SHARE;
    let pill_w = section
        .current_branch
        .as_deref()
        .map(|cur| pill_width(ui, cur, PillKind::Current) + 8.0)
        .unwrap_or(0.0);
    let mut left = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_max(
                Pos2::new(rect.left(), rect.top()),
                Pos2::new(split_x, rect.bottom()),
            ))
            .layout(Layout::left_to_right(Align::Center)),
    );
    left.spacing_mut().item_spacing.x = 0.0;
    left.add_space(PAD_LIST);
    if show_identity {
        // Status dot.
        let (dot_rect, _) = left.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
        left.painter()
            .circle_filled(dot_rect.center(), 4.0, repo_status_color(section.status));
        left.add_space(8.0);
        // Repo name (UI sans), fitted to what is left of its current-branch
        // pill so neither can be pushed off the line.
        let name_w = (split_x - (dot_rect.right() + 16.0 + pill_w) - PAD_LIST).max(24.0);
        let name = middle_truncate_to_width(
            &left,
            &section.repo_name,
            &chrome_font(TYPE_CONTROL),
            name_w,
        );
        left.add(
            egui::Label::new(
                RichText::new(name)
                    .font(chrome_font(TYPE_CONTROL))
                    .color(Palette::T_PRIMARY),
            )
            .selectable(false),
        );
        // The current branch is part of the repo's identity, so it rides the
        // name rather than trailing the strip.
        if let Some(cur) = &section.current_branch {
            left.add_space(8.0);
            pill(&mut left, cur, PillKind::Current);
        }
    }

    // Right cluster: the repo's Fetch, with the status words in front of it.
    let mut right = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_max(
                Pos2::new(split_x, rect.top()),
                Pos2::new(rect.right(), rect.bottom()),
            ))
            .layout(Layout::right_to_left(Align::Center)),
    );
    right.spacing_mut().item_spacing.x = 0.0;
    right.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.add_space(PAD_LIST);
        // Repo-level Fetch (issue 03): one click away even with remotes hidden,
        // because the old Remote group header is gone.
        if kit_button(ui, KitButton::Quiet, "Fetch").clicked() {
            events.push(TreeEvent::FetchRequested {
                root: section.root_id.clone(),
            });
        }
        if show_identity {
            ui.add_space(PAD_LIST);
            ui.add(
                egui::Label::new(
                    RichText::new(header_summary(section))
                        .font(chrome_font(TYPE_CONTROL))
                        .color(section.status.color()),
                )
                .selectable(false)
                .truncate(),
            );
        }
    });
}

/// The header's status in words: the current branch's upstream counts, then the
/// repository's state — both read off the section the builder already derived,
/// so the numbers and the dot can never disagree.
fn header_summary(section: &RepoSection) -> String {
    let mut parts: Vec<String> = sync_badge(section.ahead, section.behind, false)
        .into_iter()
        .map(|(_, words)| words)
        .collect();
    parts.push(section.status.words().to_string());
    parts.join(" · ")
}

/// Shared semantic colors, identical to the sidebar and sync badges.
fn repo_status_color(status: RepoStatus) -> Color32 {
    status.color()
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
    child.spacing_mut().item_spacing.x = 0.0;
    child.add_space(PAD_LIST + depth as f32 * step);
    // A subgroup is scaffolding with a name, so it shows the folder it stands
    // for before the name.
    icons::icon(&mut child, Icon::FOLDER, KIT_ICON, Palette::T_MUTED);
    child.add_space(6.0);
    // The segment is data, like a branch name (spec §19) — and a larger step
    // than a section label's, so the two cannot be confused at a glance.
    child.add(
        egui::Label::new(
            RichText::new(format!("{label}/"))
                .font(data_font(TYPE_CONTROL))
                .color(Palette::T_SECONDARY),
        )
        .selectable(false),
    );
    child.add_space(8.0);
    child.add(egui::Label::new(
        RichText::new(count.to_string())
            .font(chrome_font(TYPE_CONTROL))
            .color(Palette::T_MUTED),
    ));
}

/// Whether one repository shows its remote groups: the view-wide switch (the Git
/// Log pane forces it) or that repository's own reveal from the rollup.
pub fn remotes_revealed(tree: &TreeState, root: &RootId) -> bool {
    tree.show_remotes || tree.remotes_revealed.contains(root)
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

/// The collapsed per-repo "REMOTE" rollup row: a chevron, the section-cased
/// label, and a count like "3 remotes · 41 branches". Clicking it reveals the
/// remote groups for the whole view (issue 03).
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
    let chevron = if remotes_revealed(tree, &section.root_id) {
        Icon::CHEVRON_DOWN
    } else {
        Icon::CHEVRON_RIGHT
    };
    icons::icon(&mut child, chevron, KIT_ICON, Palette::T_MUTED);
    child.add_space(6.0);
    child.add(
        egui::Label::new(
            RichText::new("REMOTE")
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
        events.push(TreeEvent::RemoteRevealToggled {
            root: section.root_id.clone(),
            revealed: true,
        });
    }
}

/// One 30px branch row, laid out as four measured zones — icon + name, tracking
/// branch, status badges, ⋯ overflow — each painted at an explicit x so the
/// columns line up down the list. Its fill comes from [`row_fill`], or from
/// [`current_row_fill`] when it carries the current branch, which also takes the
/// `current` badge. Clicking reports [`TreeEvent::RowClicked`] (never checks out —
/// the caller decides).
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
    // Remote rows are reference material (issue 13): quieter, labelled with
    // their remote's name, and never carrying the current marker.
    let is_remote = branch.kind == BranchKind::Remote;
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
    let fill = if is_current && !is_remote {
        current_row_fill(row_state)
    } else {
        row_fill(row_state)
    };
    if fill != Color32::TRANSPARENT {
        // Paint on the row's own layer, before the row content: a dedicated
        // `Order::Background` layer renders *above* the scroll content in the
        // final pass, so a fill parked there would bury the branch name. Same
        // layer + insertion order keeps the fill behind the row's own text.
        ui.painter().rect_filled(rect, CornerRadius::same(3), fill);
    }

    // --- The row's four zones, laid out once over the full rect ----------------
    // icon + name | tracking branch | status badges | overflow. Every boundary
    // comes from measuring what actually paints there, so nothing drifts with
    // the length of a branch name.
    let chips = &meta.badge;
    let overflow_id = egui::Id::new(("branches_overflow_anchor", id, &branch.name));
    // The action names its repo's scope in multi-repo projects so a bare
    // "Checkout" never applies to an ambiguous repo (issue 14).
    let scope = if props.multi_repo {
        format!(" in {}", section.repo_name)
    } else {
        String::new()
    };

    let name_x = rect.left() + PAD_LIST + indent;
    let text_x = name_x + KIT_ICON + 6.0;
    let shows_pill = is_current && !is_remote;
    let pill_w = if shows_pill {
        pill_width(ui, "current", PillKind::Current)
    } else {
        0.0
    };
    // The ⋯ column belongs to every row at rest, so its space is reserved
    // whether or not the pointer is on the row.
    let shows_overflow = props.shows_row_actions;
    let overflow_x = rect.right() - PAD_LIST - CLICK_TARGET_MIN;
    let content_right = if shows_overflow {
        overflow_x - 8.0
    } else {
        rect.right() - PAD_LIST
    };
    let mut badges_w: f32 = chips
        .iter()
        .map(|(_kind, label)| sync_chip_width(ui, label) + 4.0)
        .sum();

    // The name/tracking split reads off the row's own width and nothing else —
    // not this row's badges, not its current marker — which is what gives the
    // tracking column one left edge down the whole list.
    let share_x = text_x + (content_right - 8.0 - text_x).max(0.0) * NAME_ZONE_SHARE;
    let name_w = (share_x - 8.0 - text_x).max(0.0);

    // Space is then yielded in a stated order: the ⋯ column and the `current`
    // badge never give way and the name keeps its share; the tracking line
    // shrinks to whatever lies between the name zone and the group, and the
    // status badges give up entirely before they would cross into the name zone.
    // In the ~210px Git Log pane this ordering is the difference between marking
    // the current branch and not marking it.
    let name_right = share_x - 8.0;
    let shows_badges = name_right - badges_w - pill_w >= text_x;
    if !shows_badges {
        badges_w = 0.0;
    }
    // Right to left: ⋯, then the current badge, then the status badges.
    let pill_x = content_right - pill_w;
    let badges_right = pill_x - if shows_pill { 6.0 } else { 0.0 };
    let badges_x = badges_right - badges_w;
    let track_x = share_x;
    let track_w = (badges_x - 8.0 - share_x).max(0.0);

    let mut activated = false;
    if shows_overflow {
        let overflow_rect = Rect::from_min_max(
            Pos2::new(overflow_x, rect.top()),
            Pos2::new(overflow_x + CLICK_TARGET_MIN, rect.bottom()),
        );
        ui.new_child(
            UiBuilder::new()
                .max_rect(overflow_rect)
                .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
        )
        .with_layout(
            Layout::centered_and_justified(egui::Direction::LeftToRight),
            |ui| {
                let more = overflow_button(ui, "More actions");
                ui.ctx()
                    .memory_mut(|m| m.data.insert_temp(overflow_id, more.rect));
                if more.clicked() {
                    events.push(TreeEvent::OverflowToggled {
                        root: section.root_id.clone(),
                        branch: branch.name.clone(),
                    });
                }
            },
        );
    }

    // The badges zone. Hover reveals the row's Checkout button, which takes the
    // same right-anchored slot the badges rest in.
    let actions_rect = Rect::from_min_max(
        Pos2::new(rect.left(), rect.top()),
        Pos2::new(badges_right, rect.bottom()),
    );
    let mut zone = ui.new_child(
        UiBuilder::new()
            .max_rect(actions_rect)
            .layout(Layout::right_to_left(Align::Center)),
    );
    zone.spacing_mut().item_spacing.x = 0.0;
    if hovered && shows_overflow {
        zone.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if kit_button(ui, KitButton::Quiet, &format!("Checkout{scope}")).clicked() {
                activated = true;
            }
        });
    } else {
        zone.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // Mid-operation state is first-class on the row (issue 09): a
            // merge in progress reads "merging…" and survives tab switches.
            if is_current && props.merge_in_progress {
                ui.add_space(8.0);
                ui.label(
                    RichText::new(crate::ui::components::mid_op_label("merging"))
                        .font(data_font(TYPE_CONTROL))
                        .color(Palette::STATE_WARNING),
                );
            }
            if shows_badges {
                for (kind, label) in chips.iter().rev() {
                    sync_chip(ui, *kind, label);
                    ui.add_space(4.0);
                }
            }
            // A remote-tracking ref deleted upstream reads "gone" (issue 17,
            // carried over from the Log pane's decoration states via the
            // branch snapshot the caller assembles).
            if branch.kind == BranchKind::Remote && branch.gone {
                ui.add_space(8.0);
                ui.label(
                    RichText::new("gone")
                        .font(chrome_font(TYPE_CHIP))
                        .color(Palette::STATE_ERROR),
                );
            }
        });
    }

    // The tracking zone: its own left edge, its own truncation budget, and no
    // zone at all when the row is too narrow to hold one.
    if let Some(up) = &meta.upstream
        && track_w > 8.0
    {
        let track_rect = Rect::from_min_max(
            Pos2::new(track_x, rect.top()),
            Pos2::new(track_x + track_w, rect.bottom()),
        );
        ui.new_child(
            UiBuilder::new()
                .max_rect(track_rect)
                .layout(Layout::left_to_right(Align::Center)),
        )
        .add(
            egui::Label::new(
                RichText::new(up.clone())
                    .font(data_font(TYPE_CONTROL))
                    .color(Palette::T_MUTED),
            )
            .truncate()
            .selectable(false),
        );
    }

    // The name zone: a leading icon so the column has a fixed left edge, then
    // the label fitted to what is actually left of the tracking zone.
    let ink = if is_remote {
        // Remote rows are reference material: quiet by design.
        Palette::T_SECONDARY
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
    // Display the leaf's own label: remote leaves carry the prefix-stripped
    // name because their group header already names the remote (issue 03), so
    // "origin/" is never re-printed here.
    let name = middle_truncate_to_width(ui, label, &data_font(TYPE_BODY), name_w);
    let name_rect = Rect::from_min_max(
        Pos2::new(name_x, rect.top()),
        Pos2::new(share_x - 8.0, rect.bottom()),
    );
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(name_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    // The zone's gaps are stated below, so egui's own inter-item spacing must
    // not add an unstated one on top of them.
    child.spacing_mut().item_spacing.x = 0.0;
    icons::icon(&mut child, Icon::GIT_BRANCH, KIT_ICON, ink);
    child.add_space(6.0);
    child.add(
        egui::Label::new(RichText::new(name).font(data_font(TYPE_BODY)).color(ink))
            .selectable(false),
    );
    // The `current` badge takes the slot the group maths reserved for it, so it
    // lines up down the list and is never the thing a narrow row squeezes out.
    if shows_pill {
        let pill_rect = Rect::from_min_max(
            Pos2::new(pill_x, rect.top()),
            Pos2::new(pill_x + pill_w, rect.bottom()),
        );
        ui.new_child(
            UiBuilder::new()
                .max_rect(pill_rect)
                .layout(Layout::left_to_right(Align::Center)),
        )
        .with_layout(Layout::left_to_right(Align::Center), |ui| {
            pill(ui, "current", PillKind::Current);
        });
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

/// How wide [`sync_chip`] will paint one badge — the row measures its badge
/// zone with this before painting anything. Every kind carries an icon, so only
/// the label's measured width varies.
fn sync_chip_width(ui: &Ui, label: &str) -> f32 {
    let font = data_font(TYPE_CHIP);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, Color32::WHITE);
    let icon_s = 10.0;
    let pad = 5.0;
    pad * 2.0 + icon_s + 2.0 + galley.size().x
}

/// One tinted sync chip: 10px icon + words (issue 03) on the §13 meaning-color
/// tint. 18px tall, radius 3, mono data type.
fn sync_chip(ui: &mut Ui, kind: crate::ui::components::SyncKind, label: &str) {
    let fg = sync_ink(kind);
    let bg = sync_bg(kind);
    let font = data_font(TYPE_CHIP);
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, fg);
    let icon_s = 10.0;
    let pad = 5.0;
    let w = sync_chip_width(ui, label);
    let h = 18.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(Palette::RADIUS_CHIP), bg);

    let icon = match kind {
        crate::ui::components::SyncKind::Ahead => Icon::ARROW_UP,
        crate::ui::components::SyncKind::Behind => Icon::ARROW_DOWN,
        crate::ui::components::SyncKind::InSync => Icon::CHECK,
        crate::ui::components::SyncKind::Gone => Icon::ALERT_TRIANGLE,
        crate::ui::components::SyncKind::Diverged => Icon::ARROW_RIGHT_LEFT,
    };
    let mut x = rect.left() + pad;
    icons::paint_icon(
        ui.painter(),
        Pos2::new(x, rect.center().y - icon_s / 2.0),
        icon_s,
        icon,
        fg,
    );
    x += icon_s + 2.0;
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
