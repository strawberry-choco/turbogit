//! The question "which operation, if any, holds this repository" is answered by
//! the [`GitExecutor`] port, not by a caller reading git's marker files.
//!
//! A rewrite gate and a status-bar announcement both act on this answer, so the
//! seam has to be the one place it comes from: a caller that could reach
//! `.git/rebase-merge` or `.git/MERGE_HEAD` itself would keep working when the
//! port changed its mind, and the two routes would disagree about who owns the
//! repository. These tests hold the app's route to the port open in both
//! directions — an answer with nothing on disk behind it, and an idle answer with
//! a marker planted on disk — because either half alone would pass an
//! implementation that consulted one source.
//!
//! Per-root identity is asserted alongside them: one workspace-wide "busy"
//! boolean is the failure mode a multi-root client is most likely to write.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use test_support::git_seed::repo_with_one_commit;
use turbogit_app::state::AppState;
use turbogit_engine::fake::FakeExecutor;
use turbogit_engine_api::OwnedOperation;

/// A real repository holding no marker files — so any answer read off disk would
/// be "idle", and an answer from the engine is unambiguous.
fn idle_root(parent: &Path, name: &str) -> PathBuf {
    repo_with_one_commit(parent, name)
}

fn state_over(roots: &[PathBuf], fake: FakeExecutor) -> AppState {
    let parent = roots[0].parent().unwrap();
    AppState::for_roots(parent, roots).with_executor(Arc::new(fake))
}

/// The engine's answer reaches the caller with nothing on disk to read: a
/// marker-file probe would answer `None` here.
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

/// The mirror direction: a marker on disk does not answer when the engine says
/// the repository is idle. Without this, the first test would also pass against a
/// caller that probed the filesystem and ignored the port.
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

/// Two roots hold two different operations, and each answers for itself: the
/// announcement and the gate are per root by construction, not one flag that
/// names whichever repository moved last.
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
