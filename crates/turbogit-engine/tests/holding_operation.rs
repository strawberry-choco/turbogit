//! The CLI adapter answers [`GitExecutor::holding_operation`] from git's own
//! marker files, ranking them the way one owner of the answer has to.
//!
//! Two properties make this adapter method rather than a caller's probe:
//!
//! - **it stays cheap enough to ask every frame**, because no git process runs.
//!   The settings' `git_executable` is pointed at a binary that cannot be spawned,
//!   so a read that shelled out would fail rather than answer — the assertion is
//!   the cost guard, not a comment about it.
//! - **one owner of the ranking**: the sequence stopped on a pick outranks the
//!   pick, and the merge machinery sits last, so a refusal and a status-bar item
//!   cannot blame different operations for one repository.
//!
//! The markers are planted by hand rather than driven through git, so the states
//! the reader has to rank — which a healthy git of one version does not produce
//! side by side — are the ones under test.

use std::path::Path;

use turbogit_domain::model::VcsSettings;
use turbogit_engine::cli::CliExecutor;
use turbogit_engine::git2_exec::Git2Executor;
use turbogit_engine_api::{GitExecutor, OwnedOperation};

/// An executor that cannot spawn git: every answer here is a filesystem read or
/// the test fails on the way to it.
fn executor() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings {
            git_executable: "/nonexistent/git-does-not-exist".to_string(),
            ..VcsSettings::default()
        },
    }
}

/// A repository directory whose `.git` is a real directory, holding no markers.
fn repo(parent: &Path, name: &str) -> std::path::PathBuf {
    let dir = parent.join(name);
    std::fs::create_dir_all(dir.join(".git")).unwrap();
    dir
}

#[test]
fn an_unheld_repository_answers_idle_without_running_git() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path(), "idle");
    assert_eq!(executor().holding_operation(&dir), None);
}

#[test]
fn each_held_state_names_its_own_operation() {
    let tmp = tempfile::tempdir().unwrap();

    let rebase = repo(tmp.path(), "rebase");
    std::fs::create_dir_all(rebase.join(".git").join("rebase-merge")).unwrap();
    assert_eq!(
        executor().holding_operation(&rebase),
        Some(OwnedOperation::Rebase)
    );

    let apply = repo(tmp.path(), "apply");
    std::fs::create_dir_all(apply.join(".git").join("rebase-apply")).unwrap();
    assert_eq!(
        executor().holding_operation(&apply),
        Some(OwnedOperation::Rebase),
        "`git rebase --apply` leaves `rebase-apply`, and it is the same operation"
    );

    let pick = repo(tmp.path(), "pick");
    std::fs::write(pick.join(".git").join("CHERRY_PICK_HEAD"), "deadbeef\n").unwrap();
    assert_eq!(
        executor().holding_operation(&pick),
        Some(OwnedOperation::CherryPick)
    );

    let revert = repo(tmp.path(), "revert");
    std::fs::write(revert.join(".git").join("REVERT_HEAD"), "deadbeef\n").unwrap();
    assert_eq!(
        executor().holding_operation(&revert),
        Some(OwnedOperation::Revert)
    );

    let merge = repo(tmp.path(), "merge");
    std::fs::write(merge.join(".git").join("MERGE_HEAD"), "deadbeef\n").unwrap();
    assert_eq!(
        executor().holding_operation(&merge),
        Some(OwnedOperation::Merge)
    );
}

/// A sequence stopped on a pick holds both markers, and the operation the
/// developer started is the one named: asking in the other order blames a pick
/// nobody picked.
#[test]
fn a_rebase_stopped_on_a_pick_is_still_a_rebase() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path(), "stopped");
    std::fs::create_dir_all(dir.join(".git").join("rebase-merge")).unwrap();
    std::fs::write(dir.join(".git").join("CHERRY_PICK_HEAD"), "deadbeef\n").unwrap();
    std::fs::write(dir.join(".git").join("MERGE_HEAD"), "deadbeef\n").unwrap();
    assert_eq!(
        executor().holding_operation(&dir),
        Some(OwnedOperation::Rebase)
    );
}

/// A pick resolved by the merge machinery holds both holder files; the merge
/// machinery runs *for* the pick, so the pick is named.
#[test]
fn a_pick_held_alongside_a_merge_head_is_still_a_pick() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path(), "pick-and-merge");
    std::fs::write(dir.join(".git").join("CHERRY_PICK_HEAD"), "deadbeef\n").unwrap();
    std::fs::write(dir.join(".git").join("MERGE_HEAD"), "deadbeef\n").unwrap();
    assert_eq!(
        executor().holding_operation(&dir),
        Some(OwnedOperation::CherryPick)
    );
}

/// A linked worktree's `.git` is a pointer FILE, and the markers live at the
/// directory it names. Following `<worktree>/.git/…` would report every worktree
/// idle — the multi-root half of the gating contract.
#[test]
fn a_worktree_is_read_from_the_directory_its_git_file_points_at() {
    let tmp = tempfile::tempdir().unwrap();
    let common = repo(tmp.path(), "common");
    std::fs::write(common.join(".git").join("MERGE_HEAD"), "deadbeef\n").unwrap();

    let worktree = tmp.path().join("wt");
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(
        worktree.join(".git"),
        format!("gitdir: {}\n", common.join(".git").display()),
    )
    .unwrap();

    assert_eq!(
        executor().holding_operation(&worktree),
        Some(OwnedOperation::Merge),
        "the pointer's target holds the markers"
    );

    // The relative form git writes for a gitdir inside the same repository.
    let nested = tmp.path().join("outer");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(nested.join(".git"), "gitdir: ../common/.git\n").unwrap();
    assert_eq!(
        executor().holding_operation(&nested),
        Some(OwnedOperation::Merge)
    );
}

/// The composed adapter answers the same question the CLI one does — a backend
/// setting must not change who owns the repository.
#[test]
fn the_composed_adapter_delegates_the_answer() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path(), "composed");
    std::fs::write(dir.join(".git").join("REVERT_HEAD"), "deadbeef\n").unwrap();
    let composed = Git2Executor::new(executor());
    assert_eq!(
        GitExecutor::holding_operation(&composed, &dir),
        Some(OwnedOperation::Revert)
    );
}
