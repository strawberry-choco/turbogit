//! Issue 21 — cascade commit ("Also commit on N selected repos").
//!
//! Headless integration tests over real git repositories (tempdir + system
//! `git`): each root is a fresh repo with local identity; the service
//! classifies them by what's in the index, executes the per-repo commit, and
//! reports per-repo outcomes through the same `Result`-shaped channel as the
//! rest of the cascade fleet (issue 09's `run_bulk`).

use std::path::{Path, PathBuf};
use std::process::Command;
use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::{CommitId, MultiRootManager, RootId};
use turbogit_engine::cli::CliExecutor;
use turbogit_services::commit_across::{PlanRow, plan, run_one};
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

/// Fresh repo on `main` with local identity and one committed file. Returns
/// the repo path.
fn fresh_repo(tmp: &Path, name: &str) -> PathBuf {
    let repo = tmp.join(name);
    std::fs::create_dir_all(&repo).expect("repo dir");
    run_git(&repo, &["init", "-q", "-b", "main"]);
    run_git(&repo, &["config", "user.email", "test@example.com"]);
    run_git(&repo, &["config", "user.name", "Test"]);
    std::fs::write(repo.join("seed.txt"), "seed\n").expect("seed");
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-q", "-m", "seed"]);
    repo
}

/// Write `content` to `path` and `git add` it; leaves the file staged but
/// uncommitted.
fn stage_file(repo: &Path, file: &str, content: &str) {
    std::fs::write(repo.join(file), content).expect("write");
    run_git(repo, &["add", "--", file]);
}

/// HEAD SHA (`git rev-parse HEAD`).
fn head(repo: &Path) -> String {
    run_git(repo, &["rev-parse", "HEAD"]).trim().to_string()
}

/// The staged change count via `git diff --cached --name-only` (the canonical
/// "anything in the index" signal). Returns an empty list for a clean
/// index.
fn staged_paths(repo: &Path) -> Vec<String> {
    let out = run_git(repo, &["diff", "--cached", "--name-only"]);
    out.lines().map(|s| s.to_string()).collect()
}

/// A registered manager over `repos`, built through the production
/// registration path.
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

fn row_for<'a>(rows: &'a [PlanRow], rid: &RootId) -> &'a PlanRow {
    rows.iter()
        .find(|r| &r.root == rid)
        .unwrap_or_else(|| panic!("missing plan row for {rid:?}"))
}

