//! Pure repo-grouped branches tree builder (Branches view redesign, issue 01).
//!
//! Turns the in-scope repos' already-classified branch snapshots (plus a
//! `show_remotes` flag and a per-repo tag list) into one grouped view model
//! whose outermost grouping is the repository. The builder never talks to git
//! and never crosses the engine seam — every input arrives already classified,
//! so it is unit-testable with constructed [`Root`]/[`Branch`] values and no
//! harness, no `AppState`, no git binary.
//!
//! Structural guarantees the renderer relies on:
//! - **Count invariant** — every directory node, remote group, and the collapsed
//!   rollup count is *derived from the children it will actually display*, never
//!   a separate estimate. `DirNode::count` is the number of leaf branches
//!   beneath it; `RemoteGroup::count` is its branches; the rollup's
//!   `M branches` equals the sum of remote-group counts.
//! - **Prefix stripping is structural**, not string surgery: inside a remote
//!   group the leaf shows only the branch's distinguishing suffix (the remote
//!   prefix is dropped before the tree is built); inside a directory group the
//!   directory part is dropped from the leaf label.
//! - **Favorites pin to the top of their containing group** at every level.
//! - The checked-out branch is **marked `active`**; per-repo status is derived
//!   from snapshot data (dirty / unpushed / unpulled).

use std::collections::{HashMap, HashSet};

use crate::ui::components;
use turbogit_app::persistence::BranchesViewOptions;
use turbogit_domain::model::{Branch, BranchKind, RecentCheckout, RefState, Root, RootId};

/// How many recently checked-out branches the Recent group shows.
///
/// The single place the documented cap is written: "Recent branches node shows
/// up to five recently checked-out branches". It is
/// a display cap on a longer history, not a cache size — the engine answers
/// HEAD's whole checkout window — and both branch-recents surfaces read it, so
/// the number cannot drift between the tab and the status-bar popup.
pub const RECENT_CAP: usize = 5;

/// Build the Recent group from the repositories' reflog-derived checkout
/// histories.
///
/// `recents` maps a repository to the checkout answers its HEAD's reflog gave,
/// newest first — git's own history, which is why the group survives a restart
/// and shows a checkout made in a terminal. Three rules shape the list, in this
/// order:
///
/// - **join** against the repository's *local* branch snapshot. The reflog names
///   whatever HEAD moved onto, tags and raw shas included, and a branch deleted
///   since; only a name a click can act on becomes a row.
/// - **merge** across repositories by the checkout's own timestamp, so the group
///   is the repository set's most recent activity and not its first
///   repository's. The sort is stable, so equal timestamps (git's clock is
///   second-resolution) keep the repositories' listed order.
/// - **cap** at [`RECENT_CAP`] *after* the join and the merge — a history of
///   deleted branches shortens the group rather than padding it, and a longer
///   live history stops at five with nothing reserved for the sixth.
///
/// The label is the **full** branch name and there are no directory nodes: the
/// group is a list of branches, not a re-statement of the directory tree, and a
/// row whose repository is not the one being painted would be a lie if it read
/// like one.
pub fn build_recent_group(
    roots: &[Root],
    recents: &HashMap<RootId, Vec<RecentCheckout>>,
) -> Vec<BranchLeaf> {
    // Ordered by the move that earned the place, so the timestamp is dropped
    // once the cap has been taken: a row's ordering is not a fact a renderer or
    // an action ever needs.
    let mut placed: Vec<(i64, BranchLeaf)> = Vec::new();
    for root in roots {
        // The engine answers newest-first, so the first move seen for a branch
        // is its newest one and every later move of the same name is the same
        // row.
        for move_ in recents.get(&root.id).map_or(&[][..], Vec::as_slice) {
            if placed
                .iter()
                .any(|(_, r)| r.root == root.id && r.branch.name == move_.name)
            {
                continue;
            }
            let Some(branch) = root
                .branches
                .iter()
                .find(|b| b.kind == BranchKind::Local && b.name == move_.name && b.exists)
            else {
                continue;
            };
            placed.push((
                move_.at,
                BranchLeaf {
                    label: branch.name.clone(),
                    root: root.id.clone(),
                    branch: branch.clone(),
                    active: root.current_branch.as_deref() == Some(branch.name.as_str()),
                    favorite: branch.favorite,
                },
            ));
        }
    }
    placed.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    placed.truncate(RECENT_CAP);
    placed.into_iter().map(|(_, row)| row).collect()
}

/// One branch row in the grouped tree, after prefix stripping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchLeaf {
    /// Display label with the containing directory/remote prefix stripped.
    pub label: String,
    /// The repository this row belongs to.
    ///
    /// Carried by the row and not inherited from the enclosing
    /// [`RepoSection`], because the Recent group is one list whose rows come
    /// from several repositories: an action taken on a row in that group would
    /// otherwise act on the wrong repository's branch of the same name.
    pub root: RootId,
    /// The source branch — keeps the full name, kind, tracking, favorite…
    pub branch: Branch,
    /// Whether this is the repo's checked-out branch.
    pub active: bool,
    /// Whether the branch is starred/favorite (pins it to the top of its group).
    pub favorite: bool,
}

/// A directory node formed from slash-separated branch names.
///
/// `count` is the total number of leaf branches anywhere beneath this node —
/// it is derived from `children`, never estimated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirNode {
    /// The directory segment (e.g. `feature`), not the full path.
    pub label: String,
    /// Number of leaf branches beneath this node (the count invariant).
    pub count: usize,
    pub children: Vec<BranchNode>,
}

