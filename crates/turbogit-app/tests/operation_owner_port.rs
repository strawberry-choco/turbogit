use std::path::{Path, PathBuf};
use std::sync::Arc;

use test_support::git_seed::repo_with_one_commit;
use turbogit_app::state::AppState;
use turbogit_engine::fake::FakeExecutor;
use turbogit_engine_api::OwnedOperation;

fn idle_root(parent: &Path, name: &str) -> PathBuf {
    repo_with_one_commit(parent, name)
}

fn state_over(roots: &[PathBuf], fake: FakeExecutor) -> AppState {
    let parent = roots[0].parent().unwrap();
    AppState::for_roots(parent, roots).with_executor(Arc::new(fake))
}

#[test]
fn the_engine_answers_the_holder_without_anything_on_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let alpha = idle_root(tmp.path(), "alpha");
    let mut fake = FakeExecutor::default();
    fake.holders.insert(alpha.clone(), OwnedOperation::Rebase);
    let state = state_over(std::slice::from_ref(&alpha), fake);

    assert_eq!(
        state.holding_operation(&alpha),
        Some(OwnedOperation::Rebase),
        "the engine's answer is the answer"
    );
}

#[test]
fn a_planted_marker_does_not_answer_when_the_engine_says_idle() {
    let tmp = tempfile::tempdir().unwrap();
    let alpha = idle_root(tmp.path(), "alpha");
    std::fs::write(alpha.join(".git").join("MERGE_HEAD"), "deadbeef\n").unwrap();
    let state = state_over(std::slice::from_ref(&alpha), FakeExecutor::default());

    assert_eq!(
        state.holding_operation(&alpha),
        None,
        "no caller may substitute git's markers for the engine's answer"
    );
}

#[test]
fn each_root_answers_its_own_holder() {
    let tmp = tempfile::tempdir().unwrap();
    let alpha = idle_root(tmp.path(), "alpha");
    let beta = idle_root(tmp.path(), "beta");
    let mut fake = FakeExecutor::default();
    fake.holders
        .insert(alpha.clone(), OwnedOperation::CherryPick);
    fake.holders.insert(beta.clone(), OwnedOperation::Revert);
    let state = state_over(&[alpha.clone(), beta.clone()], fake);

    assert_eq!(
        state.holding_operation(&alpha),
        Some(OwnedOperation::CherryPick)
    );
    assert_eq!(state.holding_operation(&beta), Some(OwnedOperation::Revert));
}
