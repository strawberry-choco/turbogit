use egui_kittest::Harness;
use tempfile::TempDir;
use test_support::git_seed::{commit, git, repo_with_one_commit};
use test_support::harness::{assert_not_painted, assert_painted, settle_quiet, shell_harness_over};
use turbogit_app::state::{AppState, PendingConfirm};

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
    git(&alpha, &["checkout", "-b", "feature"]);
    commit(&alpha, "feature.txt", "y\n", "feature work");
    git(&alpha, &["branch", "--set-upstream-to=main", "feature"]);
    (tmp, project)
}

fn repo_with_merged_feature() -> (TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = repo_with_one_commit(&project, "alpha");
    git(&alpha, &["checkout", "-b", "feature"]);
    git(&alpha, &["checkout", "main"]);
    (tmp, project)
}

fn feedback_harness(project_dir: std::path::PathBuf) -> Harness<'static, AppState> {
    shell_harness_over(AppState::new(project_dir), egui::vec2(1024.0, 768.0))
}

#[test]
fn delete_branch_confirm_warns_when_ahead_and_unmerged() {
    let (_tmp, project) = repo_with_unmerged_feature();
    let mut harness = feedback_harness(project);

    {
        let st = harness.state_mut();
        let root = st.selected_root.clone().unwrap();
        st.ui.confirm = Some(PendingConfirm::DeleteLocalBranch {
            root,
            name: "feature".into(),
        });
    }
    settle_quiet(&mut harness);

    assert_painted(
        &harness,
        "Delete local branch 'feature'? This cannot be undone.",
    );
    assert_painted(&harness, "ahead");
    assert_painted(&harness, "not merged");
}

#[test]
fn delete_branch_confirm_omits_warning_when_merged() {
    let (_tmp, project) = repo_with_merged_feature();
    let mut harness = feedback_harness(project);

    {
        let st = harness.state_mut();
        let root = st.selected_root.clone().unwrap();
        st.ui.confirm = Some(PendingConfirm::DeleteLocalBranch {
            root,
            name: "feature".into(),
        });
    }
    settle_quiet(&mut harness);

    assert_painted(
        &harness,
        "Delete local branch 'feature'? This cannot be undone.",
    );
    assert_not_painted(&harness, "not merged");
}
