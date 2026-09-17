//! Recursive project tree (sidebar-project-tree issue 01): the pure model
//! behind the sidebar's PROJECTS section, replacing the flat
//! `groups → repos` shape with a tree rooted at the project directory.
//!
//! A node is either a **folder node** (a directory that is not itself a
//! repository root) or a **repo node** (a repository root carrying the
//! status dot, branch, ahead/behind, and conflict/dirty counts; it may
//! carry child nodes when repositories nest). Pure presentation logic over
//! the flat registered-root list, per ADR-0018: the builder consumes the
//! exact same roots the flat model consumes; discovery is unchanged.
//!
//! Three functions form the primary seam, all tested without any UI:
//! [`build_tree`] (builder + collapse rule + path labels + subtree
//! totals), [`filter_tree`] (the live text filter), and [`filter_repos`]
//! (the predicate filter the smart-group narrowing rides on). Both filters
//! narrow the tree to the surviving repos and then re-apply the collapse
//! rule, so a folder with one surviving repo collapses again.

use std::path::{Path, PathBuf};

use turbogit_domain::model::{Root, RootId};

/// Sidebar and branch headers share state meanings and colors.
pub use crate::theme::RepoState as DotState;

/// A repo node: a repository root with its git state and, when
/// repositories nest, the nodes beneath it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoNode {
    pub id: RootId,
    /// The repo's own name (its path's file name), kept in full.
    pub name: String,
    /// The display label: the repo's bare name, or the abbreviated
    /// **path label** when the repo was promoted out of a collapsed
    /// folder chain (each collapsed folder's first character, joined
    /// with `name` by `/`).
    pub label: String,
    /// Full path (the live filter matches against it).
    pub path: PathBuf,
    /// Current branch label, `None` when detached.
    pub branch: Option<String>,
    /// The row's status dot (CONTEXT.md: clean / dirty / conflict / diverged).
    pub dot: DotState,
    /// Outgoing commits vs upstream (↑ badge).
    pub ahead: usize,
    /// Incoming commits vs upstream (↓ badge).
    pub behind: usize,
    /// Conflicted paths (the smart-group conflict predicate).
    pub conflicts: usize,
    /// Modified + unversioned paths (the smart-group dirty predicate).
    pub dirty_count: usize,
    /// Child nodes when repositories nest (a repo inside a repo).
    pub children: Vec<ProjectNode>,
}

/// A folder node: a directory that is not itself a repository root,
/// grouping the repos and folders beneath it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderNode {
    /// The folder's own name (its path's file name).
    pub name: String,
    /// Child nodes in name order.
    pub children: Vec<ProjectNode>,
    /// Repos anywhere in the subtree (the folder's repo-count badge).
    pub total: usize,
    /// Repos in the subtree with a dirty or conflicted dot (the dirty badge).
    pub dirty: usize,
}

/// One node of the project tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectNode {
    Folder(FolderNode),
    Repo(RepoNode),
}

/// The fully collapsed tree rooted at the project directory. The project
/// directory is itself just the topmost node: a repo node when it has a
/// `.git`, a folder node otherwise. May be empty when the project has no
/// repos.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectTree {
    /// Top-level displayed nodes, in name order.
    pub nodes: Vec<ProjectNode>,
}

/// Every repo node of the tree in pre-order (tree order matches the
/// sidebar's paint order).
pub fn iter_repos(tree: &ProjectTree) -> impl Iterator<Item = &RepoNode> {
    fn walk<'a>(nodes: &'a [ProjectNode], out: &mut Vec<&'a RepoNode>) {
        for node in nodes {
            match node {
                ProjectNode::Repo(r) => {
                    out.push(r);
                    walk(&r.children, out);
                }
                ProjectNode::Folder(f) => walk(&f.children, out),
            }
        }
    }
    let mut out = Vec::new();
    walk(&tree.nodes, &mut out);
    out.into_iter()
}

