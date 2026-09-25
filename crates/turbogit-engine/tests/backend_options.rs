//! What the two **Git engine** backends genuinely differ on.
//!
//! Replaces the parity suite that proved two of the three picker options agreed:
//! with the pair collapsed there is one CLI adapter and one composed adapter, and
//! the honest assertion is a *difference* between them, not a round-trip. The
//! difference is git's DWIM — which the glossary already warns is not guaranteed
//! across adapters — observed as the tracking ref a branch created from a remote
//! branch ends up with.

use std::path::{Path, PathBuf};
use std::process::Command;

use turbogit_domain::model::{GitBackend, VcsSettings};
use turbogit_engine::GitExecutor;
use turbogit_engine::build_executor;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("git must be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// One repo with a same-content `origin`, plus `feat` pushed there and deleted
/// locally — the state a remote branch row is clicked from.
fn repo_with_remote_branch() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let bare = project.join("origin.git");
    git(
        &project,
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    let work = project.join("work");
    git(
        &project,
        &["init", "-q", "-b", "main", work.to_str().unwrap()],
    );
    std::fs::write(work.join("README.md"), "x\n").unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "-q", "-m", "init"]);
    git(&work, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(&work, &["push", "-q", "-u", "origin", "main"]);
    git(&work, &["branch", "feat", "HEAD"]);
    git(&work, &["push", "-q", "origin", "feat"]);
    git(&work, &["branch", "-D", "feat"]);
    (tmp, work)
}

fn tracking_of(exec: &dyn GitExecutor, repo: &Path, name: &str) -> Option<String> {
    exec.branches(repo)
        .unwrap()
        .into_iter()
        .find(|b| b.name == name)
        .and_then(|b| b.tracking)
        .map(|u| u.git_ref())
}

#[test]
fn the_cli_backend_records_the_upstream_that_git_dwims_from_a_remote_start_point() {
    let (_tmp, repo) = repo_with_remote_branch();
    let exec = build_executor(&VcsSettings {
        backend: GitBackend::Cli,
        ..VcsSettings::default()
    });
    exec.branch_create(&repo, "feat", true, Some("origin/feat"))
        .expect("creating feat from origin/feat");
    assert_eq!(
        tracking_of(exec.as_ref(), &repo, "feat").as_deref(),
        Some("origin/feat"),
        "git's own DWIM records the upstream when the CLI checks out a remote branch"
    );
}

#[test]
fn the_composed_backend_creates_the_branch_without_recording_an_upstream() {
    let (_tmp, repo) = repo_with_remote_branch();
    let exec = build_executor(&VcsSettings {
        backend: GitBackend::InProcessReads,
        ..VcsSettings::default()
    });
    exec.branch_create(&repo, "feat", true, Some("origin/feat"))
        .expect("creating feat from origin/feat");
    assert_eq!(
        tracking_of(exec.as_ref(), &repo, "feat").as_deref(),
        None,
        "libgit2 records no tracking ref, so the composed adapter must be the one \
         that answers this create — that absence is the difference between the \
         two surviving backends"
    );
}
