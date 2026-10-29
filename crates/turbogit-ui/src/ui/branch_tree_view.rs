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
//! calls, and decides no policy, the caller applies the returned events
//! between painting the tree and painting any floating surface that depends
//! on them (the right-click context menu still opens in the frame it was
//! requested).
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
    Align, Color32, CornerRadius, Layout, Pos2, Rect, Response, RichText, ScrollArea, Sense, Ui,
    UiBuilder, Vec2, WidgetInfo, WidgetType,
};
use turbogit_app::state::TreeState;
use turbogit_domain::model::{Branch, BranchKind, RootId};

use crate::theme::{
    Palette, TYPE_BODY, TYPE_CHIP, TYPE_CONTROL, TYPE_SECTION, chrome_font, data_font,
};
use crate::ui::branch_widget::stale_badge;
use crate::ui::branches::{branch_matches, matches_query, row_meta};
use crate::ui::branches_tree::{self, BranchNode, BranchView, RemoteGroup, RepoSection};
use crate::ui::components::{
    BRANCH_ROW_H, KIT_BUTTON_H, KIT_ICON, KitButton, PAD_LIST, RowState, SECTION_H, badge_words,
    badge_words_width, current_row_fill, kit_button, kit_button_width, middle_truncate_to_width,
    paint_rail, ref_chip_with_ink, ref_chip_with_ink_width, row_fill, row_ink, section_header,
    state_words, state_words_width, sync_badge, sync_ink,
};
use crate::ui::icons::{self, Icon};
use crate::ui::widgets::menu_host::{self, MenuId};
use crate::ui::widgets::{self, ref_chip};

const REPO_HEADER_H: f32 = 30.0;

const NAME_ZONE_SHARE: f32 = 0.5;

const HEADER_IDENTITY_SHARE: f32 = 0.45;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeEvent {
    RowClicked {
        root: RootId,
        branch: String,
    },
    RowActivated {
        root: RootId,
        branch: String,
    },
    ContextMenuRequested {
        root: RootId,
        branch: String,
    },
    GroupToggled(TreeGroup),
    RemoteToggled {
        root: RootId,
        remote: String,
    },
    RemoteRevealToggled {
        root: RootId,
        revealed: bool,
    },
    FetchRequested {
        root: RootId,
    },
    RenameStarted {
        root: RootId,
        branch: String,
    },
    /// The inline rename editor confirmed; `new` is the trimmed draft.
    RenameCommitted {
        root: RootId,
        old: String,
        new: String,
    },
    RenameCancelled,
    /// Create a branch, from the empty state (`name` empty: "the first
    /// branch") or the no-match state (`name` = the typed query).
    CreateBranchRequested {
        name: String,
    },
    ClearFilterRequested,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeGroup {
    Recent,
    Local,
    Tags,
}

pub struct TreeProps<'a> {
    pub view: &'a BranchView,
    pub filter: &'a str,
    /// The repo scope narrowing, `None` = every in-scope repo.
    pub repo_filter: Option<&'a RootId>,
    pub tags_by_root: &'a HashMap<RootId, Vec<(String, turbogit_domain::model::RefState)>>,
    pub multi_repo: bool,
    pub busy: bool,
    /// Any in-scope root has branch data or a HEAD, `false` shows the
    /// reading/empty states instead of the list.
    pub has_any_data: bool,
    pub merge_in_progress: bool,
    pub last_fetch: Option<chrono::DateTime<chrono::Utc>>,
    pub now: chrono::DateTime<chrono::Utc>,
    pub allows_rename: bool,
    pub allows_context_menu: bool,
    /// egui id salt for the scroll area, unique per surface.
    pub id_salt: &'a str,
    pub full_height: bool,
    pub collapse_remotes_by_default: bool,
}