#[test]
fn plan_classifies_repos_by_whether_anything_is_staged() {
    let engine = CliExecutor {
        settings: Default::default(),
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    let staged = fresh_repo(tmp.path(), "staged");
    let empty = fresh_repo(tmp.path(), "empty");
    let _mgr = manager(&engine, &[staged.clone(), empty.clone()]);

    // alpha: a staged edit; ui: a clean tree.
    stage_file(&staged, "feature.txt", "alpha feature\n");

    let rows = plan(&engine, &[staged.clone(), empty.clone()]);
    let staged_row = row_for(&rows, &id(&staged));
    let empty_row = row_for(&rows, &id(&empty));

    assert!(
        staged_row.outcome.is_ok(),
        "staged repo should be marked will-run: {:?}",
        staged_row.outcome
    );
    assert_eq!(
        staged_row.outcome.as_ref().unwrap(),
        &1,
        "staged-changes count should be reported (footer text needs it)"
    );

    assert!(
        empty_row.outcome.is_err(),
        "clean repo should be marked skipped: {:?}",
        empty_row.outcome
    );
}

#[test]
fn run_one_commits_staged_changes_and_returns_the_new_head() {
    let engine = CliExecutor {
        settings: Default::default(),
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = fresh_repo(tmp.path(), "alpha");
    let before = head(&repo);

    stage_file(&repo, "feature.txt", "alpha feature\n");

    let new = run_one(&engine, &repo, "alpha feature", false).expect("commit");
    let after = head(&repo);

    assert_ne!(before, after, "HEAD should have advanced");
    assert_eq!(new.to_string(), after, "returned SHA should match HEAD");
    assert!(
        staged_paths(&repo).is_empty(),
        "index should be empty after commit, got {:?}",
        staged_paths(&repo)
    );
    // Message landed in the log.
    let log = run_git(&repo, &["log", "-1", "--format=%s"]);
    assert_eq!(log.trim(), "alpha feature");
}

#[test]
fn run_one_reports_an_engine_failure_when_the_index_is_empty() {
    // The service does not pre-filter — an out-of-band caller could ask for a
    // commit on a root whose index went empty between plan() and run_one().
    // The engine refuses and the error surfaces verbatim.
    let engine = CliExecutor {
        settings: Default::default(),
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = fresh_repo(tmp.path(), "alpha");

    let err = run_one(&engine, &repo, "nothing staged", false).expect_err("should fail");
    let formatted = format!("{err:?}");

    // git's "nothing to commit, working tree clean" lands on stdout, which
    // the engine does not surface here — the cascade-run monitor renders the
    // verbatim `TgError` we forward (the "git exited with code 1:" prefix
    // below). The contract that matters for the cascade is: an empty index
    // is a *run failure*, surfaced as a TgError::Cli, not a silent no-op.
    assert!(
        formatted.contains("code 1") || formatted.contains("Cli"),
        "expected a Cli error carrying exit code 1, got: {formatted}"
    );
    // The error must be a TgError so the cascade-run monitor renders it
    // consistently with the rest of the fleet.
    let _: TgError = err;
}

#[test]
fn run_one_amend_replaces_the_head_message_and_keeps_the_tree() {
    let engine = CliExecutor {
        settings: Default::default(),
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = fresh_repo(tmp.path(), "alpha");
    // Start from a clean tree, amend a real prior commit: stage a new edit
    // and amend the existing HEAD with --amend.
    stage_file(&repo, "feature.txt", "amend feature\n");
    run_one(&engine, &repo, "first message", false).expect("initial commit");
    let before_sha = head(&repo);

    // Amend the message; no further file changes needed (`git commit --amend`
    // re-uses the current index, which is now empty after the previous
    // commit).
    run_one(&engine, &repo, "amended message", true).expect("amend");

    let after_sha = head(&repo);
    assert_ne!(
        before_sha, after_sha,
        "amend should produce a new SHA even with no tree change"
    );
    let log = run_git(&repo, &["log", "-1", "--format=%s"]);
    assert_eq!(log.trim(), "amended message");
}

#[test]
fn plan_and_run_one_together_only_commit_roots_with_staged_content() {
    // The fan-out contract: a mixed selection commits only the roots that
    // had something in the index at plan time. The skipped roots' heads are
    // untouched and the run-one sequence over the planned roots produces one
    // new commit per root.
    let engine = CliExecutor {
        settings: Default::default(),
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    let alpha = fresh_repo(tmp.path(), "alpha");
    let ui = fresh_repo(tmp.path(), "ui");
    let lib = fresh_repo(tmp.path(), "lib");
    let _mgr = manager(&engine, &[alpha.clone(), ui.clone(), lib.clone()]);

    stage_file(&alpha, "feature.txt", "alpha\n");
    stage_file(&lib, "feature.txt", "lib\n");
    // ui: clean tree, nothing staged.

    let rows = plan(&engine, &[alpha.clone(), ui.clone(), lib.clone()]);
    let candidates: Vec<&Path> = vec![alpha.as_path(), ui.as_path(), lib.as_path()];
    let planned: Vec<&Path> = rows
        .iter()
        .filter(|r| r.outcome.is_ok())
        .filter_map(|r| candidates.iter().copied().find(|p| id(p) == r.root))
        .collect();

    assert_eq!(
        planned
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>(),
        vec![alpha.display().to_string(), lib.display().to_string()],
        "ui should be skipped; alpha and lib should be planned"
    );

    let alpha_before = head(&alpha);
    let lib_before = head(&lib);
    let ui_before = head(&ui);

    let mut results: Vec<(RootId, TgResult<CommitId>)> = Vec::new();
    for root in &planned {
        let result = run_one(&engine, root, "shared message", false);
        results.push((id(root), result));
    }

    for (rid, result) in &results {
        assert!(result.is_ok(), "{rid:?} should have committed cleanly");
    }

    // alpha + lib: new HEAD.
    assert_ne!(head(&alpha), alpha_before);
    assert_ne!(head(&lib), lib_before);
    // ui: untouched.
    assert_eq!(head(&ui), ui_before);
}
