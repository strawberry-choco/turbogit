//! Issue 28 — the merge dialog's cascade hand-off: `BulkOp::Merge` joins the
//! bulk machinery so the cascade preflight matrix and run monitor can carry
//! "merge the same source branch into the sibling repos" as a plan.
//!
//! Headless integration tests over real git repositories (tempdir + system
//! `git`).

use std::path::{Path, PathBuf};
use std::process::Command;

use turbogit_domain::model::{MergeOpts, MultiRootManager, RootId, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_services::bulk_ops::{self, BulkOp, BulkPlan};
use turbogit_services::multi_root::{build_root, register};

/// Run `git <args>` in `dir`, asserting success; returns stdout.
fn run_git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git").args(args).current_dir(dir).output();
    let output = output.expect("spawning git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// Append `text` to `<dir>/<name>`, stage, commit, return HEAD SHA.
fn commit(dir: &Path, name: &str, text: &str) -> String {
    let file = dir.join(name);
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
        .expect("opening work file");
    use std::io::Write;
    writeln!(f, "{text}").expect("appending work file");
    drop(f);
    run_git(dir, &["add", "."]);
    run_git(dir, &["commit", "-m", text]);
    run_git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

fn engine() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

/// Repo on `main` (one base commit) with a `feature` branch one commit
/// ahead — strictly fast-forwardable.
fn ff_repo(tmp: &Path, name: &str) -> PathBuf {
    let repo = tmp.join(name);
    std::fs::create_dir_all(&repo).unwrap();
    run_git(&repo, &["init", "-q", "-b", "main"]);
    run_git(&repo, &["config", "user.email", "test@example.com"]);
    run_git(&repo, &["config", "user.name", "Test"]);
    commit(&repo, "base.txt", "base");
    run_git(&repo, &["checkout", "-q", "-b", "feature"]);
    commit(&repo, "feature.txt", "feature-1");
    run_git(&repo, &["checkout", "-q", "main"]);
    repo
}

// -- Preflight ----------------------------------------------------------------

#[test]
fn merge_preflight_skips_dirty_worktrees_but_runs_without_an_upstream() {
    let engine = engine();
    let tmp = tempfile::tempdir().expect("tempdir");
    let (clean, dirty) = {
        let clean = ff_repo(tmp.path(), "clean");
        let dirty = ff_repo(tmp.path(), "dirty");
        std::fs::write(dirty.join("uncommitted.txt"), "wip").unwrap();
        (clean, dirty)
    };
    let mut mgr = MultiRootManager::default();
    for p in [&clean, &dirty] {
        register(&mut mgr, build_root(&engine, p).expect("root snapshot"));
    }
    let pf = bulk_ops::preflight(BulkOp::Merge, &mgr.roots, &|_| (0, 0));
    assert_eq!(pf.total(), 2);
    assert_eq!(pf.will_run(), 1, "only the dirty repo is skipped");
    let skipped = pf
        .rows
        .iter()
        .filter(|r| r.outcome.is_err())
        .map(|r| r.root.clone())
        .collect::<Vec<_>>();
    assert_eq!(skipped, vec![RootId(dirty_path(tmp.path()).into())]);
}

fn dirty_path(tmp: &Path) -> PathBuf {
    tmp.join("dirty")
}

// -- Run step -----------------------------------------------------------------

#[test]
fn merge_step_merges_the_source_branch_with_the_given_options() {
    let engine = engine();
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = ff_repo(tmp.path(), "repo");
    let before = run_git(&repo, &["rev-parse", "main"]).trim().to_string();
    let root = build_root(&engine, &repo).expect("root snapshot");

    // The dialog builds its options through merge_flags — use the same
    // pairing here so the step exercises the flags the dialog would send.
    let opts = turbogit_services::integrate_service::merge_flags(
        turbogit_domain::model::MergeStrategy::NoCommit,
        false,
        false,
        false,
    );
    let plan = BulkPlan {
        op: BulkOp::Merge,
        roots: Vec::new(),
        rebase: false,
        branch: "feature".to_string(),
        command: String::new(),
    };
    bulk_ops::run_step(
        BulkOp::Merge,
        &engine,
        &root,
        &plan,
        &VcsSettings::default(),
        &opts,
    )
    .expect("the step should succeed");

    // --no-commit semantics: the working tree carries the merge, staged, but
    // main itself has not moved.
    assert_eq!(
        run_git(&repo, &["rev-parse", "main"]).trim(),
        before,
        "no merge commit is created under --no-commit"
    );
    assert!(
        repo.join(".git").join("MERGE_HEAD").exists(),
        "the merge is left in progress for the user to commit"
    );
    assert_eq!(
        run_git(&repo, &["status", "--short"]).trim(),
        "A  feature.txt",
        "the incoming changes are staged"
    );
}

#[test]
fn merge_step_refuses_an_empty_target() {
    let engine = engine();
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = ff_repo(tmp.path(), "repo");
    let root = build_root(&engine, &repo).expect("root snapshot");
    let plan = BulkPlan {
        op: BulkOp::Merge,
        roots: Vec::new(),
        rebase: false,
        branch: String::new(),
        command: String::new(),
    };
    let err = bulk_ops::run_step(
        BulkOp::Merge,
        &engine,
        &root,
        &plan,
        &VcsSettings::default(),
        &MergeOpts::default(),
    )
    .expect_err("an empty target must be refused");
    assert!(
        err.to_string().to_lowercase().contains("empty"),
        "expected an empty-target refusal, got: {err}"
    );
}