/// Paint the branch tree and report what the user did.
///
/// Renders the repo-grouped view model: per-repo sections (header, Local
/// group with directory subgroups, Remote area, expanded groups or the
/// collapsed rollup, and a Tags group), filtered live by `props.filter`, or
/// the reading/empty/no-match states instead of a blank panel. Mutates only
/// `tree` (scroll, the rename draft, the filter-transition bookkeeping) and
/// returns every policy-level action as events.
pub fn branch_tree(ui: &mut Ui, props: &TreeProps<'_>, tree: &mut TreeState) -> Vec<TreeEvent> {
    let mut events = Vec::new();

    if !props.has_any_data {
        if props.busy {
            reading_state(ui);
        } else {
            no_branches_state(ui, &mut events);
        }
        return events;
    }

    let filter = props.filter.to_string();
    let filtering = !filter.trim().is_empty();

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
            if filtering
                && in_scope.iter().all(|s| {
                    section_match_count(
                        s,
                        &filter,
                        remotes_revealed(tree, &s.root_id),
                        props.view.options.show_tags,
                    ) == 0
                })
            {
                no_match_state(ui, filter.trim(), &mut events);
                return;
            }
            paint_recent_group(
                ui,
                props,
                tree,
                &filter,
                &scroll_target,
                &mut y_cursor,
                &mut target_y,
                &mut events,
            );
            if props.view.options.group_by_repository {
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
            } else {
                paint_ungrouped_sections(
                    ui,
                    props,
                    tree,
                    &in_scope,
                    &filter,
                    &scroll_target,
                    &mut y_cursor,
                    &mut target_y,
                    &mut events,
                );
            }
        });
    tree.scroll = out.state.offset.y;
    if tree.scroll_to.take().is_some()
        && let Some(ty) = target_y
    {
        tree.scroll = ty;
    }

    events
}

fn section_match_count(
    section: &RepoSection,
    filter: &str,
    remotes: bool,
    show_tags: bool,
) -> usize {
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
    let tags = if show_tags {
        section
            .tags
            .iter()
            .filter(|t| matches_query(&t.name, filter))
            .count()
    } else {
        0
    };
    locals + remotes + tags
}

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

fn leaf_count(nodes: &[BranchNode]) -> usize {
    branches_tree::leaf_count(nodes)
}

#[allow(clippy::too_many_arguments)] // same painter-thread as `paint_repo_section`
fn paint_recent_group(
    ui: &mut Ui,
    props: &TreeProps<'_>,
    tree: &mut TreeState,
    filter: &str,
    scroll_target: &Option<String>,
    y_cursor: &mut f32,
    target_y: &mut Option<f32>,
    events: &mut Vec<TreeEvent>,
) {
    if !props.view.options.show_recent {
        return;
    }
    let rows: Vec<&branches_tree::BranchLeaf> = props
        .view
        .recent
        .iter()
        .filter(|r| match props.repo_filter {
            Some(f) => &r.root == f,
            None => true,
        })
        .filter(|r| branch_matches(&r.branch, filter))
        .collect();
    if rows.is_empty() {
        return;
    }
    let header = section_header(ui, "Recent", rows.len(), tree.groups.recent, |_| {});
    if header.clicked() {
        events.push(TreeEvent::GroupToggled(TreeGroup::Recent));
    }
    *y_cursor += SECTION_H;
    if !tree.groups.recent {
        return;
    }
    for row in rows {
        if Some(row.branch.name.as_str()) == scroll_target.as_deref() {
            *target_y = Some(*y_cursor);
        }
        *y_cursor += BRANCH_ROW_H;
        branch_row(
            ui,
            props,
            tree,
            &row.root,
            row.active,
            &row.label,
            &row.branch,
            0.0,
            events,
        );
    }
}

#[allow(clippy::too_many_arguments)] // the painter-thread this function shares
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
    repo_header(ui, section, !invisible, events);
    *y_cursor += REPO_HEADER_H;

    let locals = filter_nodes(&section.locals, filter);
    paint_local_group(
        ui,
        props,
        tree,
        &[&locals],
        scroll_target,
        y_cursor,
        target_y,
        events,
    );
    paint_remote_area(
        ui,
        props,
        tree,
        section,
        filter,
        scroll_target,
        y_cursor,
        target_y,
        events,
    );
    let tags = section_tags(section, filter);
    paint_tags_group(
        ui,
        props,
        tree,
        &[&tags],
        scroll_target,
        y_cursor,
        target_y,
        events,
    );
}

