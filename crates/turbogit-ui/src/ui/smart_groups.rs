//! Built-in smart groups (issue #06, screens 01/04): computed sections of
//! the workspace tree that collect repos by a predicate over per-root
//! status — ahead/behind, conflicts, dirty count — never a manual list.
//!
//! Pure presentation logic (sidebar / hunk_nav precedent): the predicates
//! and the membership computation live here; `super::sidebar` renders the
//! rows and applies the active group as a tree filter. Because membership
//! is recomputed from live repo state on every frame, it follows refreshes
//! and rescans with no manual action.

use crate::ui::sidebar::{SidebarGroup, SidebarRepo, SidebarTree};
use turbogit_app::smart_rules::{RepoFacts, SmartGroupRule};

/// The active group filter: a built-in predicate or a user-defined rule
/// (issue #07), both narrowing the tree the same way.
#[derive(Debug, Clone)]
pub enum GroupFilter {
    BuiltIn(SmartGroup),
    Custom(SmartGroupRule),
}

impl GroupFilter {
    pub fn matches(&self, repo: &SidebarRepo) -> bool {
        match self {
            GroupFilter::BuiltIn(group) => group.matches(repo),
            GroupFilter::Custom(rule) => rule.matches(&repo_facts(repo)),
        }
    }
}

/// Project one sidebar row onto the plain facts a rule's predicate reads.
fn repo_facts(repo: &SidebarRepo) -> RepoFacts {
    RepoFacts {
        name: repo.name.clone(),
        path: repo.path.to_string_lossy().into_owned(),
        branch: repo.branch.clone(),
        dirty: repo.dirty_count,
        ahead: repo.ahead,
        behind: repo.behind,
    }
}

/// How many repos of the tree a user-defined rule collects (the custom
/// row's badge). Recomputed from live state by the caller each frame.
pub fn rule_count(tree: &SidebarTree, rule: &SmartGroupRule) -> usize {
    tree.groups
        .iter()
        .flat_map(|g| &g.repos)
        .filter(|r| rule.matches(&repo_facts(r)))
        .count()
}

/// The built-in groups, each a predicate over one repo's per-root status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SmartGroup {
    /// Ahead **and** behind the upstream — the two histories diverged.
    Diverged,
    /// Has unresolved merge conflicts.
    Conflicted,
    /// Has local commits the upstream lacks (ahead > 0).
    Unpushed,
    /// Has incoming commits not yet pulled (behind > 0) — the status bar's
    /// `unpulled` counter as a sidebar group (issue 02).
    Unpulled,
    /// Has uncommitted work (modified or unversioned paths).
    Dirty,
}

impl SmartGroup {
    /// Built-ins in their fixed display order (screens 01/04; issue 02 adds
    /// `unpulled commits` after `unpushed commits`).
    pub const BUILTINS: &'static [SmartGroup] = &[
        SmartGroup::Diverged,
        SmartGroup::Conflicted,
        SmartGroup::Unpushed,
        SmartGroup::Unpulled,
        SmartGroup::Dirty,
    ];

    /// The row label (screen 01's lowercase style).
    pub fn label(&self) -> &'static str {
        match self {
            SmartGroup::Diverged => "diverged",
            SmartGroup::Conflicted => "has conflicts",
            SmartGroup::Unpushed => "unpushed commits",
            SmartGroup::Unpulled => "unpulled commits",
            SmartGroup::Dirty => "dirty worktree",
        }
    }

    /// The group's predicate over one repo row. Membership overlaps: a
    /// diverged repo (ahead + behind) also matches "unpushed commits" and
    /// "unpulled commits".
    pub fn matches(&self, repo: &SidebarRepo) -> bool {
        match self {
            SmartGroup::Diverged => repo.ahead > 0 && repo.behind > 0,
            SmartGroup::Conflicted => repo.conflicts > 0,
            SmartGroup::Unpushed => repo.ahead > 0,
            SmartGroup::Unpulled => repo.behind > 0,
            SmartGroup::Dirty => repo.dirty_count > 0,
        }
    }
}

/// One non-empty group and its live member count (the row's badge).
#[derive(Debug, Clone)]
pub struct SmartGroupEntry {
    pub group: SmartGroup,
    pub count: usize,
}

/// Compute the built-in groups over the whole tree, in display order.
/// Zero-member groups are dropped (issue #06: hidden, not painted empty).
pub fn smart_groups(tree: &SidebarTree) -> Vec<SmartGroupEntry> {
    SmartGroup::BUILTINS
        .iter()
        .filter_map(|group| {
            let count = tree
                .groups
                .iter()
                .flat_map(|g| &g.repos)
                .filter(|r| group.matches(r))
                .count();
            (count > 0).then_some(SmartGroupEntry {
                group: *group,
                count,
            })
        })
        .collect()
}

