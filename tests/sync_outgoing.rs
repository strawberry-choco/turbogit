//! Issue #5 — outgoing commits (local-ahead SHAs) through the engine seam and
//! the multi-root fan-out.
//!
//! Headless integration tests over real git repositories (tempdir + system
//! `git`): one repo per scenario, a bare remote as upstream, and assertions on
//! full commit SHAs in log order.

use std::path::{Path, PathBuf};
use std::process::Command;
use turbogit::core::multi_root::{build_root, register};
use turbogit::core::sync_service::outgoing_per_root;
use turbogit::engine::cli::CliExecutor;
use turbogit::engine::GitExecutor;
use turbogit::model::{MultiRootManager, RootId, VcsSettings};

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
/// HEAD SHA (`git rev-parse HEAD`).
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

/// Fresh repo on `main` with local identity configured, a bare remote at
/// `<tmp>/remote.git`, and `main` pushed to it (commit c1); then two more
/// local commits c2 and c3 that are ahead of `origin/main`.
///
/// Returns `(tempdir guard, repo path, [c1, c2, c3] SHAs)`.
fn repo_ahead_of_origin() -> (tempfile::TempDir, PathBuf, Vec<String>) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    let remote = tmp.path().join("remote.git");
    std::fs::create_dir_all(&repo).expect("repo dir");

    run_git(&repo, &["init", "-b", "main"]);
    run_git(&repo, &["config", "user.email", "test@example.com"]);
    run_git(&repo, &["config", "user.name", "Test"]);
    let c1 = commit(&repo, "c1");

    run_git(&repo, &["init", "--bare", remote.to_str().unwrap()]);
    run_git(
        &repo,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    run_git(&repo, &["push", "-u", "origin", "main"]);

    let c2 = commit(&repo, "c2");
    let c3 = commit(&repo, "c3");

    (tmp, repo, vec![c1, c2, c3])
}

#[test]
fn outgoing_commits_lists_local_ahead_shas_in_log_order() {
    let (_tmp, repo, shas) = repo_ahead_of_origin();
    // c1 was pushed to origin/main by the setup; only c2 and c3 are
    // local-ahead. rev-list/log order is newest-first.
    let expected = vec![shas[2].clone(), shas[1].clone()];

    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };
    let got = engine
        .outgoing_commits(&repo, "main", "origin/main")
        .expect("outgoing_commits");
    assert_eq!(got, expected);

    // Cross-check against raw git output for the same range.
    let raw = run_git(&repo, &["log", "--format=%H", "@{u}..HEAD"]);
    let via_git: Vec<String> = raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    assert_eq!(via_git, expected);
}

#[test]
fn outgoing_per_root_returns_one_list_per_root() {
    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };
    let mut mgr = MultiRootManager::default();

    // Two independent repos, each with an upstream and two local-ahead commits.
    let (_tmp1, repo1, shas1) = repo_ahead_of_origin();
    let (_tmp2, repo2, shas2) = repo_ahead_of_origin();
    register(&mut mgr, build_root(&engine, &repo1).expect("root 1"));
    register(&mut mgr, build_root(&engine, &repo2).expect("root 2"));

    let results = outgoing_per_root(&engine, &mgr);

    assert_eq!(results.len(), 2, "one entry per registered root");
    assert_eq!(results[0].0, RootId(repo1.clone()));
    assert_eq!(results[1].0, RootId(repo2.clone()));

    let out1 = results[0].1.as_ref().expect("root 1 result is Ok");
    let out2 = results[1].1.as_ref().expect("root 2 result is Ok");
    assert_eq!(out1, &vec![shas1[2].clone(), shas1[1].clone()]);
    assert_eq!(out2, &vec![shas2[2].clone(), shas2[1].clone()]);
}

#[test]
fn outgoing_per_root_yields_empty_for_root_without_upstream() {
    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };
    let mut mgr = MultiRootManager::default();

    // Good root: upstream + two local-ahead commits.
    let (_tmp_good, good_repo, shas) = repo_ahead_of_origin();

    // Second repo: current branch, but never pushed — no remote, no tracking.
    let tmp_solo = tempfile::tempdir().expect("tempdir");
    let solo_repo = tmp_solo.path().join("solo");
    std::fs::create_dir_all(&solo_repo).expect("solo repo dir");
    run_git(&solo_repo, &["init", "-b", "main"]);
    run_git(&solo_repo, &["config", "user.email", "test@example.com"]);
    run_git(&solo_repo, &["config", "user.name", "Test"]);
    commit(&solo_repo, "only local");

    register(
        &mut mgr,
        build_root(&engine, &good_repo).expect("good root"),
    );
    register(
        &mut mgr,
        build_root(&engine, &solo_repo).expect("solo root"),
    );

    let results = outgoing_per_root(&engine, &mgr);
    assert_eq!(results.len(), 2);

    let mut by_id = std::collections::HashMap::new();
    for (id, result) in results {
        by_id.insert(id, result);
    }

    let good = by_id
        .get(&RootId(good_repo.clone()))
        .expect("good root present")
        .as_ref()
        .expect("good root result is Ok");
    assert_eq!(good, &vec![shas[2].clone(), shas[1].clone()]);

    let solo = by_id
        .get(&RootId(solo_repo.clone()))
        .expect("solo root present")
        .as_ref()
        .expect("missing upstream must be graceful Ok, not Err");
    assert!(
        solo.is_empty(),
        "root without upstream must yield an empty outgoing list"
    );
}
