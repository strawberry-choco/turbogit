//! The unmerged-commit list a branch delete discloses has to name every commit
//! it lists: a row whose subject is missing is a row that says nothing about
//! what the delete destroyed.
//!
//! The shas and the subjects must come from the SAME commit set. The ids are a
//! range (`HEAD..branch`); a subject lookup over the branch's recent history is
//! not the same set at all, because a branch that merged the current branch
//! back into itself reaches the shared commits too, and `git log` orders by
//! commit date — so a shared commit newer than one of the outgoing commits
//! takes its slot in the listing and leaves the row blank.
//!
//! The fixture pins committer dates for exactly that interleaving; a fixture
//! built at wall-clock speed would put every outgoing commit ahead of every
//! shared one and pass for the wrong reason.

use std::path::{Path, PathBuf};
use std::process::Command;

use test_support::git_seed::{commit, git, repo_with_one_commit};
use turbogit_app::state::AppState;

/// `git <args>` in `dir` with the committer (and author) date forced to
/// `date`, so the log order the fixture produces is the one it names.
fn git_dated(dir: &Path, date: &str, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_DATE", format!("{date}T12:00:00 +0000"))
        .env("GIT_COMMITTER_DATE", format!("{date}T12:00:00 +0000"))
        .output()
        .expect("git must be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} at {date} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn rev(dir: &Path, r: &str) -> String {
    git(dir, &["rev-parse", r]).trim().to_string()
}

/// A branch whose unmerged set is interleaved by date with commits the current
/// branch already has:
///
/// ```text
/// main     C1 ───────────────────── C2      (2020-01-04)
///            ╲                         ╲
/// doomed      ─── O1 ─────────────────── M   (O1 2020-01-03, M 2020-01-05)
/// ```
///
/// `M` is the merge of `main` back into `doomed`, so `doomed` reaches `C2` as
/// well as `O1`. Only `O1` and `M` are unmerged, and `C2` is newer than `O1` —
/// which is what crowds `O1` out of a recent-history listing capped at two.
/// HEAD stays on `main`, the branch a delete of `doomed` is unmerged *into*.
fn doomed_with_interleaved_history(project: &Path) -> PathBuf {
    let work = repo_with_one_commit(project, "alpha");
    // C1 — the shared base, oldest of them all.
    git_dated(
        &work,
        "2020-01-01",
        &["commit", "--amend", "-q", "-m", "base"],
    );
    commit(&work, "main-side.txt", "main\n", "main work");
    git_dated(
        &work,
        "2020-01-04",
        &["commit", "--amend", "-q", "--no-edit"],
    );
    let main_tip = rev(&work, "HEAD");

    git(&work, &["checkout", "-q", "-b", "doomed", "HEAD~1"]);
    commit(&work, "doomed-side.txt", "doomed\n", "the doomed commit");
    git_dated(
        &work,
        "2020-01-03",
        &["commit", "--amend", "-q", "--no-edit"],
    );

    git(
        &work,
        &["merge", "-q", "--no-ff", &main_tip, "-m", "merge main"],
    );
    git_dated(
        &work,
        "2020-01-05",
        &["commit", "--amend", "-q", "--no-edit"],
    );

    git(&work, &["checkout", "-q", "main"]);
    work
}

/// Every commit a delete of `doomed` discloses carries the subject it was
/// committed with — no blank rows, whatever the branch's date order is.
#[test]
fn every_unmerged_commit_a_delete_discloses_has_its_subject() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let work = doomed_with_interleaved_history(&project);
    let state = AppState::for_roots(&project, std::slice::from_ref(&work));
    let root = state.multi.roots[0].id.clone();

    let disclosed = state
        .unmerged_commits(&root, "doomed")
        .expect("doomed is ahead of main, so a delete discloses its commits");

    // The range, not the recent history: `O1` and the merge `M`, and NOT the
    // shared `main work` commit `M` also reaches.
    assert_eq!(
        disclosed
            .commits
            .iter()
            .map(|c| c.subject.clone())
            .collect::<Vec<_>>(),
        vec![
            Some("merge main".to_string()),
            Some("the doomed commit".to_string()),
        ],
        "every disclosed commit is named, newest first"
    );
    assert_eq!(
        disclosed.commits.len(),
        2,
        "the shared commit is not unmerged and must not appear"
    );
    assert!(
        !disclosed
            .commits
            .iter()
            .any(|c| c.subject.as_deref() == Some("main work")),
        "the listing is the unmerged set, not the branch's recent history"
    );
}

/// The same read on a branch nothing merged back into it — the ordinary shape,
/// which the fix must not disturb.
#[test]
fn a_plain_ahead_branch_still_discloses_both_of_its_commits() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let work = repo_with_one_commit(&project, "alpha");
    git(&work, &["checkout", "-q", "-b", "doomed"]);
    commit(&work, "one.txt", "1\n", "first doomed");
    commit(&work, "two.txt", "2\n", "second doomed");
    git(&work, &["checkout", "-q", "main"]);

    let state = AppState::for_roots(&project, std::slice::from_ref(&work));
    let root = state.multi.roots[0].id.clone();
    let disclosed = state
        .unmerged_commits(&root, "doomed")
        .expect("two commits ahead of main");

    assert_eq!(
        disclosed
            .commits
            .iter()
            .map(|c| c.subject.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("second doomed"), Some("first doomed")],
        "newest first, both named"
    );
}
