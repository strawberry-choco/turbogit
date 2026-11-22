//! The branch model, ported from the egui `branches_tree` suite: the
//! rollup's pluralized counts, the reveal predicate asked per repository,
//! the directory counts, and the ordering rule.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use turbogit_app::persistence::BranchesViewOptions;
use turbogit_domain::model::{Branch, BranchKind, Remote, Root, RootId, RootStatus};
use turbogit_ui_nsh::screens::branches::{BranchNode, build_branch_view, leaf_count};

fn root_with_counts(id: &str, remote_count: usize, branch_count: usize) -> Root {
    let path = PathBuf::from(id);
    Root {
        id: RootId(Arc::from(path.clone())),
        path,
        remotes: (0..remote_count)
            .map(|i| Remote {
                name: format!("remote-{i}"),
                fetch_url: None,
                push_url: None,
            })
            .collect(),
        branches: (0..branch_count)
            .map(|i| Branch {
                name: format!("archive/year/branch-{i}"),
                kind: BranchKind::Remote,
                tracking: None,
                favorite: i % 3 == 0,
                protected: false,
                exists: true,
                ahead: 0,
                behind: 0,
                gone: false,
                last_touched: chrono::DateTime::from_timestamp(1_000, 0),
                tip: None,
                remote: Some(format!("remote-{}", i % remote_count.max(1))),
            })
            .collect(),
        current_branch: None,
        head: None,
        status: RootStatus::default(),
    }
}

fn local(name: &str, favorite: bool) -> Branch {
    Branch {
        name: name.to_string(),
        kind: BranchKind::Local,
        tracking: None,
        favorite,
        protected: false,
        exists: true,
        ahead: 0,
        behind: 0,
        gone: false,
        last_touched: Some(chrono::DateTime::from_timestamp(1_000, 0).unwrap()),
        tip: None,
        remote: None,
    }
}

fn root_with_branches(id: &str, branches: Vec<Branch>, current: &str) -> Root {
    let path = PathBuf::from(id);
    Root {
        id: RootId(Arc::from(path.clone())),
        path,
        remotes: Vec::new(),
        branches,
        current_branch: Some(current.to_string()),
        head: Some("abc".to_string()),
        status: RootStatus::default(),
    }
}

#[test]
fn rollup_pluralizes_zero_one_and_many_remotes_and_branches() {
    for (remotes, branches, expected) in [
        (0, 0, None),
        (0, 1, Some("0 remotes · 1 branch")),
        (0, 26, Some("0 remotes · 26 branches")),
        (1, 0, Some("1 remote · 0 branches")),
        (1, 1, Some("1 remote · 1 branch")),
        (1, 26, Some("1 remote · 26 branches")),
        (3, 0, Some("3 remotes · 0 branches")),
        (3, 1, Some("3 remotes · 1 branch")),
        (3, 26, Some("3 remotes · 26 branches")),
    ] {
        let roots = [root_with_counts("/repo", remotes, branches)];
        for show_remotes in [false, true] {
            let view = build_branch_view(
                &roots.iter().collect::<Vec<_>>(),
                &HashMap::new(),
                &|_| show_remotes,
                &BranchesViewOptions::default(),
            );
            assert_eq!(view.repos[0].remote_rollup().as_deref(), expected);
        }
    }
}

#[test]
fn the_reveal_predicate_is_asked_per_repository() {
    let roots = [
        root_with_counts("/revealed", 1, 26),
        root_with_counts("/rolled-up", 1, 6_739),
    ];
    let view = build_branch_view(
        &roots.iter().collect::<Vec<_>>(),
        &HashMap::new(),
        &|root| root.0.to_string_lossy() == "/revealed",
        &BranchesViewOptions::default(),
    );
    assert_eq!(
        view.repos[0].remote_groups.len(),
        1,
        "the revealed repo builds its groups"
    );
    assert!(
        view.repos[1].remote_groups.is_empty(),
        "the rolled-up repo builds none"
    );
    assert_eq!(view.repos[1].remote_branch_count, 6_739);
}

#[test]
fn rollup_counts_all_remote_leaves_without_cross_repo_totals() {
    let roots = [
        root_with_counts("/empty", 0, 0),
        root_with_counts("/unfetched", 1, 0),
        root_with_counts("/small", 1, 26),
        root_with_counts("/long-lived", 3, 6_739),
    ];
    let hidden = build_branch_view(
        &roots.iter().collect::<Vec<_>>(),
        &HashMap::new(),
        &|_| false,
        &BranchesViewOptions::default(),
    );
    let expanded = build_branch_view(
        &roots.iter().collect::<Vec<_>>(),
        &HashMap::new(),
        &|_| true,
        &BranchesViewOptions::default(),
    );
    for ((root, hidden), expanded) in roots.iter().zip(&hidden.repos).zip(&expanded.repos) {
        assert!(hidden.remote_groups.is_empty());
        assert_eq!(hidden.remote_count, root.remotes.len());
        assert_eq!(
            hidden.remote_branch_count,
            root.branches
                .iter()
                .filter(|b| b.kind == BranchKind::Remote)
                .count()
        );
        assert_eq!(hidden.remote_rollup(), expanded.remote_rollup());
        assert_eq!(
            hidden.remote_branch_count,
            expanded
                .remote_groups
                .iter()
                .map(|group| group.count)
                .sum::<usize>()
        );
    }
    assert_eq!(
        hidden.repos[2].remote_rollup().as_deref(),
        Some("1 remote · 26 branches")
    );
    assert_eq!(
        hidden.repos[3].remote_rollup().as_deref(),
        Some("3 remotes · 6739 branches")
    );
}

