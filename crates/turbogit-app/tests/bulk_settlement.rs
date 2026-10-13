//! What a completed bulk run leaves behind: the aggregate toast and the
//! durable per-root entries.
//!
//! These two facts were derivable only by reading the settlement arm, because
//! nothing else in the suite asks the run's *own settlement* for them. Both are
//! driven through the Headless harness — [`AppState::for_roots`] → `dispatch` →
//! `drain_events` — and assert what a caller reads back afterwards: the toast
//! text and the feed. Nothing here reads the arm, the monitor's rows, or a
//! private function.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use test_support::git_seed::git;
use turbogit_app::activity::ActivityKind;
use turbogit_app::state::{AppState, ToastKind};
use turbogit_domain::model::RootId;
use turbogit_services::bulk_ops::{BulkOp, BulkPlan};
use turbogit_services::bulk_run::RowState;

/// A repository with one commit on `main` and an `origin` already pushed, so
/// upstream reads resolve without the test wiring remotes by hand.
fn repo(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q", "-b", "main"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("base.txt"), "base\n").unwrap();
    git(&path, &["add", "."]);
    git(&path, &["commit", "-q", "-m", "init"]);
    let bare = parent.join(format!("{name}.origin"));
    let _ = std::fs::remove_dir_all(&bare);
    git(
        parent,
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    git(&path, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(&path, &["push", "-q", "origin", "main"]);
    git(&path, &["branch", "--set-upstream-to=origin/main", "main"]);
    path
}

/// `tempfile` owns the tree; the caller keeps the project path.
fn project(tag: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join(tag);
    let frontend = project.join("frontend");
    std::fs::create_dir_all(&frontend).unwrap();
    (tmp, project, frontend)
}

fn app_over(project: &Path, roots: &[PathBuf]) -> AppState {
    let mut state = AppState::for_roots(project, roots);
    state.ui.repo_selection = roots.iter().map(|p| RootId(p.clone().into())).collect();
    state
}

/// A rebasing pull over roots the matrix refuses for three different reasons:
/// a protected branch, a dirty tree and an untracked branch, with a second
/// protected root so one reason is stated twice.
fn fleet_with_three_reasons() -> (tempfile::TempDir, PathBuf, Vec<PathBuf>) {
    let (tmp, project, frontend) = project("bulk-skip-reasons");
    let protected_one = repo(&frontend, "protected-one");
    let protected_two = repo(&frontend, "protected-two");
    let dirty = repo(&frontend, "dirty");
    let untracked = repo(&frontend, "untracked");

    // On `feature`, so neither is protected: the first is refused for its
    // working tree, the second for having nothing to pull from.
    for (path, branch) in [(&dirty, "feature"), (&untracked, "feature")] {
        git(path, &["checkout", "-q", "-b", branch]);
    }
    std::fs::write(dirty.join("base.txt"), "edited\n").unwrap();

    (
        tmp,
        project,
        vec![protected_one, protected_two, dirty, untracked],
    )
}

/// Pump the event pump until every monitor row is terminal and the run's own
/// completion event has settled. Not a `test_support::harness` settle: there is
/// no painted frame here, and the loop exits on a state predicate.
fn settle_run(state: &mut AppState) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        state.drain_events();
        let Some(view) = state.ui.bulk_run.as_ref() else {
            panic!("monitor closed before the run ended");
        };
        let busy_row = view
            .rows
            .iter()
            .any(|r| matches!(r.state, RowState::Queued { .. } | RowState::Running));
        // Terminal rows can land before the separate BulkCompleted event.
        if !busy_row && !state.ui.busy {
            return;
        }
        assert!(Instant::now() < deadline, "run did not end within 10s");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A rebasing pull plans no roots at all when every selected root is refused,
/// and the settlement says which reasons in a stable order: sorted, and each
/// distinct reason named once even when several roots were refused for it.
///
/// The count stays the number of *roots* — four were skipped — while the list
/// beside it is the three reasons they were skipped for. That is the only place
/// the two numbers are both visible, and a reader who had them disagree would
/// have no way to tell which one was meant.
#[test]
fn a_run_that_ran_nothing_settles_with_sorted_de_duplicated_skip_reasons() {
    let (_tmp, project, roots) = fleet_with_three_reasons();
    let mut state = app_over(&project, &roots);

    // Shipped defaults protect `main`, and this run rebases, so the matrix
    // refuses every root for a reason of its own.
    let plan = BulkPlan {
        op: BulkOp::PullAll,
        roots: Vec::new(),
        rebase: true,
        branch: String::new(),
        command: String::new(),
    };
    state.run_bulk_confirmed(plan);
    settle_run(&mut state);

    let toast = state.ui.toast.as_ref().expect("the run reports");
    assert_eq!(
        toast.kind,
        ToastKind::Warning,
        "nothing ran, and four roots were refused: {:?}",
        toast.message
    );
    assert_eq!(
        toast.message,
        "Pull all · nothing ran — 4 root(s) skipped \
         (dirty worktree, no upstream, protected branch — main)",
        "the reasons are sorted and each is named once"
    );
}

/// A refusal the guard raised while the step was running — a Retry forcing a
/// protected root — is owed its own durable entry, because the aggregate toast
/// above names only the repos. The preflight's own Warning ("… skipped") is not
/// this entry: this one is the execution-time refusal, and it is an Error.
#[test]
fn a_protected_branch_refusal_settles_its_own_entry_naming_the_root_and_the_error() {
    let (_tmp, project, frontend) = project("bulk-refusal-entry");
    let alpha = repo(&frontend, "alpha");
    let mut state = app_over(&project, std::slice::from_ref(&alpha));

    let plan = BulkPlan {
        op: BulkOp::PullAll,
        roots: Vec::new(),
        rebase: true,
        branch: String::new(),
        command: String::new(),
    };
    state.run_bulk_confirmed(plan);
    settle_run(&mut state);
    // Retry forces the attempt the preflight refused, so the guard that bears
    // is the one in the step: the pull refuses again and rewrites nothing.
    state.bulk_retry_skipped();
    settle_run(&mut state);

    let refusals: Vec<_> = state
        .ui
        .activity
        .entries
        .iter()
        .filter(|e| e.kind == ActivityKind::Error && e.repo.as_deref() == Some("alpha"))
        .collect();
    assert_eq!(
        refusals.len(),
        1,
        "one durable refusal per refused root: {refusals:?}"
    );
    let refusal = refusals[0];
    assert_eq!(
        refusal.message, "Pull all: 'main' is a protected branch — rebasing it is not allowed",
        "the entry names the run and git's own reason"
    );
}
