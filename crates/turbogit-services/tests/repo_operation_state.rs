//! Ticket 11 — the repository's owning operation, read from git's own markers.
//!
//! Gating a rewrite verb on repository state is only as good as the state read,
//! so this suite drives **real git** into each in-flight state and asks the
//! service who owns the repository. Two properties matter and neither is
//! obvious from the marker names alone:
//!
//! - **one owner, named**: each state leaves its own marker (measured on git
//!   2.50: `CHERRY_PICK_HEAD`, `REVERT_HEAD`, `MERGE_HEAD`, `rebase-merge` /
//!   `rebase-apply`), and a refusal that says only "busy" cannot be acted on.
//!   The two overlaps the reader has to rank — a sequence stopped on a pick, a
//!   pick resolved by the merge machinery — are built by hand, because a
//!   healthy git of one version does not produce them.
//! - **per worktree**: a linked worktree has a `.git` FILE pointing at the
//!   directory that holds HEAD and the markers. Reading `<worktree>/.git/…`
//!   reports every worktree idle, which is how a multi-root app comes to rewrite
//!   inside a repository that is mid-operation.
//!
//! [`integrate_service::in_progress`] is asserted too — it is deliberately NOT
//! the wider question, and the difference is a caller's recovery path.

use std::path::Path;

use test_support::git_seed::{commit, git, repo_with_conflict, repo_with_one_commit};
use turbogit_services::integrate_service::{OwnedOperation, in_progress, owning_operation};

/// The line two branches rewrite differently, so whichever second operation is
/// started from either side stops on a conflict and stays in flight.
fn diverged_repo(tmp: &Path) -> std::path::PathBuf {
    let repo = repo_with_one_commit(tmp, "repo");
    git(&repo, &["checkout", "-q", "-b", "side"]);
    commit(&repo, "conf.txt", "side\n", "conf: side");
    git(&repo, &["checkout", "-q", "main"]);
    commit(&repo, "conf.txt", "main\n", "conf: main");
    repo
}

/// Run `git` expecting it to fail: the refusal that stops the operation mid-flight
/// is the fixture, not a problem with it.
fn git_expected_failure(repo: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git on PATH");
    assert!(
        !out.status.success(),
        "`git {args:?}` was expected to stop the operation mid-flight: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn a_clean_repository_has_no_owner() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = diverged_repo(tmp.path());
    assert_eq!(owning_operation(&repo), None, "nothing is in flight");
    assert!(!in_progress(&repo));
}

/// `git merge` of the diverged side: `MERGE_HEAD` alone, so the merge is the
/// owner and nothing else answers.
#[test]
fn a_held_merge_names_the_merge() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo_with_conflict(tmp.path(), "repo");
    assert_eq!(
        owning_operation(&repo),
        Some(OwnedOperation::Merge),
        "the refusal must name the merge, not just say busy"
    );
    assert!(in_progress(&repo));
}

/// `git cherry-pick` of the diverged commit holds `CHERRY_PICK_HEAD`. Measured
/// against git 2.50, that is the only marker a held pick leaves — the four
/// states are disjoint, which is what lets a refusal name the owner.
#[test]
fn a_held_cherry_pick_names_the_pick() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = diverged_repo(tmp.path());
    let side_tip = git(&repo, &["rev-parse", "side"]).trim().to_string();
    git_expected_failure(&repo, &["cherry-pick", &side_tip]);
    assert!(
        repo.join(".git").join("CHERRY_PICK_HEAD").exists(),
        "the marker a held pick leaves is the one the reader asks for"
    );
    assert_eq!(owning_operation(&repo), Some(OwnedOperation::CherryPick));
    assert!(in_progress(&repo));
}

/// The precedence the reader is ordered by, on a state git 2.50 does not produce
/// on its own: a sequence stopped on a held pick. The pick is the machinery the
/// rebase is using, not the operation the developer started, so the rebase owns
/// the repository and that is what the refusal must say.
#[test]
fn a_rebase_directory_outranks_the_pick_it_is_stopped_on() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = diverged_repo(tmp.path());
    std::fs::create_dir_all(repo.join(".git/rebase-merge")).unwrap();
    std::fs::write(repo.join(".git/CHERRY_PICK_HEAD"), "deadbeef\n").unwrap();
    std::fs::write(repo.join(".git/MERGE_HEAD"), "deadbeef\n").unwrap();
    assert_eq!(
        owning_operation(&repo),
        Some(OwnedOperation::Rebase),
        "one owner, and it is the sequence that was started"
    );
}