#[allow(clippy::too_many_arguments)] // same painter-thread as `paint_repo_section`
fn paint_ungrouped_sections(
    ui: &mut Ui,
    props: &TreeProps<'_>,
    tree: &mut TreeState,
    sections: &[&RepoSection],
    filter: &str,
    scroll_target: &Option<String>,
    y_cursor: &mut f32,
    target_y: &mut Option<f32>,
    events: &mut Vec<TreeEvent>,
) {
    let locals: Vec<Vec<BranchNode>> = sections
        .iter()
        .map(|s| filter_nodes(&s.locals, filter))
        .collect();
    let local_refs: &[&[BranchNode]] = &locals.iter().map(Vec::as_slice).collect::<Vec<_>>();
    paint_local_group(
        ui,
        props,
        tree,
        local_refs,
        scroll_target,
        y_cursor,
        target_y,
        events,
    );
    for section in sections {
        paint_remote_area(
            ui,
            props,
            tree,
            section,
            filter,
            scroll_target,
            y_cursor,
            target_y,
            events,
        );
    }
    let tags: Vec<Vec<branches_tree::TagLeaf>> =
        sections.iter().map(|s| section_tags(s, filter)).collect();
    let tag_refs: &[&[branches_tree::TagLeaf]] =
        &tags.iter().map(Vec::as_slice).collect::<Vec<_>>();
    paint_tags_group(
        ui,
        props,
        tree,
        tag_refs,
        scroll_target,
        y_cursor,
        target_y,
        events,
    );
}

#[allow(clippy::too_many_arguments)] // same painter-thread as `paint_repo_section`
fn paint_local_group(
    ui: &mut Ui,
    props: &TreeProps<'_>,
    tree: &mut TreeState,
    forests: &[&[BranchNode]],
    scroll_target: &Option<String>,
    y_cursor: &mut f32,
    target_y: &mut Option<f32>,
    events: &mut Vec<TreeEvent>,
) {
    let count = forests.iter().map(|f| leaf_count(f)).sum();
    let header = section_header(ui, "Local", count, tree.groups.local, |_| {});
    if header.clicked() {
        events.push(TreeEvent::GroupToggled(TreeGroup::Local));
    }
    *y_cursor += SECTION_H;
    if !tree.groups.local {
        return;
    }
    for forest in forests {
        paint_nodes(
            ui,
            props,
            tree,
            forest,
            0,
            scroll_target,
            y_cursor,
            target_y,
            events,
        );
    }
}

#[allow(clippy::too_many_arguments)] // same painter-thread as `paint_repo_section`
fn paint_remote_area(
    ui: &mut Ui,
    props: &TreeProps<'_>,
    tree: &mut TreeState,
    section: &RepoSection,
    filter: &str,
    scroll_target: &Option<String>,
    y_cursor: &mut f32,
    target_y: &mut Option<f32>,
    events: &mut Vec<TreeEvent>,
) {
    if remotes_revealed(tree, &section.root_id) {
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
            if !remote_collapsed(tree, props, &rg.root, &rg.remote) {
                paint_nodes(
                    ui,
                    props,
                    tree,
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
        remote_rollup_row(ui, tree, section, events);
        *y_cursor += BRANCH_ROW_H;
    }
}

#[allow(clippy::too_many_arguments)] // same painter-thread as `paint_repo_section`
fn paint_tags_group(
    ui: &mut Ui,
    props: &TreeProps<'_>,
    tree: &mut TreeState,
    lists: &[&[branches_tree::TagLeaf]],
    scroll_target: &Option<String>,
    y_cursor: &mut f32,
    target_y: &mut Option<f32>,
    events: &mut Vec<TreeEvent>,
) {
    if !props.view.options.show_tags {
        return;
    }
    let count = lists.iter().map(|l| l.len()).sum();
    let header = section_header(ui, "Tags", count, tree.groups.tags, |_| {});
    if header.clicked() {
        events.push(TreeEvent::GroupToggled(TreeGroup::Tags));
    }
    *y_cursor += SECTION_H;
    if !tree.groups.tags {
        return;
    }
    for list in lists {
        for t in *list {
            if Some(t.name.as_str()) == scroll_target.as_deref() {
                *target_y = Some(*y_cursor);
            }
            *y_cursor += BRANCH_ROW_H;
            tag_row(ui, t);
        }
    }
}

fn section_tags(section: &RepoSection, filter: &str) -> Vec<branches_tree::TagLeaf> {
    let mut tags: Vec<branches_tree::TagLeaf> = section
        .tags
        .iter()
        .filter(|t| matches_query(&t.name, filter))
        .cloned()
        .collect();
    tags.sort_by(|a, b| a.name.cmp(&b.name));
    tags
}

#[allow(clippy::too_many_arguments)]
fn paint_nodes(
    ui: &mut Ui,
    props: &TreeProps<'_>,
    tree: &mut TreeState,
    nodes: &[BranchNode],
    depth: usize,
    scroll_target: &Option<String>,
    y_cursor: &mut f32,
    target_y: &mut Option<f32>,
    events: &mut Vec<TreeEvent>,
) {
    let step = KIT_ICON;
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
                    &l.root,
                    l.active,
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

fn repo_header(
    ui: &mut Ui,
    section: &RepoSection,
    show_identity: bool,
    events: &mut Vec<TreeEvent>,
) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, REPO_HEADER_H), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, Palette::SURFACE);
    let split_x = rect.left() + rect.width() * HEADER_IDENTITY_SHARE;
    let chip_w = section
        .current_branch
        .as_deref()
        .map(|cur| ref_chip_with_ink_width(ui, cur) + 8.0)
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
        let name_w = (split_x - rect.left() - 2.0 * PAD_LIST - chip_w).max(24.0);
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
        if let Some(cur) = &section.current_branch {
            left.add_space(8.0);
            ref_chip(&mut left, cur);
        }
    }

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
        if kit_button(ui, KitButton::Quiet, "Fetch").clicked() {
            events.push(TreeEvent::FetchRequested {
                root: section.root_id.clone(),
            });
        }
        if show_identity {
            let summary = fit_summary(ui, &header_summary(section), ui.available_width());
            if !summary.is_empty() {
                ui.add_space(PAD_LIST);
                state_words(ui, &summary, section.status.color());
            }
        }
    });
}