/// A node in a branch group's tree: a directory group or a leaf.
///
/// The leaf is boxed, exactly as the builder forest's own [`B::Leaf`] variant
/// boxes it: [`BranchLeaf`] carries a whole [`Branch`], so an inline leaf makes
/// every directory node pay the largest variant's size. The tree is built and
/// walked per frame, and no caller owns a leaf — the rows are read through the
/// node they sit in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BranchNode {
    Leaf(Box<BranchLeaf>),
    Dir(DirNode),
}

/// One tag row in the grouped tree: the tag's name and its per-ref state
/// (plan D8) — pushed / local-only, read from the ref decorations. The
/// pure `RefState` comes from the domain, so the view model carries it
/// without crossing a layer boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagLeaf {
    pub name: String,
    pub state: RefState,
}

/// One remote's group of branches (only emitted when `show_remotes`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteGroup {
    /// The remote's own name, git's spelling: the collapse key and the prefix
    /// the leaves are stripped of.
    pub remote: String,
    /// The repository this group belongs to — the same rule as
    /// [`BranchLeaf::root`], and for the same reason: one repository's collapsed
    /// `origin` must not collapse another's.
    pub root: RootId,
    /// What the header prints. The remote's name on its own while the
    /// repository is the grouping level, and the repository named alongside it
    /// once repository grouping is turned off — two repositories each with an
    /// `origin` otherwise paint two identical headers with nothing to tell them
    /// apart.
    pub label: String,
    /// Number of branches in this group (the count invariant).
    pub count: usize,
    pub children: Vec<BranchNode>,
}

/// Branch headers and sidebar dots share one status vocabulary.
pub use crate::theme::RepoState as RepoStatus;

/// One repository's section in the grouped tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoSection {
    pub root_id: RootId,
    pub repo_name: String,
    /// The repo's checked-out branch, if any.
    pub current_branch: Option<String>,
    pub status: RepoStatus,
    /// The current branch's commits ahead of / behind its upstream. The header's
    /// summary states these as words; they come from the same snapshot as
    /// `status`, never a second read.
    pub ahead: usize,
    pub behind: usize,
    /// Local branches as a tree (directory subgroups + stripped labels).
    pub locals: Vec<BranchNode>,
    /// Total remote-branch count across all remotes (the rollup's `M branches`;
    /// equals the sum of `remote_groups` counts when `show_remotes`).
    pub remote_branch_count: usize,
    /// Number of remotes (the rollup's `N remotes`).
    pub remote_count: usize,
    /// Per-remote groups — present only when `show_remotes` is on.
    pub remote_groups: Vec<RemoteGroup>,
    /// Tags for this repo (still sorted by the caller), each carrying its
    /// per-ref state.
    pub tags: Vec<TagLeaf>,
}

impl RepoSection {
    /// The collapsed "N remotes · M branches" rollup text, or `None` when the
    /// repo has neither configured remotes nor remote-tracking branches.
    pub fn remote_rollup(&self) -> Option<String> {
        if self.remote_count == 0 && self.remote_branch_count == 0 {
            None
        } else {
            Some(format!(
                "{} · {}",
                components::counted(self.remote_count, "remote", "remotes"),
                components::counted(self.remote_branch_count, "branch", "branches")
            ))
        }
    }
}

/// The full grouped view model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchView {
    pub repos: Vec<RepoSection>,
    /// The Recent group: the model's one list that is not a section's.
    ///
    /// Repository grouping puts a band and three groups per repository, and that
    /// is the reference's shape; this is TurboGit's differentiator, the same
    /// affordance answered across the whole repository set, ordered by when each
    /// move actually happened rather than by which repository was asked first.
    /// Empty paints no group.
    pub recent: Vec<BranchLeaf>,
    /// `Some(n)` when more than one repo is in scope — the "all N repos" chip.
    pub all_repos_count: Option<usize>,
    /// True when exactly one repo is in scope (its section renders invisibly).
    pub single_repo: bool,
    /// The four toggles this model was built under.
    ///
    /// Carried on the model rather than passed to the renderer separately, so
    /// the labels the builder decided and the levels the renderer paints answer
    /// to one reading of the same settings. `Group by Directory` is spent by the
    /// time a model exists, because it decides leaf labels and nesting; the
    /// repository, Recent and Tags choices are read by the renderer, which owns
    /// which levels get painted. No toggle removes a branch from the model, only
    /// from the list.
    pub options: BranchesViewOptions,
}

/// Intermediate builder node; dirs and leaves share a label so they sort
/// together within a containing group. The leaf is boxed because a `BranchLeaf`
/// carries a whole `Branch`, which the directory variant never pays for.
enum B {
    Leaf(Box<BranchLeaf>),
    Dir(String, Vec<B>),
}

impl B {
    fn label(&self) -> &str {
        match self {
            B::Leaf(l) => &l.label,
            B::Dir(name, _) => name,
        }
    }
    fn favorite(&self) -> bool {
        match self {
            B::Leaf(l) => l.favorite,
            B::Dir(..) => false,
        }
    }
    fn active(&self) -> bool {
        match self {
            B::Leaf(l) => l.active,
            B::Dir(..) => false,
        }
    }
}

