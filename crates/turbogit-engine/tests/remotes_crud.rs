//! Issue 33 — Manage remotes surface, engine-port seam.
//!
//! The remote manager CRUDs remotes: list fetch/push URLs, set a fetch or push
//! URL, rename, remove, and set a branch's upstream. These tests pin the
//! `GitExecutor` port contract against the real CLI adapter over temporary
//! repositories.

use test_support::git_seed::git;
use turbogit_domain::model::VcsSettings;
use turbogit_engine::GitExecutor;
use turbogit_engine::cli::CliExecutor;

// `temp_repo` stays local because no `git_seed` recipe produces a commit-free
// repository: `set_branch_upstream_sets_the_tracking_branch` makes its own first
// commit, so `refs/heads/main` exists before anything tracks it.

/// Fresh initialized repo on `main` with local identity configured, and NO
/// commits.
fn temp_repo(tag: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join(tag);
    std::fs::create_dir_all(&repo).expect("repo dir");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    (tmp, repo)
}

fn executor() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

#[test]
fn remotes_lists_fetch_and_push_urls_separately() {
    let (_tmp, repo) = temp_repo("remote-list");
    git(
        &repo,
        &["remote", "add", "origin", "https://fetch.git/repo.git"],
    );
    git(
        &repo,
        &[
            "remote",
            "set-url",
            "--push",
            "origin",
            "https://push.git/repo.git",
        ],
    );

    let remotes = executor().remotes(&repo).expect("remotes succeed");

    assert_eq!(remotes.len(), 1, "one remote configured");
    let origin = &remotes[0];
    assert_eq!(origin.name, "origin");
    assert_eq!(
        origin.fetch_url.as_deref(),
        Some("https://fetch.git/repo.git"),
        "fetch URL preserved"
    );
    assert_eq!(
        origin.push_url.as_deref(),
        Some("https://push.git/repo.git"),
        "distinct push URL surfaced"
    );
}

#[test]
fn set_remote_url_updates_a_fetch_or_push_url() {
    let (_tmp, repo) = temp_repo("remote-set-url");
    git(
        &repo,
        &["remote", "add", "origin", "https://old.git/repo.git"],
    );
    let exe = executor();

    exe.set_remote_url(&repo, "origin", Some("https://new.git/repo.git"), None)
        .expect("set fetch url succeeds");
    let remotes = exe.remotes(&repo).expect("remotes succeed");
    assert_eq!(
        remotes[0].fetch_url.as_deref(),
        Some("https://new.git/repo.git")
    );
    assert_eq!(
        remotes[0].push_url.as_deref(),
        Some("https://new.git/repo.git")
    );

    exe.set_remote_url(&repo, "origin", None, Some("https://push.git/repo.git"))
        .expect("set push url succeeds");
    let remotes = exe.remotes(&repo).expect("remotes succeed");
    assert_eq!(
        remotes[0].fetch_url.as_deref(),
        Some("https://new.git/repo.git")
    );
    assert_eq!(
        remotes[0].push_url.as_deref(),
        Some("https://push.git/repo.git")
    );
}

#[test]
fn set_remote_url_updates_both_sides_together() {
    let (_tmp, repo) = temp_repo("remote-set-url-both");
    git(
        &repo,
        &["remote", "add", "origin", "https://old.git/repo.git"],
    );
    let exe = executor();

    exe.set_remote_url(
        &repo,
        "origin",
        Some("https://fetch.git/repo.git"),
        Some("https://push.git/repo.git"),
    )
    .expect("set both urls succeeds");

    let remotes = exe.remotes(&repo).expect("remotes succeed");
    assert_eq!(
        remotes[0].fetch_url.as_deref(),
        Some("https://fetch.git/repo.git")
    );
    assert_eq!(
        remotes[0].push_url.as_deref(),
        Some("https://push.git/repo.git")
    );
}

#[test]
fn rename_remote_renames_the_remote() {
    let (_tmp, repo) = temp_repo("remote-rename");
    git(
        &repo,
        &["remote", "add", "origin", "https://fetch.git/repo.git"],
    );
    let exe = executor();

    exe.rename_remote(&repo, "origin", "upstream")
        .expect("rename succeeds");

    let remotes = exe.remotes(&repo).expect("remotes succeed");
    let names: Vec<&str> = remotes.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, vec!["upstream"], "renamed remote shows new name");
}

#[test]
fn remove_remote_removes_the_remote() {
    let (_tmp, repo) = temp_repo("remote-remove");
    git(
        &repo,
        &["remote", "add", "origin", "https://fetch.git/repo.git"],
    );
    git(
        &repo,
        &["remote", "add", "backup", "https://fetch.git/backup.git"],
    );
    let exe = executor();

    exe.remove_remote(&repo, "origin").expect("remove succeeds");

    let remotes = exe.remotes(&repo).expect("remotes succeed");
    let names: Vec<&str> = remotes.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, vec!["backup"], "removed remote no longer listed");
}

#[test]
fn set_branch_upstream_sets_the_tracking_branch() {
    let (_tmp, repo) = temp_repo("remote-upstream");
    git(
        &repo,
        &["remote", "add", "origin", "https://fetch.git/repo.git"],
    );
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "c1"]);

    // Provide the `origin/main` tracking ref git requires to exist
    // (`--set-upstream-to` refuses an unknown upstream), without any network.
    git(&repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);

    executor()
        .set_branch_upstream(&repo, "main", "origin/main")
        .expect("set upstream succeeds");
    assert_eq!(
        config_or_absent(&repo, "branch.main.remote").as_deref(),
        Some("origin"),
        "remote tracked"
    );
    assert_eq!(
        config_or_absent(&repo, "branch.main.merge").as_deref(),
        Some("refs/heads/main"),
        "merge ref tracked"
    );
}

/// Read a single git config value, or `None` when unset.
///
/// A third opposite contract, and not the same one as a failing command: here the
/// exit status **is** the value. `git config --get` exits 1 precisely when the key is
/// absent, which is what this suite reads back — asserting success would fail every
/// unset key, and discarding the status would report an empty string, not `None`.
fn config_or_absent(repo: &std::path::Path, key: &str) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["config", "--get", key])
        .current_dir(repo)
        .output()
        .expect("spawning git");
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}
