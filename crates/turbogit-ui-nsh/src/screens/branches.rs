//! The Branches surface: the repository-grouped branch tree, its toolbar,
//! the scope chip, the grouping toggles, the undo banner, and the row menu.
//! The tree's model is built once per change and held in a signal; the tree
//! element only rebuilds when the held model moves.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use nsh_base::{SelectValue, theme};
use nsh_core::element::desktop::MenuItem;
use nsh_core::prelude::*;
use turbogit_app::operation::Operation;
use turbogit_app::persistence::BranchesViewOptions;
use turbogit_app::shell_writes;
use turbogit_app::state::{AppState, NewBranchBase, PendingConfirm, Toast};
use turbogit_domain::model::{Branch, BranchKind, BranchSpec, RefState, Root, RootId};

use crate::bridge::AppHandle;
use crate::screens::shelf::counted;
use crate::test_ids;
use crate::theme as data;
use crate::ui::button::{ButtonProps, ButtonVariant, button};
use crate::ui::icon_button::{IconButtonProps, icon_button};
use crate::ui::input::{InputProps, input};
use crate::ui::popover::{PopoverProps, popover};
use crate::ui::row::{RowProps, RowState, row};
use crate::ui::tree::{TreeEntryProps, TreeProps, tree};

const TICK_MS: u64 = 160;
const UNDO_WINDOW: Duration = Duration::from_secs(10);

/// Case-insensitive subsequence match: `mre` surfaces `multi-root-executor`.
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

/// Every whitespace-separated word matches as a subsequence.
pub fn matches_query(haystack: &str, query: &str) -> bool {
    let q = query.trim();
    if q.is_empty() {
        return true;
    }
    q.split_whitespace().all(|w| fuzzy_word(haystack, w))
}

/// A branch matches by its name or by its tip commit's subject.
pub fn branch_matches(branch: &Branch, query: &str) -> bool {
    if matches_query(&branch.name, query) {
        return true;
    }
    branch
        .tip
        .as_ref()
        .is_some_and(|tip| matches_query(&tip.message, query))
}

/// The stale threshold the quiet age badge reads.
pub const STALE_THRESHOLD: chrono::Duration = chrono::Duration::days(28);

pub fn is_stale(branch: &Branch, now: chrono::DateTime<chrono::Utc>) -> bool {
    branch
        .last_touched
        .is_some_and(|touched| now.signed_duration_since(touched) > STALE_THRESHOLD)
}

/// Compact age label (`now`, `12m`, `3h`, `2d`, `3w`, `4mo`, `1y`).
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

/// The sync words beside a branch: `↑2 ↓1`, or `gone`.
pub fn sync_words(ahead: usize, behind: usize, gone: bool) -> Option<String> {
    if gone {
        Some("gone".to_string())
    } else if ahead > 0 || behind > 0 {
        Some(format!("↑{ahead} ↓{behind}"))
    } else {
        None
    }
}

/// Middle truncation that keeps both identifying ends.
pub fn middle_truncate(text: &str, keep: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= keep || keep < 3 {
        return text.to_string();
    }
    let head = (keep - 1) / 2;
    let tail = keep - 1 - head;
    let mut out: String = chars[..head].iter().collect();
    out.push('…');
    out.extend(chars[chars.len() - tail..].iter());
    out
}

/// The repository-state verdict, the egui ladder's order.
pub fn repo_state(root: &Root, ahead: usize, behind: usize) -> data::RepoState {
    if !root.status.conflicted.is_empty() {
        data::RepoState::Conflict
    } else if root.status.modified() + root.status.unversioned() > 0 {
        data::RepoState::Dirty
    } else if ahead > 0 && behind > 0 {
        data::RepoState::Diverged
    } else if behind > 0 {
        data::RepoState::Unpulled
    } else if ahead > 0 {
        data::RepoState::Unpushed
    } else {
        data::RepoState::Clean
    }
}

