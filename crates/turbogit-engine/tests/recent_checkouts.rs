use std::path::Path;

use test_support::git_seed::{commit, git};
use turbogit_domain::model::{RecentCheckout, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_engine::git2_exec::Git2Executor;
use turbogit_engine_api::GitExecutor;

fn cli() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

fn three_checkouts(dir: &Path) {
    git(dir, &["config", "user.email", "t@example.com"]);
    git(dir, &["config", "user.name", "t"]);
    commit(dir, "f.txt", "one\n", "seed");
    for b in ["a", "b", "c"] {
        git(dir, &["branch", b]);
    }
    for b in ["a", "b", "c"] {
        git(dir, &["checkout", "-q", b]);
    }
    commit(dir, "f.txt", "two\n", "on c");
}

#[test]
fn checkout_history_answers_newest_first_and_skips_other_reflog_entries() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    three_checkouts(&repo);

    let out = cli().recent_checkouts(&repo).expect("reflog read");
    let names: Vec<&str> = out.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["c", "b", "a"],
        "checkout moves only, newest first"
    );
    assert!(
        out.windows(2).all(|w| w[0].at >= w[1].at),
        "timestamps must not run backwards: {out:?}"
    );
}

#[test]
fn both_backends_answer_the_same_checkout_history() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    three_checkouts(&repo);

    let engine = cli();
    let git2 = Git2Executor::new(CliExecutor {
        settings: VcsSettings::default(),
    });
    assert_eq!(
        engine.recent_checkouts(&repo).unwrap(),
        git2.recent_checkouts(&repo).unwrap(),
        "the two engines agree, whatever the backend setting picks"
    );
}

#[test]
fn a_repo_with_no_commits_yet_answers_empty_not_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);

    let empty: Vec<RecentCheckout> = Vec::new();
    let engine = cli();
    assert_eq!(
        engine.recent_checkouts(&repo).unwrap(),
        empty,
        "an unborn repo answers the empty history"
    );
    let git2 = Git2Executor::new(CliExecutor {
        settings: VcsSettings::default(),
    });
    assert_eq!(git2.recent_checkouts(&repo).unwrap(), empty);
}

#[test]
fn a_directory_that_is_not_a_repository_still_fails() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(cli().recent_checkouts(tmp.path()).is_err());
}
