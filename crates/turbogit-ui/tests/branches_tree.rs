use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use turbogit_domain::model::{Branch, BranchKind, Remote, Root, RootId, RootStatus};
use turbogit_ui::ui::branches_tree::{build_branch_view, leaf_count};

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
                // Old refs must count just like fresh ones in a long-lived repo.
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
            let view = build_branch_view(&roots, &HashMap::new(), &|_| show_remotes);
            assert_eq!(view.repos[0].remote_rollup().as_deref(), expected);
        }
    }
}

/// The builder is asked per repository, so a rolled-up repo contributes no
/// remote nodes while its neighbour's are built.
#[test]
fn the_reveal_predicate_is_asked_per_repository() {
    let roots = [
        root_with_counts("/revealed", 1, 26),
        root_with_counts("/rolled-up", 1, 6_739),
    ];
    let view = build_branch_view(&roots, &HashMap::new(), &|root| {
        root.0.to_string_lossy() == "/revealed"
    });

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
fn rollup_counts_all_old_remote_leaves_without_cross_repo_totals() {
    let roots = [
        root_with_counts("/empty", 0, 0),
        root_with_counts("/unfetched", 1, 0),
        root_with_counts("/small", 1, 26),
        root_with_counts("/long-lived", 3, 6_739),
        root_with_counts("/retained-refs", 0, 1),
    ];
    let hidden = build_branch_view(&roots, &HashMap::new(), &|_| false);
    let expanded = build_branch_view(&roots, &HashMap::new(), &|_| true);

    for ((root, hidden), expanded) in roots.iter().zip(&hidden.repos).zip(&expanded.repos) {
        let snapshot_count = root
            .branches
            .iter()
            .filter(|branch| branch.kind == BranchKind::Remote)
            .count();
        assert!(hidden.remote_groups.is_empty());
        assert_eq!(hidden.remote_count, root.remotes.len());
        assert_eq!(hidden.remote_branch_count, snapshot_count);
        assert_eq!(hidden.remote_rollup(), expanded.remote_rollup());
        assert_eq!(
            hidden.remote_branch_count,
            expanded.remote_groups.iter().map(|group| group.count).sum()
        );
        for group in &expanded.remote_groups {
            assert_eq!(group.count, leaf_count(&group.children));
        }
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