/// One leaf of the branch forest.
#[derive(Clone, Debug, PartialEq)]
pub struct BranchLeaf {
    pub label: String,
    pub root: RootId,
    pub branch: Branch,
    pub active: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BranchNode {
    Leaf(Box<BranchLeaf>),
    Dir {
        label: String,
        count: usize,
        children: Vec<BranchNode>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct TagLeaf {
    pub name: String,
    pub state: RefState,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RemoteGroup {
    pub remote: String,
    pub root: RootId,
    pub label: String,
    pub count: usize,
    pub children: Vec<BranchNode>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RepoSection {
    pub root_id: RootId,
    pub repo_name: String,
    pub current_branch: Option<String>,
    pub status: data::RepoState,
    pub ahead: usize,
    pub behind: usize,
    pub locals: Vec<BranchNode>,
    pub remote_branch_count: usize,
    pub remote_count: usize,
    pub remote_groups: Vec<RemoteGroup>,
    pub tags: Vec<TagLeaf>,
}

impl RepoSection {
    /// The folded `N remotes · M branches` line, `None` with neither.
    pub fn remote_rollup(&self) -> Option<String> {
        if self.remote_count == 0 && self.remote_branch_count == 0 {
            None
        } else {
            Some(format!(
                "{} · {}",
                counted(self.remote_count, "remote", "remotes"),
                counted(self.remote_branch_count, "branch", "branches")
            ))
        }
    }

    /// The header's own line: drift first, then the state word.
    pub fn header_summary(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.ahead > 0 && self.behind > 0 {
            parts.push(format!("↑{} ↓{}", self.ahead, self.behind));
        } else if self.ahead > 0 {
            parts.push(format!("↑{}", self.ahead));
        } else if self.behind > 0 {
            parts.push(format!("↓{}", self.behind));
        }
        let words = self.status.words();
        if !words.is_empty() {
            parts.push(words.to_string());
        }
        parts.join(" · ")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BranchView {
    pub repos: Vec<RepoSection>,
    pub all_repos_count: Option<usize>,
    pub single_repo: bool,
    pub options: BranchesViewOptions,
}

pub fn leaf_count(nodes: &[BranchNode]) -> usize {
    nodes
        .iter()
        .map(|node| match node {
            BranchNode::Leaf(_) => 1,
            BranchNode::Dir { children, .. } => leaf_count(children),
        })
        .sum()
}

enum Pending {
    Leaf(Box<BranchLeaf>),
    Dir(String, Vec<Pending>),
}

fn insert(nodes: &mut Vec<Pending>, dirs: &[&str], leaf: BranchLeaf) {
    match dirs.split_first() {
        Some((first, rest)) => {
            let at = nodes
                .iter()
                .position(|node| matches!(node, Pending::Dir(name, _) if name == first));
            let child = match at {
                Some(at) => &mut nodes[at],
                None => {
                    nodes.push(Pending::Dir((*first).to_string(), Vec::new()));
                    nodes.last_mut().expect("just pushed")
                }
            };
            if let Pending::Dir(_, children) = child {
                insert(children, rest, leaf);
            }
        }
        None => nodes.push(Pending::Leaf(Box::new(leaf))),
    }
}

fn finalize(nodes: &mut [Pending]) {
    for node in nodes.iter_mut() {
        if let Pending::Dir(_, children) = node {
            finalize(children);
        }
    }
    nodes.sort_by(|a, b| {
        let (af, aa, al) = rank(a);
        let (bf, ba, bl) = rank(b);
        bf.cmp(&af)
            .then_with(|| ba.cmp(&aa))
            .then_with(|| al.cmp(bl))
    });
}

fn rank(node: &Pending) -> (bool, bool, &str) {
    match node {
        Pending::Leaf(leaf) => (leaf.branch.favorite, leaf.active, &leaf.label),
        Pending::Dir(name, _) => (false, false, name),
    }
}

fn pending_count(nodes: &[Pending]) -> usize {
    nodes
        .iter()
        .map(|node| match node {
            Pending::Leaf(_) => 1,
            Pending::Dir(_, children) => pending_count(children),
        })
        .sum()
}

fn convert(nodes: Vec<Pending>) -> Vec<BranchNode> {
    nodes
        .into_iter()
        .map(|node| match node {
            Pending::Leaf(leaf) => BranchNode::Leaf(leaf),
            Pending::Dir(label, children) => {
                let count = pending_count(&children);
                BranchNode::Dir {
                    label,
                    count,
                    children: convert(children),
                }
            }
        })
        .collect()
}

fn build_tree(
    branches: &[Branch],
    current: &Option<String>,
    strip_prefix: Option<&str>,
    root: &RootId,
    group_dirs: bool,
) -> Vec<BranchNode> {
    let mut forest: Vec<Pending> = Vec::new();
    for branch in branches {
        let name = branch.name.as_str();
        let stripped = match strip_prefix {
            Some(prefix) if name.starts_with(prefix) && name[prefix.len()..].starts_with('/') => {
                &name[prefix.len() + 1..]
            }
            _ => name,
        };
        let (label, dirs): (String, Vec<&str>) = if group_dirs {
            let mut parts: Vec<&str> = stripped.split('/').collect();
            let label = parts.pop().unwrap_or("").to_string();
            (label, parts)
        } else {
            (stripped.to_string(), Vec::new())
        };
        let leaf = BranchLeaf {
            label,
            root: root.clone(),
            branch: branch.clone(),
            active: current.as_deref() == Some(name) && branch.kind == BranchKind::Local,
        };
        insert(&mut forest, &dirs, leaf);
    }
    finalize(&mut forest);
    convert(forest)
}

fn header_status(root: &Root) -> (data::RepoState, usize, usize) {
    let current = root.current_branch.as_ref().and_then(|name| {
        root.branches
            .iter()
            .find(|b| b.kind == BranchKind::Local && &b.name == name)
    });
    let (ahead, behind) = current.map_or((0, 0), |b| (b.ahead, b.behind));
    (repo_state(root, ahead, behind), ahead, behind)
}

fn shared_names(roots: &[&Root], names: impl Fn(&Root) -> Vec<String>) -> HashSet<String> {
    let mut seen = HashSet::new();
    let mut shared = HashSet::new();
    for root in roots {
        for name in names(root) {
            if !seen.insert(name.clone()) {
                shared.insert(name);
            }
        }
    }
    shared
}

fn qualify(nodes: &mut [BranchNode], shared: &HashSet<String>, repo: &str) {
    for node in nodes {
        match node {
            BranchNode::Leaf(leaf) => {
                if shared.contains(&leaf.branch.name) {
                    leaf.label = format!("{} · {repo}", leaf.label);
                }
            }
            BranchNode::Dir { children, .. } => qualify(children, shared, repo),
        }
    }
}

/// The full grouped model under the persisted toggles. The reveal predicate
/// is asked per repository, so a rolled-up repository costs no tree build.
pub fn build_branch_view(
    roots: &[&Root],
    tags_by_root: &HashMap<RootId, Vec<(String, RefState)>>,
    show_remotes: &dyn Fn(&RootId) -> bool,
    options: &BranchesViewOptions,
) -> BranchView {
    let single_repo = roots.len() == 1;
    let all_repos_count = (roots.len() > 1).then_some(roots.len());
    let qualify_names = !options.group_by_repository && roots.len() > 1;
    let shared_locals = shared_names(roots, |root| {
        root.branches
            .iter()
            .filter(|b| b.kind == BranchKind::Local)
            .map(|b| b.name.clone())
            .collect()
    });
    let shared_tags = shared_names(roots, |root| {
        tags_by_root
            .get(&root.id)
            .map(|tags| tags.iter().map(|(name, _)| name.clone()).collect())
            .unwrap_or_default()
    });

    let mut repos = Vec::with_capacity(roots.len());
    for root in roots {
        let repo_name = root.id.name();
        let current = &root.current_branch;
        let (status, ahead, behind) = header_status(root);
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

        let mut locals_tree =
            build_tree(&locals, current, None, &root.id, options.group_by_directory);
        if qualify_names {
            qualify(&mut locals_tree, &shared_locals, &repo_name);
        }

        let mut by_remote: Vec<(String, Vec<Branch>)> = Vec::new();
        for branch in &remotes {
            let remote = branch
                .remote
                .clone()
                .unwrap_or_else(|| "origin".to_string());
            match by_remote.iter_mut().find(|(name, _)| *name == remote) {
                Some((_, list)) => list.push(branch.clone()),
                None => by_remote.push((remote, vec![branch.clone()])),
            }
        }
        by_remote.sort_by(|a, b| a.0.cmp(&b.0));

        let remote_groups = if show_remotes(&root.id) {
            by_remote
                .iter()
                .map(|(remote, list)| RemoteGroup {
                    label: if qualify_names {
                        format!("{remote} · {repo_name}")
                    } else {
                        remote.clone()
                    },
                    remote: remote.clone(),
                    root: root.id.clone(),
                    count: list.len(),
                    children: build_tree(
                        list,
                        current,
                        Some(remote),
                        &root.id,
                        options.group_by_directory,
                    ),
                })
                .collect()
        } else {
            Vec::new()
        };

        let mut tags: Vec<TagLeaf> = tags_by_root
            .get(&root.id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|(name, state)| TagLeaf { name, state })
            .collect();
        if qualify_names {
            for tag in &mut tags {
                if shared_tags.contains(&tag.name) {
                    tag.name = format!("{} · {repo_name}", tag.name);
                }
            }
        }

        repos.push(RepoSection {
            root_id: root.id.clone(),
            repo_name,
            current_branch: current.clone(),
            status,
            ahead,
            behind,
            locals: locals_tree,
            remote_branch_count: remotes.len(),
            remote_count: root.remotes.len(),
            remote_groups,
            tags,
        });
    }

    BranchView {
        repos,
        all_repos_count,
        single_repo,
        options: *options,
    }
}

fn filter_nodes(nodes: &[BranchNode], query: &str) -> Vec<BranchNode> {
    if query.trim().is_empty() {
        return nodes.to_vec();
    }
    let mut out = Vec::new();
    for node in nodes {
        match node {
            BranchNode::Leaf(leaf) => {
                if branch_matches(&leaf.branch, query) {
                    out.push(node.clone());
                }
            }
            BranchNode::Dir {
                label, children, ..
            } => {
                let kept = filter_nodes(children, query);
                if !kept.is_empty() {
                    out.push(BranchNode::Dir {
                        label: label.clone(),
                        count: leaf_count(&kept),
                        children: kept,
                    });
                }
            }
        }
    }
    out
}

/// One row of the painted tree, with everything its paint and its press need.
#[derive(Clone, Debug, PartialEq)]
pub enum RowInfo {
    Repo {
        root: RootId,
        name: String,
        current: Option<String>,
        summary: String,
        count: usize,
        expanded: bool,
    },
    LocalGroup {
        count: usize,
        open: bool,
    },
    RemoteHeader {
        root: RootId,
        rollup: String,
        open: bool,
    },
    RemoteGroup {
        root: RootId,
        remote: String,
        count: usize,
        open: bool,
    },
    TagsGroup {
        count: usize,
        open: bool,
    },
    Dir {
        label: String,
        count: usize,
    },
    Leaf {
        root: RootId,
        label: String,
        branch: Branch,
        active: bool,
        selected: bool,
    },
    Tag {
        name: String,
        state: RefState,
    },
}

/// The held tree: the built rows, the row data by id, and the expansion and
/// selection the tree shows.
pub struct TreeModel {
    pub rows: Vec<TreeEntryProps>,
    pub info: HashMap<String, RowInfo>,
    pub expanded: Vec<SharedString>,
    pub selected: Option<SharedString>,
    pub any_data: bool,
    pub matches: usize,
}

impl PartialEq for TreeModel {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

fn remotes_revealed(state: &AppState, root: &RootId) -> bool {
    state.ui.branches_tree.show_remotes || state.ui.branches_tree.remotes_revealed.contains(root)
}

fn repo_collapsed(state: &AppState, filtering: bool, root: &RootId) -> bool {
    !filtering && state.ui.branches_tree.collapsed_repos.contains(root)
}

fn remote_collapsed(state: &AppState, root: &RootId, remote: &str) -> bool {
    state
        .ui
        .branches_tree
        .collapsed_remotes
        .contains(&(root.clone(), remote.to_string()))
}

#[allow(clippy::too_many_arguments)]
fn push_nodes(
    rows: &mut Vec<TreeEntryProps>,
    info: &mut HashMap<String, RowInfo>,
    expanded: &mut Vec<SharedString>,
    node_matches: &mut usize,
    root: &RootId,
    nodes: &[BranchNode],
    prefix: &str,
    selected: &Option<(String, String)>,
) {
    for node in nodes {
        match node {
            BranchNode::Leaf(leaf) => {
                *node_matches += 1;
                let id = format!(
                    "{}:branch:{}:{}",
                    root.name(),
                    match leaf.branch.kind {
                        BranchKind::Local => "local",
                        BranchKind::Remote => "remote",
                    },
                    leaf.branch.name
                );
                info.insert(
                    id.clone(),
                    RowInfo::Leaf {
                        root: root.clone(),
                        label: leaf.label.clone(),
                        branch: leaf.branch.clone(),
                        active: leaf.active,
                        selected: selected.as_ref().is_some_and(|(sel_root, sel_name)| {
                            sel_root == &root.name() && sel_name == &leaf.branch.name
                        }),
                    },
                );
                rows.push(TreeEntryProps::new(id, leaf.label.clone()));
            }
            BranchNode::Dir {
                label,
                count,
                children,
            } => {
                let id = format!("{prefix}:dir:{label}");
                info.insert(
                    id.clone(),
                    RowInfo::Dir {
                        label: label.clone(),
                        count: *count,
                    },
                );
                expanded.push(SharedString::from(id.clone()));
                let mut entry = TreeEntryProps::new(id, format!("{label}/ ({count})"));
                let mut child_rows = Vec::new();
                push_nodes(
                    &mut child_rows,
                    info,
                    expanded,
                    node_matches,
                    root,
                    children,
                    prefix,
                    selected,
                );
                entry.children = child_rows;
                rows.push(entry);
            }
        }
    }
}

/// Builds the held tree from the state: the model, the filter, the folds,
/// and the selection.
pub fn build_tree_model(state: &AppState) -> TreeModel {
    let tags = state.caches.tags_by_root().clone();
    let roots: Vec<&Root> = state.multi.roots.iter().collect();
    let reveal = |root: &RootId| remotes_revealed(state, root);
    let view = build_branch_view(&roots, &tags, &reveal, &state.ui.branches_view);
    let query = state.ui.branches_filter.clone();
    let filtering = !query.trim().is_empty();
    let sections: Vec<&RepoSection> = view
        .repos
        .iter()
        .filter(|section| match &state.ui.branches_repo_filter {
            Some(filter) => filter == &section.root_id,
            None => true,
        })
        .collect();
    let any_data = state
        .multi
        .roots
        .iter()
        .any(|root| !root.branches.is_empty() || root.head.is_some());
    let selected: Option<(String, String)> = state
        .ui
        .branches_tree
        .selected_root
        .as_ref()
        .and_then(|root| {
            state
                .ui
                .branches_tree
                .selected
                .as_ref()
                .map(|name| (root.name(), name.clone()))
        });

    let mut rows = Vec::new();
    let mut info = HashMap::new();
    let mut expanded = Vec::new();
    let mut matches = 0;

    if view.options.group_by_repository {
        for section in &sections {
            let repo_key = section.root_id.name();
            let collapsed = repo_collapsed(state, filtering, &section.root_id);
            let locals = filter_nodes(&section.locals, &query);
            let local_count = leaf_count(&locals);
            let id = format!("repo:{repo_key}");
            info.insert(
                id.clone(),
                RowInfo::Repo {
                    root: section.root_id.clone(),
                    name: section.repo_name.clone(),
                    current: section.current_branch.clone(),
                    summary: section.header_summary(),
                    count: local_count,
                    expanded: !collapsed,
                },
            );
            if !collapsed {
                expanded.push(SharedString::from(id.clone()));
            }
            let mut entry = TreeEntryProps::new(id, section.repo_name.clone());
            if !collapsed {
                let mut children = Vec::new();

                let local_id = format!("{repo_key}:group:local");
                let local_open = state.ui.branches_tree.groups.local;
                info.insert(
                    local_id.clone(),
                    RowInfo::LocalGroup {
                        count: local_count,
                        open: local_open,
                    },
                );
                if local_open {
                    expanded.push(SharedString::from(local_id.clone()));
                }
                let mut local_entry =
                    TreeEntryProps::new(local_id, format!("Local ({local_count})"));
                if local_open {
                    let mut child_rows = Vec::new();
                    push_nodes(
                        &mut child_rows,
                        &mut info,
                        &mut expanded,
                        &mut matches,
                        &section.root_id,
                        &locals,
                        &repo_key,
                        &selected,
                    );
                    local_entry.children = child_rows;
                }
                children.push(local_entry);

                if remotes_revealed(state, &section.root_id) && !state.ui.branches_tree.show_remotes
                {
                    let remote_id = format!("{repo_key}:group:remote");
                    let rollup = section.remote_rollup().unwrap_or_default();
                    info.insert(
                        remote_id.clone(),
                        RowInfo::RemoteHeader {
                            root: section.root_id.clone(),
                            rollup: rollup.clone(),
                            open: true,
                        },
                    );
                    expanded.push(SharedString::from(remote_id.clone()));
                    let mut remote_entry =
                        TreeEntryProps::new(remote_id, format!("Remote · {rollup}"));
                    for group in &section.remote_groups {
                        let nodes = filter_nodes(&group.children, &query);
                        if nodes.is_empty() && filtering {
                            continue;
                        }
                        let group_id = format!("{repo_key}:remote:{}", group.remote);
                        let open = !remote_collapsed(state, &section.root_id, &group.remote);
                        info.insert(
                            group_id.clone(),
                            RowInfo::RemoteGroup {
                                root: section.root_id.clone(),
                                remote: group.remote.clone(),
                                count: group.count,
                                open,
                            },
                        );
                        if open {
                            expanded.push(SharedString::from(group_id.clone()));
                        }
                        let mut group_entry = TreeEntryProps::new(
                            group_id,
                            format!("{} ({})", group.remote, group.count),
                        );
                        if open {
                            let mut child_rows = Vec::new();
                            push_nodes(
                                &mut child_rows,
                                &mut info,
                                &mut expanded,
                                &mut matches,
                                &section.root_id,
                                &nodes,
                                &repo_key,
                                &selected,
                            );
                            group_entry.children = child_rows;
                        }
                        remote_entry.children.push(group_entry);
                    }
                    children.push(remote_entry);
                } else if !remotes_revealed(state, &section.root_id) {
                    let rollup_id = format!("{repo_key}:group:remote");
                    let rollup = section.remote_rollup().unwrap_or_default();
                    info.insert(
                        rollup_id.clone(),
                        RowInfo::RemoteHeader {
                            root: section.root_id.clone(),
                            rollup: rollup.clone(),
                            open: false,
                        },
                    );
                    children.push(TreeEntryProps::new(rollup_id, format!("REMOTE · {rollup}")));
                }

                if view.options.show_tags {
                    let mut kept: Vec<TagLeaf> = section
                        .tags
                        .iter()
                        .filter(|tag| matches_query(&tag.name, &query))
                        .cloned()
                        .collect();
                    kept.sort_by(|a, b| a.name.cmp(&b.name));
                    matches += kept.len();
                    let tag_id = format!("{repo_key}:group:tags");
                    let open = state.ui.branches_tree.groups.tags;
                    info.insert(
                        tag_id.clone(),
                        RowInfo::TagsGroup {
                            count: kept.len(),
                            open,
                        },
                    );
                    if open {
                        expanded.push(SharedString::from(tag_id.clone()));
                    }
                    let mut tag_entry =
                        TreeEntryProps::new(tag_id, format!("Tags ({})", kept.len()));
                    if open {
                        for tag in kept {
                            let id = format!("{repo_key}:tag:{}", tag.name);
                            info.insert(
                                id.clone(),
                                RowInfo::Tag {
                                    name: tag.name.clone(),
                                    state: tag.state,
                                },
                            );
                            tag_entry = tag_entry.child(TreeEntryProps::new(id, tag.name.clone()));
                        }
                    }
                    children.push(tag_entry);
                }
                entry.children = children;
            }
            rows.push(entry);
        }
    } else {
        let mut locals: Vec<BranchNode> = Vec::new();
        let mut remotes: Vec<(RootId, Vec<BranchNode>)> = Vec::new();
        let mut tags: Vec<(RootId, Vec<TagLeaf>)> = Vec::new();
        for section in &sections {
            locals.extend(filter_nodes(&section.locals, &query));
            if remotes_revealed(state, &section.root_id) {
                for group in &section.remote_groups {
                    remotes.push((
                        section.root_id.clone(),
                        filter_nodes(&group.children, &query),
                    ));
                }
            }
            let mut kept: Vec<TagLeaf> = section
                .tags
                .iter()
                .filter(|tag| matches_query(&tag.name, &query))
                .cloned()
                .collect();
            kept.sort_by(|a, b| a.name.cmp(&b.name));
            tags.push((section.root_id.clone(), kept));
        }
        let local_count = leaf_count(&locals);
        let local_id = "all:group:local".to_string();
        let local_open = state.ui.branches_tree.groups.local;
        info.insert(
            local_id.clone(),
            RowInfo::LocalGroup {
                count: local_count,
                open: local_open,
            },
        );
        if local_open {
            expanded.push(SharedString::from(local_id.clone()));
        }
        let mut local_entry = TreeEntryProps::new(local_id, format!("Local ({local_count})"));
        if local_open {
            let mut child_rows = Vec::new();
            if let Some(section) = sections.first() {
                push_nodes(
                    &mut child_rows,
                    &mut info,
                    &mut expanded,
                    &mut matches,
                    &section.root_id,
                    &locals,
                    "all",
                    &selected,
                );
            }
            local_entry.children = child_rows;
        }
        rows.push(local_entry);

        for (root, nodes) in remotes {
            let repo = root.name();
            let remote_id = format!("{repo}:group:remote");
            info.insert(
                remote_id.clone(),
                RowInfo::RemoteHeader {
                    root: root.clone(),
                    rollup: String::new(),
                    open: true,
                },
            );
            expanded.push(SharedString::from(remote_id.clone()));
            let mut remote_entry = TreeEntryProps::new(remote_id, format!("Remote · {repo}"));
            push_nodes(
                &mut remote_entry.children,
                &mut info,
                &mut expanded,
                &mut matches,
                &root,
                &nodes,
                &repo,
                &selected,
            );
            rows.push(remote_entry);
        }

        if view.options.show_tags {
            let total: usize = tags.iter().map(|(_, list)| list.len()).sum();
            matches += total;
            let tag_id = "all:group:tags".to_string();
            let open = state.ui.branches_tree.groups.tags;
            info.insert(tag_id.clone(), RowInfo::TagsGroup { count: total, open });
            if open {
                expanded.push(SharedString::from(tag_id.clone()));
            }
            let mut tag_entry = TreeEntryProps::new(tag_id, format!("Tags ({total})"));
            if open {
                for (root, list) in tags {
                    for tag in list {
                        let id = format!("{}:tag:{}", root.name(), tag.name);
                        info.insert(
                            id.clone(),
                            RowInfo::Tag {
                                name: tag.name.clone(),
                                state: tag.state,
                            },
                        );
                        tag_entry = tag_entry.child(TreeEntryProps::new(id, tag.name.clone()));
                    }
                }
            }
            rows.push(tag_entry);
        }
    }

    let selected = state
        .ui
        .branches_tree
        .selected_root
        .as_ref()
        .and_then(|root| {
            state
                .ui
                .branches_tree
                .selected
                .as_ref()
                .map(|name| SharedString::from(format!("{}:branch:local:{name}", root.name())))
        });

    TreeModel {
        rows,
        info,
        expanded,
        selected,
        any_data,
        matches,
    }
}

/// The cheap key a tick compares before rebuilding the held tree.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TreeKey {
    revision: u64,
    evidence: u64,
    branches: Vec<usize>,
    remotes: Vec<usize>,
    tag_counts: Vec<usize>,
    options: (bool, bool, bool),
    filter: String,
    scope: Option<String>,
    collapsed_repos: Vec<String>,
    collapsed_remotes: Vec<(String, String)>,
    revealed: Vec<String>,
    groups: (bool, bool, bool),
    selected: Option<String>,
    busy: bool,
}

fn tree_key(state: &AppState) -> TreeKey {
    let mut collapsed_repos: Vec<String> = state
        .ui
        .branches_tree
        .collapsed_repos
        .iter()
        .map(|id| id.name())
        .collect();
    collapsed_repos.sort();
    let mut revealed: Vec<String> = state
        .ui
        .branches_tree
        .remotes_revealed
        .iter()
        .map(|id| id.name())
        .collect();
    revealed.sort();
    let mut collapsed_remotes: Vec<(String, String)> = state
        .ui
        .branches_tree
        .collapsed_remotes
        .iter()
        .map(|(root, remote)| (root.name(), remote.clone()))
        .collect();
    collapsed_remotes.sort();
    TreeKey {
        revision: state.caches.revision(),
        evidence: state.caches.evidence_revision(),
        branches: state.multi.roots.iter().map(|r| r.branches.len()).collect(),
        remotes: state.multi.roots.iter().map(|r| r.remotes.len()).collect(),
        tag_counts: state
            .multi
            .roots
            .iter()
            .map(|r| state.caches.tags(&r.id).map_or(0, <[_]>::len))
            .collect(),
        options: (
            state.ui.branches_view.group_by_directory,
            state.ui.branches_view.group_by_repository,
            state.ui.branches_view.show_tags,
        ),
        filter: state.ui.branches_filter.clone(),
        scope: state.ui.branches_repo_filter.as_ref().map(|id| id.name()),
        collapsed_repos,
        collapsed_remotes,
        revealed,
        groups: (
            state.ui.branches_tree.groups.local,
            state.ui.branches_tree.groups.remote,
            state.ui.branches_tree.groups.tags,
        ),
        selected: state.ui.branches_tree.selected.clone(),
        busy: state.ui.busy,
    }
}

/// The Branches tab over a live state.
pub fn branches_screen(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    handle.update(|s| s.request_branch_tags());
    let model = signal(Arc::new(handle.peek(build_tree_model)));
    let key = signal(handle.peek(tree_key));

    let tick = handle.clone();
    let tick_model = model;
    let tick_key = key;
    set_interval(Duration::from_millis(TICK_MS), move || {
        let next = tick.read(tree_key);
        if next != tick_key.get() {
            tick_key.set(next);
            tick_model.set(Arc::new(tick.peek(build_tree_model)));
        }
    });

    let tree_handle = handle.clone();
    let tree_model = model;
    let banner_handle = handle.clone();
    let banner_model = model;
    let content_handle = handle.clone();
    let content_model = model;

    view()
        .flex_1()
        .flex_col()
        .bg(t.background)
        .test_id(&test_ids::route("branches"))
        .child(toolbar(handle.clone()))
        .child(undo_banner(banner_handle, banner_model))
        .child(dynamic(move || {
            let held = content_model.get();
            if !held.any_data {
                return empty_state(content_handle.clone()).into_element();
            }
            let filtering = !content_handle.peek(|s| s.ui.branches_filter.trim().is_empty());
            if filtering && held.matches == 0 {
                return no_match_state(content_handle.clone()).into_element();
            }
            branch_tree(tree_handle.clone(), tree_model).into_element()
        }))
}

/// The toolbar: search, the scope chip, settings, and New Branch.
fn toolbar(handle: AppHandle) -> impl IntoElement {
    let search_handle = handle.clone();
    let new_handle = handle.clone();
    let settings_handle = handle.clone();
    let multi = handle.peek(|s| s.multi.roots.len() > 1);
    let mut bar = view()
        .flex_row()
        .items_center()
        .gap(px(data::SPACING_X))
        .px(px(data::PANEL_PADDING))
        .py(px(data::SPACING_Y))
        .child(
            view().flex_1().child(input(
                InputProps::new()
                    .placeholder("Search branches")
                    .accessibility_label("Search branches")
                    .on_change(move |text: &str| {
                        let text = text.to_string();
                        search_handle.update(move |s| s.ui.branches_filter = text);
                    })
                    .test_id(&test_ids::branches("search")),
            )),
        );
    if multi {
        bar = bar.child(scope_chip(handle.clone()));
    }
    bar.child(settings_popover(settings_handle))
        .child({
            let fetch_handle = handle.clone();
            let fetch_root_handle = handle.clone();
            button(
                ButtonProps::new("Fetch")
                    .variant(ButtonVariant::Ghost)
                    .test_id(&test_ids::branches("fetch"))
                    .on_press(move || {
                        let root = fetch_root_handle.peek(|s| {
                            s.ui.branches_repo_filter
                                .clone()
                                .or_else(|| s.multi.roots.first().map(|r| r.id.clone()))
                        });
                        if let Some(root) = root {
                            fetch_handle.update(move |s| fetch_root(s, &root));
                        }
                    }),
            )
        })
        .child({
            let pull_handle = handle.clone();
            let pull_root_handle = handle.clone();
            button(
                ButtonProps::new("Pull")
                    .variant(ButtonVariant::Ghost)
                    .test_id(&test_ids::branches("pull"))
                    .on_press(move || {
                        let root = pull_root_handle.peek(|s| {
                            s.ui.branches_repo_filter
                                .clone()
                                .or_else(|| s.multi.roots.first().map(|r| r.id.clone()))
                        });
                        if let Some(root) = root {
                            pull_handle.update(move |s| s.dispatch_pull("Pull", root, None));
                        }
                    }),
            )
        })
        .child({
            let push_handle = handle.clone();
            button(
                ButtonProps::new("Push…")
                    .variant(ButtonVariant::Secondary)
                    .test_id(&test_ids::branches("push"))
                    .on_press(move || push_handle.update(|s| s.open_push())),
            )
        })
        .child({
            let tag_handle = handle.clone();
            button(
                ButtonProps::new("Tag…")
                    .variant(ButtonVariant::Secondary)
                    .test_id(&test_ids::branches("tag"))
                    .on_press(move || tag_handle.update(|s| s.open_tag_dialog(None, None))),
            )
        })
        .child(button(
            ButtonProps::new("New Branch")
                .variant(ButtonVariant::Primary)
                .test_id(&test_ids::branches("new"))
                .on_press(move || {
                    new_handle.update(|s| s.open_new_branch(None, NewBranchBase::Head, true, None));
                }),
        ))
}

/// The scope chip: the repository filter, with its picker.
fn scope_chip(handle: AppHandle) -> impl IntoElement {
    let label_handle = handle.clone();
    let sweep_handle = handle.clone();
    let rows_handle = handle.clone();
    let label = move || {
        label_handle.read(|s| match &s.ui.branches_repo_filter {
            Some(id) => format!("filtered to {}", id.name()),
            None => format!("all {} repos", s.multi.roots.len()),
        })
    };
    let trigger = view()
        .role(AccessibilityRole::Button)
        .label("Branch scope")
        .test_id(&test_ids::branches("scope"))
        .px(px(data::DENSITY_COMPACT_BUTTON_PAD_X))
        .py(px(data::DENSITY_COMPACT_BUTTON_PAD_Y))
        .rounded(px(data::CONTROL_RADIUS))
        .bg(theme().secondary)
        .child(text(move || SharedString::from(label())).text_size(px(data::TYPE_CONTROL)));
    let roots = rows_handle.peek(|s| {
        s.multi
            .roots
            .iter()
            .map(|root| root.id.clone())
            .collect::<Vec<_>>()
    });
    let mut content = view()
        .flex_col()
        .gap(px(data::DENSITY_LIST_GAP))
        .child(scope_row(
            sweep_handle.clone(),
            "All repos".to_string(),
            None,
            rows_handle.peek(|s| s.ui.branches_repo_filter.is_none()),
        ));
    for root in roots {
        let name = root.name();
        let active = rows_handle.peek(|s| s.ui.branches_repo_filter.as_ref() == Some(&root));
        content = content.child(scope_row(
            sweep_handle.clone(),
            format!("Show {name}"),
            Some(root),
            active,
        ));
    }
    popover(PopoverProps::new(trigger).width(240.0).child(content))
}

fn scope_row(
    handle: AppHandle,
    label: String,
    root: Option<RootId>,
    active: bool,
) -> impl IntoElement {
    let press_handle = handle;
    let key = test_ids::branches(format!("scope-{label}"));
    row(RowProps::new()
        .state(if active {
            RowState::Selected
        } else {
            RowState::Default
        })
        .test_id(&key)
        .on_press(move || {
            let root = root.clone();
            press_handle.update(move |s| {
                s.ui.branches_repo_filter = root.clone();
                s.ui.branches_scope_picker_open = false;
            });
        })
        .child(text(label).text_size(px(data::TYPE_CONTROL))))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ToggleRow {
    Directory,
    Repository,
    Tags,
}

impl ToggleRow {
    fn label(self) -> &'static str {
        match self {
            ToggleRow::Directory => "Group by Directory",
            ToggleRow::Repository => "Group by Repository",
            ToggleRow::Tags => "Show Tags",
        }
    }

    fn id(self) -> &'static str {
        match self {
            ToggleRow::Directory => "toggle-directory",
            ToggleRow::Repository => "toggle-repository",
            ToggleRow::Tags => "toggle-tags",
        }
    }
}

fn toggle_row(handle: AppHandle, payload: ToggleRow, on: bool) -> impl IntoElement {
    let t = theme();
    let press_handle = handle;
    row(RowProps::new()
        .state(RowState::Default)
        .test_id(&test_ids::branches(payload.id()))
        .on_press(move || {
            press_handle.update(move |s| {
                let options = &mut s.ui.branches_view;
                match payload {
                    ToggleRow::Directory => {
                        options.group_by_directory = !options.group_by_directory
                    }
                    ToggleRow::Repository => {
                        options.group_by_repository = !options.group_by_repository
                    }
                    ToggleRow::Tags => options.show_tags = !options.show_tags,
                }
                s.save_durable_ui();
            });
        })
        .child(
            text(format!(
                "{} {}",
                if on { "☑" } else { "☐" },
                payload.label()
            ))
            .text_size(px(data::TYPE_CONTROL))
            .text_color(t.foreground),
        ))
}

/// The gear's three toggles, and the durable save after a change.
fn settings_popover(handle: AppHandle) -> impl IntoElement {
    let view_handle = handle.clone();
    let settings_handle = handle.clone();
    let options = handle.peek(|s| s.ui.branches_view);
    let trigger = icon_button(
        IconButtonProps::new("⚙", "Branch list settings").test_id(&test_ids::branches("settings")),
    );
    let props = PopoverProps::new(trigger)
        .width(300.0)
        .child(
            view()
                .flex_col()
                .gap(px(data::DENSITY_LIST_GAP))
                .test_id(&test_ids::branches("settings-menu"))
                .child(toggle_row(view_handle.clone(), ToggleRow::Directory, options.group_by_directory))
                .child(toggle_row(view_handle.clone(), ToggleRow::Repository, options.group_by_repository))
                .child(toggle_row(view_handle, ToggleRow::Tags, options.show_tags))
                .child(
                    text("Group by Directory nests folder/ inside a header; Group by Repository puts one set per repository in scope; Show Tags lists tags in their own group.")
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(theme().muted_foreground),
                ),
        )
        .on_open_change(move |open| {
            settings_handle.update(move |s| s.ui.branches_settings_open = open);
        });
    popover(props)
}

/// The undo strip under the toolbar: the deleted name, the unmerged link,
/// the remote-cleanup offer, and Undo.
fn undo_banner(handle: AppHandle, model: Signal<Arc<TreeModel>>) -> impl IntoElement {
    let t = theme();
    let paint_handle = handle.clone();
    when(
        move || {
            paint_handle.read(|s| {
                s.ui.branches_undo
                    .as_ref()
                    .is_some_and(|undo| undo.created_at.elapsed() <= UNDO_WINDOW)
            })
        },
        move || undo_banner_body(handle.clone(), model, t.muted_foreground),
    )
}

fn undo_banner_body(
    handle: AppHandle,
    model: Signal<Arc<TreeModel>>,
    ink: AdaptiveColor,
) -> impl IntoElement {
    let read_handle = handle.clone();
    let (name, link, unmerged, upstream, root) = read_handle.peek(|s| {
        let undo = s.ui.branches_undo.clone().expect("the strip is showing");
        (
            undo.name.clone(),
            undo.unmerged.as_ref().map(|u| u.link_label()),
            undo.unmerged.clone(),
            undo.upstream.clone(),
            undo.root.clone(),
        )
    });
    let unmerged_handle = read_handle.clone();
    let upstream_handle = read_handle.clone();
    let undo_handle = read_handle.clone();
    let undo_model = model;
    let mut strip = view()
        .flex_row()
        .items_center()
        .gap(px(data::SPACING_X))
        .px(px(data::PANEL_PADDING))
        .py(px(data::DENSITY_DENSE_BUTTON_PAD_Y))
        .test_id(&test_ids::branches("undo"))
        .child(
            text(format!("Deleted '{name}'"))
                .text_size(px(data::TYPE_CONTROL))
                .text_color(ink),
        );
    if let Some(link) = link {
        strip = strip.child(
            view()
                .role(AccessibilityRole::Button)
                .label(link.clone())
                .test_id(&test_ids::branches("unmerged-link"))
                .on_press(move || {
                    let unmerged = unmerged.clone();
                    unmerged_handle.update(move |s| {
                        if let Some(unmerged) = unmerged.clone() {
                            s.open_unmerged_commits(&unmerged);
                        }
                    });
                })
                .child(
                    text(link)
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(data::ACCENT_TEXT),
                ),
        );
    }
    if let Some(upstream) = upstream {
        let label = format!("Remove the tracking remote branch {}", upstream.git_ref());
        strip = strip.child(
            view()
                .role(AccessibilityRole::Button)
                .label(label.clone())
                .test_id(&test_ids::branches("remove-remote"))
                .on_press(move || {
                    let upstream = upstream.clone();
                    let root = root.clone();
                    upstream_handle.update(move |s| {
                        s.ui.confirm = Some(PendingConfirm::DeleteRemoteBranch {
                            root: root.clone(),
                            remote: upstream.remote.clone(),
                            name: upstream.branch.clone(),
                        });
                    });
                })
                .child(
                    text(label)
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(ink),
                ),
        );
    }
    strip.child(view().flex_1()).child(button(
        ButtonProps::new("Undo delete")
            .variant(ButtonVariant::Secondary)
            .test_id(&test_ids::branches("undo-action"))
            .on_press(move || {
                undo_handle.update(|s| {
                    if let Some(undo) = s.ui.branches_undo.take() {
                        let op =
                            shell_writes::branch::restore(&undo.root, &undo.name, &undo.tip_sha);
                        s.dispatch(op);
                    }
                });
                let next = undo_handle.read(build_tree_model);
                undo_model.set(Arc::new(next));
            }),
    ))
}

fn empty_state(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let busy = handle.peek(|s| s.ui.busy);
    let create_handle = handle;
    let mut state = view()
        .flex_1()
        .items_center()
        .justify_center()
        .test_id(&test_ids::branches("empty"))
        .child(
            view()
                .flex_col()
                .items_center()
                .gap(px(data::SPACING_Y))
                .child(
                    text(if busy {
                        "reading branches…"
                    } else {
                        "This repo has no branches yet"
                    })
                    .text_size(px(data::TYPE_BODY))
                    .text_color(t.foreground),
                ),
        );
    if !busy {
        state = state.child(button(
            ButtonProps::new("Create the first branch")
                .variant(ButtonVariant::Primary)
                .test_id(&test_ids::branches("create-first"))
                .on_press(move || {
                    create_handle
                        .update(|s| s.open_new_branch(None, NewBranchBase::Head, true, None));
                }),
        ));
    }
    state
}

fn no_match_state(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let query = handle.peek(|s| s.ui.branches_filter.trim().to_string());
    let create_handle = handle;
    let create_query = query.clone();
    view()
        .flex_1()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .p(px(data::PANEL_PADDING))
        .test_id(&test_ids::branches("no-match"))
        .child(
            text(format!("no branch called {query}. Create it?"))
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground),
        )
        .child(button(
            ButtonProps::new(format!("Create \"{query}\""))
                .variant(ButtonVariant::Secondary)
                .test_id(&test_ids::branches("create-query"))
                .on_press(move || {
                    let name = create_query.clone();
                    create_handle.update(move |s| {
                        s.open_new_branch(None, NewBranchBase::Head, true, Some(name))
                    });
                }),
        ))
}

/// The tree itself: the held rows, the app's expansion and selection, and
/// the row paint with the menu on a secondary press.
fn branch_tree(handle: AppHandle, model: Signal<Arc<TreeModel>>) -> impl IntoElement {
    let items = model.get().rows.clone();
    let select_handle = handle.clone();
    let action_handle = handle.clone();
    let expand_handle = handle;
    let render_handle = action_handle.clone();
    let rows = model;
    let props = TreeProps::new(items)
        .accessibility_label("Branches")
        .test_id(&test_ids::branches("tree"))
        .expanded(Prop::bind(move || Some(model.get().expanded.clone())))
        .selected(Prop::bind(move || match model.get().selected.clone() {
            Some(id) => SelectValue::Controlled(Some(id)),
            None => SelectValue::Controlled(None),
        }))
        .on_expanded_change(move |next: &[SharedString]| {
            let next: Vec<String> = next.iter().map(|id| id.to_string()).collect();
            expand_handle.update(move |s| reconcile_expansion(s, &next));
        })
        .on_select(move |id: &str| {
            let id = id.to_string();
            select_handle.update(move |s| select_row(s, &id));
        })
        .on_action(move |id: &str| {
            let id = id.to_string();
            action_handle.update(move |s| activate_row(s, &id));
        })
        .render_item(move |entry: &nsh_base::TreeEntry| {
            let info = rows.with(|model| model.info.get(entry.id.as_str()).cloned());
            paint_row(render_handle.clone(), entry, info)
        });
    tree(props)
}

/// One row's paint: the band, the label, the branch's state words, and the
/// secondary press that opens the row menu.
fn paint_row(handle: AppHandle, entry: &nsh_base::TreeEntry, info: Option<RowInfo>) -> AnyElement {
    let t = theme();
    let depth = entry.depth;
    let indent = depth as f32 * data::INDENT;
    match info {
        Some(RowInfo::Leaf {
            root,
            label,
            branch,
            active,
            selected,
        }) => {
            let now = chrono::Utc::now();
            let stale = is_stale(&branch, now);
            let ink = if branch.kind == BranchKind::Remote || stale {
                t.muted_foreground
            } else {
                t.foreground
            };
            let badge = branch
                .tracking
                .as_ref()
                .map(|_| sync_words(branch.ahead, branch.behind, branch.gone))
                .unwrap_or_default();
            let upstream = branch.tracking.as_ref().map(|up| up.git_ref());
            let menu_handle = handle.clone();
            let menu_root = root.clone();
            let menu_branch = branch.clone();
            let mut props = RowProps::new()
                .state(if selected || active {
                    RowState::Selected
                } else {
                    RowState::Default
                })
                .test_id(&test_ids::branch_row(&root.name(), &branch.name))
                .accessibility_label(branch.name.clone())
                .on_context_menu(move |event: &ContextMenuEvent| {
                    open_branch_menu(&menu_handle, &menu_root, &menu_branch, event.page_position);
                })
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .pl(px(indent))
                        .child(text("⑂").text_size(px(data::TYPE_CONTROL)).text_color(ink))
                        .child(
                            text(label)
                                .text_size(px(data::TYPE_CONTROL))
                                .text_color(ink),
                        ),
                );
            if active {
                props = props.child(
                    text("✓")
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(data::ACCENT_TEXT),
                );
            }
            props = props.child(view().flex_1());
            if let Some(upstream) = upstream {
                props = props.child(
                    text(upstream)
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(t.muted_foreground),
                );
            }
            if let Some(badge) = badge {
                props = props.child(text(badge).text_size(px(data::TYPE_CHIP)).text_color(
                    if branch.gone {
                        data::STATE_ERROR
                    } else {
                        t.muted_foreground
                    },
                ));
            }
            if let Some(touched) = branch.last_touched {
                let age = now
                    .signed_duration_since(touched)
                    .to_std()
                    .unwrap_or_default();
                props = props.child(
                    text(stale_badge(age))
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(t.muted_foreground),
                );
            }
            row(props).into_element()
        }
        Some(RowInfo::Repo {
            root,
            name,
            current,
            summary,
            count,
            expanded,
        }) => {
            let fold_handle = handle;
            let fold_key = entry.id.to_string();
            let mut props = RowProps::new()
                .state(RowState::Default)
                .test_id(&test_ids::branch_band(&root.name(), "repo"))
                .accessibility_label(name.clone())
                .on_press(move || {
                    let key = fold_key.clone();
                    fold_handle.update(move |s| toggle_expansion(s, &key));
                })
                .child(
                    text(format!("{} {name}", if expanded { "▾" } else { "▸" }))
                        .text_size(px(data::TYPE_CONTROL))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.foreground),
                );
            if let Some(current) = current {
                props = props.child(
                    text(current)
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(data::ACCENT_TEXT),
                );
            }
            if !summary.is_empty() {
                props = props.child(
                    text(summary)
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(t.muted_foreground),
                );
            }
            row(props.child(view().flex_1()).child(
                text(count.to_string())
                    .text_size(px(data::TYPE_CHIP))
                    .text_color(t.muted_foreground),
            ))
            .into_element()
        }
        Some(RowInfo::LocalGroup { count, open }) => band_row(
            handle,
            &entry.id,
            format!("{} LOCAL ({count})", if open { "▾" } else { "▸" }),
            "local",
        ),
        Some(RowInfo::RemoteHeader { root, rollup, open }) => {
            let label = if rollup.is_empty() {
                format!("{} REMOTE", if open { "▾" } else { "▸" })
            } else {
                format!("{} REMOTE · {rollup}", if open { "▾" } else { "▸" })
            };
            band_row_at(handle, &entry.id, label, &root, "remote")
        }
        Some(RowInfo::RemoteGroup {
            root,
            remote,
            count,
            open,
        }) => band_row_at(
            handle,
            &entry.id,
            format!("{} {remote} ({count})", if open { "▾" } else { "▸" }),
            &root,
            &format!("remote-{remote}"),
        ),
        Some(RowInfo::TagsGroup { count, open }) => band_row(
            handle,
            &entry.id,
            format!("{} TAGS ({count})", if open { "▾" } else { "▸" }),
            "tags",
        ),
        Some(RowInfo::Dir { label, count }) => {
            let t = theme();
            row(RowProps::new()
                .state(RowState::Default)
                .test_id(&test_ids::branches(format!("dir-{label}")))
                .child(
                    text(format!("{label}/ ({count})"))
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.muted_foreground)
                        .pl(px(indent)),
                ))
            .into_element()
        }
        Some(RowInfo::Tag { name, state }) => {
            let marker = match state {
                RefState::Default => "",
                RefState::Gone => "gone",
                RefState::Pushed => "pushed",
                RefState::LocalOnly => "local only",
            };
            row(RowProps::new()
                .state(RowState::Default)
                .test_id(&test_ids::branches(format!("tag-{name}")))
                .child(
                    text(name)
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.muted_foreground)
                        .pl(px(indent)),
                )
                .child(view().flex_1())
                .child(
                    text(marker)
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(match state {
                            RefState::Gone => data::STATE_ERROR,
                            RefState::Pushed => data::STATE_SUCCESS,
                            RefState::LocalOnly => data::STATE_WARNING,
                            RefState::Default => t.muted_foreground,
                        }),
                ))
            .into_element()
        }
        None => view().into_element(),
    }
}

fn band_row(handle: AppHandle, id: &str, label: String, key: &str) -> AnyElement {
    let t = theme();
    let fold_handle = handle;
    let fold_key = id.to_string();
    let test = test_ids::branches(format!("band-{key}"));
    row(RowProps::new()
        .state(RowState::Default)
        .test_id(&test)
        .accessibility_label(label.clone())
        .on_press(move || {
            let key = fold_key.clone();
            fold_handle.update(move |s| toggle_expansion(s, &key));
        })
        .child(
            text(label)
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground),
        ))
    .into_element()
}