fn fit_summary(ui: &Ui, text: &str, avail: f32) -> String {
    if state_words_width(ui, text) <= avail {
        return text.to_owned();
    }
    let mut keep = text.chars().count();
    while keep > 1 {
        keep -= 1;
        let candidate: String = text
            .chars()
            .take(keep)
            .chain(std::iter::once('…'))
            .collect();
        if state_words_width(ui, &candidate) <= avail {
            return candidate;
        }
    }
    "…".to_owned()
}

fn header_summary(section: &RepoSection) -> String {
    let mut parts: Vec<String> = sync_badge(section.ahead, section.behind, false)
        .into_iter()
        .map(|(_, label)| label)
        .collect();
    parts.push(section.status.words().to_string());
    parts.retain(|part| !part.is_empty());
    parts.join(" · ")
}

fn row_visible(ui: &Ui, rect: Rect) -> bool {
    ui.is_rect_visible(rect)
}

fn dir_header(ui: &mut Ui, label: &str, count: usize, depth: usize, step: f32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, BRANCH_ROW_H), Sense::hover());
    if !row_visible(ui, rect) {
        return;
    }
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.spacing_mut().item_spacing.x = 0.0;
    child.add_space(PAD_LIST + depth as f32 * step);
    icons::icon(&mut child, Icon::FOLDER, KIT_ICON, Palette::T_MUTED);
    child.add_space(6.0);
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

pub fn remotes_revealed(tree: &TreeState, root: &RootId) -> bool {
    tree.show_remotes || tree.remotes_revealed.contains(root)
}

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
    child.add(
        egui::Label::new(
            RichText::new(rg.remote.clone())
                .font(data_font(TYPE_SECTION))
                .color(Palette::T_SECONDARY),
        )
        .truncate(),
    );
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

