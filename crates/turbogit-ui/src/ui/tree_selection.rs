//! Multi-selection over the recursive project tree (sidebar-project-tree
//! issue 02): folder tri-state checkboxes at any depth, repo checkboxes
//! independent of their children, and the selection summary walking the
//! tree in pre-order.
//!
//! Pure presentation logic (multi_selection precedent): the checked set is
//! plain data — a `HashSet<RootId>` living in `UiState` — and every rule
//! here takes the tree nodes plus that set, so this module never owns
//! state. A **folder node** exposes a tri-state checkbox over its whole
//! subtree; a **repo node** keeps its own checkbox even when it contains
//! nested repos — it never gets a separate folder checkbox. The summary
//! aggregates one row per selected repo in tree order, so it matches the
//! sidebar's paint order.

use std::collections::HashSet;

use turbogit_domain::model::RootId;

use super::multi_selection::{CheckState, LastCommit, SelectionRow, SelectionSummary, age_label};
use super::project_tree::{FolderNode, ProjectNode, ProjectTree, RepoNode, iter_repos};

/// Toggle one repo's checkbox: repos check independently of their group
/// and of any nested repos.
pub fn toggle_repo(selection: &mut HashSet<RootId>, repo: &RepoNode) {
    if !selection.remove(&repo.id) {
        selection.insert(repo.id.clone());
    }
}

/// A folder checkbox's state, derived from every repo in its subtree at
/// any depth: none checked → unchecked, some but not all → partial, all →
/// checked.
pub fn folder_state(selection: &HashSet<RootId>, folder: &FolderNode) -> CheckState {
    let (checked, total) = subtree_repos(folder).fold((0usize, 0usize), |(c, t), r| {
        (c + selection.contains(&r.id) as usize, t + 1)
    });
    match (checked, total) {
        (0, _) => CheckState::Unchecked,
        (c, t) if c == t => CheckState::Checked,
        _ => CheckState::Partial,
    }
}

/// Toggle a folder's checkbox: unchecked or partial → check every repo in
/// the subtree; checked → uncheck them all.
pub fn toggle_folder(selection: &mut HashSet<RootId>, folder: &FolderNode) {
    if folder_state(selection, folder) == CheckState::Checked {
        for r in subtree_repos(folder) {
            selection.remove(&r.id);
        }
    } else {
        for r in subtree_repos(folder) {
            selection.insert(r.id.clone());
        }
    }
}

/// The selected repo rows, in tree order (pre-order, matching the
/// sidebar's paint order).
pub fn selected_repos<'a>(tree: &'a ProjectTree, selection: &HashSet<RootId>) -> Vec<&'a RepoNode> {
    iter_repos(tree)
        .filter(|r| selection.contains(&r.id))
        .collect()
}

/// Compute the selection summary: aggregate stats and one table row per
/// selected repo, in tree order. `last_commit` reads the root caches (a
/// `(subject, unix time)` hit, or `None` when the log is not loaded);
/// `now` is the reference unix time for age formatting.
pub fn selection_summary(
    tree: &ProjectTree,
    selection: &HashSet<RootId>,
    last_commit: &dyn Fn(&RootId) -> Option<(String, i64)>,
    now: i64,
) -> SelectionSummary {
    let mut s = SelectionSummary::default();
    for repo in selected_repos(tree, selection) {
        s.repos += 1;
        s.ahead += repo.ahead;
        s.behind += repo.behind;
        s.dirty_files += repo.dirty_count;
        s.rows.push(SelectionRow {
            // The summary matches the sidebar, so a promoted repo carries
            // its path label.
            name: repo.label.clone(),
            branch: repo.branch.clone(),
            ahead: repo.ahead,
            behind: repo.behind,
            dirty: repo.dirty_count,
            last_commit: last_commit(&repo.id).map(|(subject, t)| LastCommit {
                subject,
                age: age_label(now.saturating_sub(t)),
            }),
        });
    }
    s
}

