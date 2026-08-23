//! Issue #8 — push dry-run passthrough through the engine seam.
//!
//! Headless integration tests over real git repositories (tempdir + system
//! `git`): one repo per scenario, a bare remote as upstream, and assertions on
//! full commit SHAs in log order.

use std::path::{Path, PathBuf};
use std::process::Command;
use turbogit::engine::cli::CliExecutor;
use turbogit::engine::GitExecutor;
use turbogit::error::TgError;
use turbogit::model::VcsSettings;

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
/// Returns `(tempdir guard, repo path, bare remote path, [c1, c2, c3] SHAs)`.
fn repo_ahead_of_origin() -> (tempfile::TempDir, PathBuf, PathBuf, Vec<String>) {
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

    (tmp, repo, remote, vec![c1, c2, c3])
}

/// Diverged history: `repo_ahead_of_origin()` plus a second clone that
/// fast-forwards the bare remote to its own commit d1, so the original repo
/// (c2, c3) and the remote (d1) have truly divergent `main` tips.
///
/// Returns `(tempdir guard, repo path, bare remote path, d1 SHA)`.
fn diverged_from_origin() -> (tempfile::TempDir, PathBuf, PathBuf, String) {
    let (tmp, repo, remote, _shas) = repo_ahead_of_origin();

    let other = tmp.path().join("other");
    run_git(tmp.path(), &["clone", remote.to_str().unwrap(), "other"]);
    // The bare remote's HEAD may point at an unborn default branch; put the
    // clone on `main` (tracking origin/main) before committing.
    run_git(&other, &["checkout", "main"]);
    run_git(&other, &["config", "user.email", "test@example.com"]);
    run_git(&other, &["config", "user.name", "Test"]);
    let d1 = commit(&other, "d1");
    run_git(&other, &["push", "origin", "main"]);
    // Let the original repo learn about d1 (updates origin/main only), so the
    // divergence is known on both sides and git reports `(non-fast-forward)`
    // rather than `(fetch first)`.
    run_git(&repo, &["fetch", "origin"]);

    (tmp, repo, remote, d1)
}

#[test]
fn push_dry_run_reports_updatable_refs_without_mutating_remote() {
    let (_tmp, repo, remote_path, shas) = repo_ahead_of_origin();
    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };

    let before = run_git(&remote_path, &["rev-parse", "main"])
        .trim()
        .to_string();
    let report = engine
        .push_dry_run(&repo, "origin", "main", false)
        .expect("dry-run succeeds on pushable state");

    // Verbatim-enough report: names the remote and the ref update.
    assert!(
        report.contains("remote.git"),
        "report should name the remote path, got: {report:?}"
    );
    assert!(
        report.contains("main -> main"),
        "report should show the ref update, got: {report:?}"
    );
    assert!(!report.contains("[rejected]"));

    // Provably non-mutating: remote ref unchanged, still at c1.
    let after = run_git(&remote_path, &["rev-parse", "main"])
        .trim()
        .to_string();
    assert_eq!(after, before);
    assert_eq!(after, shas[0], "remote main must still be c1");
}

#[test]
fn push_dry_run_reports_rejection_on_non_fast_forward() {
    let (_tmp, repo, remote_path, d1) = diverged_from_origin();
    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };

    let err = engine
        .push_dry_run(&repo, "origin", "main", false)
        .expect_err("diverged push must be rejected");

    match err {
        TgError::Cli { code, stderr } => {
            assert_ne!(code, 0);
            assert!(
                stderr.contains("[rejected]"),
                "stderr should mark the rejected ref, got: {stderr:?}"
            );
            assert!(
                stderr.contains("(non-fast-forward)"),
                "stderr should give the reason, got: {stderr:?}"
            );
        }
        other => panic!("expected TgError::Cli, got: {other:?}"),
    }

    // Provably non-mutating even on rejection.
    let remote_main = run_git(&remote_path, &["rev-parse", "main"])
        .trim()
        .to_string();
    assert_eq!(remote_main, d1, "remote main must still be d1");
}
