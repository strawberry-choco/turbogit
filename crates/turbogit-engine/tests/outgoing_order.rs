//! The outgoing list promises an ORDER, not only membership: the push dialog
//! reads its commit rows straight off it, and `cli::outgoing_commits` documents
//! `git rev-list upstream..branch` as newest-first. The composed adapter answers
//! this read in-process, so the promise is pinned on both sides against the same
//! merge-shaped history.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use test_support::git_seed::{GitEnv, git, git_with_env, repo_with_origin};
use turbogit_domain::model::VcsSettings;
use turbogit_engine::cli::CliExecutor;
use turbogit_engine::{GitExecutor, build_executor};

/// Author and committer dates, so a listing's order is a fact about the fixture
/// rather than about the clock it ran under. The side line is OLDER than the
/// first-parent commit sitting beside it, which is the shape an unsorted walk is
/// free to get wrong.
fn dated(iso: &str) -> GitEnv {
    GitEnv {
        author_date: Some(iso.to_string()),
        committer_date: Some(iso.to_string()),
        ..GitEnv::fixture()
    }
}

fn commit_dated(repo: &Path, name: &str, iso: &str) -> String {
    let env = dated(iso);
    std::fs::write(repo.join(format!("{name}.txt")), format!("{name}\n")).expect("writing");
    git_with_env(repo, &["add", "."], &env);
    git_with_env(repo, &["commit", "-q", "-m", name], &env);
    git(repo, &["rev-parse", "HEAD"]).trim().to_string()
}

fn executors() -> Vec<(&'static str, Arc<dyn GitExecutor>)> {
    vec![
        (
            "cli",
            Arc::new(CliExecutor {
                settings: VcsSettings::default(),
            }),
        ),
        ("composed", build_executor(&VcsSettings::default())),
    ]
}

/// `main` ahead of `origin/main` by a merge, its first-parent commit, and an
/// older two-commit line reached only through the merge's second parent.
fn merged_and_ahead() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = repo_with_origin(tmp.path(), "repo");
    git(&repo, &["checkout", "-q", "-b", "side"]);
    commit_dated(&repo, "side1", "2024-01-01T10:00:00+00:00");
    commit_dated(&repo, "side2", "2024-01-01T10:30:00+00:00");
    git(&repo, &["checkout", "-q", "main"]);
    commit_dated(&repo, "mid", "2024-01-01T11:00:00+00:00");
    git_with_env(
        &repo,
        &["merge", "-q", "--no-edit", "-m", "merge", "side"],
        &dated("2024-01-01T12:00:00+00:00"),
    );
    (tmp, repo)
}

fn rev(repo: &Path, spec: &str) -> String {
    git(repo, &["rev-parse", spec]).trim().to_string()
}

#[test]
fn the_outgoing_list_is_newest_first_under_both_adapters() {
    let (_tmp, repo) = merged_and_ahead();
    let expected = vec![
        rev(&repo, "main"),
        rev(&repo, "main~1"),
        rev(&repo, "main^2"),
        rev(&repo, "main^2~1"),
    ];

    for (name, exec) in executors() {
        let got = exec
            .outgoing_commits(&repo, "main", "origin/main")
            .unwrap_or_else(|e| panic!("{name} adapter could not list outgoing: {e}"));
        assert_eq!(
            got, expected,
            "the {name} adapter must list the tip first, its first-parent work \
             next, and the older second-parent line last"
        );
    }
}

#[test]
fn both_adapters_answer_the_same_order_as_git_rev_list() {
    let (_tmp, repo) = merged_and_ahead();
    let verbatim: Vec<String> = git(&repo, &["rev-list", "origin/main..main"])
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect();
    assert_eq!(verbatim.len(), 4, "the fixture lists four commits");

    for (name, exec) in executors() {
        let got = exec
            .outgoing_commits(&repo, "main", "origin/main")
            .unwrap_or_else(|e| panic!("{name} adapter could not list outgoing: {e}"));
        assert_eq!(
            got, verbatim,
            "the {name} adapter disagrees with git itself"
        );
    }
}