fn remote_rollup_row(
    ui: &mut Ui,
    tree: &mut TreeState,
    section: &RepoSection,
    events: &mut Vec<TreeEvent>,
) {
    let width = ui.available_width();
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

#[allow(clippy::too_many_arguments)]
fn branch_row(
    ui: &mut Ui,
    props: &TreeProps<'_>,
    tree: &mut TreeState,
    id: &RootId,
    is_current: bool,
    label: &str,
    branch: &Branch,
    indent: f32,
    events: &mut Vec<TreeEvent>,
) {
    if props.allows_rename
        && branch.kind == BranchKind::Local
        && tree.selected_root.as_ref() == Some(id)
        && tree.renaming.as_deref() == Some(branch.name.as_str())
    {
        rename_editor(ui, tree, id, branch, events);
        return;
    }

    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, BRANCH_ROW_H), Sense::hover());
    if !row_visible(ui, rect) {
        return;
    }
    let now = props.now;
    let meta = row_meta(branch, now);
    let is_remote = branch.kind == BranchKind::Remote;
    let selected = tree.selected_root.as_ref() == Some(id)
        && tree.selected.as_deref() == Some(branch.name.as_str());
    let hovered = ui.rect_contains_pointer(rect);

    let response = ui.interact(
        rect,
        ui.auto_id_with(("branch_row", id, &branch.name)),
        Sense::click(),
    );

    let row_state = if selected {
        RowState::RowSelected
    } else if hovered {
        RowState::Hover
    } else {
        RowState::Default
    };
    let marked = selected || (is_current && !is_remote);
    let fill = if selected {
        row_fill(row_state)
    } else if is_current && !is_remote {
        current_row_fill(row_state)
    } else {
        row_fill(row_state)
    };
    if fill != Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(crate::theme::CHIP_RADIUS), fill);
    }
    if marked {
        paint_rail(ui.painter(), rect);
    }

    let chips = &meta.badge;

    let name_x = rect.left() + PAD_LIST + indent;
    let text_x = name_x + KIT_ICON + 6.0;
    let shows_pill = is_current && !is_remote;
    let pill_w = if shows_pill { KIT_ICON + 8.0 } else { 0.0 };
    let content_right = rect.right() - PAD_LIST;
    let mut badges_w: f32 = pill_w
        + chips
            .iter()
            .map(|(kind, label)| badge_words_width(ui, *kind, label) + 4.0)
            .sum::<f32>();

    let share_x = text_x + (content_right - 8.0 - text_x).max(0.0) * NAME_ZONE_SHARE;
    let name_w = (share_x - 8.0 - text_x).max(0.0);

    let name_right = share_x - 8.0;
    let shows_badges = name_right - badges_w >= text_x;
    if !shows_badges {
        badges_w = pill_w;
    }
    let badges_right = content_right;
    let badges_x = badges_right - badges_w;
    let track_x = share_x;
    let track_w = (badges_x - 8.0 - share_x).max(0.0);

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
    zone.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if shows_pill {
            let _ = current_marker(ui);
        }
        if is_current && props.merge_in_progress {
            ui.add_space(8.0);
            state_words(
                ui,
                &crate::ui::components::mid_op_label("merging"),
                Palette::STATE_WARNING,
            );
        }
        if shows_badges {
            for (kind, label) in chips.iter().rev() {
                ui.add_space(4.0);
                badge_words(ui, *kind, label, sync_ink(*kind));
            }
        }
        if branch.kind == BranchKind::Remote && branch.gone {
            ui.add_space(8.0);
            state_words(ui, "gone", Palette::STATE_ERROR);
        }
    });

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
                RichText::new(up.git_ref())
                    .font(data_font(TYPE_CONTROL))
                    .color(Palette::T_MUTED),
            )
            .truncate()
            .selectable(false),
        );
    }

    let ink = if is_remote {
        Palette::INK_2
    } else {
        row_ink(meta.stale)
    };
    let name = middle_truncate_to_width(
        ui,
        label,
        &data_font(TYPE_CHIP),
        (name_w - widgets::COMPACT_CHIP_GEOMETRY.pad_x * 2.0).max(0.0),
    );
    let name_rect = Rect::from_min_max(
        Pos2::new(name_x, rect.top()),
        Pos2::new(share_x - 8.0, rect.bottom()),
    );
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(name_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.spacing_mut().item_spacing.x = 0.0;
    icons::icon(&mut child, Icon::GIT_BRANCH, KIT_ICON, ink);
    child.add_space(6.0);
    ref_chip_with_ink(&mut child, &name, ink);

    if response.clicked() {
        events.push(TreeEvent::RowClicked {
            root: id.clone(),
            branch: branch.name.clone(),
        });
    } else if props.allows_context_menu && response.secondary_clicked() {
        let target = menu_host::target_of(id, &branch.name);
        let pos = ui
            .input(|i| i.pointer.interact_pos())
            .unwrap_or_else(|| rect.left_bottom());
        menu_host::note_anchor(ui, MenuId::new("branches", &target), pos);
        events.push(TreeEvent::ContextMenuRequested {
            root: id.clone(),
            branch: branch.name.clone(),
        });
    }
    if response.double_clicked() {
        events.push(TreeEvent::RowActivated {
            root: id.clone(),
            branch: branch.name.clone(),
        });
    }
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &branch.name));
}