/// Build the recursive project tree from exactly the flat registered-root
/// list, applying the collapse rule at every depth.
///
/// - a repo node always displays;
/// - a folder node displays only when its subtree holds at least two repo
///   nodes;
/// - a folder whose subtree holds exactly one repo collapses and that repo
///   is promoted to the folder's parent, labeled with the abbreviated path
///   chain (the topmost collapsing folder's first character per level).
pub fn build_tree(
    project_dir: &Path,
    roots: &[Root],
    ahead_behind: &dyn Fn(&RootId) -> Option<(usize, usize)>,
) -> ProjectTree {
    assemble(
        project_dir,
        roots.iter().map(|r| repo_data(r, ahead_behind)),
    )
}

/// Narrow the tree to the repos matching `query` (name, full path, branch,
/// or any ancestor folder name), then re-collapse the survivors with the
/// same rule and path labels. The empty query returns the tree unchanged.
pub fn filter_tree(project_dir: &Path, tree: &ProjectTree, query: &str) -> ProjectTree {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return tree.clone();
    }
    filter_repos(project_dir, tree, |r| {
        r.name.to_lowercase().contains(&q)
            || r.path.to_string_lossy().to_lowercase().contains(&q)
            || r.branch
                .as_deref()
                .is_some_and(|b| b.to_lowercase().contains(&q))
            || ancestor_folder_matches(project_dir, r, &q)
    })
}

/// Narrow the tree to the repos the predicate accepts, then re-collapse
/// the survivors with the same rule and path labels. The smart-group
/// narrowing rides on this (issue 01: "narrows to its member repos and
/// re-collapses the same way").
pub fn filter_repos(
    project_dir: &Path,
    tree: &ProjectTree,
    keep: impl Fn(&RepoNode) -> bool,
) -> ProjectTree {
    let survivors = iter_repos(tree).filter(|r| keep(r)).map(|r| RepoData {
        id: r.id.clone(),
        name: r.name.clone(),
        path: r.path.clone(),
        branch: r.branch.clone(),
        dot: r.dot,
        ahead: r.ahead,
        behind: r.behind,
        conflicts: r.conflicts,
        dirty_count: r.dirty_count,
    });
    assemble(project_dir, survivors)
}

// --- Internal model -----------------------------------------------------------

/// The repo fields that survive a re-build (a filter re-collapses the tree
/// from the surviving repos' paths).
#[derive(Debug, Clone)]
struct RepoData {
    id: RootId,
    name: String,
    path: PathBuf,
    branch: Option<String>,
    dot: DotState,
    ahead: usize,
    behind: usize,
    conflicts: usize,
    dirty_count: usize,
}

/// The pre-collapse tree: directories and repos by name, before the
/// display rule shapes it.
enum RawNode {
    Folder {
        name: String,
        children: Vec<RawNode>,
    },
    Repo {
        row: RepoData,
        children: Vec<RawNode>,
    },
}

/// Map each registered root onto its row data (the fields a repo node
/// carries beyond its children and label).
fn repo_data(root: &Root, ahead_behind: &dyn Fn(&RootId) -> Option<(usize, usize)>) -> RepoData {
    let (ahead, behind) = ahead_behind(&root.id).unwrap_or((0, 0));
    RepoData {
        id: root.id.clone(),
        name: basename(&root.path),
        path: root.path.clone(),
        branch: root.current_branch.clone(),
        dot: dot_state(root, ahead, behind),
        ahead,
        behind,
        conflicts: root.status.conflicted.len(),
        dirty_count: root.status.modified() + root.status.unversioned(),
    }
}

/// Build the raw tree for every repo in `repos`, then apply the display
/// rule. The project directory is the topmost node: a repo node when it
/// has a `.git`, a folder node otherwise.
fn assemble(project_dir: &Path, repos: impl Iterator<Item = RepoData>) -> ProjectTree {
    let mut root = RawNode::Folder {
        name: basename(project_dir),
        children: Vec::new(),
    };
    let mut root_row: Option<RepoData> = None;
    for r in repos {
        match r.path.strip_prefix(project_dir) {
            // The project directory itself is a repository root.
            Ok(rel) if rel.as_os_str().is_empty() => root_row = Some(r),
            Ok(rel) => {
                let comps: Vec<String> = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                insert_components(&mut root, &comps, r);
            }
            // A root outside the project directory cannot come from
            // discovery; keep it visible at the top level rather than
            // dropping it.
            Err(_) => insert_components(&mut root, &[basename(&r.path)], r),
        }
    }
    sort_rec(&mut root);
    let nodes = if let Some(row) = root_row {
        let children = match &mut root {
            RawNode::Folder { children, .. } => std::mem::take(children),
            RawNode::Repo { .. } => unreachable!("the root starts as a folder"),
        };
        display(RawNode::Repo { row, children })
    } else {
        display(root)
    };
    ProjectTree { nodes }
}