fn band_row_at(handle: AppHandle, id: &str, label: String, root: &RootId, key: &str) -> AnyElement {
    let t = theme();
    let fold_handle = handle;
    let fold_key = id.to_string();
    let test = test_ids::branch_band(&root.name(), key);
    row(RowProps::new()
        .state(RowState::Default)
        .test_id(&test)
        .accessibility_label(label.clone())
        .on_press(move || {
            let key = fold_key.clone();
            fold_handle.update(move |s| toggle_expansion(s, &key));
        })
        .child(
            text(label)
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground),
        ))
    .into_element()
}

/// The expansion a press made: the ids the tree now holds, compared with the
/// model it was built from, so one press toggles exactly one fold.
fn reconcile_expansion(state: &mut AppState, next: &[String]) {
    let held = build_tree_model(state);
    let before: HashSet<String> = held.expanded.iter().map(|id| id.to_string()).collect();
    let after: HashSet<String> = next.iter().cloned().collect();
    let changed: Vec<String> = before.symmetric_difference(&after).cloned().collect();
    for id in changed {
        toggle_expansion(state, &id);
    }
}

/// Toggles the fold the id names: a repository, the local group, a remote
/// rollup or remote group, or the tags group.
fn toggle_expansion(state: &mut AppState, id: &str) {
    if let Some(rest) = id.strip_prefix("repo:") {
        if let Some(root) = root_of_id(state, rest)
            && !state.ui.branches_tree.collapsed_repos.remove(&root)
        {
            state.ui.branches_tree.collapsed_repos.insert(root);
        }
        return;
    }
    if id.contains(":group:local") {
        state.ui.branches_tree.groups.local = !state.ui.branches_tree.groups.local;
        state.save_durable_ui();
        return;
    }
    if id.contains(":group:tags") {
        state.ui.branches_tree.groups.tags = !state.ui.branches_tree.groups.tags;
        state.save_durable_ui();
        return;
    }
    if id.contains(":group:remote") {
        if let Some(root) = id
            .split_once(":group:remote")
            .and_then(|(name, _)| root_of_id(state, name))
            && !state.ui.branches_tree.remotes_revealed.remove(&root)
        {
            state.ui.branches_tree.remotes_revealed.insert(root);
        }
        return;
    }
    if let Some((name, remote)) = id.split_once(":remote:")
        && let Some(root) = root_of_id(state, name)
    {
        let key = (root, remote.to_string());
        if !state.ui.branches_tree.collapsed_remotes.remove(&key) {
            state.ui.branches_tree.collapsed_remotes.insert(key);
        }
    }
}