/// Insert a leaf into the forest, creating directory nodes for each segment of
/// `dirs`. Favorites and dirs are layered by the caller; here we only nest.
fn insert(nodes: &mut Vec<B>, dirs: &[&str], leaf: BranchLeaf) {
    match dirs.split_first() {
        Some((first, rest)) => {
            let pos = nodes.iter().position(|n| match n {
                B::Dir(d, _) => d == first,
                _ => false,
            });
            let child = if let Some(i) = pos {
                &mut nodes[i]
            } else {
                nodes.push(B::Dir((*first).to_string(), Vec::new()));
                nodes.last_mut().unwrap()
            };
            if let B::Dir(_, c) = child {
                insert(c, rest, leaf);
            }
        }
        None => nodes.push(B::Leaf(Box::new(leaf))),
    }
}

/// Sort each level: favorites first, then active, then alphabetical; recurse.
fn finalize(nodes: &mut [B]) {
    for n in nodes.iter_mut() {
        if let B::Dir(_, c) = n {
            finalize(c);
        }
    }
    nodes.sort_by(|a, b| {
        b.favorite()
            .cmp(&a.favorite())
            .then_with(|| b.active().cmp(&a.active()))
            .then_with(|| a.label().cmp(b.label()))
    });
}

/// Count the leaf branches beneath a forest (the count invariant source).
fn count_leaves(nodes: &[B]) -> usize {
    nodes
        .iter()
        .map(|n| match n {
            B::Leaf(_) => 1,
            B::Dir(_, c) => count_leaves(c),
        })
        .sum()
}

/// Convert the builder forest into the final [`BranchNode`] tree, attaching the
/// derived `count` to every directory node.
fn convert(nodes: Vec<B>) -> Vec<BranchNode> {
    nodes
        .into_iter()
        .map(|n| match n {
            B::Leaf(l) => BranchNode::Leaf(l),
            B::Dir(label, children) => BranchNode::Dir(DirNode {
                label,
                count: count_leaves(&children),
                children: convert(children),
            }),
        })
        .collect()
}

/// Build the directory/leaf hierarchy for a set of branches.
///
/// `strip_prefix` — when `Some(remote)`, the leading `remote/` segment is
/// removed from each branch name *before* the hierarchy is built, so the leaf
/// label inside a remote group never re-prints the remote (structural prefix
/// stripping). For local branches pass `None`.
///
/// `group_dirs` is the `Group by Directory` toggle: when it is off the name is
/// **flattened, never truncated** — the leaf keeps the whole slash-prefixed name
/// and no directory node is created, so the same branches stay in the list with
/// one level fewer above them.
///
/// `root` is stamped on every leaf the forest holds. The renderer could take it
/// from the enclosing section today, and would be wrong the moment it paints one
/// of the lists that spans sections.
fn build_tree(
    branches: &[Branch],
    current: &Option<String>,
    strip_prefix: Option<&str>,
    root: &RootId,
    group_dirs: bool,
) -> Vec<BranchNode> {
    let mut forest: Vec<B> = Vec::new();
    for b in branches {
        // Skip the remote prefix segment exactly once.
        let name = b.name.as_str();
        let stripped = match strip_prefix {
            Some(p) if name.starts_with(p) && name[p.len()..].starts_with('/') => {
                &name[p.len() + 1..]
            }
            _ => name,
        };
        let (leaf_label, dirs): (String, Vec<&str>) = if group_dirs {
            let mut parts: Vec<&str> = stripped.split('/').collect();
            let leaf_label = parts.pop().unwrap_or("").to_string();
            (leaf_label, parts)
        } else {
            (stripped.to_string(), Vec::new())
        };
        let leaf = BranchLeaf {
            label: leaf_label,
            root: root.clone(),
            branch: b.clone(),
            active: current.as_deref() == Some(name) && b.kind == BranchKind::Local,
            favorite: b.favorite,
        };
        insert(&mut forest, &dirs, leaf);
    }
    finalize(&mut forest);
    convert(forest)
}

/// Derive a repo's header status from its snapshot (issue 01 §6), together with
/// the current branch's ahead/behind counts it was derived from — the header's
/// summary needs the numbers, and they must come from this same read.
fn header_status(root: &Root) -> (RepoStatus, usize, usize) {
    let current = root.current_branch.as_ref().and_then(|name| {
        root.branches
            .iter()
            .find(|b| b.kind == BranchKind::Local && b.name == *name)
    });
    let (ahead, behind) = current.map_or((0, 0), |b| (b.ahead, b.behind));
    (RepoStatus::from_root(root, ahead, behind), ahead, behind)
}

/// Count the leaf branches beneath a node forest (the count invariant source).
///
/// Public so the renderer can size the "Local" / "Remote" sub-headers from the
/// same numbers the builder derived, keeping the painted counts trustworthy.
pub fn leaf_count(nodes: &[BranchNode]) -> usize {
    nodes
        .iter()
        .map(|n| match n {
            BranchNode::Leaf(_) => 1,
            BranchNode::Dir(d) => leaf_count(&d.children),
        })
        .sum()
}

/// Build the full grouped view model with every toggle **on**.
///
/// The all-on model is the shape the surface had before any of the four were
/// togglable, and it is what a caller that owns no gear renders — the Git Log
/// pane's branches tree among them. The Branches surface calls
/// [`build_branch_view_with`] with the project's persisted toggles instead.
pub fn build_branch_view(
    roots: &[Root],
    tags_by_root: &HashMap<RootId, Vec<(String, RefState)>>,
    recents_by_root: &HashMap<RootId, Vec<RecentCheckout>>,
    show_remotes: &dyn Fn(&RootId) -> bool,
) -> BranchView {
    build_branch_view_with(
        roots,
        tags_by_root,
        recents_by_root,
        show_remotes,
        &BranchesViewOptions::default(),
    )
}

