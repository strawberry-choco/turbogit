//! Issue #25 — push-scope resolution and protected-branch remediation.
//!
//! Pure snapshot tests over [`Root`] values: which roots a Push dialog scope
//! covers, which roots in a scope sit on a protected branch, and pushing an
//! explicit root subset through the engine seam (fake executor — no git on
//! PATH needed for the resolution pieces; the execution test uses the fake).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use turbogit_domain::model::{Branch, BranchKind, Root, RootId, RootStatus, VcsSettings};
use turbogit_engine::fake::{Call, FakeExecutor};
use turbogit_services::sync_service::{
    PushScope, protected_roots, push_dry_run_roots, push_roots, roots_in_scope, summarize_dry_runs,
};

fn settings_with(patterns: &[&str]) -> VcsSettings {
    VcsSettings {
        protected_branch_patterns: patterns.iter().map(|s| s.to_string()).collect(),
        ..VcsSettings::default()
    }
}

/// A root sitting on `branch` with (optionally) an upstream tracking ref.
fn root_on_branch(path: &str, branch: &str) -> Root {
    let mut r = root(path);
    r.current_branch = Some(branch.to_string());
    r
}

/// `root_on_branch` with a local branch that tracks `<remote>/<branch>`.
fn root_tracking(path: &str, branch: &str, remote: &str) -> Root {
    let mut r = root_on_branch(path, branch);
    r.branches = vec![Branch {
        name: branch.to_string(),
        kind: BranchKind::Local,
        tracking: Some(format!("{remote}/{branch}")),
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
    r.remotes = vec![turbogit_domain::model::Remote {
        name: remote.to_string(),
        fetch_url: Some(String::new()),
        push_url: None,
    }];
    r
}

/// `push_roots` pushes exactly the given subset, resolving each root's
/// remote from its tracking ref (or first remote as fallback), and refusing
/// force-pushes to protected roots without reaching the engine.
#[test]
fn push_roots_pushes_only_the_given_subset_and_guards_protected_roots() {
    let engine = FakeExecutor::new();
    let s = settings_with(&["main"]);

    let alpha = root_tracking("/proj/alpha", "main", "origin");
    let beta = root_tracking("/proj/beta", "feature", "backup");
    #[allow(clippy::useless_vec)]
    let roots = vec![alpha, beta];
    let refs: Vec<&Root> = roots.iter().collect();

    let results = push_roots(&engine, &refs, &s, false, false, false, false, None);

    assert_eq!(results.len(), 2, "one result per root in the subset");
    assert!(results.iter().all(|(_, r)| r.is_ok()));
    let calls = engine.calls.lock().unwrap();
    assert_eq!(
        calls.as_slice(),
        [
            Call::Push {
                root: PathBuf::from("/proj/alpha"),
                remote: "origin".into(),
                branch: "main".into(),
                force: false,
                tags: false,
                no_verify: false,
                set_upstream: false,
                selected_oldest: None,
            },
            Call::Push {
                root: PathBuf::from("/proj/beta"),
                remote: "backup".into(),
                branch: "feature".into(),
                force: false,
                tags: false,
                no_verify: false,
                set_upstream: false,
                selected_oldest: None,
            },
        ],
        "each root pushes its own tracking remote"
    );
    drop(calls);

    // Force + a protected root in the subset: that root is refused with no
    // engine call; the unprotected root still pushes.
    let engine2 = FakeExecutor::new();
    let gamma = root_tracking("/proj/gamma", "main", "origin");
    let delta = root_tracking("/proj/delta", "topic", "origin");
    #[allow(clippy::useless_vec)]
    let roots2 = vec![gamma, delta];
    let refs2: Vec<&Root> = roots2.iter().collect();

    let results2 = push_roots(&engine2, &refs2, &s, true, false, false, false, None);

    assert!(
        results2[0].1.is_err(),
        "force-push to the protected root must be refused"
    );
    assert!(results2[1].1.is_ok(), "the unprotected root still pushes");
    assert!(
        engine2.calls.lock().unwrap().iter().all(
            |c| !matches!(c, Call::Push { root, .. } if root == &PathBuf::from("/proj/gamma"))
        ),
        "the protected root must never reach the engine"
    );
}

fn root(path: &str) -> Root {
    Root {
        id: RootId(Arc::from(Path::new(path))),
        path: PathBuf::from(path),
        remotes: Vec::new(),
        branches: Vec::new(),
        current_branch: None,
        head: None,
        status: RootStatus::default(),
    }
}

fn root_id(path: &str) -> RootId {
    RootId(Arc::from(Path::new(path)))
}

fn ids<'a>(roots: impl IntoIterator<Item = &'a Root>) -> Vec<PathBuf> {
    roots.into_iter().map(|r| r.id.0.to_path_buf()).collect()
}

fn selection(paths: &[&str]) -> HashSet<RootId> {
    paths.iter().map(|p| root_id(p)).collect()
}

/// `This repo` covers only the selected root; `Selected` covers the checked
/// repos in registration order; `Project subtree` covers every root under the
/// project directory; `All` covers every registered root — including any
/// registered outside the project tree.
#[test]
fn scope_resolves_this_repo_selection_subtree_and_all() {
    let project = Path::new("/proj");
    let alpha = root("/proj/alpha");
    let beta = root("/proj/beta");
    // A root registered outside the project tree (added manually), which
    // separates `Project subtree` from `All`.
    let outer = root("/elsewhere/gamma");
    let roots = vec![alpha, beta, outer];
    let sel = selection(&["/proj/beta"]);

    let this = roots_in_scope(
        PushScope::ThisRepo,
        &roots,
        &sel,
        Some(&root_id("/proj/alpha")),
        project,
    );
    assert_eq!(ids(this), [PathBuf::from("/proj/alpha")]);

    let selected = roots_in_scope(PushScope::Selection, &roots, &sel, None, project);
    assert_eq!(ids(selected), [PathBuf::from("/proj/beta")]);

    let subtree = roots_in_scope(PushScope::Subtree, &roots, &HashSet::new(), None, project);
    assert_eq!(
        ids(subtree),
        [PathBuf::from("/proj/alpha"), PathBuf::from("/proj/beta")],
        "subtree keeps registration order and excludes roots outside the project tree"
    );

    let all = roots_in_scope(PushScope::All, &roots, &HashSet::new(), None, project);
    assert_eq!(all.len(), 3, "All covers every registered root");
}

/// Roots whose current branch matches a protected pattern are reported with
/// the branch name and every matching pattern — exactly what the remediation
/// banner names ("alpha and beta track main, which matches main|release/*").
/// Detached roots (no current branch) can never be protected.
#[test]
fn protected_roots_reports_name_branch_and_matching_patterns() {
    let alpha = root_on_branch("/proj/alpha", "main");
    let beta = root_on_branch("/proj/beta", "release/1.0");
    let feature = root_on_branch("/proj/feature", "feature/work");
    let detached = root("/proj/detached");
    let roots = vec![alpha, beta, feature, detached];

    let protected = protected_roots(&settings_with(&["main", "release/*"]), &roots);

    assert_eq!(
        protected.len(),
        2,
        "the feature branch and detached root stay out"
    );
    assert_eq!(protected[0].name, "alpha");
    assert_eq!(protected[0].branch, "main");
    assert_eq!(protected[0].patterns, ["main"]);
    assert_eq!(protected[1].name, "beta");
    assert_eq!(protected[1].branch, "release/1.0");
    assert_eq!(protected[1].patterns, ["release/*"]);
    assert_eq!(
        protected[0].id.0.to_path_buf(),
        PathBuf::from("/proj/alpha"),
        "the id lets the dialog remove the root from scope"
    );
}

/// Dry-runs run per root in scope with the SAME per-root remote resolution as
/// the real push; `summarize_dry_runs` produces the aggregate line's numbers
/// (distinct remotes, ref updates parsed from verbatim reports, rejections).
#[test]
fn dry_run_roots_resolves_per_root_and_summary_counts_refs_and_rejections() {
    let engine = FakeExecutor::new();
    let _ = settings_with(&[]);
    let alpha = root_tracking("/proj/alpha", "main", "origin");
    let beta = root_tracking("/proj/beta", "main", "backup");
    #[allow(clippy::useless_vec)]
    let roots = vec![alpha, beta];
    let refs: Vec<&Root> = roots.iter().collect();

    let results = push_dry_run_roots(&engine, &refs, false, None);

    assert_eq!(results.len(), 2);
    let calls = engine.calls.lock().unwrap();
    assert!(
        calls.iter().any(|c| matches!(c, Call::PushDryRun { root, remote, branch, force }
            if root == &PathBuf::from("/proj/alpha") && remote == "origin" && branch == "main" && !force)),
        "per-root dry-runs must resolve each root's tracking remote, got: {calls:?}"
    );
    assert!(
        calls
            .iter()
            .any(|c| matches!(c, Call::PushDryRun { remote, .. } if remote == "backup")),
        "the second root must target its own remote"
    );
    drop(calls);

    // Summary: distinct remotes, "->" ref-update lines per report, and one
    // rejection for the failed root. Expected values are hand-worked from the
    // report literals, not recomputed from the code under test.
    let hand_results = vec![
        (
            "origin".to_string(),
            Ok(" * [new branch]      main -> main".to_string()),
        ),
        (
            "backup".to_string(),
            Ok("   a1b2c3d..e4f5a6b  main -> main\n   a1b2c3d..e4f5a6b  tag2 -> tag2".to_string()),
        ),
        ("broken".to_string(), Err("rejected".into())),
    ];
    let summary = summarize_dry_runs(&hand_results);
    assert_eq!(summary.remotes, 3, "distinct remotes");
    assert_eq!(summary.refs, 3, "ref-update lines across accepted reports");
    assert_eq!(summary.rejected, 1, "one rejected root");
}
