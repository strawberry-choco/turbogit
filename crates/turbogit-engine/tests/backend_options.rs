use std::path::{Path, PathBuf};

use std::sync::Arc;
use test_support::git_seed::{GitEnv, git_with_env};
use turbogit_domain::model::VcsSettings;
use turbogit_engine::GitExecutor;
use turbogit_engine::cli::CliExecutor;
use turbogit_engine::git2_exec::Git2Executor;

fn composed() -> Arc<dyn GitExecutor> {
    Arc::new(Git2Executor::new(CliExecutor {
        settings: VcsSettings::default(),
    }))
}

fn cli() -> Arc<dyn GitExecutor> {
    Arc::new(CliExecutor {
        settings: VcsSettings::default(),
    })
}

fn repo_with_remote_branch() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let bare = project.join("origin.git");
    git_with_env(
        &project,
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
        &GitEnv::fixture(),
    );
    let work = project.join("work");
    git_with_env(
        &project,
        &["init", "-q", "-b", "main", work.to_str().unwrap()],
        &GitEnv::fixture(),
    );
    std::fs::write(work.join("README.md"), "x\n").unwrap();
    git_with_env(&work, &["add", "."], &GitEnv::fixture());
    git_with_env(&work, &["commit", "-q", "-m", "init"], &GitEnv::fixture());
    git_with_env(
        &work,
        &["remote", "add", "origin", bare.to_str().unwrap()],
        &GitEnv::fixture(),
    );
    git_with_env(
        &work,
        &["push", "-q", "-u", "origin", "main"],
        &GitEnv::fixture(),
    );
    git_with_env(&work, &["branch", "feat", "HEAD"], &GitEnv::fixture());
    git_with_env(&work, &["push", "-q", "origin", "feat"], &GitEnv::fixture());
    git_with_env(&work, &["branch", "-D", "feat"], &GitEnv::fixture());
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
fn the_cli_adapter_records_the_upstream_that_git_dwims_from_a_remote_start_point() {
    let (_tmp, repo) = repo_with_remote_branch();
    let exec = cli();
    exec.branch_create(&repo, "feat", true, Some("origin/feat"))
        .expect("creating feat from origin/feat");
    assert_eq!(
        tracking_of(exec.as_ref(), &repo, "feat").as_deref(),
        Some("origin/feat"),
        "git's own DWIM records the upstream when the CLI checks out a remote branch"
    );
}

#[test]
fn the_composed_adapter_creates_the_branch_without_recording_an_upstream() {
    let (_tmp, repo) = repo_with_remote_branch();
    let exec = composed();
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
