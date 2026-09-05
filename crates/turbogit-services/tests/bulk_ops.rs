//! Issue 09 — bulk operations: the preflight-gated, per-repo fan-out.
//!
//! Headless integration tests over real git repositories (tempdir + system
//! `git`): each root gets a bare remote as upstream, and assertions observe
//! real repository state plus the per-root results `run_bulk` reports.

use std::path::{Path, PathBuf};
use std::process::Command;
use turbogit_domain::error::TgResult;
use turbogit_domain::model::{MultiRootManager, RootId, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_services::bulk_ops::{BulkOp, BulkPlan, run_bulk};
use turbogit_services::multi_root::{build_root, register};

/// Run `git <args>` in `dir`, asserting success; returns stdout.
fn run_git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawning git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// Append a line to `file.txt` in `dir`, stage, commit, and return the new
/// HEAD SHA.
fn commit(dir: &Path, msg: &str) -> String {
    let file = dir.join("file.txt");
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
        .expect("opening work file");
    use std::io::Write;
    writeln!(f, "{msg}").expect("appending work file");
    drop(f);

    run_git(dir, &["add", "."]);
    run_git(dir, &["commit", "-m", msg]);
    run_git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

/// Fresh repo on `main` with local identity, a bare remote at
/// `<tmp>/<name>.origin.git`, and `main` pushed to it. Returns
/// `(repo path, remote path)`.
fn repo_with_upstream(tmp: &Path, name: &str) -> (PathBuf, PathBuf) {
    let repo = tmp.join(name);
    let remote = tmp.join(format!("{name}.origin.git"));
    std::fs::create_dir_all(&repo).expect("repo dir");

    run_git(&repo, &["init", "-q", "-b", "main"]);
    run_git(&repo, &["config", "user.email", "test@example.com"]);
    run_git(&repo, &["config", "user.name", "Test"]);
    commit(&repo, "c1");

    run_git(
        &repo,
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );
    run_git(
        &repo,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    run_git(&repo, &["push", "-q", "-u", "origin", "main"]);
    (repo, remote)
}

fn manager(engine: &CliExecutor, repos: &[PathBuf]) -> MultiRootManager {
    let mut mgr = MultiRootManager::default();
    for repo in repos {
        register(&mut mgr, build_root(engine, repo).expect("root snapshot"));
    }
    mgr
}

fn id(repo: &Path) -> RootId {
    RootId(repo.into())
}

/// The planned root ids, in result order.
fn result_roots(results: &[(RootId, TgResult<()>)]) -> Vec<RootId> {
    results.iter().map(|(r, _)| r.clone()).collect()
}

/// Assert every reported outcome is `Ok`.
#[track_caller]
fn assert_all_ok(results: &[(RootId, TgResult<()>)]) {
    for (rid, r) in results {
        assert!(r.is_ok(), "{rid:?} should have run cleanly: {r:?}");
    }
}

/// The tip SHA a bare remote's `main` points at.
fn remote_tip(remote: &Path) -> String {
    run_git(remote, &["rev-parse", "main"]).trim().to_string()
}

/// A scratch clone of `remote` with local identity, for seeding upstream work.
fn scratch_clone(tmp: &Path, remote: &Path) -> PathBuf {
    let scratch = tmp.join("scratch");
    run_git(
        tmp,
        &[
            "clone",
            "-q",
            remote.to_str().unwrap(),
            scratch.to_str().unwrap(),
        ],
    );
    run_git(&scratch, &["config", "user.email", "test@example.com"]);
    run_git(&scratch, &["config", "user.name", "Test"]);
    scratch
}

#[test]
fn run_bulk_pushes_only_the_planned_roots_and_reports_each_outcome() {
    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    let (alpha, alpha_remote) = repo_with_upstream(tmp.path(), "alpha");
    let (ui, ui_remote) = repo_with_upstream(tmp.path(), "ui");
    let (lib, _lib_remote) = repo_with_upstream(tmp.path(), "lib");
    let mgr = manager(&engine, &[alpha.clone(), ui.clone(), lib.clone()]);

    // Every root gets one local-ahead commit…
    let alpha_sha = commit(&alpha, "alpha work");
    let ui_sha = commit(&ui, "ui work");
    commit(&lib, "lib work");

    // …but the plan scopes the push to alpha and ui only.
    let plan = BulkPlan {
        op: BulkOp::PushAll,
        roots: vec![id(&alpha), id(&ui)],
        rebase: false,
        branch: String::new(),
        command: String::new(),
    };
    let results = run_bulk(&engine, &mgr, &VcsSettings::default(), &plan);

    assert_eq!(
        result_roots(&results),
        vec![id(&alpha), id(&ui)],
        "one entry per planned root, nothing for out-of-scope roots"
    );
    assert_all_ok(&results);
    assert_eq!(
        remote_tip(&alpha_remote),
        alpha_sha,
        "alpha's remote advanced"
    );
    assert_eq!(remote_tip(&ui_remote), ui_sha, "ui's remote advanced");

    // lib stays ahead of its untouched remote.
    let lib_remote = tmp.path().join("lib.origin.git");
    let local_tip = run_git(&lib, &["rev-parse", "HEAD"]).trim().to_string();
    assert_ne!(
        remote_tip(&lib_remote),
        local_tip,
        "out-of-scope root's remote must not advance"
    );
}

#[test]
fn run_bulk_fetch_all_updates_only_the_planned_roots_tracking_refs() {
    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    let (alpha, alpha_remote) = repo_with_upstream(tmp.path(), "alpha");
    let (ui, _ui_remote) = repo_with_upstream(tmp.path(), "ui");
    let mgr = manager(&engine, &[alpha.clone(), ui.clone()]);

    // Incoming work on alpha's remote, made through a scratch clone.
    let scratch = scratch_clone(tmp.path(), &alpha_remote);
    let incoming = commit(&scratch, "incoming");
    run_git(&scratch, &["push", "-q", "origin", "main"]);

    let plan = BulkPlan {
        op: BulkOp::FetchAll,
        roots: vec![id(&alpha)],
        rebase: false,
        branch: String::new(),
        command: String::new(),
    };
    let results = run_bulk(&engine, &mgr, &VcsSettings::default(), &plan);

    assert_eq!(result_roots(&results), vec![id(&alpha)]);
    assert_all_ok(&results);
    let alpha_tracking = run_git(&alpha, &["rev-parse", "origin/main"])
        .trim()
        .to_string();
    assert_eq!(
        alpha_tracking, incoming,
        "alpha's remote-tracking ref advanced"
    );
    let ui_tracking = run_git(&ui, &["rev-parse", "origin/main"])
        .trim()
        .to_string();
    assert_ne!(
        ui_tracking, incoming,
        "out-of-scope root's tracking ref stayed put"
    );
}

#[test]
fn run_bulk_pull_all_fast_forwards_planned_roots_with_incoming_work() {
    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    let (alpha, alpha_remote) = repo_with_upstream(tmp.path(), "alpha");
    let (ui, _ui_remote) = repo_with_upstream(tmp.path(), "ui");
    let mgr = manager(&engine, &[alpha.clone(), ui.clone()]);

    // Incoming work on alpha's remote, made through a scratch clone.
    let scratch = scratch_clone(tmp.path(), &alpha_remote);
    let incoming = commit(&scratch, "incoming");
    run_git(&scratch, &["push", "-q", "origin", "main"]);

    // The plan scopes the pull to alpha; ui is out of scope.
    let plan = BulkPlan {
        op: BulkOp::PullAll,
        roots: vec![id(&alpha)],
        rebase: false,
        branch: String::new(),
        command: String::new(),
    };
    let results = run_bulk(&engine, &mgr, &VcsSettings::default(), &plan);

    assert_eq!(result_roots(&results), vec![id(&alpha)]);
    assert_all_ok(&results);
    let alpha_tip = run_git(&alpha, &["rev-parse", "HEAD"]).trim().to_string();
    assert_eq!(
        alpha_tip, incoming,
        "alpha fast-forwarded onto the incoming commit"
    );
    let ui_tip = run_git(&ui, &["rev-parse", "HEAD"]).trim().to_string();
    assert_ne!(ui_tip, incoming, "out-of-scope root did not pull");
}

#[test]
fn run_bulk_pull_honors_the_rebase_policy() {
    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    let (alpha, alpha_remote) = repo_with_upstream(tmp.path(), "alpha");
    let mgr = manager(&engine, std::slice::from_ref(&alpha));

    // Local commit, then incoming upstream work → a diverged history that a
    // rebase policy rewrites (no merge commit), while merge would create one.
    // The upstream commit touches a different file so the rebase applies
    // cleanly instead of conflicting.
    commit(&alpha, "local work");
    let scratch = scratch_clone(tmp.path(), &alpha_remote);
    std::fs::write(scratch.join("other.txt"), "upstream\n").expect("upstream file");
    run_git(&scratch, &["add", "."]);
    run_git(&scratch, &["commit", "-m", "incoming"]);
    run_git(&scratch, &["push", "-q", "origin", "main"]);

    let plan = BulkPlan {
        op: BulkOp::PullAll,
        roots: vec![id(&alpha)],
        rebase: true,
        branch: String::new(),
        command: String::new(),
    };
    let results = run_bulk(&engine, &mgr, &VcsSettings::default(), &plan);

    assert_eq!(result_roots(&results), vec![id(&alpha)]);
    assert_all_ok(&results);
    let log = run_git(&alpha, &["log", "--format=%s"]);
    assert!(log.contains("local work"), "local work survived: {log:?}");
    assert!(log.contains("incoming"), "upstream work arrived: {log:?}");
    assert!(
        !log.to_lowercase().contains("merge"),
        "rebase policy must not create a merge commit: {log:?}"
    );
}

#[test]
fn run_bulk_stash_all_parks_dirty_worktrees_in_scope() {
    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    let (alpha, _alpha_remote) = repo_with_upstream(tmp.path(), "alpha");
    let (ui, _ui_remote) = repo_with_upstream(tmp.path(), "ui");
    let mgr = manager(&engine, &[alpha.clone(), ui.clone()]);

    // Both roots get a dirty worktree; only alpha is in scope.
    for repo in [&alpha, &ui] {
        std::fs::write(repo.join("file.txt"), "dirty\n").expect("dirty file");
    }

    let plan = BulkPlan {
        op: BulkOp::StashAll,
        roots: vec![id(&alpha)],
        rebase: false,
        branch: String::new(),
        command: String::new(),
    };
    let results = run_bulk(&engine, &mgr, &VcsSettings::default(), &plan);

    assert_eq!(result_roots(&results), vec![id(&alpha)]);
    assert_all_ok(&results);
    let alpha_status = run_git(&alpha, &["status", "--porcelain"]);
    assert!(
        alpha_status.trim().is_empty(),
        "alpha's worktree was stashed clean: {alpha_status:?}"
    );
    let alpha_stashes = run_git(&alpha, &["stash", "list"]);
    assert!(
        !alpha_stashes.trim().is_empty(),
        "alpha's changes landed in the stash"
    );
    let ui_status = run_git(&ui, &["status", "--porcelain"]);
    assert!(
        !ui_status.trim().is_empty(),
        "out-of-scope root keeps its dirty worktree"
    );
}

#[test]
fn run_bulk_reports_a_failing_root_as_err_and_keeps_running_the_rest() {
    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    let (alpha, _alpha_remote) = repo_with_upstream(tmp.path(), "alpha");
    let (ui, _ui_remote) = repo_with_upstream(tmp.path(), "ui");
    let mgr = manager(&engine, &[alpha.clone(), ui.clone()]);

    // Both roots get commits; alpha's remote is removed so its push fails
    // structurally while ui's succeeds.
    commit(&alpha, "alpha work");
    commit(&ui, "ui work");
    run_git(&alpha, &["remote", "remove", "origin"]);

    let plan = BulkPlan {
        op: BulkOp::PushAll,
        roots: vec![id(&alpha), id(&ui)],
        rebase: false,
        branch: String::new(),
        command: String::new(),
    };
    let results = run_bulk(&engine, &mgr, &VcsSettings::default(), &plan);

    assert_eq!(result_roots(&results), vec![id(&alpha), id(&ui)]);
    assert!(
        results[0].1.is_err(),
        "root whose remote vanished reports Err: {results:?}"
    );
    assert!(
        results[1].1.is_ok(),
        "the other root still ran: {results:?}"
    );
}

// -- Cascade create & checkout branch (issue 11) -----------------------------

/// Put `repo`'s local `main` one commit behind `origin/main`: commit, push,
/// then reset the local branch back. Returns the upstream tip SHA.
fn make_behind(repo: &Path) -> String {
    let tip = commit(repo, "incoming upstream work");
    run_git(repo, &["push", "-q", "origin", "main"]);
    run_git(repo, &["reset", "-q", "--hard", "HEAD~1"]);
    tip
}

#[test]
fn create_branch_step_checks_out_existing_and_bases_behind_repos_on_their_upstream() {
    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    let (clean, _) = repo_with_upstream(tmp.path(), "clean");
    let (existing, _) = repo_with_upstream(tmp.path(), "existing");
    let (behind, _) = repo_with_upstream(tmp.path(), "behind");

    // existing already carries feature/x at its first commit; main moves on.
    let old_tip = run_git(&existing, &["rev-parse", "HEAD"])
        .trim()
        .to_string();
    run_git(&existing, &["branch", "feature/x"]);
    commit(&existing, "more main work");

    // behind: local main is one commit behind origin/main.
    let upstream_tip = make_behind(&behind);

    let roots: Vec<turbogit_domain::model::Root> = [&clean, &existing, &behind]
        .iter()
        .map(|p| build_root(&engine, p).expect("root snapshot"))
        .collect();

    for root in &roots {
        turbogit_services::bulk_ops::run_step(
            BulkOp::CreateBranch,
            &engine,
            root,
            &BulkPlan {
                op: BulkOp::CreateBranch,
                roots: Vec::new(),
                rebase: true,
                branch: "feature/x".to_string(),
                command: String::new(),
            },
            &VcsSettings::default(),
            &Default::default(),
        )
        .expect("the step should succeed");
    }

    // clean: created from HEAD and checked out.
    let head = run_git(&clean, &["rev-parse", "HEAD"]).trim().to_string();
    assert_eq!(
        run_git(&clean, &["branch", "--show-current"]).trim(),
        "feature/x",
        "clean repo is on the new branch"
    );
    assert_eq!(
        run_git(&clean, &["rev-parse", "feature/x"]).trim(),
        head,
        "the new branch sits at the repo's HEAD"
    );

    // existing: checked out, never recreated — the branch keeps its old tip.
    assert_eq!(
        run_git(&existing, &["branch", "--show-current"]).trim(),
        "feature/x"
    );
    assert_eq!(
        run_git(&existing, &["rev-parse", "feature/x"]).trim(),
        old_tip,
        "an existing match is checked out as-is, not recreated at HEAD"
    );

    // behind + apply broadly: based on the upstream tip, not stale HEAD.
    assert_eq!(
        run_git(&behind, &["branch", "--show-current"]).trim(),
        "feature/x"
    );
    assert_eq!(
        run_git(&behind, &["rev-parse", "feature/x"]).trim(),
        upstream_tip,
        "a behind repo bases the new branch on its upstream"
    );
}

#[test]
fn create_branch_step_without_apply_broadly_ignores_the_upstream_state() {
    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    let (behind, _) = repo_with_upstream(tmp.path(), "behind");
    let upstream_tip = make_behind(&behind);
    let local_tip = run_git(&behind, &["rev-parse", "HEAD"]).trim().to_string();
    assert_ne!(local_tip, upstream_tip);

    let root = build_root(&engine, &behind).expect("root snapshot");
    turbogit_services::bulk_ops::run_step(
        BulkOp::CreateBranch,
        &engine,
        &root,
        &BulkPlan {
            op: BulkOp::CreateBranch,
            roots: Vec::new(),
            rebase: false,
            branch: "feature/x".to_string(),
            command: String::new(),
        },
        &VcsSettings::default(),
        &Default::default(),
    )
    .expect("the step should succeed");

    assert_eq!(
        run_git(&behind, &["rev-parse", "feature/x"]).trim(),
        local_tip,
        "policy off → the branch is cut from local HEAD even when behind"
    );
}

// -- Custom command (issue 13) ------------------------------------------------

#[test]
fn run_bulk_executes_a_custom_command_on_every_planned_root() {
    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    let (alpha, _alpha_remote) = repo_with_upstream(tmp.path(), "alpha");
    let (ui, _ui_remote) = repo_with_upstream(tmp.path(), "ui");
    let mgr = manager(&engine, &[alpha.clone(), ui.clone()]);

    // The user-typed form includes the leading "git"; the plan carries it
    // like the create-&-checkout cascade carries its branch name.
    let plan = BulkPlan {
        op: BulkOp::Custom,
        roots: vec![id(&alpha), id(&ui)],
        rebase: false,
        branch: String::new(),
        command: "git branch bulk-marker".to_string(),
    };
    let results = run_bulk(&engine, &mgr, &VcsSettings::default(), &plan);
    assert_all_ok(&results);
    assert_eq!(result_roots(&results), vec![id(&alpha), id(&ui)]);

    for repo in [&alpha, &ui] {
        let branches = run_git(repo, &["branch", "--list", "bulk-marker"]);
        assert!(
            branches.contains("bulk-marker"),
            "{repo:?} should carry the created branch"
        );
    }
}

#[test]
fn a_failing_custom_command_reports_the_git_error_per_root_without_stopping_the_fleet() {
    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    let (alpha, _alpha_remote) = repo_with_upstream(tmp.path(), "alpha");
    let (ui, _ui_remote) = repo_with_upstream(tmp.path(), "ui");
    // ui already carries the branch, so `git branch` fails there with a
    // "already exists" stderr while alpha succeeds.
    run_git(&ui, &["branch", "bulk-marker"]);
    let mgr = manager(&engine, &[alpha.clone(), ui.clone()]);

    let plan = BulkPlan {
        op: BulkOp::Custom,
        roots: vec![id(&alpha), id(&ui)],
        rebase: false,
        branch: String::new(),
        command: "branch bulk-marker".to_string(),
    };
    let results = run_bulk(&engine, &mgr, &VcsSettings::default(), &plan);
    assert_eq!(results[0].0, id(&alpha));
    assert!(
        results[0].1.is_ok(),
        "alpha ran cleanly: {:?}",
        results[0].1
    );
    assert_eq!(results[1].0, id(&ui));
    let err = results[1].1.as_ref().expect_err("ui's branch exists");
    assert!(
        err.to_string().contains("already exists"),
        "git stderr surfaced: {err}"
    );
}

#[test]
fn an_unparseable_custom_command_fails_the_step_before_touching_git() {
    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    let (alpha, _alpha_remote) = repo_with_upstream(tmp.path(), "alpha");
    let mgr = manager(&engine, std::slice::from_ref(&alpha));

    let plan = BulkPlan {
        op: BulkOp::Custom,
        roots: vec![id(&alpha)],
        rebase: false,
        branch: String::new(),
        command: "git".to_string(),
    };
    let results = run_bulk(&engine, &mgr, &VcsSettings::default(), &plan);
    assert!(
        results[0].1.is_err(),
        "an empty command list cannot run: {:?}",
        results[0].1
    );
}