fn root_of_id(state: &AppState, name: &str) -> Option<RootId> {
    state
        .multi
        .roots
        .iter()
        .find(|root| root.id.name() == name)
        .map(|root| root.id.clone())
}

/// Selects the row: a leaf writes its root and name; bands clear the
/// selection.
fn select_row(state: &mut AppState, id: &str) {
    let Some((root_name, rest)) = id.split_once(":branch:") else {
        state.ui.branches_tree.selected = None;
        state.ui.branches_tree.selected_root = None;
        return;
    };
    let name = rest.split_once(':').map_or(rest, |(_, name)| name);
    if let Some(root) = root_of_id(state, root_name) {
        state.ui.branches_tree.selected = Some(name.to_string());
        state.ui.branches_tree.selected_root = Some(root);
        state.ui.branches_tree.context_menu = None;
    }
}

/// Activates the row: a leaf checks out; a band toggles.
fn activate_row(state: &mut AppState, id: &str) {
    let Some((root_name, rest)) = id.split_once(":branch:") else {
        toggle_expansion(state, id);
        return;
    };
    let name = rest.split_once(':').map_or(rest, |(_, name)| name);
    let Some(root) = root_of_id(state, root_name) else {
        return;
    };
    let Some(branch) = state
        .multi
        .by_id(&root)
        .and_then(|r| r.branches.iter().find(|b| b.name == name))
        .cloned()
    else {
        return;
    };
    checkout_branch(state, &root, &branch);
}