const CURRENT_MARKER: &str = "Current";

fn current_marker(ui: &mut Ui) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(KIT_ICON), Sense::hover());
    if ui.is_rect_visible(rect) {
        icons::paint_icon(
            ui.painter(),
            rect.min,
            KIT_ICON,
            Icon::CHECK,
            Palette::ACCENT_TEXT,
        );
    }
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, CURRENT_MARKER));
    response
}

fn rename_editor(
    ui: &mut Ui,
    tree: &mut TreeState,
    root: &RootId,
    branch: &Branch,
    events: &mut Vec<TreeEvent>,
) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 56.0), Sense::hover());
    let mut child = ui.new_child(
        UiBuilder::new()
            // An explicit id, so the `TextEdit` below and its stored caret sit at
            // the same id every frame. `UiBuilder::id` is `IdSource::Explicit`,
            // which seeds the child's auto-id counter from this id alone, so the
            // counter no longer depends on how many rows above this one were
            // built. Rows now stop being built once they leave the viewport, and
            // a counter-derived id would move under the caret on the first tick.
            .id(ui.id().with(("branch_rename", root, branch.name.as_str())))
            .max_rect(rect)
            .layout(Layout::top_down(Align::Min)),
    );
    child.add_space(6.0);
    let enter = child.input(|i| i.key_pressed(egui::Key::Enter));
    child.horizontal(|ui| {
        ui.add_space(PAD_LIST);
        let buttons_w = kit_button_width(ui, "Apply rename")
            + ui.spacing().item_spacing.x
            + kit_button_width(ui, "Cancel");
        let input_w = (ui.available_width() - buttons_w).max(120.0);
        let input = ui.allocate_ui_with_layout(
            Vec2::new(input_w, KIT_BUTTON_H),
            Layout::left_to_right(Align::Center),
            |ui| widgets::text_input(ui, &branch.name, &mut tree.rename_draft),
        );
        input.inner.request_focus();
        if kit_button(ui, KitButton::Secondary, "Apply rename").clicked() || enter {
            events.push(TreeEvent::RenameCommitted {
                root: root.clone(),
                old: branch.name.clone(),
                new: tree.rename_draft.trim().to_string(),
            });
        }
        if kit_button(ui, KitButton::Quiet, "Cancel").clicked() {
            events.push(TreeEvent::RenameCancelled);
        }
    });
    if let Some(up) = &branch.tracking {
        child.add_space(2.0);
        child.horizontal(|ui| {
            ui.add_space(PAD_LIST);
            ui.label(
                RichText::new(format!(
                    "tracking {} does not follow the new name — set it again after",
                    up.git_ref()
                ))
                .font(chrome_font(TYPE_CONTROL))
                .color(Palette::T_MUTED),
            );
        });
    }
}

fn tag_row(ui: &mut Ui, tag: &crate::ui::branches_tree::TagLeaf) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, BRANCH_ROW_H), Sense::hover());
    if !row_visible(ui, rect) {
        return;
    }
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

fn ref_state_marker(state: turbogit_domain::model::RefState) -> Option<(&'static str, Color32)> {
    use turbogit_domain::model::RefState;
    match state {
        RefState::Default => None,
        RefState::Gone => Some(("gone", Palette::STATE_ERROR)),
        RefState::Pushed => Some(("pushed", Palette::STATE_SUCCESS)),
        RefState::LocalOnly => Some(("local only", Palette::STATE_WARNING)),
    }
}

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

fn no_branches_state(ui: &mut Ui, events: &mut Vec<TreeEvent>) {
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

pub fn warm_tags(state: &mut turbogit_app::state::AppState) {
    state.warm_branch_tags();
}
