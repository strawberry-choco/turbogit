//! Per-file line counts for one commit (`GitExecutor::commit_file_stats`,
//! issue 01 of the logs-panels redesign): the changed-files pane's `+N −M`
//! column and the details pane's churn bar both read from this payload.
//!
//! Both real adapters are pinned against the same fixture repository, so the
//! libgit2 backend cannot drift from the CLI's `git diff-tree --numstat` —
//! and the rename rows are asserted to carry exactly the paths `commit_files`
//! reports, which is what lets the pane join the two lists by path.

use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;
use turbogit_domain::model::VcsSettings;
use turbogit_engine::GitExecutor;
use turbogit_engine::cli::CliExecutor;
use turbogit_engine::git2_exec::Git2Executor;

fn run_git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git should be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A repository whose second commit exercises every numstat row shape at once:
/// an edit (`notes.txt`), a deletion (`gone.txt`), a pure rename
/// (`keep.txt` → `kept.txt`), and — in the root commit — a binary blob.
fn seeded_repo() -> (TempDir, CliExecutor, PathBuf, String, String) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("stats");
    std::fs::create_dir_all(&repo).expect("repo dir");
    run_git(&repo, &["init", "-q", "-b", "main"]);
    run_git(&repo, &["config", "user.email", "stats@example.com"]);
    run_git(&repo, &["config", "user.name", "Stats Author"]);
    run_git(&repo, &["config", "core.autocrlf", "false"]);
    run_git(&repo, &["config", "diff.noprefix", "false"]);

    std::fs::write(repo.join("notes.txt"), "a\nb\nc\n").expect("write notes");
    std::fs::write(repo.join("gone.txt"), "temp\n").expect("write gone");
    std::fs::write(repo.join("keep.txt"), "stable\ncontent\n").expect("write keep");
    // Not valid UTF-8: git classifies this as binary and reports `-` `-`.
    std::fs::write(repo.join("logo.png"), [0x89, b'P', 0x00, 0xFF, 0xFE]).expect("write logo");
    run_git(&repo, &["add", "--", "."]);
    run_git(&repo, &["commit", "-q", "-m", "root commit"]);
    let root = run_git(&repo, &["rev-parse", "HEAD"]).trim().to_string();

    std::fs::write(repo.join("notes.txt"), "a\nX\nc\nd\n").expect("edit notes");
    std::fs::remove_file(repo.join("gone.txt")).expect("remove gone");
    run_git(&repo, &["mv", "keep.txt", "kept.txt"]);
    run_git(&repo, &["add", "--", "."]);
    run_git(&repo, &["commit", "-q", "-m", "edit, delete and rename"]);
    let head = run_git(&repo, &["rev-parse", "HEAD"]).trim().to_string();

    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };
    (tmp, engine, repo, root, head)
}

/// `(path, insertions, deletions)` sorted by path — git orders rename rows by
/// their old side, which is not the order the pane shows.
fn sorted(mut stats: Vec<(PathBuf, usize, usize)>) -> Vec<(String, usize, usize)> {
    stats.sort();
    stats
        .into_iter()
        .map(|(path, ins, dels)| (path.to_string_lossy().replace('\\', "/"), ins, dels))
        .collect()
}

#[test]
fn cli_reports_per_file_counts_for_every_row_shape() {
    let (_tmp, engine, repo, _root, head) = seeded_repo();

    let stats = sorted(engine.commit_file_stats(&repo, &head).expect("stats"));
    assert_eq!(
        stats,
        vec![
            ("gone.txt".to_string(), 0, 1),
            // A pure rename changes no lines; its counts are real zeros.
            ("kept.txt".to_string(), 0, 0),
            ("notes.txt".to_string(), 2, 1),
        ]
    );
}

#[test]
fn cli_reports_binary_rows_as_zero_counts_instead_of_dropping_them() {
    let (_tmp, engine, repo, root, _head) = seeded_repo();

    let stats = sorted(engine.commit_file_stats(&repo, &root).expect("stats"));
    assert_eq!(
        stats,
        vec![
            ("gone.txt".to_string(), 1, 0),
            ("keep.txt".to_string(), 2, 0),
            ("logo.png".to_string(), 0, 0),
            ("notes.txt".to_string(), 3, 0),
        ]
    );
}

/// The pane joins the counts onto `commit_files` by path — a rename reported
/// under its old side here would silently lose its numbers.
#[test]
fn stats_paths_are_the_paths_commit_files_reports() {
    let (_tmp, engine, repo, _root, head) = seeded_repo();

    let mut files: Vec<String> = engine
        .commit_files(&repo, &head)
        .expect("files")
        .iter()
        .map(|c| c.path.to_string_lossy().replace('\\', "/"))
        .collect();
    files.sort();
    let mut stats: Vec<String> = engine
        .commit_file_stats(&repo, &head)
        .expect("stats")
        .iter()
        .map(|(p, _, _)| p.to_string_lossy().replace('\\', "/"))
        .collect();
    stats.sort();
    assert_eq!(files, stats);
}

#[test]
fn git2_reports_the_same_counts_as_the_cli() {
    let (_tmp, cli, repo, root, head) = seeded_repo();
    let git2 = Git2Executor::new(cli);

    for commit in [&root, &head] {
        let expected = sorted(
            git2.cli
                .commit_file_stats(&repo, commit)
                .expect("cli stats"),
        );
        let actual = sorted(git2.commit_file_stats(&repo, commit).expect("git2 stats"));
        assert_eq!(actual, expected, "backend drift for commit {commit}");
    }
}
