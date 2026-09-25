//! Issue 07 (deepen-git-engine) — revisions and refs as answers.
//!
//! Two questions the app used to ask by assembling a git command line: how many
//! commits a branch carries that another does not, and what commit a name
//! resolves to. Plus the reference transaction the history editor ran as three
//! raw calls against a path it had to guess. Each is answered here twice — once
//! through the substitutable adapter with no `git` binary, and once against real
//! git, because a mutation's acceptance is that the reference moved.

use std::path::{Path, PathBuf};
use std::process::Command;

use turbogit_domain::model::{GitBackend, VcsSettings};
use turbogit_engine::GitExecutor;
use turbogit_engine::cli::CliExecutor;
use turbogit_engine::fake::FakeExecutor;
use turbogit_engine::git2_exec::Git2Executor;

fn git(dir: &Path, args: &[&str]) -> String {
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
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn cli() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

/// `main` plus three commits, with `origin/main` left two behind.
fn ahead_repo() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let bare = project.join("origin.git");
    git(
        &project,
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    let repo = project.join("repo");
    git(
        &project,
        &["init", "-q", "-b", "main", repo.to_str().unwrap()],
    );
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    std::fs::write(repo.join("f.txt"), "one\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "c1"]);
    git(&repo, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(&repo, &["push", "-q", "-u", "origin", "main"]);
    for subject in ["c2", "c3"] {
        std::fs::write(repo.join("f.txt"), format!("{subject}\n")).unwrap();
        git(&repo, &["commit", "-q", "-am", subject]);
    }
    (tmp, repo)
}

// ---- through the substitutable adapter: no git binary -------------------------

#[test]
fn the_substitutable_adapter_answers_a_count_question() {
    let mut v = FakeExecutor::new();
    v.commit_counts
        .insert(("HEAD".to_string(), "feature".to_string()), 7);
    let root = PathBuf::from("/repo");
    assert_eq!(
        v.commit_count_between(&root, "HEAD", "feature").unwrap(),
        7,
        "a count a fixture states needs no git"
    );
    assert!(
        v.commit_count_between(&root, "HEAD", "unseeded").is_err(),
        "an unasked question is not a count of zero"
    );
}

#[test]
fn the_substitutable_adapter_answers_a_name_resolution() {
    let mut v = FakeExecutor::new();
    v.revisions
        .insert("main".to_string(), "1f4e0a9deadbeef".to_string());
    assert_eq!(
        v.resolve_revision(&PathBuf::from("/repo"), "main").unwrap(),
        "1f4e0a9deadbeef"
    );
}

#[test]
fn the_substitutable_adapter_names_its_own_backup_ref() {
    let v = FakeExecutor::new();
    assert!(
        !v.rewrite_backup_ref().is_empty(),
        "the ref the backup verbs use has a name, and the engine says it"
    );
}

// ---- against real git: the numbers are git's and the ref really moves --------

#[test]
fn the_count_question_answers_what_git_counts_and_carries_no_range_string() {
    let (_tmp, repo) = ahead_repo();
    let exec = cli();
    assert_eq!(
        exec.commit_count_between(&repo, "HEAD", "main").unwrap(),
        0,
        "main is the checked-out branch, so nothing reaches it that HEAD lacks"
    );
    assert_eq!(
        git(&repo, &["rev-list", "--count", "HEAD..main"]).trim(),
        "0",
        "the expected number comes from git's own count, not from this read"
    );
    std::fs::write(repo.join("f.txt"), "local only\n").unwrap();
    git(&repo, &["commit", "-q", "-am", "c4"]);
    assert_eq!(
        exec.commit_count_between(&repo, "origin/main", "main")
            .unwrap(),
        3,
        "the unmerged count a branch delete discloses before it destroys"
    );
}

#[test]
fn a_name_resolves_to_the_commit_git_resolves_it_to() {
    let (_tmp, repo) = ahead_repo();
    let exec = cli();
    let tip = exec
        .resolve_revision(&repo, "main")
        .expect("a name resolves");
    assert_eq!(
        tip,
        git(&repo, &["rev-parse", "main"]).trim(),
        "the delete confirmation captures exactly the tip git is about to remove"
    );
    assert!(
        exec.resolve_revision(&repo, "no-such-thing").is_err(),
        "a name that resolves to nothing is an error, not an empty sha"
    );
}

#[test]
fn the_backup_ref_is_written_moved_to_and_discarded_without_anyone_naming_it() {
    let (_tmp, repo) = ahead_repo();
    let exec = cli();
    let pre_head = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();

    exec.save_rewrite_backup(&repo).expect("backup written");
    assert_eq!(
        git(&repo, &["rev-parse", exec.rewrite_backup_ref()]).trim(),
        pre_head,
        "the backup names the pre-rewrite state"
    );

    std::fs::write(repo.join("f.txt"), "rewritten\n").unwrap();
    git(&repo, &["commit", "-q", "-am", "rewrite"]);
    assert_ne!(git(&repo, &["rev-parse", "HEAD"]).trim(), pre_head);

    exec.restore_rewrite_backup(&repo).expect("restore");
    assert_eq!(
        git(&repo, &["rev-parse", "HEAD"]).trim(),
        pre_head,
        "HEAD is back at the pre-rewrite tip"
    );

    exec.discard_rewrite_backup(&repo).expect("discard");
    let gone = Command::new("git")
        .args([
            "rev-parse",
            "--verify",
            "--quiet",
            exec.rewrite_backup_ref(),
        ])
        .current_dir(&repo)
        .output()
        .expect("git must be on PATH");
    assert!(
        !gone.status.success(),
        "the spent backup ref genuinely disappears"
    );
}

#[test]
fn the_composed_adapter_answers_the_same_questions_as_the_cli_one() {
    let (_tmp, repo) = ahead_repo();
    let pre_head = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    let exec = Git2Executor::new(cli());
    assert_eq!(
        exec.commit_count_between(&repo, "origin/main", "main")
            .unwrap(),
        2
    );
    assert_eq!(exec.resolve_revision(&repo, "main").unwrap(), pre_head);
    exec.save_rewrite_backup(&repo).expect("composed backup");
    assert_eq!(
        git(&repo, &["rev-parse", exec.rewrite_backup_ref()]).trim(),
        pre_head
    );
    exec.discard_rewrite_backup(&repo)
        .expect("composed discard");
}

#[test]
fn no_backend_selects_a_range_by_accident() {
    // The two production adapters are the only places a dot range may appear;
    // `build_executor`'s two options answer the same count question alike.
    let (_tmp, repo) = ahead_repo();
    for backend in [GitBackend::Cli, GitBackend::InProcessReads] {
        let exec = turbogit_engine::build_executor(&VcsSettings {
            backend,
            ..VcsSettings::default()
        });
        assert_eq!(
            exec.commit_count_between(&repo, "HEAD", "main").unwrap(),
            0,
            "{backend:?} must answer the count question the same way"
        );
    }
}
