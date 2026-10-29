use std::collections::{HashMap, HashSet};

use crate::ui::components;
use turbogit_app::persistence::BranchesViewOptions;
use turbogit_domain::model::{Branch, BranchKind, RecentCheckout, RefState, Root, RootId};

pub const RECENT_CAP: usize = 5;

pub fn build_recent_group(
    roots: &[Root],
    recents: &HashMap<RootId, Vec<RecentCheckout>>,
) -> Vec<BranchLeaf> {
    let mut placed: Vec<(i64, BranchLeaf)> = Vec::new();
    for root in roots {
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
    pub branch: Branch,
    /// Whether this is the repo's checked-out branch.
    pub active: bool,
    pub favorite: bool,
}

/// A directory node formed from slash-separated branch names.
///
/// `count` is the total number of leaf branches anywhere beneath this node,
/// it is derived from `children`, never estimated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirNode {
    pub label: String,
    pub count: usize,
    pub children: Vec<BranchNode>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BranchNode {
    Leaf(Box<BranchLeaf>),
    Dir(DirNode),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagLeaf {
    pub name: String,
    pub state: RefState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteGroup {
    /// The remote's own name, git's spelling: the collapse key and the prefix
    /// the leaves are stripped of.
    pub remote: String,
    pub root: RootId,
    pub label: String,
    pub count: usize,
    pub children: Vec<BranchNode>,
}

pub use crate::theme::RepoState as RepoStatus;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoSection {
    pub root_id: RootId,
    pub repo_name: String,
    pub current_branch: Option<String>,
    pub status: RepoStatus,
    /// The current branch's commits ahead of / behind its upstream. The header's
    /// summary states these as words; they come from the same snapshot as
    /// `status`, never a second read.
    pub ahead: usize,
    pub behind: usize,
    pub locals: Vec<BranchNode>,
    /// Total remote-branch count across all remotes (the rollup's `M branches`;
    /// equals the sum of `remote_groups` counts when `show_remotes`).
    pub remote_branch_count: usize,
    pub remote_count: usize,
    /// Per-remote groups, present only when `show_remotes` is on.
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchView {
    pub repos: Vec<RepoSection>,
    pub recent: Vec<BranchLeaf>,
    pub all_repos_count: Option<usize>,
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

fn count_leaves(nodes: &[B]) -> usize {
    nodes
        .iter()
        .map(|n| match n {
            B::Leaf(_) => 1,
            B::Dir(_, c) => count_leaves(c),
        })
        .sum()
}

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

fn build_tree(
    branches: &[Branch],
    current: &Option<String>,
    strip_prefix: Option<&str>,
    root: &RootId,
    group_dirs: bool,
) -> Vec<BranchNode> {
    let mut forest: Vec<B> = Vec::new();
    for b in branches {
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

fn header_status(root: &Root) -> (RepoStatus, usize, usize) {
    let current = root.current_branch.as_ref().and_then(|name| {
        root.branches
            .iter()
            .find(|b| b.kind == BranchKind::Local && b.name == *name)
    });
    let (ahead, behind) = current.map_or((0, 0), |b| (b.ahead, b.behind));
    (RepoStatus::from_root(root, ahead, behind), ahead, behind)
}

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
/// togglable, and it is what a caller that owns no gear renders, the Git Log
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
/// reflog-derived checkout history for the same reason, the branch snapshot has
/// no recency in it, and feeds the view-level Recent group; passing an empty map
/// paints no group. `show_remotes` is asked per repository whether that repo's
/// remotes are showing, so a rolled-up repo is skipped while its tree is built
/// rather than built and then hidden, which is why this is a predicate and not a
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

        assert!(labels.contains(&"remote-only".to_string()));
        assert!(labels.contains(&"x".to_string()));
        assert!(
            !leaf_labels(&origin.children)
                .iter()
                .any(|l| l.starts_with("origin/")),
            "remote prefix must not re-print inside its group"
        );
    }

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
        match &section.locals[0] {
            BranchNode::Leaf(l) => assert_eq!(l.label, "zebra", "favorite pinned to top"),
            BranchNode::Dir(..) => panic!("unexpected dir"),
        }
    }

    #[test]
    fn finalize_orders_favorites_then_active_then_alphabetical() {
        let repo = root_with(
            "/alpha",
            &[
                local("feature-z", false, 0, 0),
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
        let order: Vec<String> = leaf_labels(&view.repos[0].locals);
        assert_eq!(
            order,
            ["zebra", "main", "feature-a", "feature-z"]
                .map(|s| s.to_string())
                .to_vec(),
            "favorites first, then the active branch, then alphabetical"
        );
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
        match &origin.children[0] {
            BranchNode::Leaf(l) => assert_eq!(l.label, "starred", "remote favorite pinned to top"),
            BranchNode::Dir(..) => panic!("unexpected dir"),
        }
    }

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
        assert_eq!(recent_names(&rows), vec!["r7", "r6", "r5", "r4", "r3"]);
    }

    #[test]
    fn recent_group_drops_names_that_are_not_local_branches() {
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
        let alpha = root_with("/alpha", &[local("a1", false, 0, 0)], Some("a1"), false);
        let beta = root_with("/beta", &[local("b1", false, 0, 0)], Some("b1"), false);
        let mut recents = HashMap::new();
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