/// The check-out verb with its two preflights: a branch held by another
/// worktree, and a dirty tree.
pub fn checkout_branch(state: &mut AppState, root: &RootId, branch: &Branch) {
    if let Some(worktree) = state
        .caches
        .worktrees(root)
        .into_iter()
        .flatten()
        .find(|wt| wt.branch == branch.name)
    {
        state.ui.confirm = Some(PendingConfirm::CheckoutInWorktree {
            branch: branch.name.clone(),
            worktree: worktree.path.clone(),
        });
        return;
    }
    let dirty = state
        .multi
        .by_id(root)
        .is_some_and(|r| r.status.modified() > 0);
    if dirty {
        state.ui.confirm = Some(PendingConfirm::CheckoutDirty {
            root: root.clone(),
            target: branch.name.clone(),
            kind: branch.kind,
            detach: false,
        });
        return;
    }
    state.checkout_branch_op(root, branch.kind, &branch.name);
}

/// The branch menu's twelve verbs in the designed order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BranchMenuAction {
    CopyBranchName,
    Checkout,
    NewBranchFrom,
    CheckoutAndPull,
    Merge,
    Rebase,
    Compare,
    DiffWithWorkingTree,
    Pull,
    Push,
    Rename,
    Delete,
}

impl BranchMenuAction {
    pub const ORDER: [Self; 12] = [
        Self::CopyBranchName,
        Self::Checkout,
        Self::NewBranchFrom,
        Self::CheckoutAndPull,
        Self::Merge,
        Self::Rebase,
        Self::Compare,
        Self::DiffWithWorkingTree,
        Self::Pull,
        Self::Push,
        Self::Rename,
        Self::Delete,
    ];