/// Insert one repo into the raw tree by descending the relative path
/// components, creating folders as needed.
fn insert_components(root: &mut RawNode, comps: &[String], row: RepoData) {
    let (last, chain) = comps
        .split_last()
        .expect("a repo path has at least one component");
    let mut children: &mut Vec<RawNode> = node_children_mut(root);
    for name in chain {
        children = child_dir_mut(children, name);
    }
    upsert_repo(children, last.clone(), row);
}

/// Descend into the child named `name` (a folder or a repo — nested repos
/// carry their children too), creating a folder when it does not exist.
fn child_dir_mut<'a>(children: &'a mut Vec<RawNode>, name: &str) -> &'a mut Vec<RawNode> {
    match children.iter().position(|c| node_name(c) == name) {
        Some(ix) => node_children_mut(&mut children[ix]),
        None => {
            children.push(RawNode::Folder {
                name: name.to_string(),
                children: Vec::new(),
            });
            node_children_mut(children.last_mut().expect("just pushed"))
        }
    }
}

/// Attach a repo at `name`: replace an existing repo row (nested repos),
/// convert a folder that turns out to be a repo itself, or push a new node.
fn upsert_repo(children: &mut Vec<RawNode>, name: String, row: RepoData) {
    if let Some(existing) = children.iter_mut().find(|c| node_name(c) == name) {
        match existing {
            // A repo already sits at this position (a repo inside a repo):
            // refresh its row.
            RawNode::Repo {
                row: existing_row, ..
            } => *existing_row = row,
            // A directory is a repository root: convert the folder to a
            // repo node, keeping the nested children.
            RawNode::Folder { children: kids, .. } => {
                let kids = std::mem::take(kids);
                *existing = RawNode::Repo {
                    row,
                    children: kids,
                };
            }
        }
    } else {
        children.push(RawNode::Repo {
            row,
            children: Vec::new(),
        });
    }
}

fn node_name(node: &RawNode) -> &str {
    match node {
        RawNode::Folder { name, .. } => name,
        RawNode::Repo { row, .. } => &row.name,
    }
}

fn node_children_mut(node: &mut RawNode) -> &mut Vec<RawNode> {
    match node {
        RawNode::Folder { children, .. } => children,
        RawNode::Repo { children, .. } => children,
    }
}

fn sort_rec(node: &mut RawNode) {
    let children = node_children_mut(node);
    children.sort_by(|a, b| node_name(a).cmp(node_name(b)));
    for child in children.iter_mut() {
        sort_rec(child);
    }
}

/// The display rule (ADR-0018 D2/D3): repo nodes always display; a folder
/// displays iff its subtree holds at least two repos; a folder holding
/// exactly one collapses, promoting its single repo with this folder's
/// first character prefixed to the repo's label.
fn display(raw: RawNode) -> Vec<ProjectNode> {
    match raw {
        RawNode::Repo { row, children } => {
            vec![ProjectNode::Repo(RepoNode {
                id: row.id,
                name: row.name.clone(),
                label: row.name,
                path: row.path,
                branch: row.branch,
                dot: row.dot,
                ahead: row.ahead,
                behind: row.behind,
                conflicts: row.conflicts,
                dirty_count: row.dirty_count,
                children: display_nodes(children),
            })]
        }
        RawNode::Folder { name, children } => {
            let displayed = display_nodes(children);
            let total = displayed.iter().map(subtree_total).sum::<usize>();
            match total {
                // An empty folder (no repos) renders nothing.
                0 => Vec::new(),
                1 => {
                    // Exactly one repo in the subtree: promote it with
                    // this folder's first character (a displayed folder
                    // needs ≥ 2 repos, so the lone child is the repo).
                    let repo = match displayed.into_iter().next() {
                        Some(ProjectNode::Repo(r)) => r,
                        _ => unreachable!("a subtree with one repo surfaces as that repo"),
                    };
                    vec![ProjectNode::Repo(RepoNode {
                        label: prefix_label(repo.label, &name),
                        ..repo
                    })]
                }
                _ => {
                    let dirty = displayed.iter().map(subtree_dirty).sum();
                    vec![ProjectNode::Folder(FolderNode {
                        name,
                        children: displayed,
                        total,
                        dirty,
                    })]
                }
            }
        }
    }
}