/// Same question one step down: a pick held while the merge machinery resolves
/// it is still the pick the developer started.
#[test]
fn a_pick_outranks_the_merge_machinery_resolving_it() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = diverged_repo(tmp.path());
    std::fs::write(repo.join(".git/CHERRY_PICK_HEAD"), "deadbeef\n").unwrap();
    std::fs::write(repo.join(".git/MERGE_HEAD"), "deadbeef\n").unwrap();
    assert_eq!(owning_operation(&repo), Some(OwnedOperation::CherryPick));
}

/// `git revert` of a commit that no longer applies writes `REVERT_HEAD`, the
/// fourth owner — and it is the one [`in_progress`] deliberately does not claim.
#[test]
fn a_held_revert_names_the_revert() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = diverged_repo(tmp.path());
    let side_tip = git(&repo, &["rev-parse", "side"]).trim().to_string();
    // Reverting side's commit from main touches the same line main rewrote.
    git_expected_failure(&repo, &["revert", "--no-edit", &side_tip]);
    assert_eq!(owning_operation(&repo), Some(OwnedOperation::Revert));
    assert!(
        !in_progress(&repo),
        "in_progress stays the three states its one recovery caller was written \
         for: it is used to decide whether to run `git rebase --abort`, and a \
         revert abort on top of a held revert is a second failure"
    );
}

/// `git rebase` stopped on a conflict holds its sequence directory, and the
/// reader names the rebase — the operation the developer started — whatever
/// else is in flight around it.
#[test]
fn a_held_rebase_names_the_rebase() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = diverged_repo(tmp.path());
    git(&repo, &["checkout", "-q", "side"]);
    git_expected_failure(&repo, &["rebase", "main"]);
    let state = owning_operation(&repo);
    assert_eq!(state, Some(OwnedOperation::Rebase));
    assert!(
        in_progress(&repo),
        "a rebase in flight is in progress for every caller that already asked"
    );
}

/// A linked worktree keeps its markers behind a `.git` FILE, so the state read
/// must follow the pointer. Without it every worktree of a repository reads as
/// idle and a rewrite is allowed inside a repository that is mid-operation.
#[test]
fn a_worktree_s_state_is_read_from_the_directory_its_git_file_points_at() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = diverged_repo(tmp.path());
    let worktree = tmp.path().join("wt");
    git(&repo, &["branch", "wt-side", "side"]);
    // The two-step form is spelled out because `worktree add` with a path and a
    // new branch name behaves differently across git versions; what matters here
    // is that the checkout at `worktree` has a `.git` pointer file.
    let added = std::process::Command::new("git")
        .args([
            "worktree",
            "add",
            "-q",
            "--detach",
            worktree.to_str().unwrap(),
            "wt-side",
        ])
        .current_dir(&repo)
        .output()
        .expect("git on PATH");
    assert!(
        added.status.success(),
        "worktree add: {}",
        String::from_utf8_lossy(&added.stderr)
    );
    assert!(
        worktree.join(".git").is_file(),
        "a linked worktree's `.git` is a pointer file, not a directory"
    );

    // Idle to start with, then held by a merge started inside the worktree.
    assert_eq!(
        owning_operation(&worktree),
        None,
        "a fresh worktree is idle"
    );
    git_expected_failure(&worktree, &["merge", "--no-edit", "main"]);
    assert_eq!(
        owning_operation(&worktree),
        Some(OwnedOperation::Merge),
        "the worktree's own merge is visible through its gitdir pointer"
    );
    // The checkout that added the worktree shares the repository but not the
    // worktree's HEAD or markers, so it reads idle while the worktree is held —
    // the per-root half of the gating contract.
    assert_eq!(
        owning_operation(&repo),
        None,
        "one worktree's merge does not hold another worktree of the same repository"
    );
}
