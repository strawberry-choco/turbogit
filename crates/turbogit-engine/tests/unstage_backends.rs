//! Whole-file unstage across both backends — real-git integration tests.
//!
//! The per-file checkbox un-stages through `GitExecutor::unstage`, so the call
//! has to behave the same whichever backend answers it. These drive
//! [`CliExecutor`] and [`Git2Executor`] against temporary repositories and
//! assert end state through git itself (`status --porcelain`,
//! `diff --cached --name-only`) — never through internal representations.
//!
//! The case that matters most is the unborn HEAD: a repository nobody has
//! committed in yet has no HEAD for `restore --staged` to reset against, and
//! BOTH backends failed the call outright before each learned to drop the
//! index entries instead.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;
use test_support::git_seed::git;
use test_support::{RecordedCall, RecordingExecutor};
use turbogit_engine::GitExecutor;
use turbogit_engine::cli::CliExecutor;
use turbogit_engine::fake::{Call, FakeExecutor};
use turbogit_engine::git2_exec::Git2Executor;

struct Repo {
    path: PathBuf,
    /// Keeps the temp directory alive for the duration of the test.
    _dir: TempDir,
}

/// An initialized repository with repo-local user config so commits work
/// headlessly, and NO commits yet. The caller keeps `Repo` alive.
fn empty_repo(name: &str) -> Repo {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join(name);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    Repo { path, _dir: dir }
}

/// The same, plus one base commit so there is a HEAD to reset against.
fn repo_with_commit(name: &str) -> Repo {
    let repo = empty_repo(name);
    std::fs::write(repo.path.join("base.txt"), "base\n").unwrap();
    git(&repo.path, &["add", "."]);
    git(&repo.path, &["commit", "-q", "-m", "init"]);
    repo
}

fn cli() -> CliExecutor {
    CliExecutor {
        settings: turbogit_domain::model::VcsSettings::default(),
    }
}

fn libgit2() -> Git2Executor {
    Git2Executor::new(cli())
}

/// Porcelain lines for the repository, so a test reads the same shape git does.
fn porcelain(repo: &Repo) -> Vec<String> {
    git(&repo.path, &["status", "--porcelain"])
        .lines()
        .map(str::to_string)
        .collect()
}

/// Paths staged in the index, which is what un-staging is supposed to empty.
fn staged_paths(repo: &Repo) -> Vec<String> {
    git(&repo.path, &["diff", "--cached", "--name-only"])
        .lines()
        .map(str::to_string)
        .collect()
}

fn write(repo: &Repo, name: &str, body: &str) {
    std::fs::write(repo.path.join(name), body).unwrap();
}

// ------------------------------------------------------------ unborn HEAD --

#[test]
fn unstage_in_a_repository_with_no_commits_succeeds_on_libgit2() {
    let repo = empty_repo("unborn-unstage");
    write(&repo, "added.txt", "hello\n");
    git(&repo.path, &["add", "added.txt"]);
    assert_eq!(staged_paths(&repo), vec!["added.txt"], "precondition");

    libgit2()
        .unstage(&repo.path, &[PathBuf::from("added.txt")])
        .unwrap();
}

#[test]
fn unstage_in_a_repository_with_no_commits_succeeds_on_the_cli() {
    let repo = empty_repo("unborn-unstage-cli");
    write(&repo, "added.txt", "hello\n");
    git(&repo.path, &["add", "added.txt"]);

    cli()
        .unstage(&repo.path, &[PathBuf::from("added.txt")])
        .unwrap();

    assert!(staged_paths(&repo).is_empty(), "index should be empty");
}

/// The observable point of the unborn case: un-staging a newly added file
/// returns it to untracked, so the file is still there rather than vanishing
/// or becoming an empty diff.
#[test]
fn unstage_returns_a_newly_added_file_to_untracked_on_an_unborn_head() {
    let repo = empty_repo("unborn-untracked");
    write(&repo, "added.txt", "hello\n");
    git(&repo.path, &["add", "added.txt"]);

    libgit2()
        .unstage(&repo.path, &[PathBuf::from("added.txt")])
        .unwrap();

    assert!(
        staged_paths(&repo).is_empty(),
        "nothing should remain staged, got {:?}",
        staged_paths(&repo)
    );
    assert_eq!(
        porcelain(&repo),
        vec!["?? added.txt"],
        "the file should be untracked again, and still on disk"
    );
    assert!(
        repo.path.join("added.txt").exists(),
        "the working-tree file must survive an un-stage"
    );
}

/// Un-staging everything on an unborn HEAD has to work too, and has to leave
/// the files themselves alone.
#[test]
fn unstage_everything_on_an_unborn_head_empties_the_index_on_libgit2() {
    let repo = empty_repo("unborn-everything");
    write(&repo, "one.txt", "one\n");
    write(&repo, "two.txt", "two\n");
    git(&repo.path, &["add", "."]);
    assert_eq!(staged_paths(&repo).len(), 2, "precondition");

    libgit2().unstage(&repo.path, &[]).unwrap();

    assert!(staged_paths(&repo).is_empty(), "index should be empty");
    assert_eq!(porcelain(&repo), vec!["?? one.txt", "?? two.txt"]);
}