#[test]
fn directory_counts_equal_their_leaves_at_every_level() {
    let repo = root_with_branches(
        "/alpha",
        vec![
            local("a/b/c", false),
            local("a/b/d", false),
            local("a/e", false),
            local("top", false),
        ],
        "top",
    );
    let view = build_branch_view(
        &[&repo],
        &HashMap::new(),
        &|_| false,
        &BranchesViewOptions::default(),
    );
    fn check(nodes: &[BranchNode]) {
        for node in nodes {
            if let BranchNode::Dir {
                label,
                count,
                children,
            } = node
            {
                assert_eq!(*count, leaf_count(children), "dir {label} count");
                check(children);
            }
        }
    }
    check(&view.repos[0].locals);
    assert_eq!(leaf_count(&view.repos[0].locals), 4);
}

#[test]
fn favorites_then_current_then_alphabetical() {
    let repo = root_with_branches(
        "/alpha",
        vec![
            local("feature-z", false),
            local("main", false),
            local("feature-a", false),
            local("zebra", true),
        ],
        "main",
    );
    let view = build_branch_view(
        &[&repo],
        &HashMap::new(),
        &|_| false,
        &BranchesViewOptions::default(),
    );
    let order: Vec<String> = view.repos[0]
        .locals
        .iter()
        .map(|node| match node {
            BranchNode::Leaf(leaf) => leaf.label.clone(),
            BranchNode::Dir { label, .. } => format!("{label}/"),
        })
        .collect();
    assert_eq!(
        order,
        ["zebra", "main", "feature-a", "feature-z"]
            .map(str::to_string)
            .to_vec(),
        "favorites first, then the active branch, then alphabetical"
    );
}

#[test]
fn directory_grouping_strips_the_prefix_and_keeps_full_names_off() {
    let grouped = root_with_branches("/alpha", vec![local("feature/long-name", false)], "main");
    let view = build_branch_view(
        &[&grouped],
        &HashMap::new(),
        &|_| false,
        &BranchesViewOptions::default(),
    );
    let dir = view.repos[0]
        .locals
        .iter()
        .find_map(|node| match node {
            BranchNode::Dir {
                label, children, ..
            } if label == "feature" => Some(children),
            _ => None,
        })
        .expect("the feature directory");
    match &dir[0] {
        BranchNode::Leaf(leaf) => assert_eq!(leaf.label, "long-name"),
        BranchNode::Dir { .. } => panic!("unexpected nested dir"),
    }

    let ungrouped = root_with_branches("/alpha", vec![local("feature/long-name", false)], "main");
    let view = build_branch_view(
        &[&ungrouped],
        &HashMap::new(),
        &|_| false,
        &BranchesViewOptions {
            group_by_directory: false,
            ..BranchesViewOptions::default()
        },
    );
    match &view.repos[0].locals[0] {
        BranchNode::Leaf(leaf) => assert_eq!(leaf.label, "feature/long-name"),
        BranchNode::Dir { .. } => panic!("ungrouped rows have no directory header"),
    }
}

#[test]
fn the_repository_state_ladder_reads_conflict_then_dirty_then_drift() {
    use turbogit_domain::model::{Change, ChangeStatus};

    let mut dirty = root_with_branches("/d", vec![local("main", false)], "main");
    dirty.status.changes.push(Change {
        path: PathBuf::from("README.md"),
        status: ChangeStatus::Modified,
        chunks: Vec::new(),
        staged: false,
        unstaged: true,
        orig_path: None,
    });
    let clean = root_with_branches("/c", vec![local("main", false)], "main");
    let mut unpushed = root_with_branches("/u", vec![local("main", false)], "main");
    unpushed.branches[0].ahead = 2;
    let mut unpulled = root_with_branches("/p", vec![local("main", false)], "main");
    unpulled.branches[0].behind = 3;

    let view = build_branch_view(
        &[&clean, &dirty, &unpushed, &unpulled],
        &HashMap::new(),
        &|_| false,
        &BranchesViewOptions::default(),
    );
    use turbogit_ui_nsh::theme::RepoState;
    assert_eq!(view.repos[0].status, RepoState::Clean);
    assert_eq!(view.repos[1].status, RepoState::Dirty);
    assert_eq!(view.repos[2].status, RepoState::Unpushed);
    assert_eq!(view.repos[3].status, RepoState::Unpulled);
}
