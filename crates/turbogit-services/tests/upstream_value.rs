//! Issue 08 (deepen-git-engine) — upstream as a value, not git's `remote/branch`.
//!
//! A branch's upstream is one idea in the app and was stored as one git string,
//! so every surface that wanted half of it split it back apart — three of those
//! splits being byte-identical copies of the same remote-resolution rule, each
//! free to get the fallback wrong. These run through the substitutable adapter
//! with no `git` binary, and pin the one rule all three push paths share.

use std::path::PathBuf;

use turbogit_domain::model::{
    Branch, BranchKind, MultiRootManager, Remote, Root, RootId, RootStatus, Upstream, VcsSettings,
};
use turbogit_engine::fake::{Call, FakeExecutor};
use turbogit_services::{bulk_ops, sync_service};

fn up(remote: &str, branch: &str) -> Upstream {
    Upstream {
        remote: remote.to_string(),
        branch: branch.to_string(),
    }
}

fn root_at(path: &str) -> Root {
    let id = PathBuf::from(path);
    Root {
        id: RootId(id.clone().into()),
        path: id,
        remotes: Vec::new(),
        branches: Vec::new(),
        current_branch: None,
        head: None,
        status: RootStatus::default(),
    }
}

/// A root on `branch`, tracking `upstream`, with `remotes` configured.
fn root_tracking(branch: &str, upstream: Option<Upstream>, remotes: &[&str]) -> Root {
    let mut r = root_at("/repo");
    r.current_branch = Some(branch.to_string());
    r.branches = vec![Branch {
        name: branch.to_string(),
        kind: BranchKind::Local,
        tracking: upstream,
        favorite: false,
        protected: false,
        exists: true,
        ahead: 0,
        behind: 0,
        gone: false,
        last_touched: None,
        tip: None,
        remote: None,
    }];
    r.remotes = remotes
        .iter()
        .map(|name| Remote {
            name: (*name).to_string(),
            fetch_url: Some(String::new()),
            push_url: None,
        })
        .collect();
    r
}

/// A branch row of `kind` named `name`, tracking `upstream`.
fn branch_row(name: &str, kind: BranchKind, tracking: Option<Upstream>) -> Branch {
    Branch {
        name: name.to_string(),
        kind,
        tracking,
        favorite: false,
        protected: false,
        exists: true,
        ahead: 0,
        behind: 0,
        gone: false,
        last_touched: None,
        tip: None,
        remote: None,
    }
}

fn mgr_with(roots: Vec<Root>) -> MultiRootManager {
    MultiRootManager {
        roots,
        ..Default::default()
    }
}

/// The remote a push from this root goes to: its upstream's remote, never a
/// per-call-site guess at how to split a string.
#[test]
fn a_non_origin_upstream_resolves_to_its_own_remote() {
    let root = root_tracking("main", Some(up("corp", "main")), &["corp"]);
    assert_eq!(sync_service::push_remote(&root), "corp");
}

#[test]
fn the_fallback_is_the_one_rule_not_a_per_site_default() {
    // No upstream: the root's first configured remote.
    let root = root_tracking("main", None, &["alt", "origin"]);
    assert_eq!(sync_service::push_remote(&root), "alt");
    // No upstream and no remotes at all: `origin`, stated once.
    let root = root_tracking("main", None, &[]);
    assert_eq!(sync_service::push_remote(&root), "origin");
}

#[test]
fn push_dry_run_push_and_bulk_push_all_agree_on_the_remote() {
    let root = root_tracking("main", Some(up("corp", "main")), &["corp"]);
    let mgr = mgr_with(vec![root.clone()]);
    let v = FakeExecutor::new();
    let settings = VcsSettings::default();

    let roots = [&root];
    sync_service::push_roots(&v, &roots, &settings, false, false, false, false, None);
    sync_service::push_dry_run_roots(&v, &roots, false, None);
    bulk_ops::run_bulk(
        &v,
        &mgr,
        &settings,
        &bulk_ops::BulkPlan {
            op: bulk_ops::BulkOp::PushAll,
            roots: vec![root.id.clone()],
            rebase: false,
            branch: String::new(),
            command: String::new(),
        },
    );

    let remotes: Vec<String> = v
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter_map(|c| match c {
            Call::Push { remote, .. } | Call::PushDryRun { remote, .. } => Some(remote.clone()),
            _ => None,
        })
        .collect();
    assert!(
        remotes.len() >= 3,
        "expected the three push paths to each reach the engine, got {remotes:?}"
    );
    assert!(
        remotes.iter().all(|r| r == "corp"),
        "the same rule must answer for all three, got {remotes:?}"
    );
}

#[test]
fn the_branch_list_carries_both_halves_without_anyone_splitting_them() {
    let root = root_tracking("release", Some(up("corp", "release")), &["corp"]);
    let tracked = root.branches.first().unwrap();
    let upstream = tracked.tracking.clone().expect("a tracked branch");
    assert_eq!(upstream.remote, "corp");
    assert_eq!(upstream.branch, "release");
    // git's spelling exists only where argv needs it.
    assert_eq!(upstream.git_ref(), "corp/release");
}

#[test]
fn a_tracking_ref_with_no_remote_part_is_not_an_upstream() {
    assert_eq!(Upstream::from_git_ref("justabranch"), None);
    assert_eq!(
        Upstream::from_git_ref("corp/feature/deep"),
        Some(Upstream {
            remote: "corp".to_string(),
            branch: "feature/deep".to_string(),
        }),
        "only the first slash separates: branch names may contain them"
    );
}

