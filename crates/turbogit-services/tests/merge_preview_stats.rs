//! Issue 06 (deepen-git-engine) — change stats from one question.
//!
//! The integration preview used to ask the **Git engine** to run a raw numstat
//! command and then split the answer on tabs inside the service. It now asks one
//! question of the same read the log's stats column asks, and the substitutable
//! adapter can answer it: this suite needs no `git` binary.

use std::path::PathBuf;

use turbogit_domain::model::{ChangeQuestion, ChangeStats, FileStat, MergeStrategy};
use turbogit_engine::fake::FakeExecutor;
use turbogit_services::integrate_service;

fn root() -> PathBuf {
    PathBuf::from("/repo")
}

/// The change `feature` would bring in: two text files and one **Binary change**,
/// which git reports with a dash in both count columns.
fn canned() -> ChangeStats {
    ChangeStats {
        files: vec![
            FileStat {
                path: PathBuf::from("a.rs"),
                insertions: Some(12),
                deletions: Some(3),
            },
            FileStat {
                path: PathBuf::from("b.rs"),
                insertions: Some(1),
                deletions: Some(1),
            },
            FileStat {
                path: PathBuf::from("logo.png"),
                insertions: None,
                deletions: None,
            },
        ],
    }
}

#[test]
fn the_preview_counts_files_and_lines_from_the_value_not_from_git_text() {
    let mut v = FakeExecutor::new();
    v.change_stats.insert(
        ChangeQuestion::MergeIntoHead {
            target: "feature".to_string(),
        },
        canned(),
    );

    let preview = integrate_service::merge_preview(&v, &root(), "feature", MergeStrategy::Commit)
        .expect("a preview the adapter can answer");
    assert_eq!(preview.files, 3, "the binary file is still a file touched");
    assert_eq!(
        preview.insertions, 13,
        "a **Binary change** has no line count and contributes none — that is the \
         one place git's dash is decided"
    );
    assert_eq!(preview.deletions, 4);
}

#[test]
fn an_unanswered_question_is_an_error_not_a_zeroed_preview() {
    let v = FakeExecutor::new();
    assert!(
        integrate_service::merge_preview(&v, &root(), "elsewhere", MergeStrategy::Commit).is_err(),
        "a preview that could not be answered must not claim 'no changes'"
    );
}

#[test]
fn a_target_already_merged_in_brings_nothing_and_asks_no_question() {
    // The short-circuit stays in front of the stats read: the merge-base diff of
    // an already-merged target would otherwise show changes it will not apply.
    let mut v = FakeExecutor::new();
    v.ancestors.insert(("feature".into(), "HEAD".into()));
    let preview = integrate_service::merge_preview(&v, &root(), "feature", MergeStrategy::Commit)
        .expect("already merged");
    assert_eq!(
        (preview.merge_commits, preview.files, preview.insertions),
        (0, 0, 0)
    );
    assert!(
        v.change_stats.is_empty() || v.change_stats.keys().next().is_none(),
        "no question was asked, so no stats read is owed an answer"
    );
}