    pub const fn index(self) -> usize {
        match self {
            Self::CopyBranchName => 0,
            Self::Checkout => 1,
            Self::NewBranchFrom => 2,
            Self::CheckoutAndPull => 3,
            Self::Merge => 4,
            Self::Rebase => 5,
            Self::Compare => 6,
            Self::DiffWithWorkingTree => 7,
            Self::Pull => 8,
            Self::Push => 9,
            Self::Rename => 10,
            Self::Delete => 11,
        }
    }
}

/// One menu item's gate: enabled, or disabled with its reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuState {
    pub enabled: bool,
    pub reason: Option<&'static str>,
}

impl MenuState {
    pub fn enabled() -> Self {
        MenuState {
            enabled: true,
            reason: None,
        }
    }

    pub fn disabled(reason: &'static str) -> Self {
        MenuState {
            enabled: false,
            reason: Some(reason),
        }
    }
}

/// The twelve items' gates, in [`BranchMenuAction::ORDER`].
pub fn branch_menu_items(branch: &Branch, current: Option<&str>) -> [MenuState; 12] {
    let is_current = current == Some(branch.name.as_str());
    let is_local = branch.kind == BranchKind::Local;
    let has_upstream = branch.tracking.is_some() && !branch.gone;
    let mut items: [MenuState; 12] = std::array::from_fn(|_| MenuState::enabled());
    items[BranchMenuAction::Checkout.index()] = if is_current {
        MenuState::disabled("already checked out")
    } else {
        MenuState::enabled()
    };
    items[BranchMenuAction::CheckoutAndPull.index()] = if is_current {
        MenuState::disabled("already checked out")
    } else if !has_upstream {
        MenuState::disabled("no upstream to pull from")
    } else if is_local {
        MenuState::disabled("pull is offered on remote branches")
    } else {
        MenuState::enabled()
    };
    items[BranchMenuAction::Merge.index()] = if is_current {
        MenuState::disabled("this is the current branch")
    } else if !is_local {
        MenuState::disabled("remote branches cannot be merged")
    } else {
        MenuState::enabled()
    };
    items[BranchMenuAction::Rebase.index()] = if is_current {
        MenuState::disabled("this is the current branch")
    } else if !is_local {
        MenuState::disabled("remote branches cannot be rebased")
    } else {
        MenuState::enabled()
    };
    items[BranchMenuAction::Compare.index()] = if is_current {
        MenuState::disabled("this is the current branch")
    } else {
        MenuState::enabled()
    };
    items[BranchMenuAction::DiffWithWorkingTree.index()] = if is_current {
        MenuState::disabled("this is the current branch")
    } else {
        MenuState::enabled()
    };
    items[BranchMenuAction::Pull.index()] = if !is_current {
        MenuState::disabled("check out this branch first")
    } else if !has_upstream {
        MenuState::disabled("no upstream to pull from")
    } else if is_local {
        MenuState::disabled("pull is offered on remote branches")
    } else {
        MenuState::enabled()
    };
    items[BranchMenuAction::Push.index()] = if branch.ahead > 0 || branch.tracking.is_none() {
        MenuState::enabled()
    } else if branch.gone {
        MenuState::disabled("upstream is gone")
    } else {
        MenuState::disabled("nothing to push")
    };
    items[BranchMenuAction::Rename.index()] = if is_local {
        MenuState::enabled()
    } else {
        MenuState::disabled("remote branches cannot be renamed")
    };
    items[BranchMenuAction::Delete.index()] = if is_current {
        MenuState::disabled("the current branch cannot be deleted")
    } else {
        MenuState::enabled()
    };
    items
}

