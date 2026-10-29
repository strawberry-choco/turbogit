use std::path::PathBuf;

use turbogit_app::operation::{OpKind, Operation};
use turbogit_app::root_caches::Affected;
use turbogit_domain::model::{RebaseOpts, RebasePlanEntry, RootId, VcsSettings};

fn root(name: &str) -> RootId {
    RootId(PathBuf::from("/repo").join(name).into())
}

fn plan_one() -> Vec<RebasePlanEntry> {
    vec![RebasePlanEntry {
        action: turbogit_domain::model::RebaseAction::Pick,
        commit: "0123456789abcdef0123456789abcdef01234567".into(),
        subject: "a commit".into(),
        message: None,
    }]
}

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn settings() -> VcsSettings {
    VcsSettings::default()
}

#[test]
fn a_fetch_label_says_how_many_roots_it_covered() {
    let one = Operation::Fetch {
        roots: vec![root("alpha")],
    };
    assert_eq!(one.label(), "Fetch");

    let many = Operation::Fetch {
        roots: vec![root("alpha"), root("beta")],
    };
    assert_eq!(many.label(), "Fetch · 2 repos");
}

#[test]
fn each_rebase_flavour_keeps_its_wording_and_shares_its_kind() {
    let r = root("alpha");
    let cases = [
        (
            Operation::rebase_onto(&r, "feature", "main", &RebaseOpts::default(), &settings()),
            "Rebase onto main",
        ),
        (
            Operation::rebase_autosquash(
                &r,
                "feature",
                "main",
                &RebaseOpts {
                    autosquash: true,
                    ..Default::default()
                },
                &settings(),
            ),
            "Autosquash rebase onto main",
        ),
        (
            Operation::rebase_interactive(&r, "feature", plan_one(), &settings(), false),
            "Interactive rebase",
        ),
        (
            Operation::rebase_interactive(&r, "feature", plan_one(), &settings(), true),
            "Interactive rebase",
        ),
        (
            Operation::rebase_branch_onto(&r, "feature", "main"),
            "Rebase feature onto main",
        ),
    ];
    for (op, label) in cases {
        assert_eq!(op.label(), label);
        assert_eq!(
            op.kind(),
            OpKind::Rebase,
            "{label:?} settles as a rebase whatever its flavour"
        );
    }
}

#[test]
fn a_merge_label_carries_the_commit_count_only_when_the_preview_knew() {
    let mk = |commits: usize| Operation::Merge {
        root: root("alpha"),
        target: "topic".into(),
        opts: turbogit_domain::model::MergeOpts::default(),
        clean: turbogit_domain::model::CleanTreeMethod::Stash,
        commits,
    };
    assert_eq!(mk(0).label(), "Merge topic");
    assert_eq!(mk(3).label(), "Merge topic (3 commits)");
}

#[test]
fn an_operation_derives_its_own_invalidation_scope() {
    let a = root("alpha");
    let b = root("beta");
    assert_eq!(
        Operation::Fetch {
            roots: vec![a.clone()]
        }
        .affected(),
        Affected::Root(a.clone()),
        "one root fetches, one root refreshes"
    );
    assert_eq!(
        Operation::Fetch {
            roots: vec![a.clone(), b.clone()],
        }
        .affected(),
        Affected::All,
        "several roots is a project-wide scope, not a guess"
    );
    assert_eq!(
        Operation::DeleteBranch {
            root: a.clone(),
            name: "spare".into()
        }
        .affected(),
        Affected::Root(a),
        "a named operation scopes to the root it works on"
    );
}

#[test]
fn only_the_worktree_operations_mutate_the_worktree_set() {
    let a = root("alpha");
    let add = Operation::WorktreeAdd {
        root: a.clone(),
        path: PathBuf::from("/repo/wt"),
        branch: "wt-branch".into(),
    };
    let remove = Operation::WorktreeRemove {
        root: a.clone(),
        path: PathBuf::from("/repo/wt"),
    };
    assert!(add.mutates_worktrees() && remove.mutates_worktrees());
    assert_eq!(add.kind(), OpKind::WorktreeAdd);
    assert_eq!(remove.kind(), OpKind::WorktreeRemove);

    let others = [
        Operation::Fetch {
            roots: vec![a.clone()],
        },
        Operation::DeleteBranch {
            root: a.clone(),
            name: "spare".into(),
        },
        Operation::shelve(a.clone(), Vec::new(), "note"),
        Operation::custom("Anything", Affected::Root(a), |_| Ok(())),
    ];
    for op in others {
        assert!(
            !op.mutates_worktrees(),
            "{} must not claim a worktree mutation",
            op.label()
        );
    }
}

#[test]
fn a_targeted_rewrite_is_its_own_kind_and_names_its_commit() {
    let r = root("alpha");
    let dropped = Operation::drop_commit(&r, "feature", SHA, &settings());
    let reworded = Operation::reword_commit(&r, "feature", SHA, "a better message", &settings());

    for op in [&dropped, &reworded] {
        assert_eq!(op.affected(), Affected::Root(r.clone()));
        assert!(
            !op.mutates_worktrees(),
            "rewriting history moves no linked worktree"
        );
    }
    assert_eq!(dropped.label(), "Drop commit 0123456");
    assert_eq!(reworded.label(), "Reword commit 0123456");
    assert_eq!(dropped.kind(), OpKind::DropCommit);
    assert_eq!(reworded.kind(), OpKind::RewordCommit);
    assert_ne!(
        dropped.kind(),
        OpKind::Other,
        "a rewrite is not anonymous custom work"
    );
    assert_ne!(
        dropped.kind(),
        OpKind::Rebase,
        "and it is not the dialog's rebase either: it settles on its own arm"
    );
}

#[test]
fn a_reword_label_names_the_commit_and_not_its_new_message() {
    let op = Operation::reword_commit(
        &root("alpha"),
        "feature",
        SHA,
        "a message nobody should see in the feed",
        &settings(),
    );
    assert_eq!(op.label(), "Reword commit 0123456");
}

#[test]
fn a_custom_operation_has_no_kind_of_its_own() {
    let op = Operation::custom("Stage hunk", Affected::Root(root("alpha")), |_| Ok(()));
    assert_eq!(op.kind(), OpKind::Other);
}