fn display_nodes(nodes: Vec<RawNode>) -> Vec<ProjectNode> {
    nodes.into_iter().flat_map(display).collect()
}

/// Repos in a node's subtree, counting repos nested beneath repo children.
fn subtree_total(node: &ProjectNode) -> usize {
    match node {
        ProjectNode::Folder(f) => f.total,
        ProjectNode::Repo(r) => 1 + r.children.iter().map(subtree_total).sum::<usize>(),
    }
}

/// Repos in a node's subtree with a dirty or conflicted dot.
fn subtree_dirty(node: &ProjectNode) -> usize {
    match node {
        ProjectNode::Folder(f) => f.dirty,
        ProjectNode::Repo(r) => {
            (matches!(r.dot, DotState::Dirty | DotState::Conflict) as usize)
                + r.children.iter().map(subtree_dirty).sum::<usize>()
        }
    }
}

/// Prefix a promoted repo's label with a collapsed folder's first Unicode
/// character (`foo/bar` promoted under `x` → `x/f/bar`).
fn prefix_label(label: String, folder: &str) -> String {
    match folder.chars().next() {
        Some(c) => format!("{c}/{label}"),
        None => label,
    }
}

/// Every folder on the repo's path from the project directory down to its
/// parent: a name match keeps the whole subtree (the flat model's
/// group-name rule, generalized to every depth).
fn ancestor_folder_matches(project_dir: &Path, repo: &RepoNode, q: &str) -> bool {
    let Ok(rel) = repo.path.strip_prefix(project_dir) else {
        return false;
    };
    rel.components()
        .take(rel.components().count().saturating_sub(1))
        .any(|c| c.as_os_str().to_string_lossy().to_lowercase().contains(q))
}

/// File-name label of a path (the `<repo>` fallback matches the shell's).
fn basename(path: &Path) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("<repo>")
        .to_string()
}

