//! Issue #04 — Activity log: every dispatched git operation and its outcome
//! appends an entry to the session-durable feed.
//!
//! Headless over a real temp repository (git on PATH): operations are
//! dispatched through the production [`AppState::dispatch`] path and events are
//! pumped through `drain_events`. Assertions are on the public
//! `state.ui.activity` surface — never on internals.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;
use test_support::git_seed::{GitEnv, git_with_env};
use turbogit_app::activity::ActivityKind;
use turbogit_app::operation::Operation;
use turbogit_app::root_caches::Affected;
use turbogit_app::state::AppState;

// --- git fixture ---------------------------------------------------------------

// No repository `user.*` here, so every commit this fixture makes is pinned per
// call through the fixture crate's runner: a headless machine has no global
// identity to borrow. That includes the whole `plant_merge_conflict` sequence —
// the first commit here used to pin the author and two variable names git does
// not have, so on a machine with no global identity it died in fixture
// construction with `Committer identity unknown` and a diagnostic naming
// `git must be on PATH`. The merge that plants the conflict needs one too, and
// takes it the same way rather than borrowing the machine's.

fn commit_readme(repo: &Path) {
    std::fs::write(repo.join("README.md"), "x\n").unwrap();
    git_with_env(repo, &["add", "."], &GitEnv::fixture());
    git_with_env(repo, &["commit", "-m", "init"], &GitEnv::fixture());
}

/// One bare local remote (`origin`) plus one local repo `alpha` on `main`
/// with `origin` configured, so a fetch is a real, valid operation.
fn repo_project() -> (TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let remote = project.join("origin.git");
    git_with_env(
        &project,
        &["init", "--bare", "-b", "main", remote.to_str().unwrap()],
        &GitEnv::fixture(),
    );
    git_with_env(
        &project,
        &["-c", "init.defaultBranch=main", "init", "alpha"],
        &GitEnv::fixture(),
    );
    let alpha = project.join("alpha");
    commit_readme(&alpha);
    git_with_env(
        &alpha,
        &["remote", "add", "origin", remote.to_str().unwrap()],
        &GitEnv::fixture(),
    );
    git_with_env(
        &alpha,
        &["push", "-u", "origin", "main"],
        &GitEnv::fixture(),
    );
    (tmp, project, alpha)
}

// --- tests ---------------------------------------------------------------------

/// Contract: a dispatched git operation that completes successfully appends
/// exactly one activity entry naming the repo and the human-readable outcome.
#[test]
fn successful_op_appends_one_activity_entry() {
    let (_tmp, project, alpha) = repo_project();
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    let root = state.multi.roots[0].id.clone();
    let alpha_path = alpha.clone();
    state.dispatch(Operation::custom(
        "Fetch from origin",
        Affected::Root(root),
        move |v| v.fetch(&alpha_path, Some("origin")),
    ));

    // The harness runs the operation inline, so one drain settles it and the
    // activity entry is already there — no thread, no deadline.
    state.drain_events();
    assert!(
        !state.ui.activity.entries.is_empty(),
        "the op produced an activity entry; last_error={:?}",
        state.last_error
    );

    assert_eq!(
        state.ui.activity.entries.len(),
        1,
        "one op, one entry: {:?}",
        state.ui.activity.entries
    );
    let e = &state.ui.activity.entries[0];
    assert_eq!(e.repo.as_deref(), Some("alpha"));
    assert_eq!(e.kind, ActivityKind::Success);
    assert_eq!(e.message, "Fetch from origin");
}

/// Contract: a dispatched git operation that fails appends an Error entry
/// whose message carries the op label and the git error.
#[test]
fn failed_op_appends_error_entry() {
    let (_tmp, project, alpha) = repo_project();
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    let root = state.multi.roots[0].id.clone();
    let missing = PathBuf::from("/nonexistent/root/never/existed");
    state.dispatch(Operation::custom(
        "Fetch from origin",
        Affected::Root(root),
        move |v| v.fetch(&missing, Some("origin")),
    ));

    // The harness runs the operation inline, so one drain settles it and the
    // activity entry is already there — no thread, no deadline.
    state.drain_events();
    assert!(
        !state.ui.activity.entries.is_empty(),
        "the op produced an activity entry; last_error={:?}",
        state.last_error
    );

    let e = &state.ui.activity.entries[0];
    assert_eq!(e.kind, ActivityKind::Error);
    assert_eq!(e.repo.as_deref(), Some("alpha"));
    assert!(
        e.message.starts_with("Fetch from origin:"),
        "error entry must name the op and the failure: {:?}",
        e.message
    );
}

/// `git` that tolerates failure — used to plant a merge conflict. Deliberately
/// NOT the fixture crate's runner, which asserts: `plant_merge_conflict`'s
/// `git merge` is *supposed* to be refused.
///
/// It takes its identity from the owner's builder, because a merge that *would*
/// have written a commit needs a committer before git runs its merge machinery:
/// with none, git refused the merge for the wrong reason and planted no
/// conflict at all.
fn git_ok_or_fail(dir: &Path, args: &[&str]) {
    let _ = GitEnv::fixture().command(dir, args).output();
}

/// Put `alpha` into an in-progress merge with one conflicted file, as if a
/// merge just stopped mid-resolution.
fn plant_merge_conflict(alpha: &Path) {
    git_with_env(
        alpha,
        &["checkout", "-q", "-b", "feature"],
        &GitEnv::fixture(),
    );
    std::fs::write(alpha.join("README.md"), "feature side\n").unwrap();
    git_with_env(
        alpha,
        &["commit", "-qam", "feature change"],
        &GitEnv::fixture(),
    );
    git_with_env(alpha, &["checkout", "-q", "main"], &GitEnv::fixture());
    std::fs::write(alpha.join("README.md"), "main side\n").unwrap();
    git_with_env(
        alpha,
        &["commit", "-qam", "main change"],
        &GitEnv::fixture(),
    );
    git_ok_or_fail(alpha, &["merge", "feature", "--no-edit"]);
    let status = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(alpha)
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&status.stdout).to_string();
    assert!(
        out.contains("UU"),
        "fixture must leave a conflicted file, got:\n{out}"
    );
}

/// Contract: an op that completes but leaves conflicts behind logs a
/// Warning entry — success coloring would hide the unresolved state.
#[test]
fn op_leaving_conflicts_logs_warning_entry() {
    let (_tmp, project, alpha) = repo_project();
    plant_merge_conflict(&alpha);
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    let root = state.multi.roots[0].id.clone();
    let alpha_path = alpha.clone();
    state.dispatch(Operation::custom(
        "Fetch from origin",
        Affected::Root(root),
        move |v| v.fetch(&alpha_path, Some("origin")),
    ));

    // The harness runs the operation inline, so one drain settles it and the
    // activity entry is already there — no thread, no deadline.
    state.drain_events();
    assert!(
        !state.ui.activity.entries.is_empty(),
        "the op produced an activity entry; last_error={:?}",
        state.last_error
    );

    let e = &state.ui.activity.entries[0];
    assert_eq!(e.kind, ActivityKind::Warning, "entry: {:?}", e);
    assert_eq!(
        e.message, "Fetch from origin · 1 unresolved conflicts",
        "entry: {:?}",
        e
    );
}

// --- pure feed semantics (repo / time filters, clear) ---------------------------