/// The labels the twelve items paint, directed at the current branch.
pub fn branch_menu_labels(
    _branch: &Branch,
    current: Option<&str>,
    repo: &str,
    multi: bool,
) -> Vec<String> {
    let directed = |verb: &str, stem: &str| match current {
        Some(current) => format!("{verb} {current}"),
        None => stem.to_string(),
    };
    vec![
        "Copy branch name".to_string(),
        if multi {
            format!("Checkout in {repo}")
        } else {
            "Checkout".to_string()
        },
        "New branch from".to_string(),
        "Checkout and pull".to_string(),
        directed("Merge into", "Merge"),
        directed("Rebase onto", "Rebase"),
        directed("Compare with", "Compare"),
        "Show Diff with Working Tree".to_string(),
        "Pull".to_string(),
        "Push".to_string(),
        "Rename branch".to_string(),
        "Delete branch".to_string(),
    ]
}

/// The row menu at `at`, the gates read off the branch's own facts.
fn open_branch_menu(handle: &AppHandle, root: &RootId, branch: &Branch, at: Point<Points>) {
    let (current, multi) = handle.peek(|s| {
        (
            s.multi.by_id(root).and_then(|r| r.current_branch.clone()),
            s.multi.roots.len() > 1,
        )
    });
    let repo = root.name();
    let states = branch_menu_items(branch, current.as_deref());
    let labels = branch_menu_labels(branch, current.as_deref(), &repo, multi);
    let mut items: Vec<MenuItem> = Vec::new();
    for (action, (label, state)) in BranchMenuAction::ORDER
        .iter()
        .zip(labels.into_iter().zip(states))
    {
        let mut item = MenuItem::new(label);
        if !state.enabled {
            item = item.disabled(true);
        } else {
            let opener = handle.clone();
            let action = *action;
            let owned_root = root.clone();
            let owned_branch = branch.clone();
            item = item.on_select(move || {
                let root = owned_root.clone();
                let branch = owned_branch.clone();
                opener.update(move |s| apply_branch_action(s, &root, &branch, action));
            });
        }
        items.push(item);
    }
    spawn(async move {
        let _ = nsh_core::element::desktop::popup_menu_at(items, at).await;
    });
}