/// Use the same precedence as the branch section header.
fn dot_state(root: &Root, ahead: usize, behind: usize) -> DotState {
    DotState::from_root(root, ahead, behind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbogit_domain::model::{Change, ChangeStatus, RootStatus};

    fn root(path: &str, branch: Option<&str>) -> Root {
        Root {
            id: RootId(PathBuf::from(path).into()),
            path: PathBuf::from(path),
            remotes: vec![],
            branches: vec![],
            current_branch: branch.map(str::to_string),
            head: None,
            status: RootStatus::default(),
        }
    }

    fn modified(path: &str) -> Change {
        Change {
            path: PathBuf::from(path),
            status: ChangeStatus::Modified,
            chunks: vec![],
            staged: false,
            unstaged: false,
            orig_path: None,
        }
    }

    fn dirty_root(path: &str, branch: Option<&str>) -> Root {
        let mut r = root(path, branch);
        r.status.changes = vec![modified("x.txt")];
        r
    }

    fn conflicted_root(path: &str, branch: Option<&str>) -> Root {
        let mut r = root(path, branch);
        r.status.conflicted = vec![PathBuf::from("x.txt")];
        r
    }

    fn repo(n: &ProjectNode) -> &RepoNode {
        match n {
            ProjectNode::Repo(r) => r,
            ProjectNode::Folder(_) => panic!("expected a repo node, got a folder"),
        }
    }

    fn folder(n: &ProjectNode) -> &FolderNode {
        match n {
            ProjectNode::Folder(f) => f,
            ProjectNode::Repo(_) => panic!("expected a folder node, got a repo"),
        }
    }

    fn labels(nodes: &[ProjectNode]) -> Vec<String> {
        nodes
            .iter()
            .map(|n| match n {
                ProjectNode::Repo(r) => r.label.clone(),
                ProjectNode::Folder(f) => f.name.clone(),
            })
            .collect()
    }

    // --- The four canonical layouts (spec §Further Notes) ---

    #[test]
    fn project_dir_that_is_itself_a_repo_renders_a_single_repo_item() {
        // `a/.git` → `a`
        let project = Path::new("/w/a");
        let tree = build_tree(project, &[root("/w/a", Some("main"))], &|_| Some((0, 0)));

        assert_eq!(tree.nodes.len(), 1, "no redundant wrapping folder");
        let r = repo(&tree.nodes[0]);
        assert_eq!(r.name, "a");
        assert_eq!(r.label, "a");
        assert_eq!(r.branch.as_deref(), Some("main"));
        assert!(r.children.is_empty());
    }

    #[test]
    fn two_repos_directly_under_a_folder_render_a_folder_over_the_repos() {
        // `a/b` + `a/c` → folder `a` over `b`, `c`
        let project = Path::new("/w/a");
        let tree = build_tree(
            project,
            &[root("/w/a/b", Some("main")), root("/w/a/c", Some("main"))],
            &|_| Some((0, 0)),
        );

        assert_eq!(tree.nodes.len(), 1);
        let f = folder(&tree.nodes[0]);
        assert_eq!(f.name, "a");
        assert_eq!((f.total, f.dirty), (2, 0));
        assert_eq!(labels(&f.children), ["b", "c"], "repos sort by name");
    }

    #[test]
    fn nested_directories_render_a_folder_at_every_level_holding_two_or_more_repos() {
        // `a/b` + `a/c/d` + `a/c/e` → folder `a` over `b` and folder `c`
        // over `d`, `e`
        let project = Path::new("/w/a");
        let tree = build_tree(
            project,
            &[
                root("/w/a/b", Some("main")),
                root("/w/a/c/d", Some("main")),
                root("/w/a/c/e", Some("main")),
            ],
            &|_| Some((0, 0)),
        );

        let a = folder(&tree.nodes[0]);
        assert_eq!(a.name, "a");
        assert_eq!((a.total, a.dirty), (3, 0));
        let b = repo(&a.children[0]);
        assert_eq!(b.name, "b");
        let c = folder(&a.children[1]);
        assert_eq!(c.name, "c");
        assert_eq!((c.total, c.dirty), (2, 0));
        assert_eq!(labels(&c.children), ["d", "e"]);
    }

    #[test]
    fn a_repo_inside_a_repo_renders_the_outer_with_the_inner_beneath() {
        // `a/.git` + `a/b/.git` → repo `a` over repo `b`
        let project = Path::new("/w/a");
        let tree = build_tree(
            project,
            &[root("/w/a", Some("main")), root("/w/a/b", Some("dev"))],
            &|_| Some((0, 0)),
        );

        assert_eq!(tree.nodes.len(), 1);
        let a = repo(&tree.nodes[0]);
        assert_eq!(a.name, "a");
        assert_eq!(a.label, "a", "the outer repo keeps its own name");
        assert_eq!(a.children.len(), 1);
        let b = repo(&a.children[0]);
        assert_eq!(b.name, "b");
        assert_eq!(b.branch.as_deref(), Some("dev"));
    }

    // --- Collapse rule and path labels ---

    #[test]
    fn a_deep_single_repo_chain_collapses_to_one_promoted_repo() {
        // `a/b/c/.git` (a = project directory) → `a/b/c`
        let project = Path::new("/w/a");
        let tree = build_tree(project, &[root("/w/a/b/c", Some("main"))], &|_| {
            Some((0, 0))
        });

        assert_eq!(tree.nodes.len(), 1, "the whole chain collapses to one item");
        let r = repo(&tree.nodes[0]);
        assert_eq!(r.name, "c");
        assert_eq!(
            r.label, "a/b/c",
            "each collapsed folder contributes its first char, the repo's own name in full"
        );
    }

    #[test]
    fn a_chain_of_single_repo_folders_promotes_with_an_abbreviated_path_label() {
        // `foo/bar/.git` under a collapsing project root → `w/f/bar`
        let project = Path::new("/w");
        let tree = build_tree(project, &[root("/w/foo/bar", Some("main"))], &|_| {
            Some((0, 0))
        });

        let r = repo(&tree.nodes[0]);
        assert_eq!(r.label, "w/f/bar");
    }

    #[test]
    fn promoted_labels_start_at_the_topmost_collapsing_folder_and_never_repeat_the_parent() {
        // The parent `w` displays (two repos in its subtree); the `top`
        // chain beneath it collapses and its label starts at `top`, not `w`.
        let project = Path::new("/w");
        let tree = build_tree(
            project,
            &[
                root("/w/top/n/m/repo", Some("main")),
                root("/w/side", Some("main")),
            ],
            &|_| Some((0, 0)),
        );

        let w = folder(&tree.nodes[0]);
        assert_eq!(
            labels(&w.children),
            ["side", "t/n/m/repo"],
            "the displayed parent w is never repeated in the label"
        );
    }

    #[test]
    fn non_ascii_folder_names_abbreviate_by_first_character() {
        // `项目/代码` → `项/代码`
        let project = Path::new("/w");
        let tree = build_tree(
            project,
            &[
                root("/w/项目/代码", Some("main")),
                root("/w/other", Some("main")),
            ],
            &|_| Some((0, 0)),
        );

        let w = folder(&tree.nodes[0]);
        assert_eq!(w.total, 2);
        assert_eq!(labels(&w.children), ["other", "项/代码"]);
    }

    // --- Subtree totals ---

    #[test]
    fn folder_subtree_totals_count_repos_and_dirty_beneath_them() {
        let project = Path::new("/w/a");
        let tree = build_tree(
            project,
            &[
                dirty_root("/w/a/b", Some("main")),
                root("/w/a/c/d", Some("main")),
                dirty_root("/w/a/c/e", Some("main")),
                conflicted_root("/w/a/f", Some("main")),
            ],
            &|_| Some((0, 0)),
        );

        let a = folder(&tree.nodes[0]);
        assert_eq!(
            (a.total, a.dirty),
            (4, 3),
            "b and e are dirty; f's conflict counts as dirty"
        );
        let c = folder(&a.children[1]);
        assert_eq!((c.total, c.dirty), (2, 1));
    }

    #[test]
    fn folder_totals_include_repos_nested_beneath_repo_children() {
        let project = Path::new("/w");
        let tree = build_tree(
            project,
            &[root("/w/x/a", Some("main")), root("/w/x/a/b", Some("main"))],
            &|_| Some((0, 0)),
        );

        let w = folder(&tree.nodes[0]);
        let x = folder(&w.children[0]);
        assert_eq!(w.total, 2, "the outer folder counts every repo beneath");
        assert_eq!(x.total, 2, "the nested repo b counts in x's subtree");
        let a = repo(&x.children[0]);
        assert_eq!(a.children.len(), 1);
    }

    #[test]
    fn an_empty_workspace_renders_no_nodes() {
        let tree = build_tree(Path::new("/w"), &[], &|_| Some((0, 0)));
        assert!(tree.nodes.is_empty());
    }

    // --- Filters re-collapse the survivors ---

    #[test]
    fn live_filter_narrows_to_matching_repos_and_recollapses_with_path_labels() {
        let project = Path::new("/w");
        let roots = vec![
            root("/w/x/y/repo1", Some("main")),
            root("/w/x/z/repo2", Some("release/2")),
        ];
        let full = build_tree(project, &roots, &|_| Some((0, 0)));

        // The full view collapses each single-repo leaf chain under the
        // displayed folder `x`.
        let w = folder(&full.nodes[0]);
        let x = folder(&w.children[0]);
        assert_eq!(labels(&x.children), ["y/repo1", "z/repo2"]);

        // By repo name: the single survivor re-collapses the whole chain.
        let t = filter_tree(project, &full, "repo1");
        assert_eq!(t.nodes.len(), 1);
        let r = repo(&t.nodes[0]);
        assert_eq!(r.label, "w/x/y/repo1");
        assert_eq!(r.name, "repo1");

        // By branch, case-insensitively.
        let t = filter_tree(project, &full, "RELEASE");
        assert_eq!(repo(&t.nodes[0]).label, "w/x/z/repo2");

        // By full path.
        let t = filter_tree(project, &full, "/w/x/y/repo1");
        assert_eq!(t.nodes.len(), 1);

        // A folder-name match keeps the whole subtree.
        let t = filter_tree(project, &full, "x");
        assert_eq!(folder(&t.nodes[0]).children.len(), 1);

        // No match → nothing renders.
        assert!(
            filter_tree(project, &full, "nothing-matches")
                .nodes
                .is_empty()
        );

        // The empty query returns the tree unchanged.
        assert_eq!(filter_tree(project, &full, ""), full);
    }

    #[test]
    fn predicate_filter_narrows_to_member_repos_and_recollapses_the_same_way() {
        let project = Path::new("/w");
        let roots = vec![
            dirty_root("/w/grp/a", Some("main")),
            dirty_root("/w/grp/b", Some("main")),
            root("/w/other/c", Some("main")),
        ];
        let full = build_tree(project, &roots, &|_| Some((0, 0)));
        let w = folder(&full.nodes[0]);
        assert_eq!(w.total, 3);

        // Two surviving members keep their shared folder.
        let t = filter_repos(project, &full, |r| r.dirty_count > 0);
        let w = folder(&t.nodes[0]);
        assert_eq!(w.total, 2);
        let grp = folder(&w.children[0]);
        assert_eq!(grp.name, "grp");
        assert_eq!(labels(&grp.children), ["a", "b"]);

        // One surviving member under a chain re-collapses to one repo.
        let t = filter_repos(project, &full, |r| r.name == "c");
        assert_eq!(t.nodes.len(), 1);
        assert_eq!(repo(&t.nodes[0]).label, "w/o/c");
    }

    #[test]
    fn iter_repos_walks_the_tree_in_pre_order() {
        let project = Path::new("/w/a");
        let tree = build_tree(
            project,
            &[
                root("/w/a", Some("main")),
                root("/w/a/b", Some("main")),
                root("/w/a/c/d", Some("main")),
            ],
            &|_| Some((0, 0)),
        );

        let names: Vec<&str> = iter_repos(&tree).map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["a", "b", "d"], "pre-order: outer before inner");
    }

    #[test]
    fn dot_state_precedence_conflict_then_diverged_then_dirty_then_clean() {
        // Migrated from the flat sidebar suite (issue 04): the row dot is
        // derived from the same precedence chain.
        let project = Path::new("/w/a");
        let mut conflicted = root("/w/a/x", Some("main"));
        conflicted.status.conflicted = vec![PathBuf::from("f.txt")];
        let mut dirty = root("/w/a/y", Some("main"));
        dirty.status.changes = vec![modified("g.txt")];
        let clean = root("/w/a/z", Some("main"));
        let roots = vec![conflicted, dirty, clean];
        let tree = build_tree(project, &roots, &|id| {
            if id.0.as_os_str() == "/w/a/z" {
                Some((2, 1))
            } else {
                Some((0, 0))
            }
        });

        let dot_of = |name: &str| {
            iter_repos(&tree)
                .find(|r| r.name == name)
                .expect("the repo is in the tree")
                .dot
        };
        assert_eq!(dot_of("x"), DotState::Conflict, "conflict outranks all");
        assert_eq!(dot_of("z"), DotState::Diverged, "diverged outranks dirty");
        assert_eq!(dot_of("y"), DotState::Dirty);
        assert_ne!(dot_of("x"), DotState::Clean);
    }

    #[test]
    fn ahead_and_behind_land_on_repo_rows_as_badge_counts() {
        // Migrated from the flat sidebar suite (issue 04): the cache lookup
        // fills the row's badge counts.
        let project = Path::new("/w/a");
        let tree = build_tree(project, &[root("/w/a/app", Some("main"))], &|_| {
            Some((3, 2))
        });

        let r = repo(&tree.nodes[0]);
        assert_eq!((r.ahead, r.behind), (3, 2));
    }
}
