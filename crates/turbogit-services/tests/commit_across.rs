use std::path::{Path, PathBuf};
use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::{CommitId, MultiRootManager, RootId};
use turbogit_engine::cli::CliExecutor;
use turbogit_services::commit_across::{PlanRow, plan, run_one};
use turbogit_services::multi_root::{build_root, register};

use test_support::git_seed::{git as run_git, repo_with_one_commit as fresh_repo};

fn stage_file(repo: &Path, file: &str, content: &str) {
    std::fs::write(repo.join(file), content).expect("write");
    run_git(repo, &["add", "--", file]);
}

fn head(repo: &Path) -> String {
    run_git(repo, &["rev-parse", "HEAD"]).trim().to_string()
}

fn staged_paths(repo: &Path) -> Vec<String> {
    let out = run_git(repo, &["diff", "--cached", "--name-only"]);
    out.lines().map(|s| s.to_string()).collect()
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
    let log = run_git(&repo, &["log", "-1", "--format=%s"]);
    assert_eq!(log.trim(), "alpha feature");
}

#[test]
fn run_one_reports_an_engine_failure_when_the_index_is_empty() {
    let engine = CliExecutor {
        settings: Default::default(),
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = fresh_repo(tmp.path(), "alpha");

    let err = run_one(&engine, &repo, "nothing staged", false).expect_err("should fail");
    let formatted = format!("{err:?}");

    assert!(
        formatted.contains("code 1") || formatted.contains("Cli"),
        "expected a Cli error carrying exit code 1, got: {formatted}"
    );
    let _: TgError = err;
}

#[test]
fn run_one_amend_replaces_the_head_message_and_keeps_the_tree() {
    let engine = CliExecutor {
        settings: Default::default(),
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = fresh_repo(tmp.path(), "alpha");
    stage_file(&repo, "feature.txt", "amend feature\n");
    run_one(&engine, &repo, "first message", false).expect("initial commit");
    let before_sha = head(&repo);

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

    assert_ne!(head(&alpha), alpha_before);
    assert_ne!(head(&lib), lib_before);
    assert_eq!(head(&ui), ui_before);
}