/// Narrow the tree to one group's member repos (issue #06: clicking a
/// group filters the tree to its members). Project groups keep their
/// structure but drop memberless rows; the top-level total recomputes.
/// `None` (no active group) returns the tree unchanged.
pub fn filter_to_group(tree: &SidebarTree, group: Option<SmartGroup>) -> SidebarTree {
    filter_to(tree, group.map(GroupFilter::BuiltIn).as_ref())
}

/// The generalized narrowing behind [`filter_to_group`]: one filter —
/// built-in or custom (issue #07) — keeps only its member repos.
pub fn filter_to(tree: &SidebarTree, filter: Option<&GroupFilter>) -> SidebarTree {
    let Some(filter) = filter else {
        return tree.clone();
    };
    let mut groups = Vec::new();
    let mut total = 0;
    for g in &tree.groups {
        let repos: Vec<_> = g
            .repos
            .iter()
            .filter(|r| filter.matches(r))
            .cloned()
            .collect();
        if !repos.is_empty() {
            total += repos.len();
            groups.push(SidebarGroup {
                name: g.name.clone(),
                repos,
            });
        }
    }
    SidebarTree { groups, total }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::sidebar::build_tree;
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

    #[test]
    fn builtins_compute_membership_and_counts_from_repo_state() {
        let project = Path::new("/w");
        let diverged = root("/w/g/diverged", Some("main")); // ahead 2, behind 1
        let mut dirty = root("/w/g/dirty", Some("main"));
        dirty.status.changes = vec![modified("a.txt"), modified("b.txt")];
        let mut conflicted = root("/w/g/conflicted", Some("main"));
        conflicted.status.conflicted = vec![PathBuf::from("c.txt")];
        let clean = root("/w/g/clean", Some("main"));
        let roots = vec![diverged, dirty, conflicted, clean];

        let tree = build_tree(project, &roots, &|id| {
            if id.0.as_os_str() == "/w/g/diverged" {
                Some((2, 1))
            } else {
                Some((0, 0))
            }
        });
        let entries = smart_groups(&tree);
        let labels: Vec<(&str, usize)> =
            entries.iter().map(|e| (e.group.label(), e.count)).collect();
        assert_eq!(
            labels,
            [
                ("diverged", 1),
                ("has conflicts", 1),
                ("unpushed commits", 1),
                // The diverged root is also behind its upstream, so it joins
                // the unpulled group as well (issue 02, mirroring the status
                // bar's unpulled counter: behind > 0).
                ("unpulled commits", 1),
                ("dirty worktree", 1),
            ],
            "the five built-ins with their member counts, in display order"
        );
    }

    #[test]
    fn groups_with_zero_members_are_dropped() {
        let project = Path::new("/w");
        let roots = vec![root("/w/g/clean", Some("main"))];
        let tree = build_tree(project, &roots, &|_| Some((0, 0)));

        assert!(
            smart_groups(&tree).is_empty(),
            "an all-clean workspace renders no smart group rows"
        );
    }

    #[test]
    fn membership_overlaps_when_one_predicate_implies_another() {
        // A diverged repo (ahead + behind) has unpushed commits too: a repo
        // may belong to several groups at once.
        let project = Path::new("/w");
        let roots = vec![root("/w/g/one", Some("main"))];
        let tree = build_tree(project, &roots, &|_| Some((1, 3)));

        let labels: Vec<&str> = smart_groups(&tree)
            .iter()
            .map(|e| e.group.label())
            .collect();
        assert!(labels.contains(&"diverged"));
        assert!(labels.contains(&"unpushed commits"));
    }

    #[test]
    fn unpushed_alone_is_not_diverged() {
        let project = Path::new("/w");
        let roots = vec![root("/w/g/one", Some("main"))];
        let tree = build_tree(project, &roots, &|_| Some((2, 0)));

        let labels: Vec<&str> = smart_groups(&tree)
            .iter()
            .map(|e| e.group.label())
            .collect();
        assert_eq!(
            labels,
            ["unpushed commits"],
            "ahead-only is unpushed, not diverged"
        );
    }

    #[test]
    fn unpulled_matches_repos_with_incoming_commits() {
        // Issue 02: the `unpulled commits` group mirrors the status bar's
        // unpulled counter — behind > 0. Ahead-only repos are unpushed (not
        // unpulled), and a diverged repo (ahead + behind) belongs to both.
        let project = Path::new("/w");
        let unpulled = root("/w/g/unpulled", Some("main")); // (0, 2)
        let unpushed = root("/w/g/unpushed", Some("main")); // (1, 0)
        let diverged = root("/w/g/diverged", Some("main")); // (1, 1)
        let tree = build_tree(project, &[unpulled, unpushed, diverged], &|id| {
            let p = id.0.as_os_str();
            if p == "/w/g/unpulled" {
                Some((0, 2))
            } else if p == "/w/g/unpushed" {
                Some((1, 0))
            } else {
                Some((1, 1))
            }
        });
        let all: Vec<&crate::ui::sidebar::SidebarRepo> =
            tree.groups.iter().flat_map(|g| &g.repos).collect();
        let members_of = |group: SmartGroup| {
            let mut names: Vec<&str> = all
                .iter()
                .filter(|r| group.matches(r))
                .map(|r| r.name.as_str())
                .collect();
            names.sort();
            names
        };
        assert_eq!(members_of(SmartGroup::Unpulled), ["diverged", "unpulled"]);
        assert_eq!(members_of(SmartGroup::Unpushed), ["diverged", "unpushed"]);
        assert_eq!(members_of(SmartGroup::Diverged), ["diverged"]);
    }

    #[test]
    fn filtering_the_tree_keeps_only_member_repos_in_place() {
        let project = Path::new("/w");
        let mut dirty = root("/w/fe/dirty", Some("main"));
        dirty.status.changes = vec![modified("a.txt")];
        let clean = root("/w/fe/clean", Some("main"));
        let unpushed = root("/w/oss/unpushed", Some("dev"));
        let tree = build_tree(project, &[dirty, clean, unpushed], &|id| {
            if id.0.as_os_str() == "/w/oss/unpushed" {
                Some((1, 0))
            } else {
                Some((0, 0))
            }
        });

        let filtered = filter_to_group(&tree, Some(SmartGroup::Unpushed));
        assert_eq!(filtered.total, 1, "top-level total narrows to members");
        assert_eq!(filtered.groups.len(), 1, "memberless groups drop out");
        assert_eq!(filtered.groups[0].name, "oss");
        assert_eq!(filtered.groups[0].repos[0].name, "unpushed");

        // Clearing the group (None) returns the tree unchanged.
        assert_eq!(filter_to_group(&tree, None).total, 3);
    }

    #[test]
    fn dirty_predicates_ignore_clean_repos() {
        let project = Path::new("/w");
        let mut dirty = root("/w/g/dirty", Some("main"));
        dirty.status.changes = vec![modified("a.txt")];
        let clean = root("/w/g/clean", Some("main"));
        let tree = build_tree(project, &[dirty, clean], &|_| Some((0, 0)));

        let dirty_members: Vec<&RootId> = tree
            .groups
            .iter()
            .flat_map(|g| &g.repos)
            .filter(|r| SmartGroup::Dirty.matches(r))
            .map(|r| &r.id)
            .collect();
        assert_eq!(dirty_members.len(), 1);
        assert_eq!(dirty_members[0].0.as_os_str(), "/w/g/dirty");
    }

    // --- User-defined rules (issue #07) ---

    fn release_rule() -> SmartGroupRule {
        SmartGroupRule {
            label: "release branches".into(),
            branch_pattern: Some("release/*".into()),
            ..SmartGroupRule::default()
        }
    }

    #[test]
    fn custom_rules_collect_matching_repos_with_live_counts() {
        let project = Path::new("/w");
        let release = root("/w/oss/lib", Some("release/1"));
        let main = root("/w/oss/app", Some("main"));
        let tree = build_tree(project, &[release, main], &|_| Some((0, 0)));

        assert_eq!(rule_count(&tree, &release_rule()), 1);
    }

    #[test]
    fn a_rule_with_no_members_counts_zero() {
        let project = Path::new("/w");
        let tree = build_tree(project, &[root("/w/oss/app", Some("main"))], &|_| {
            Some((0, 0))
        });

        assert_eq!(rule_count(&tree, &release_rule()), 0);
    }

    #[test]
    fn filtering_the_tree_to_a_custom_rule_keeps_only_members() {
        let project = Path::new("/w");
        let release = root("/w/oss/lib", Some("release/1"));
        let main = root("/w/oss/app", Some("main"));
        let mut dirty_release = root("/w/frontend/rel", Some("release/9"));
        dirty_release.status.changes = vec![modified("a.txt")];
        let tree = build_tree(project, &[release, main, dirty_release], &|_| Some((0, 0)));

        let filtered = filter_to(&tree, Some(&GroupFilter::Custom(release_rule())));
        assert_eq!(filtered.total, 2, "top-level total narrows to members");
        assert_eq!(filtered.groups.len(), 2, "memberless groups drop out");
        let names: Vec<&str> = filtered
            .groups
            .iter()
            .flat_map(|g| &g.repos)
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(names, ["rel", "lib"]);

        // No active filter returns the tree unchanged.
        assert_eq!(filter_to(&tree, None).total, 3);
    }

    #[test]
    fn builtin_filters_ride_the_same_generalized_narrowing() {
        let project = Path::new("/w");
        let mut dirty = root("/w/fe/dirty", Some("main"));
        dirty.status.changes = vec![modified("a.txt")];
        let clean = root("/w/fe/clean", Some("main"));
        let tree = build_tree(project, &[dirty, clean], &|_| Some((0, 0)));

        let filtered = filter_to(&tree, Some(&GroupFilter::BuiltIn(SmartGroup::Dirty)));
        assert_eq!(filtered.total, 1);
        assert_eq!(filtered.groups[0].repos[0].name, "dirty");
    }
}