/// Build the full grouped view model under `options`.
///
/// `roots` are the in-scope repositories; `tags_by_root` carries each repo's
/// already-classified tag list (tags live outside the `Root` branch snapshot
/// and are supplied pre-read). `recents_by_root` carries each repo's
/// reflog-derived checkout history for the same reason — the branch snapshot has
/// no recency in it — and feeds the view-level Recent group; passing an empty map
/// paints no group. `show_remotes` is asked per repository whether that repo's
/// remotes are showing, so a rolled-up repo is skipped while its tree is built
/// rather than built and then hidden — which is why this is a predicate and not a
/// flag.
///
/// Only two of the four toggles reach the model, and both are the ones a label
/// depends on: `Group by Directory` (see [`build_tree`]) and, through it, the
/// repository qualifier that un-grouped rows need because no band names the
/// repository once grouping is off. `Show Recent Branches` and `Show Tags` hide a
/// whole group, which is a decision the renderer makes from
/// [`BranchView::options`]; the data stays in the model either way, because a
/// toggle never changes what the surface knows.
pub fn build_branch_view_with(
    roots: &[Root],
    tags_by_root: &HashMap<RootId, Vec<(String, RefState)>>,
    recents_by_root: &HashMap<RootId, Vec<RecentCheckout>>,
    show_remotes: &dyn Fn(&RootId) -> bool,
    options: &BranchesViewOptions,
) -> BranchView {
    let single_repo = roots.len() == 1;
    let all_repos_count = if roots.len() > 1 {
        Some(roots.len())
    } else {
        None
    };
    let recent = build_recent_group(roots, recents_by_root);

    // With no repository band to say which `alpha` a row came from, a row whose
    // name several repositories also have has to say it itself. The rule is
    // ambiguity, not the toggle: a name only one repository has stays bare, so
    // un-grouping never renames a row that was never in doubt.
    let qualify = !options.group_by_repository && roots.len() > 1;
    let shared_locals = shared_names(roots, |root| {
        root.branches
            .iter()
            .filter(|b| b.kind == BranchKind::Local)
            .map(|b| b.name.clone())
            .collect()
    });
    let shared_tags = shared_names(roots, |root| {
        tags_by_root.get(&root.id).map_or(Vec::new(), |list| {
            list.iter().map(|t| t.0.clone()).collect()
        })
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
        if qualify {
            qualify_leaves(&mut locals_tree, &shared_locals, &repo_name);
        }

        // Group remotes by the remote each row already carries — a
        // remote-tracking listing names its own remote, so nothing splits here.
        let mut by_remote: Vec<(String, Vec<Branch>)> = Vec::new();
        for b in &remotes {
            let remote = b.remote.clone().unwrap_or_else(|| "origin".to_string());
            match by_remote.iter_mut().find(|(r, _)| *r == remote) {
                Some((_, list)) => list.push(b.clone()),
                None => by_remote.push((remote, vec![b.clone()])),
            }
        }
        by_remote.sort_by(|a, b| a.0.cmp(&b.0));

        let revealed = show_remotes(&root.id);
        let remote_groups = if revealed {
            by_remote
                .iter()
                .map(|(remote, list)| {
                    let children = build_tree(
                        list,
                        current,
                        Some(remote),
                        &root.id,
                        options.group_by_directory,
                    );
                    RemoteGroup {
                        // Two repositories each with an `origin` paint one
                        // identical header once the band that separated them is
                        // gone, so the un-grouped header names the repository.
                        label: if qualify {
                            format!("{remote} · {repo_name}")
                        } else {
                            remote.clone()
                        },
                        remote: remote.clone(),
                        root: root.id.clone(),
                        count: list.len(),
                        children,
                    }
                })
                .collect()
        } else {
            Vec::new()
        };

        let mut tags = tags_by_root
            .get(&root.id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|(name, state)| TagLeaf { name, state })
            .collect::<Vec<_>>();
        if qualify {
            for tag in &mut tags {
                if shared_tags.contains(&tag.name) {
                    tag.name = format!("{} · {}", tag.name, repo_name);
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
        recent,
        all_repos_count,
        single_repo,
        options: *options,
    }
}

/// The names more than one of `roots` answers with: a row carrying one of these
/// cannot be told apart from its twin without its repository.
fn shared_names(roots: &[Root], names: impl Fn(&Root) -> Vec<String>) -> HashSet<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut shared: HashSet<String> = HashSet::new();
    for root in roots {
        for name in names(root) {
            if !seen.insert(name.clone()) {
                shared.insert(name);
            }
        }
    }
    shared
}

/// Append the owning repository's name to every leaf whose branch name is
/// ambiguous across the repositories in scope, at every directory depth.
fn qualify_leaves(nodes: &mut [BranchNode], shared: &HashSet<String>, repo_name: &str) {
    for node in nodes {
        match node {
            BranchNode::Leaf(leaf) => {
                if shared.contains(&leaf.branch.name) {
                    leaf.label = format!("{} · {}", leaf.label, repo_name);
                }
            }
            BranchNode::Dir(dir) => qualify_leaves(&mut dir.children, shared, repo_name),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use chrono::{DateTime, Utc};
    use turbogit_domain::model::{
        Branch, BranchKind, BranchTip, Change, ChangeStatus, RecentCheckout, Root, RootId,
        RootStatus, Upstream,
    };

    use super::*;

    fn ts(epoch: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(epoch, 0).expect("post-epoch")
    }

    /// A local branch whose tracking relationship is seeded from its counts.
    /// "Has a tracking relationship at all" is the loose reading — either side
    /// moved — which is `DivergenceMeaning::AnyDrift`, the same rule the remote
    /// row's marker asks. It reaches the predicate through the one
    /// styling-adjacent reference this module is permitted, so the builder
    /// still answers a question the renderer's vocabulary owns.
    fn local(name: &str, favorite: bool, ahead: usize, behind: usize) -> Branch {
        Branch {
            name: name.to_string(),
            kind: BranchKind::Local,
            tracking: if RepoStatus::has_drift(ahead, behind) {
                Upstream::from_git_ref("origin/main")
            } else {
                None
            },
            favorite,
            protected: false,
            exists: true,
            ahead,
            behind,
            gone: false,
            last_touched: Some(ts(1000)),
            tip: None,
            remote: None,
        }
    }

    fn remote(name: &str, remote: &str) -> Branch {
        Branch {
            name: name.to_string(),
            kind: BranchKind::Remote,
            tracking: None,
            favorite: false,
            protected: false,
            exists: true,
            ahead: 0,
            behind: 0,
            gone: false,
            last_touched: None,
            tip: None,
            remote: Some(remote.to_string()),
        }
    }

    fn root_with(id: &str, branches: &[Branch], current: Option<&str>, dirty: bool) -> Root {
        let changes = if dirty {
            vec![Change {
                path: PathBuf::from("README.md"),
                status: ChangeStatus::Modified,
                chunks: vec![],
                staged: false,
                unstaged: true,
                orig_path: None,
            }]
        } else {
            vec![]
        };
        Root {
            id: RootId(Arc::from(PathBuf::from(id))),
            path: PathBuf::from(id),
            remotes: vec![
                turbogit_domain::model::Remote {
                    name: "origin".to_string(),
                    fetch_url: None,
                    push_url: None,
                },
                turbogit_domain::model::Remote {
                    name: "upstream".to_string(),
                    fetch_url: None,
                    push_url: None,
                },
            ],
            branches: branches.to_vec(),
            current_branch: current.map(|s| s.to_string()),
            head: Some("abc".to_string()),
            status: RootStatus {
                changes,
                conflicted: vec![],
                conflict_kinds: vec![],
            },
        }
    }

    fn leaf_labels(nodes: &[BranchNode]) -> Vec<String> {
        let mut out = Vec::new();
        for n in nodes {
            match n {
                BranchNode::Leaf(l) => out.push(l.label.clone()),
                BranchNode::Dir(d) => {
                    out.push(format!("{}/", d.label));
                    out.extend(leaf_labels(&d.children));
                }
            }
        }
        out
    }

    fn count_leaves_pub(nodes: &[BranchNode]) -> usize {
        nodes
            .iter()
            .map(|n| match n {
                BranchNode::Leaf(_) => 1,
                BranchNode::Dir(d) => count_leaves_pub(&d.children),
            })
            .sum()
    }

    // --- remotes hidden by default with correct hidden count -----------------

    #[test]
    fn remotes_hidden_by_default_collapse_into_rollup() {
        let repo = root_with(
            "/alpha",
            &[
                local("main", false, 0, 0),
                local("feature-a", false, 0, 0),
                remote("origin/main", "origin"),
                remote("origin/remote-only", "origin"),
            ],
            Some("main"),
            false,
        );
        let view = build_branch_view(
            &[repo],
            &HashMap::new(),
            &std::collections::HashMap::new(),
            &|_| false,
        );

        let section = &view.repos[0];
        // Remotes collapse: no per-remote groups, but the rollup count is exact.
        assert!(section.remote_groups.is_empty(), "remotes stay collapsed");
        assert_eq!(section.remote_count, 2, "both remotes counted");
        assert_eq!(
            section.remote_branch_count, 2,
            "both remote branches counted"
        );
        assert_eq!(
            section.remote_rollup().as_deref(),
            Some("2 remotes · 2 branches"),
            "rollup reads the derived counts"
        );
    }

    #[test]
    fn remotes_on_yields_per_remote_groups() {
        let repo = root_with(
            "/alpha",
            &[
                local("main", false, 0, 0),
                remote("origin/main", "origin"),
                remote("origin/feature-x", "origin"),
                remote("upstream/main", "upstream"),
            ],
            Some("main"),
            false,
        );
        let view = build_branch_view(
            &[repo],
            &HashMap::new(),
            &std::collections::HashMap::new(),
            &|_| true,
        );

        let section = &view.repos[0];
        assert_eq!(section.remote_groups.len(), 2);
        assert_eq!(section.remote_groups[0].remote, "origin");
        assert_eq!(section.remote_groups[0].count, 2);
        assert_eq!(section.remote_groups[1].remote, "upstream");
        assert_eq!(section.remote_groups[1].count, 1);
    }

    // --- directory grouping on both kinds ------------------------------------

    #[test]
    fn local_directory_grouping_splits_on_slashes() {
        let repo = root_with(
            "/alpha",
            &[
                local("main", false, 0, 0),
                local("feature/a", false, 0, 0),
                local("feature/b", false, 0, 0),
            ],
            Some("main"),
            false,
        );
        let view = build_branch_view(
            &[repo],
            &HashMap::new(),
            &std::collections::HashMap::new(),
            &|_| false,
        );
        let section = &view.repos[0];

        // One directory node "feature" with two leaves, count == 2.
        assert_eq!(section.locals.len(), 2);
        let dir = section
            .locals
            .iter()
            .find_map(|n| match n {
                BranchNode::Dir(d) if d.label == "feature" => Some(d),
                _ => None,
            })
            .expect("feature/ directory node");
        assert_eq!(dir.count, 2);
        assert_eq!(count_leaves_pub(&section.locals), 3);
    }

    #[test]
    fn remote_directory_grouping_under_each_remote() {
        let repo = root_with(
            "/alpha",
            &[
                local("main", false, 0, 0),
                remote("origin/feature/x", "origin"),
                remote("origin/feature/y", "origin"),
                remote("origin/hotfix", "origin"),
            ],
            Some("main"),
            false,
        );
        let view = build_branch_view(
            &[repo],
            &HashMap::new(),
            &std::collections::HashMap::new(),
            &|_| true,
        );
        let section = &view.repos[0];
        let origin = &section.remote_groups[0];

        // origin carries one directory "feature" (2 leaves) + "hotfix" leaf.
        let dir = origin
            .children
            .iter()
            .find_map(|n| match n {
                BranchNode::Dir(d) if d.label == "feature" => Some(d),
                _ => None,
            })
            .expect("origin/feature/ directory node");
        assert_eq!(dir.count, 2);
        assert_eq!(origin.count, 3);
    }

    // --- count invariant everywhere ------------------------------------------

    #[test]
    fn count_invariant_holds_across_nested_dirs() {
        let repo = root_with(
            "/alpha",
            &[
                local("a/b/c", false, 0, 0),
                local("a/b/d", false, 0, 0),
                local("a/e", false, 0, 0),
                local("top", false, 0, 0),
            ],
            Some("top"),
            false,
        );
        let view = build_branch_view(
            &[repo],
            &HashMap::new(),
            &std::collections::HashMap::new(),
            &|_| false,
        );
        let section = &view.repos[0];

        fn check(nodes: &[BranchNode]) {
            for n in nodes {
                if let BranchNode::Dir(d) = n {
                    assert_eq!(
                        d.count,
                        count_leaves_pub(&d.children),
                        "dir {} count must equal its leaves",
                        d.label
                    );
                    check(&d.children);
                }
            }
        }
        check(&section.locals);
        assert_eq!(count_leaves_pub(&section.locals), 4);
    }

    #[test]
    fn rollup_branch_count_equals_sum_of_remote_groups() {
        let repo = root_with(
            "/alpha",
            &[
                local("main", false, 0, 0),
                remote("origin/a", "origin"),
                remote("origin/b", "origin"),
                remote("upstream/a", "upstream"),
            ],
            Some("main"),
            false,
        );
        let mut tags = HashMap::new();
        tags.insert(
            RootId(Arc::from(PathBuf::from("/alpha"))),
            vec![("v1.0".to_string(), RefState::Default)],
        );
        let view = build_branch_view(&[repo], &tags, &std::collections::HashMap::new(), &|_| true);
        let section = &view.repos[0];

        let sum: usize = section.remote_groups.iter().map(|g| g.count).sum();
        assert_eq!(
            sum, section.remote_branch_count,
            "rollup M == sum of groups"
        );
    }

    // --- prefix stripping both kinds ------------------------------------------

    #[test]
    fn local_prefix_stripped_in_directory_leaf() {
        let repo = root_with(
            "/alpha",
            &[local("feature/long-name", false, 0, 0)],
            Some("main"),
            false,
        );
        let view = build_branch_view(
            &[repo],
            &HashMap::new(),
            &std::collections::HashMap::new(),
            &|_| false,
        );
        let section = &view.repos[0];
        let labels = leaf_labels(&section.locals);
        assert!(
            labels.contains(&"long-name".to_string()),
            "leaf drops the directory part: {labels:?}"
        );
        assert!(
            !labels.iter().any(|l| l.contains("feature/long-name")),
            "full path must not re-print on the leaf"
        );
    }

    #[test]
    fn remote_prefix_not_reprinted_inside_remote_group() {
        let repo = root_with(
            "/alpha",
            &[
                local("main", false, 0, 0),
                remote("origin/remote-only", "origin"),
                remote("origin/feature/x", "origin"),
            ],
            Some("main"),
            false,
        );
        let view = build_branch_view(
            &[repo],
            &HashMap::new(),
            &std::collections::HashMap::new(),
            &|_| true,
        );
        let section = &view.repos[0];
        let origin = &section.remote_groups[0];
        let labels: Vec<String> = leaf_labels(&origin.children);

        // Inside origin the leaf shows "remote-only" and "x" (part of feature/),
        // never "origin/…".
        assert!(labels.contains(&"remote-only".to_string()));
        assert!(labels.contains(&"x".to_string()));
        assert!(
            !leaf_labels(&origin.children)
                .iter()
                .any(|l| l.starts_with("origin/")),
            "remote prefix must not re-print inside its group"
        );
    }

    // --- active marking + favorites pinned to group top ----------------------

    #[test]
    fn checked_out_branch_marked_active() {
        let repo = root_with(
            "/alpha",
            &[local("main", false, 0, 0), local("feature-a", false, 0, 0)],
            Some("main"),
            false,
        );
        let view = build_branch_view(
            &[repo],
            &HashMap::new(),
            &std::collections::HashMap::new(),
            &|_| false,
        );
        let section = &view.repos[0];
        let main = section
            .locals
            .iter()
            .find_map(|n| match n {
                BranchNode::Leaf(l) if l.label == "main" => Some(l),
                _ => None,
            })
            .expect("main leaf");
        assert!(main.active, "current branch is marked active");
        let feat = section
            .locals
            .iter()
            .find_map(|n| match n {
                BranchNode::Leaf(l) if l.label == "feature-a" => Some(l),
                _ => None,
            })
            .expect("feature-a leaf");
        assert!(!feat.active);
    }

    #[test]
    fn favorites_pin_to_top_of_group() {
        let repo = root_with(
            "/alpha",
            &[
                local("main", false, 0, 0),
                local("feature-a", false, 0, 0),
                local("zebra", true, 0, 0),
            ],
            Some("main"),
            false,
        );
        let view = build_branch_view(
            &[repo],
            &HashMap::new(),
            &std::collections::HashMap::new(),
            &|_| false,
        );
        let section = &view.repos[0];
        // The favorite "zebra" sits first among locals (above main/feature-a).
        match &section.locals[0] {
            BranchNode::Leaf(l) => assert_eq!(l.label, "zebra", "favorite pinned to top"),
            BranchNode::Dir(..) => panic!("unexpected dir"),
        }
    }

    #[test]
    fn favorites_pin_within_remote_group_when_shown() {
        let mut fav = remote("origin/starred", "origin");
        fav.favorite = true;
        let repo = root_with(
            "/alpha",
            &[
                local("main", false, 0, 0),
                fav,
                remote("origin/plain", "origin"),
            ],
            Some("main"),
            false,
        );
        let view = build_branch_view(
            &[repo],
            &HashMap::new(),
            &std::collections::HashMap::new(),
            &|_| true,
        );
        let section = &view.repos[0];
        let origin = &section.remote_groups[0];
        // "starred" leads its group though "plain" sorts first alphabetically.
        match &origin.children[0] {
            BranchNode::Leaf(l) => assert_eq!(l.label, "starred", "remote favorite pinned to top"),
            BranchNode::Dir(..) => panic!("unexpected dir"),
        }
    }

    // --- per-repo status derivation ------------------------------------------

    #[test]
    fn repo_status_derived_from_snapshot() {
        let clean = root_with("/c", &[local("main", false, 0, 0)], Some("main"), false);
        let dirty = root_with("/d", &[local("main", false, 0, 0)], Some("main"), true);
        let unpushed = root_with("/u", &[local("main", false, 2, 0)], Some("main"), false);
        let unpulled = root_with("/p", &[local("main", false, 0, 3)], Some("main"), false);

        let view_clean = build_branch_view(
            &[clean],
            &HashMap::new(),
            &std::collections::HashMap::new(),
            &|_| false,
        );
        let view_dirty = build_branch_view(
            &[dirty],
            &HashMap::new(),
            &std::collections::HashMap::new(),
            &|_| false,
        );
        let view_push = build_branch_view(
            &[unpushed],
            &HashMap::new(),
            &std::collections::HashMap::new(),
            &|_| false,
        );
        let view_pull = build_branch_view(
            &[unpulled],
            &HashMap::new(),
            &std::collections::HashMap::new(),
            &|_| false,
        );

        assert_eq!(view_clean.repos[0].status, RepoStatus::Clean);
        assert_eq!(view_dirty.repos[0].status, RepoStatus::Dirty);
        assert_eq!(view_push.repos[0].status, RepoStatus::Unpushed);
        assert_eq!(view_pull.repos[0].status, RepoStatus::Unpulled);
    }

    // --- scope behavior -------------------------------------------------------

    #[test]
    fn single_repo_emits_invisible_section_and_no_all_repos_count() {
        let repo = root_with("/alpha", &[local("main", false, 0, 0)], Some("main"), false);
        let view = build_branch_view(
            &[repo],
            &HashMap::new(),
            &std::collections::HashMap::new(),
            &|_| false,
        );
        assert!(view.single_repo, "one repo → invisible section");
        assert_eq!(
            view.all_repos_count, None,
            "no all-N chip for a single repo"
        );
        assert_eq!(view.repos.len(), 1);
        assert_eq!(view.repos[0].repo_name, "alpha");
    }

    #[test]
    fn multi_repo_exposes_all_n_count_and_per_repo_sections() {
        let a = root_with("/alpha", &[local("main", false, 0, 0)], Some("main"), false);
        let b = root_with("/beta", &[local("main", false, 0, 0)], Some("main"), false);
        let view = build_branch_view(
            &[a, b],
            &HashMap::new(),
            &std::collections::HashMap::new(),
            &|_| false,
        );
        assert!(!view.single_repo);
        assert_eq!(view.all_repos_count, Some(2));
        assert_eq!(view.repos.len(), 2);
        let names: Vec<&str> = view.repos.iter().map(|r| r.repo_name.as_str()).collect();
        assert!(names.contains(&"alpha") && names.contains(&"beta"));
    }

    // --- tags per repo --------------------------------------------------------

    #[test]
    fn tags_attach_per_repo() {
        let repo = root_with("/alpha", &[local("main", false, 0, 0)], Some("main"), false);
        let mut tags = HashMap::new();
        tags.insert(
            RootId(Arc::from(PathBuf::from("/alpha"))),
            vec![
                ("v1.0".to_string(), RefState::Pushed),
                ("v2.0".to_string(), RefState::LocalOnly),
            ],
        );
        let view = build_branch_view(&[repo], &tags, &std::collections::HashMap::new(), &|_| {
            false
        });
        assert_eq!(
            view.repos[0].tags,
            vec![
                TagLeaf {
                    name: "v1.0".to_string(),
                    state: RefState::Pushed
                },
                TagLeaf {
                    name: "v2.0".to_string(),
                    state: RefState::LocalOnly
                },
            ]
        );
    }

    // --- BranchTip still carried on the leaf ---------------------------------
    // No surface paints it since the branch detail panel went; the view model
    // still carries the tip because `Branch` is domain data, and a future
    // commit column is the reason to keep filling it.

    #[test]
    fn leaf_carries_its_tip_commit() {
        let mut b = local("main", false, 0, 0);
        b.tip = Some(BranchTip {
            short_hash: "deadbeef".to_string(),
            message: "init".to_string(),
            author: "t".to_string(),
            time: ts(1000),
        });
        let repo = root_with("/alpha", &[b], Some("main"), false);
        let view = build_branch_view(
            &[repo],
            &HashMap::new(),
            &std::collections::HashMap::new(),
            &|_| false,
        );
        let leaf = match &view.repos[0].locals[0] {
            BranchNode::Leaf(l) => l,
            _ => panic!("leaf"),
        };
        assert_eq!(leaf.branch.tip.as_ref().unwrap().short_hash, "deadbeef");
    }

    // --- the Recent group: capped at five, joined, merged by recency ----------

    /// A checkout history for the named branches, listed newest-first the way
    /// the engine answers it and one second apart.
    fn checkouts(names: &[&str]) -> Vec<RecentCheckout> {
        names
            .iter()
            .enumerate()
            .map(|(i, n)| RecentCheckout {
                name: n.to_string(),
                at: 1_000 - i as i64,
            })
            .collect()
    }

    fn recent_names(rows: &[BranchLeaf]) -> Vec<&str> {
        rows.iter().map(|r| r.branch.name.as_str()).collect()
    }

    #[test]
    fn recent_group_is_capped_at_five_newest_first() {
        let branches: Vec<Branch> = (1..=7)
            .map(|i| local(&format!("r{i}"), false, 0, 0))
            .collect();
        let repo = root_with("/alpha", &branches, Some("r7"), false);
        let mut recents = HashMap::new();
        recents.insert(
            repo.id.clone(),
            checkouts(&["r7", "r6", "r5", "r4", "r3", "r2", "r1"]),
        );

        let rows = build_recent_group(&[repo], &recents);
        // "Recent branches node shows up to five recently checked-out branches".
        // The four older moves are not in the group and
        // no overflow affordance is reserved for them: the escape is unverified.
        assert_eq!(recent_names(&rows), vec!["r7", "r6", "r5", "r4", "r3"]);
    }

    #[test]
    fn recent_group_drops_names_that_are_not_local_branches() {
        // The reflog records every move, including onto a tag, a raw sha, or a
        // branch since deleted; joining against the snapshot is what keeps the
        // group listing branches a click can act on.
        let repo = root_with(
            "/alpha",
            &[local("main", false, 0, 0), local("kept", false, 0, 0)],
            Some("main"),
            false,
        );
        let mut recents = HashMap::new();
        recents.insert(
            repo.id.clone(),
            checkouts(&["gone", "v1.0", "kept", "main"]),
        );

        let rows = build_recent_group(&[repo], &recents);
        assert_eq!(recent_names(&rows), vec!["kept", "main"]);
    }

    #[test]
    fn recent_group_never_lists_a_branch_twice_and_keeps_its_newest_move() {
        let repo = root_with(
            "/alpha",
            &[local("a", false, 0, 0), local("b", false, 0, 0)],
            Some("a"),
            false,
        );
        let mut recents = HashMap::new();
        recents.insert(repo.id.clone(), checkouts(&["a", "b", "a"]));

        let rows = build_recent_group(&[repo], &recents);
        assert_eq!(recent_names(&rows), vec!["a", "b"]);
    }

    #[test]
    fn recent_group_merges_repositories_by_checkout_time_not_by_repository() {
        // Recent is a view-level group: the one place in this tree that spans
        // repositories, which is what makes it a differentiator over the
        // reference's per-repository shape.
        let alpha = root_with("/alpha", &[local("a1", false, 0, 0)], Some("a1"), false);
        let beta = root_with("/beta", &[local("b1", false, 0, 0)], Some("b1"), false);
        let mut recents = HashMap::new();
        // `beta`'s move is the newer one although `alpha` is listed first.
        recents.insert(
            alpha.id.clone(),
            vec![RecentCheckout {
                name: "a1".to_string(),
                at: 500,
            }],
        );
        recents.insert(
            beta.id.clone(),
            vec![RecentCheckout {
                name: "b1".to_string(),
                at: 900,
            }],
        );

        let rows = build_recent_group(&[alpha, beta], &recents);
        assert_eq!(recent_names(&rows), vec!["b1", "a1"]);
        assert_eq!(rows[0].root, RootId(Arc::from(PathBuf::from("/beta"))));
        assert_eq!(rows[1].root, RootId(Arc::from(PathBuf::from("/alpha"))));
    }

    #[test]
    fn recent_group_paints_three_rows_for_three_moves_of_history() {
        let repo = root_with(
            "/alpha",
            &[
                local("r1", false, 0, 0),
                local("r2", false, 0, 0),
                local("r3", false, 0, 0),
            ],
            Some("r3"),
            false,
        );
        let mut recents = HashMap::new();
        recents.insert(repo.id.clone(), checkouts(&["r3", "r2", "r1"]));

        let rows = build_recent_group(&[repo], &recents);
        // The under-cap case: a cap implemented as five reserved slots shows
        // empty rows here.
        assert_eq!(rows.len(), 3, "a short history paints a short group");
    }

    #[test]
    fn recent_group_is_empty_when_nothing_was_ever_checked_out() {
        let repo = root_with("/alpha", &[local("main", false, 0, 0)], Some("main"), false);
        assert!(
            build_recent_group(&[repo], &HashMap::new()).is_empty(),
            "a repository whose reflog holds no checkout has no Recent group"
        );
    }
}