#[test]
fn unstage_everything_on_an_unborn_head_empties_the_index_on_the_cli() {
    let repo = empty_repo("unborn-everything-cli");
    write(&repo, "one.txt", "one\n");
    write(&repo, "two.txt", "two\n");
    git(&repo.path, &["add", "."]);

    cli().unstage(&repo.path, &[]).unwrap();

    assert!(staged_paths(&repo).is_empty(), "index should be empty");
    assert_eq!(porcelain(&repo), vec!["?? one.txt", "?? two.txt"]);
}

/// The unborn case is the one the checkbox depends on, and it is the one both
/// backends used to get wrong, so pin that they now agree on it.
#[test]
fn unstage_of_a_newly_added_file_agrees_across_backends_on_an_unborn_head() {
    let for_libgit2 = empty_repo("unborn-parity-libgit2");
    let for_cli = empty_repo("unborn-parity-cli");
    for repo in [&for_libgit2, &for_cli] {
        write(repo, "added.txt", "hello\n");
        git(&repo.path, &["add", "added.txt"]);
    }

    libgit2()
        .unstage(&for_libgit2.path, &[PathBuf::from("added.txt")])
        .unwrap();
    cli()
        .unstage(&for_cli.path, &[PathBuf::from("added.txt")])
        .unwrap();

    assert_eq!(porcelain(&for_libgit2), porcelain(&for_cli));
    assert_eq!(porcelain(&for_libgit2), vec!["?? added.txt"]);
}

// ------------------------------------------------------- committed branch --

/// The ordinary case must survive the change: a modified file un-stages back
/// to unstaged-modified on both backends.
#[test]
fn unstage_a_modified_file_returns_it_to_unstaged_on_libgit2() {
    let repo = repo_with_commit("committed-unstage");
    write(&repo, "base.txt", "changed\n");
    git(&repo.path, &["add", "base.txt"]);

    libgit2()
        .unstage(&repo.path, &[PathBuf::from("base.txt")])
        .unwrap();

    assert!(staged_paths(&repo).is_empty(), "index should be empty");
    assert_eq!(
        porcelain(&repo),
        vec![" M base.txt"],
        "the edit should still be in the working tree, unstaged"
    );
}

/// Both backends have to agree, since the checkbox cannot know which one is
/// configured underneath it.
#[test]
fn unstage_of_a_modified_file_agrees_across_backends() {
    let for_libgit2 = repo_with_commit("parity-libgit2");
    let for_cli = repo_with_commit("parity-cli");
    for repo in [&for_libgit2, &for_cli] {
        write(repo, "base.txt", "changed\n");
        git(&repo.path, &["add", "base.txt"]);
    }

    libgit2()
        .unstage(&for_libgit2.path, &[PathBuf::from("base.txt")])
        .unwrap();
    cli()
        .unstage(&for_cli.path, &[PathBuf::from("base.txt")])
        .unwrap();

    assert_eq!(porcelain(&for_libgit2), porcelain(&for_cli));
    assert_eq!(porcelain(&for_libgit2), vec![" M base.txt"]);
}

/// A deleted file is un-staged back to a staged-free deletion, which is a
/// different index operation from adding a path.
#[test]
fn unstage_of_a_deletion_on_libgit2_leaves_the_deletion_in_the_worktree() {
    let repo = repo_with_commit("committed-deletion");
    std::fs::remove_file(repo.path.join("base.txt")).unwrap();
    git(&repo.path, &["add", "-A"]);
    assert_eq!(staged_paths(&repo), vec!["base.txt"], "precondition");

    libgit2()
        .unstage(&repo.path, &[PathBuf::from("base.txt")])
        .unwrap();

    assert!(staged_paths(&repo).is_empty(), "index should be empty");
    assert_eq!(porcelain(&repo), vec![" D base.txt"]);
}

// -------------------------------------------------------- call recording --

/// Un-staging used to be invisible to both call recorders, so no suite could
/// see a checkbox un-stage anything. Both now report the paths — and the
/// recording wrapper still forwards, so this runs against a real repository.
#[test]
fn the_recording_executor_reports_an_unstage() {
    let repo = repo_with_commit("recorder-unstage");
    write(&repo, "base.txt", "changed\n");
    git(&repo.path, &["add", "base.txt"]);

    let recorder = RecordingExecutor::new(Arc::new(cli()));
    recorder
        .unstage(&repo.path, &[PathBuf::from("base.txt")])
        .unwrap();

    assert!(
        recorder
            .recorded()
            .contains(&RecordedCall::Unstage(vec![PathBuf::from("base.txt")])),
        "the recorder saw {:?}",
        recorder.recorded()
    );
    assert!(staged_paths(&repo).is_empty(), "and still forwarded");
}

#[test]
fn the_fake_executor_reports_an_unstage() {
    let fake = FakeExecutor::default();
    fake.unstage(Path::new("/repo"), &[PathBuf::from("base.txt")])
        .unwrap();

    assert!(
        fake.calls
            .lock()
            .unwrap()
            .iter()
            .any(|c| matches!(c, Call::Unstage(p) if p == &[PathBuf::from("base.txt")]))
    );
}
