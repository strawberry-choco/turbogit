//! The reflog read behind the Branches surface's Recent group.
//!
//! The Recent node is derived, not bookkept: it is HEAD's own checkout history,
//! so it survives an app restart and it shows a checkout made in a terminal.
//! Expected values here come from the fixture's own checkout order — an
//! independent source of truth — and both adapters are driven, because the
//! surface cannot know which engine the user configured.

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

/// A repository on `main` with `a`, `b` and `c`, checked out in that order, so
/// the newest entry is the move onto `c` — then one more commit, a reflog entry
/// that is not a checkout and must not reach the answer.
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
    // The moves the fixture made, newest first. `main` is not among them
    // because nothing ever moved *onto* it, and the two commits are not
    // checkouts at all.
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

    // An unborn HEAD has no reflog to read, and git says so with a non-zero
    // exit. The port's answer is the empty history: "nothing has been checked
    // out yet" is a normal state of a fresh repository, and a surface asked to
    // read it would otherwise have to re-derive that from an error string.
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
    // Absence of history is an answer; absence of a repository is not — repo
    // discovery depends on the two staying distinguishable.
    assert!(cli().recent_checkouts(tmp.path()).is_err());
}