/// Every repo node under a folder, in pre-order (its subtree at any depth,
/// including repos nested beneath repo children).
fn subtree_repos(folder: &FolderNode) -> impl Iterator<Item = &RepoNode> {
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
    walk(&folder.children, &mut out);
    out.into_iter()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use turbogit_domain::model::{Change, ChangeStatus, Root, RootId, RootStatus};

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

    fn id_of(path: &str) -> RootId {
        RootId(PathBuf::from(path).into())
    }

    /// R(a) > { R(b), F(c) > { R(d), R(e) } } — a repo with nested repos
    /// and a folder two levels down.
    fn nested_tree() -> ProjectTree {
        let project = Path::new("/w/a");
        super::super::project_tree::build_tree(
            project,
            &[
                root("/w/a", Some("main")),
                root("/w/a/b", Some("main")),
                root("/w/a/c/d", Some("main")),
                root("/w/a/c/e", Some("main")),
            ],
            &|_| Some((0, 0)),
        )
    }

    fn folder_c(tree: &ProjectTree) -> &FolderNode {
        let ProjectNode::Repo(a) = &tree.nodes[0] else {
            panic!("expected a repo root");
        };
        match &a.children[1] {
            ProjectNode::Folder(c) => c,
            _ => panic!("expected folder c"),
        }
    }

    fn repo_a(tree: &ProjectTree) -> &RepoNode {
        match &tree.nodes[0] {
            ProjectNode::Repo(a) => a,
            _ => panic!("expected a repo root"),
        }
    }

    #[test]
    fn folder_state_is_derived_from_subtree_repos_at_any_depth() {
        let tree = nested_tree();
        let c = folder_c(&tree);
        let mut selection = HashSet::new();

        assert_eq!(folder_state(&selection, c), CheckState::Unchecked);

        selection.insert(id_of("/w/a/c/d"));
        assert_eq!(
            folder_state(&selection, c),
            CheckState::Partial,
            "one of two repos selected is partial"
        );

        selection.insert(id_of("/w/a/c/e"));
        assert_eq!(folder_state(&selection, c), CheckState::Checked);
    }

    #[test]
    fn toggling_a_folder_checks_or_clears_every_repo_under_it() {
        let tree = nested_tree();
        let c = folder_c(&tree);
        let mut selection = HashSet::new();

        // Unchecked → checks the entire subtree.
        toggle_folder(&mut selection, c);
        assert_eq!(selection.len(), 2);
        assert!(selection.contains(&id_of("/w/a/c/d")));
        assert!(selection.contains(&id_of("/w/a/c/e")));
        assert_eq!(folder_state(&selection, c), CheckState::Checked);

        // Checked → clears the entire subtree.
        toggle_folder(&mut selection, c);
        assert!(selection.is_empty());

        // Partial → completes, never clears.
        selection.insert(id_of("/w/a/c/d"));
        toggle_folder(&mut selection, c);
        assert_eq!(selection.len(), 2);
        assert_eq!(folder_state(&selection, c), CheckState::Checked);
    }

    #[test]
    fn toggling_a_repo_affects_only_that_repo_even_with_nested_repos() {
        let tree = nested_tree();
        let mut selection = HashSet::new();
        let a = repo_a(&tree);

        toggle_repo(&mut selection, a);
        assert_eq!(selection.len(), 1, "the outer repo is selected alone");
        assert!(selection.contains(&a.id.clone()));

        // Its nested repos stay unselected, and the folder beneath it
        // stays unchecked (a repo-with-children never gets a folder
        // checkbox).
        assert!(!selection.contains(&id_of("/w/a/b")));
        assert_eq!(
            folder_state(&selection, folder_c(&tree)),
            CheckState::Unchecked
        );

        toggle_repo(&mut selection, a);
        assert!(selection.is_empty());
    }

    #[test]
    fn selected_repos_walk_the_tree_in_pre_order() {
        let tree = nested_tree();
        let mut selection = HashSet::new();
        selection.insert(id_of("/w/a/c/e"));
        selection.insert(id_of("/w/a"));
        selection.insert(id_of("/w/a/b"));

        let names: Vec<&str> = selected_repos(&tree, &selection)
            .iter()
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(names, ["a", "b", "e"], "tree order, not click order");
    }

    #[test]
    fn selection_summary_aggregates_over_the_recursive_walk_in_tree_order() {
        let project = Path::new("/w/a");
        let mut a = root("/w/a", Some("main"));
        a.status.changes = (0..5).map(|i| modified(&format!("f{i}.txt"))).collect();
        let tree = super::super::project_tree::build_tree(
            project,
            &[
                a,
                root("/w/a/b", Some("main")),
                root("/w/a/c/d", Some("main")),
                root("/w/a/c/e", Some("dev")),
            ],
            &|id| {
                let p = id.0.as_os_str();
                if p == "/w/a" {
                    Some((2, 1))
                } else if p == "/w/a/b" {
                    Some((1, 0))
                } else if p == "/w/a/c/e" {
                    Some((0, 1))
                } else {
                    Some((0, 0))
                }
            },
        );
        let mut selection = HashSet::new();
        selection.insert(id_of("/w/a"));
        selection.insert(id_of("/w/a/c/e"));

        let s = selection_summary(&tree, &selection, &|_| None, 1_000_000);
        assert_eq!(s.repos, 2);
        assert_eq!(s.ahead, 2, "a's ↑2 plus e's ↑0");
        assert_eq!(s.behind, 2, "a's ↓1 plus e's ↓1");
        assert_eq!(s.dirty_files, 5, "a's 5 dirty files plus e's 0");
        let rows: Vec<&str> = s.rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(rows, ["a", "e"], "one row per selected repo in tree order");
        assert_eq!(s.rows[0].branch.as_deref(), Some("main"));
        assert_eq!((s.rows[0].ahead, s.rows[0].behind), (2, 1));
        assert_eq!(s.rows[0].dirty, 5);
        assert_eq!(s.rows[1].branch.as_deref(), Some("dev"));
        assert_eq!((s.rows[1].ahead, s.rows[1].behind), (0, 1));
    }

    #[test]
    fn summary_rows_carry_the_last_commit_when_cached() {
        let tree = nested_tree();
        let mut selection = HashSet::new();
        selection.insert(id_of("/w/a"));

        let s = selection_summary(
            &tree,
            &selection,
            &|id| {
                if id == &id_of("/w/a") {
                    Some(("preflight matrix".to_string(), 999_140))
                } else {
                    None
                }
            },
            1_000_000,
        );
        assert_eq!(s.rows.len(), 1);
        let lc = s.rows[0].last_commit.as_ref().expect("the lookup hit");
        assert_eq!(lc.subject, "preflight matrix");
        assert_eq!(lc.age, "14m");
    }

    #[test]
    fn promoted_repos_show_their_path_label_in_selection_and_summary() {
        let project = Path::new("/w");
        let tree = super::super::project_tree::build_tree(
            project,
            &[root("/w/foo/bar", Some("main"))],
            &|_| Some((0, 0)),
        );
        let mut selection = HashSet::new();
        let ProjectNode::Repo(r) = &tree.nodes[0] else {
            panic!("expected a promoted repo");
        };
        toggle_repo(&mut selection, r);

        let selected: Vec<&str> = selected_repos(&tree, &selection)
            .iter()
            .map(|r| r.label.as_str())
            .collect();
        assert_eq!(selected, ["w/f/bar"]);

        let s = selection_summary(&tree, &selection, &|_| None, 1_000_000);
        assert_eq!(s.repos, 1);
        assert_eq!(s.rows[0].name, "w/f/bar", "the summary matches the sidebar");
    }
}
