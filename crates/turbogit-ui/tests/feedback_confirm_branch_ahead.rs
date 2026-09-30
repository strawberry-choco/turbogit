//! Issue #02 — Delete-branch confirmation warns when ahead of main and
//! unmerged.
//!
//! Contract (issue #02):
//! - The Delete local branch confirm dialog adds a "branch is ahead of
//!   `<upstream>` and not merged" warning line when the branch is ahead
//!   of its tracking upstream and that upstream is NOT an ancestor of
//!   the branch.
//! - The warning is omitted when the branch is fully merged (the
//!   default safe-delete path).

use egui_kittest::Harness;
use tempfile::TempDir;
use test_support::git_seed::{commit, git, repo_with_one_commit};
use test_support::harness::{assert_not_painted, assert_painted, settle_quiet, shell_harness_over};
use turbogit_app::state::{AppState, PendingConfirm};

// --- git fixture ---------------------------------------------------------------

/// A repo on `main` with a local `feature` branch that has commits
/// ahead of `main` (i.e. the upstream is NOT an ancestor of feature).
///
/// The floor is `git_seed::repo_with_one_commit`. **The branch half is kept local, not
/// `repo_with_feature_branch`**: the confirmation under test reads a *local*
/// upstream (`--set-upstream-to=main feature`), which is the whole precondition. A
/// remote upstream would measure `ahead` against a ref that does not exist here.
///
fn repo_with_unmerged_feature() -> (TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = repo_with_one_commit(&project, "alpha");
    // Branch from main, add a commit on top, and set main as the
    // upstream. `feature` is now ahead of main and main is not an
    // ancestor of `feature`.
    git(&alpha, &["checkout", "-b", "feature"]);
    commit(&alpha, "feature.txt", "y\n", "feature work");
    git(&alpha, &["branch", "--set-upstream-to=main", "feature"]);
    (tmp, project)
}

/// A repo on `main` with a `feature` branch whose commits are all on
/// main too (i.e. main IS an ancestor of feature — safe to delete).
fn repo_with_merged_feature() -> (TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = repo_with_one_commit(&project, "alpha");
    git(&alpha, &["checkout", "-b", "feature"]);
    // No new commit: feature points at the same commit as main.
    git(&alpha, &["checkout", "main"]);
    (tmp, project)
}

// --- harness -------------------------------------------------------------------

/// The shared `shell_harness_over`: dark tokens every frame, embedded fonts once,
/// worker events drained every frame exactly as `src/app.rs` does.
fn feedback_harness(project_dir: std::path::PathBuf) -> Harness<'static, AppState> {
    shell_harness_over(AppState::new(project_dir), egui::vec2(1024.0, 768.0))
}

// --- tests ---------------------------------------------------------------------

/// Contract: when the branch is ahead of upstream and upstream is not
/// an ancestor, the Delete local branch confirm dialog paints an
/// "ahead of main · not merged" warning.
#[test]
fn delete_branch_confirm_warns_when_ahead_and_unmerged() {
    let (_tmp, project) = repo_with_unmerged_feature();
    let mut harness = feedback_harness(project);

    {
        let st = harness.state_mut();
        st.ui.confirm = Some(PendingConfirm::DeleteLocalBranch {
            name: "feature".into(),
        });
    }
    settle_quiet(&mut harness);

    // Standard confirm body.
    assert_painted(
        &harness,
        "Delete local branch 'feature'? This cannot be undone.",
    );
    // Ahead-of-main warning.
    assert_painted(&harness, "ahead");
    assert_painted(&harness, "not merged");
}

/// Contract: when the branch is fully merged into main, the warning is
/// OMITTED (the safe-delete path is the default behavior).
#[test]
fn delete_branch_confirm_omits_warning_when_merged() {
    let (_tmp, project) = repo_with_merged_feature();
    let mut harness = feedback_harness(project);

    {
        let st = harness.state_mut();
        st.ui.confirm = Some(PendingConfirm::DeleteLocalBranch {
            name: "feature".into(),
        });
    }
    settle_quiet(&mut harness);

    // The standard confirm body is present.
    assert_painted(
        &harness,
        "Delete local branch 'feature'? This cannot be undone.",
    );
    // The ahead-of-main warning is NOT painted.
    assert_not_painted(&harness, "not merged");
}
