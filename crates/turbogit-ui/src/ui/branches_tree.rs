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

use std::collections::HashMap;

use turbogit_domain::model::{Branch, BranchKind, RefState, Root, RootId};

/// One branch row in the grouped tree, after prefix stripping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchLeaf {
    /// Display label with the containing directory/remote prefix stripped.
    pub label: String,
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BranchNode {
    Leaf(BranchLeaf),
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
    pub remote: String,
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
                "{} remote{} · {} {}",
                self.remote_count,
                if self.remote_count == 1 { "" } else { "s" },
                self.remote_branch_count,
                if self.remote_branch_count == 1 {
                    "branch"
                } else {
                    "branches"
                }
            ))
        }
    }
}

/// The full grouped view model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchView {
    pub repos: Vec<RepoSection>,
    /// `Some(n)` when more than one repo is in scope — the "all N repos" chip.
    pub all_repos_count: Option<usize>,
    /// True when exactly one repo is in scope (its section renders invisibly).
    pub single_repo: bool,
}

/// Intermediate builder node; dirs and leaves share a label so they sort
/// together within a containing group.
enum B {
    Leaf(BranchLeaf),
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
        None => nodes.push(B::Leaf(leaf)),
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
fn build_tree(
    branches: &[Branch],
    current: &Option<String>,
    strip_prefix: Option<&str>,
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
        let mut parts: Vec<&str> = stripped.split('/').collect();
        let leaf_label = parts.pop().unwrap_or("").to_string();
        let dirs: Vec<&str> = parts;
        let leaf = BranchLeaf {
            label: leaf_label,
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

/// Build the full grouped view model.
///
/// `roots` are the in-scope repositories; `tags_by_root` carries each repo's
/// already-classified tag list (tags live outside the `Root` branch snapshot
/// and are supplied pre-read). `show_remotes` is asked per repository whether
/// that repo's remotes are showing, so a rolled-up repo is skipped while its
/// tree is built rather than built and then hidden — which is why this is a
/// predicate and not a flag.
pub fn build_branch_view(
    roots: &[Root],
    tags_by_root: &HashMap<RootId, Vec<(String, RefState)>>,
    show_remotes: &dyn Fn(&RootId) -> bool,
) -> BranchView {
    let single_repo = roots.len() == 1;
    let all_repos_count = if roots.len() > 1 {
        Some(roots.len())
    } else {
        None
    };

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

        let locals_tree = build_tree(&locals, current, None);

        // Group remotes by their `remote` field (falling back to the first
        // path segment of the name).
        let mut by_remote: Vec<(String, Vec<Branch>)> = Vec::new();
        for b in &remotes {
            let remote = b
                .remote
                .clone()
                .or_else(|| b.name.split('/').next().map(|s| s.to_string()))
                .unwrap_or_else(|| "origin".to_string());
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
                    let children = build_tree(list, current, Some(remote));
                    RemoteGroup {
                        remote: remote.clone(),
                        count: list.len(),
                        children,
                    }
                })
                .collect()
        } else {
            Vec::new()
        };

        let tags = tags_by_root
            .get(&root.id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|(name, state)| TagLeaf { name, state })
            .collect();

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
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use chrono::{DateTime, Utc};
    use turbogit_domain::model::{
        Branch, BranchKind, BranchTip, Change, ChangeStatus, Root, RootId, RootStatus,
    };

    use super::*;

    fn ts(epoch: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(epoch, 0).expect("post-epoch")
    }

    fn local(name: &str, favorite: bool, ahead: usize, behind: usize) -> Branch {
        Branch {
            name: name.to_string(),
            kind: BranchKind::Local,
            tracking: if ahead + behind > 0 {
                Some("origin/main".to_string())
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
        let view = build_branch_view(&[repo], &HashMap::new(), &|_| false);

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
        let view = build_branch_view(&[repo], &HashMap::new(), &|_| true);

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
        let view = build_branch_view(&[repo], &HashMap::new(), &|_| false);
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
        let view = build_branch_view(&[repo], &HashMap::new(), &|_| true);
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
        let view = build_branch_view(&[repo], &HashMap::new(), &|_| false);
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
        let view = build_branch_view(&[repo], &tags, &|_| true);
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
        let view = build_branch_view(&[repo], &HashMap::new(), &|_| false);
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
        let view = build_branch_view(&[repo], &HashMap::new(), &|_| true);
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
        let view = build_branch_view(&[repo], &HashMap::new(), &|_| false);
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
        let view = build_branch_view(&[repo], &HashMap::new(), &|_| false);
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
        let view = build_branch_view(&[repo], &HashMap::new(), &|_| true);
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

        let view_clean = build_branch_view(&[clean], &HashMap::new(), &|_| false);
        let view_dirty = build_branch_view(&[dirty], &HashMap::new(), &|_| false);
        let view_push = build_branch_view(&[unpushed], &HashMap::new(), &|_| false);
        let view_pull = build_branch_view(&[unpulled], &HashMap::new(), &|_| false);

        assert_eq!(view_clean.repos[0].status, RepoStatus::Clean);
        assert_eq!(view_dirty.repos[0].status, RepoStatus::Dirty);
        assert_eq!(view_push.repos[0].status, RepoStatus::Unpushed);
        assert_eq!(view_pull.repos[0].status, RepoStatus::Unpulled);
    }

    // --- scope behavior -------------------------------------------------------

    #[test]
    fn single_repo_emits_invisible_section_and_no_all_repos_count() {
        let repo = root_with("/alpha", &[local("main", false, 0, 0)], Some("main"), false);
        let view = build_branch_view(&[repo], &HashMap::new(), &|_| false);
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
        let view = build_branch_view(&[a, b], &HashMap::new(), &|_| false);
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
        let view = build_branch_view(&[repo], &tags, &|_| false);
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

    // --- BranchTip still carried on the leaf (renderer uses it) --------------

    #[test]
    fn leaf_carries_tip_for_detail_panel() {
        let mut b = local("main", false, 0, 0);
        b.tip = Some(BranchTip {
            short_hash: "deadbeef".to_string(),
            message: "init".to_string(),
            author: "t".to_string(),
            time: ts(1000),
        });
        let repo = root_with("/alpha", &[b], Some("main"), false);
        let view = build_branch_view(&[repo], &HashMap::new(), &|_| false);
        let leaf = match &view.repos[0].locals[0] {
            BranchNode::Leaf(l) => l,
            _ => panic!("leaf"),
        };
        assert_eq!(leaf.branch.tip.as_ref().unwrap().short_hash, "deadbeef");
    }
}