/// Runs one branch verb through the state layer's operations and confirms.
pub fn apply_branch_action(
    state: &mut AppState,
    root: &RootId,
    branch: &Branch,
    action: BranchMenuAction,
) {
    match action {
        BranchMenuAction::CopyBranchName => {
            nsh_core::element::set_clipboard(branch.name.clone());
            state.ui.toast = Some(Toast::success(format!("Copied {}", branch.name)));
        }
        BranchMenuAction::Checkout => checkout_branch(state, root, branch),
        BranchMenuAction::NewBranchFrom => state.open_new_branch(
            Some(root.clone()),
            NewBranchBase::Branch(BranchSpec::from(branch)),
            true,
            None,
        ),
        BranchMenuAction::CheckoutAndPull => {
            state.dispatch_pull("Checkout and pull", root.clone(), Some(branch.name.clone()));
        }
        BranchMenuAction::Pull => state.dispatch_pull("Pull", root.clone(), None),
        BranchMenuAction::Push => state.open_push_for_branch(root, &branch.name),
        BranchMenuAction::Rename => state.open_rename_branch(root, &branch.name),
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
        BranchMenuAction::DiffWithWorkingTree => state.open_get_from_branch(root, &branch.name),
        BranchMenuAction::Delete => match branch.kind {
            BranchKind::Remote => {
                let (remote, name) = branch
                    .name
                    .split_once('/')
                    .map(|(remote, name)| (remote.to_string(), name.to_string()))
                    .unwrap_or(("origin".into(), branch.name.clone()));
                state.ui.confirm = Some(PendingConfirm::DeleteRemoteBranch {
                    root: root.clone(),
                    remote,
                    name,
                });
            }
            BranchKind::Local => {
                if let Some(worktree) = state
                    .caches
                    .worktrees(root)
                    .into_iter()
                    .flatten()
                    .find(|wt| wt.branch == branch.name)
                {
                    state.ui.confirm = Some(PendingConfirm::CheckoutInWorktree {
                        branch: branch.name.clone(),
                        worktree: worktree.path.clone(),
                    });
                    return;
                }
                let current = state
                    .multi
                    .by_id(root)
                    .and_then(|r| r.current_branch.clone());
                let count = state.unmerged_commit_count(root, &branch.name);
                state.ui.branches_delete_consequence = current.map(|current| match count {
                    Some(0) => format!(
                        "everything on this branch already exists on {current} — safe to delete"
                    ),
                    Some(n) => format!(
                        "this branch has {n} commit(s) not on {current} — they become unreachable after deleting"
                    ),
                    None => String::new(),
                });
                state.ui.branches_delete_unmerged = state.unmerged_commits(root, &branch.name);
                state.ui.confirm = Some(PendingConfirm::DeleteLocalBranch {
                    root: root.clone(),
                    name: branch.name.clone(),
                });
            }
        },
    }
}

/// The Fetch the repository band offers: one root, one operation.
pub fn fetch_root(state: &mut AppState, root: &RootId) {
    state.dispatch(Operation::Fetch {
        roots: vec![root.clone()],
    });
}