/// The retyped `Branch::tracking` is not an on-disk shape: nothing persists a
/// branch listing today. This is the assertion to notice if that changes,
/// because the day it does, a migration is owed and this test should become one.
#[test]
fn no_persisted_state_stores_the_old_spelling() {
    let text = ron::ser::to_string_pretty(
        &turbogit_domain::model::ProjectState::default(),
        ron::ser::PrettyConfig::new(),
    )
    .expect("default state serializes");
    assert!(
        !text.contains("tracking"),
        "a branch listing reached the serialized project state; the retired \
         `tracking: Option<String>` spelling now needs an on-disk migration: {text}"
    );
}

/// The rule the five inline derivations each restated, owned by one function:
/// the first row in `root.branches` named like the current branch answers, and
/// it answers with that row's tracking. Every value-producing adapter emits a
/// local row ahead of a remote row of the same short name — `git branch -a`
/// lists locals then remotes, `git2_exec` pushes its local loop first, and the
/// `remotes/origin/main` row carries the short name `main` — so the first match
/// is the local row, and the remote row of the same name never shadows it.
#[test]
fn tracked_upstream_answers_from_the_local_row_a_remote_row_of_the_same_name_follows() {
    let mut root = root_tracking("main", Some(up("corp", "main")), &["corp"]);
    root.branches = vec![
        branch_row("main", BranchKind::Local, Some(up("corp", "main"))),
        branch_row("main", BranchKind::Remote, None),
    ];

    assert_eq!(
        sync_service::tracked_upstream(&root).map(|u| u.git_ref()),
        Some("corp/main".to_string()),
        "the first row named like the current branch answers; the remote row \
         of the same short name follows it and shadows nothing",
    );
}

/// The same fixture with the order the adapters could never produce is *not*
/// the contract: this pins which of the two rows a remote-shadows-local
/// ordering would answer from, so the day an adapter emits remote rows first
/// the assertion above changes for a reason rather than silently.
#[test]
fn a_remote_row_named_like_the_current_branch_is_answered_from_when_it_leads() {
    let mut root = root_tracking("main", Some(up("corp", "main")), &["corp"]);
    root.branches = vec![
        branch_row("main", BranchKind::Remote, None),
        branch_row("main", BranchKind::Local, Some(up("corp", "main"))),
    ];

    assert_eq!(
        sync_service::tracked_upstream(&root),
        None,
        "first match wins: a leading remote row answers with its own (absent) \
         tracking, which is why the adapters' local-first order is the contract",
    );
}

/// The upstream half of the push rule, now read by the same one function the
/// remote half is. The remote half's three-way agreement is pinned above; this
/// is its upstream counterpart, and it goes through `bulk_ops::preflight`
/// because that path is engine-free — the one observable surface that reads the
/// upstream half without a git binary and states a `NoUpstream` skip for it.
#[test]
fn the_upstream_half_is_the_one_rule_for_the_preflight_and_the_remote() {
    let settings = VcsSettings::default();

    // The tracking remote and the first configured remote disagree on purpose:
    // a per-site fallback would answer "alt" here.
    let tracked = root_tracking("main", Some(up("corp", "main")), &["alt", "origin"]);
    let untracked = root_tracking("feature", None, &["alt", "origin"]);

    let preflight = bulk_ops::preflight(
        bulk_ops::BulkOp::PushAll,
        &[tracked.clone(), untracked.clone()],
        &settings,
        false,
        &|_| (0, 0),
    );
    let outcomes: Vec<&_> = preflight.rows.iter().map(|r| &r.outcome).collect();
    assert!(
        outcomes[0].is_ok(),
        "a tracked root is not skipped for a missing upstream; got {:?}",
        outcomes[0],
    );
    assert_eq!(
        outcomes[1],
        &Err(bulk_ops::SkipReason::NoUpstream),
        "an untracked root is the one NoUpstream names; got {:?}",
        outcomes[1],
    );

    // And both halves answer from the tracked upstream, never from a per-site
    // fallback: the remote half names the tracking remote, and the tracked
    // value itself is the branch's own entry.
    assert_eq!(sync_service::push_remote(&tracked), "corp");
    assert_eq!(
        sync_service::tracked_upstream(&tracked).map(|u| u.git_ref()),
        Some("corp/main".to_string()),
    );
    // The untracked root is where the remote half's fallback does apply — and
    // it applies once, through the same rule the tracked case reads.
    assert_eq!(sync_service::push_remote(&untracked), "alt");
    assert_eq!(sync_service::tracked_upstream(&untracked), None);
}

/// The five-fold swap at the service's own surface: `outgoing_per_root` returns
/// one row per root and answers `Ok` for both a tracked and an untracked root
/// (the untracked one's "nothing known to be ahead" arm), which is the shape
/// the fold preserved.
#[test]
fn outgoing_per_root_keeps_its_two_arms_for_tracked_and_untracked_roots() {
    let tracked = root_tracking("main", Some(up("corp", "main")), &["corp"]);
    let untracked = root_tracking("feature", None, &["corp"]);
    let mgr = mgr_with(vec![tracked.clone(), untracked.clone()]);
    let v = FakeExecutor::new();

    let rows = sync_service::outgoing_per_root(&v, &mgr);
    assert_eq!(rows.len(), 2, "one row per root");
    for (id, result) in &rows {
        assert!(
            result.is_ok(),
            "no root errors on its outgoing read; {id:?} gave {result:?}"
        );
        assert!(
            result.as_ref().is_ok_and(|commits| commits.is_empty()),
            "the fake serves no outgoing commits, so every row is empty",
        );
    }
}
