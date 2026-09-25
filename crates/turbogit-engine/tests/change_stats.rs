//! Per-file change counts (`GitExecutor::change_stats`, logs-panels redesign
//! issue 01 + deepen-git-engine issue 06): the changed-files pane's `+N −M`
//! column, the details pane's churn bar, and the integration preview's
//! "N files · +X −Y" all read from this one question.
//!
//! Both real adapters are pinned against the same fixture repository, so the
//! libgit2 backend cannot drift from the CLI's `git diff-tree --numstat`, and
//! the two dash answers cannot drift either. The merge question is pinned
//! against what git itself reports, because that half's acceptance *is* the
//! numbers matching git.

use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;
use turbogit_domain::model::{ChangeQuestion, ChangeStats, VcsSettings};
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
/// their old side, which is not the order the pane shows. `None` is git's dash.
fn sorted(stats: ChangeStats) -> Vec<(String, Option<usize>, Option<usize>)> {
    let mut rows: Vec<_> = stats
        .files
        .into_iter()
        .map(|f| {
            (
                f.path.to_string_lossy().replace('\\', "/"),
                f.insertions,
                f.deletions,
            )
        })
        .collect();
    rows.sort();
    rows
}

fn commit_stats(commit: &str) -> ChangeQuestion {
    ChangeQuestion::Commit {
        commit: commit.to_string(),
    }
}

#[test]
fn cli_reports_per_file_counts_for_every_row_shape() {
    let (_tmp, engine, repo, _root, head) = seeded_repo();

    let stats = sorted(
        engine
            .change_stats(&repo, &commit_stats(&head))
            .expect("stats"),
    );
    assert_eq!(
        stats,
        vec![
            ("gone.txt".to_string(), Some(0), Some(1)),
            // A pure rename changes no lines; its counts are real zeros.
            ("kept.txt".to_string(), Some(0), Some(0)),
            ("notes.txt".to_string(), Some(2), Some(1)),
        ]
    );
}

#[test]
fn cli_reports_a_binary_row_as_having_no_line_counts_rather_than_zero() {
    let (_tmp, engine, repo, root, _head) = seeded_repo();

    let stats = sorted(
        engine
            .change_stats(&repo, &commit_stats(&root))
            .expect("stats"),
    );
    assert_eq!(
        stats,
        vec![
            ("gone.txt".to_string(), Some(1), Some(0)),
            ("keep.txt".to_string(), Some(2), Some(0)),
            ("logo.png".to_string(), None, None,),
            ("notes.txt".to_string(), Some(3), Some(0)),
        ]
    );
    let all = engine
        .change_stats(&repo, &commit_stats(&root))
        .expect("stats");
    assert_eq!(
        (all.file_count(), all.insertions(), all.deletions()),
        (4, 6, 0),
        "the binary file counts as a file touched and contributes no lines — \
         the one place git's dash becomes a number"
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
        .change_stats(&repo, &commit_stats(&head))
        .expect("stats")
        .files
        .iter()
        .map(|f| f.path.to_string_lossy().replace('\\', "/"))
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
                .change_stats(&repo, &commit_stats(commit))
                .expect("cli stats"),
        );
        let actual = sorted(
            git2.change_stats(&repo, &commit_stats(commit))
                .expect("git2 stats"),
        );
        assert_eq!(actual, expected, "backend drift for commit {commit}");
    }
}

/// The other caller's question, kept git-backed because its acceptance is that
/// the preview's numbers are git's: what merging `feature` into `main` brings
/// in is the merge base to `feature`, not the whole distance between the tips.
#[test]
fn the_merge_question_counts_what_the_merge_would_bring_in() {
    let (_tmp, engine, repo, _root, _head) = seeded_repo();
    run_git(&repo, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(repo.join("feature.txt"), "one\ntwo\n").expect("write feature");
    run_git(&repo, &["add", "feature.txt"]);
    run_git(&repo, &["commit", "-q", "-m", "feature work"]);
    run_git(&repo, &["checkout", "-q", "main"]);
    std::fs::write(repo.join("notes.txt"), "a\nX\nc\nd\nmain moved on\n").expect("edit main");
    run_git(&repo, &["commit", "-q", "-am", "main moved on"]);

    let stats = engine
        .change_stats(
            &repo,
            &ChangeQuestion::MergeIntoHead {
                target: "feature".to_string(),
            },
        )
        .expect("merge stats");
    assert_eq!(
        sorted(stats.clone()),
        vec![("feature.txt".to_string(), Some(2), Some(0))],
        "only what the merge brings in, not what main did meanwhile"
    );

    // Independent of the adapter: git's own three-dot numstat, unparsed here.
    let git_report = run_git(&repo, &["diff", "--numstat", "HEAD...feature"]);
    assert_eq!(
        git_report.trim(),
        "2\t0\tfeature.txt",
        "the fixture's expected numbers come from git, not from this read"
    );
    assert_eq!(
        (stats.file_count(), stats.insertions(), stats.deletions()),
        (1, 2, 0)
    );
}
