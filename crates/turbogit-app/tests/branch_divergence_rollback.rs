//! The multi-root divergence rollback offer, at the app seam.
//!
//! A branch operation that lands in some roots and fails in others does not
//! leave a cosmetic inconsistency: the succeeded roots are now on a branch the
//! failed roots are not, and nothing but a rollback (or hand repair) puts that
//! back. So the app must (a) notice the divergence from the run's PER-ROOT
//! outcomes, never from a global flag, (b) offer to roll back exactly the roots
//! the run CHANGED, and (c) record the divergence and the answer to the offer
//! in the durable activity feed.
//!
//! Every assertion here is per root on purpose. "A dialog appeared" would pass
//! on an offer that named the wrong repository, and the repository is the whole
//! content of the offer.
//!
//! One deliberate asymmetry against [`AppState`] fetch reporting: a root the
//! preflight SKIPPED is not part of the divergence. A skip leaves that root
//! untouched by policy and disclosed it before the run; a failure left it
//! untouched by accident, after the other roots had already moved.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use test_support::git_seed::git;
use turbogit_app::state::{AppState, RollbackOutcome};
use turbogit_services::bulk_ops::{BulkOp, BulkPlan};
use turbogit_services::bulk_run::RowState;

/// A repo with one commit on `main` and `origin` set up, so a branch cascade's
/// preflight admits it.
fn temp_repo(parent: &Path, name: &str) -> PathBuf {
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
    path
}

/// Two clean, synced repos — `alpha` and `broken` — where a cascade create of
/// `feature/x` succeeds in `alpha` and fails in `broken`.
///
/// **The failure is a ref-directory collision, not a dirty worktree.** A dirty
/// root is refused by the preflight matrix before the run and so reports a
/// SKIPPED row; a skipped root is exactly the case this offer must NOT fire on.
/// `broken` carries a `feature` ref where `feature/x` needs a directory, so git
/// cannot create the ref at run time (`parent is not directory`) while the root
/// stays clean, readable and admitted by preflight — a failure no backend can
/// answer differently, and one the repository survives.
fn diverging_project(tag: &str) -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join(tag);
    let frontend = project.join("frontend");
    std::fs::create_dir_all(&frontend).unwrap();
    let alpha = temp_repo(&frontend, "alpha");
    let broken = temp_repo(&frontend, "broken");
    git(&broken, &["branch", "feature"]);
    (tmp, project, alpha, broken)
}

fn rid(path: &Path) -> turbogit_domain::model::RootId {
    turbogit_domain::model::RootId(path.to_path_buf().into())
}

/// The app over `roots`, with the fleet selection set the way the preflight
/// matrix's "will run" rows leave it — the cascade's per-root undo capture is
/// scoped to the selection, so a suite that skips this step tests a run no
/// developer could have confirmed.
fn app_selected(project: &Path, roots: &[PathBuf]) -> AppState {
    let mut state = AppState::for_roots(project, roots);
    state.ui.repo_selection = roots.iter().map(|p| rid(p)).collect();
    state
}

fn app_two(project: &Path, alpha: &Path, broken: &Path) -> AppState {
    app_selected(project, &[alpha.to_path_buf(), broken.to_path_buf()])
}

fn cascade(alpha: &Path, broken: &Path, branch: &str) -> BulkPlan {
    BulkPlan {
        op: BulkOp::CreateBranch,
        roots: vec![rid(alpha), rid(broken)],
        rebase: false,
        branch: branch.to_string(),
        command: String::new(),
    }
}

/// Step the pump until the run's record lands in the history with `summary`,
/// then until the completion's side effects (offer, activity) are in state.
fn wait_for_summary(state: &mut AppState, summary: &str) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        state.drain_events();
        if state.ui.bulk_history.iter().any(|r| r.summary() == summary) {
            state.drain_events();
            return;
        }
        assert!(
            Instant::now() < deadline,
            "run did not record {summary}; records: {:?}",
            state
                .ui
                .bulk_history
                .iter()
                .map(|r| (r.summary(), r.repos.clone()))
                .collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The current branch of a repo on disk — read from git, never from app state,
/// so a rollback is proven to have reached the repository.
fn current(path: &Path) -> String {
    git(path, &["branch", "--show-current"]).trim().to_string()
}

fn has_branch(path: &Path, name: &str) -> bool {
    git(path, &["branch", "--list", name]).contains(name)
}

/// The newest feed entry whose message mentions `needle`.
fn entry<'a>(
    state: &'a AppState,
    needle: &str,
) -> Option<&'a turbogit_app::activity::ActivityEntry> {
    state
        .ui
        .activity
        .entries
        .iter()
        .rev()
        .find(|e| e.message.contains(needle))
}

// -- detection ---------------------------------------------------------------

#[test]
fn a_branch_run_that_landed_in_one_root_and_failed_in_another_arms_the_offer_on_those_roots() {
    let (_tmp, project, alpha, broken) = diverging_project("diverge-arm");
    let mut state = app_two(&project, &alpha, &broken);

    state.run_bulk_confirmed(cascade(&alpha, &broken, "feature/x"));
    wait_for_summary(&mut state, "1 ok · 1 failed");

    let offer = state.ui.branch_divergence.clone().expect(
        "a changed root beside a failed root is a divergence, and the app offers to undo it",
    );

    assert_eq!(offer.branch, "feature/x");
    // Per root, not a flag: the offer names WHICH repository moved.
    let changed: Vec<&str> = offer.changed.iter().map(|r| r.name.as_str()).collect();
    let failed: Vec<&str> = offer.failed.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        changed,
        vec!["alpha"],
        "only the root the run changed is rollback-able"
    );
    assert_eq!(
        failed,
        vec!["broken"],
        "the root that failed is the other half of the divergence"
    );
    assert_eq!(
        offer.changed[0].branch, "feature/x",
        "the changed root now stands on the target"
    );
    assert_eq!(
        offer.failed[0].branch, "main",
        "the failed root never moved off its own branch"
    );
    assert!(
        offer.failed[0]
            .reason
            .as_deref()
            .is_some_and(|r| !r.is_empty()),
        "the offer says why the run did not land"
    );

    // The divergence is a durable feed record, not a toast: the feed is what
    // makes a multi-root operation auditable after the surface is gone.
    let record = entry(&state, "diverge").expect("the divergence is recorded durably");
    assert_eq!(
        record.kind,
        turbogit_app::activity::ActivityKind::Warning,
        "a divergence needs attention, so it is not filed as a success"
    );
    assert!(
        record.message.contains("alpha") && record.message.contains("broken"),
        "the durable record names both halves: {}",
        record.message
    );
}

#[test]
fn a_run_where_every_root_landed_arms_no_offer() {
    let (_tmp, project, alpha, broken) = diverging_project("diverge-clean");
    // `broken` is repaired before the run: no collision, so nothing fails.
    git(&broken, &["branch", "-D", "feature"]);
    let mut state = app_two(&project, &alpha, &broken);

    state.run_bulk_confirmed(cascade(&alpha, &broken, "release/y"));
    wait_for_summary(&mut state, "2 ok");

    assert!(
        state.ui.branch_divergence.is_none(),
        "every root moved together, so nothing diverged and there is nothing to roll back"
    );
    assert!(
        current(&alpha) == "release/y" && current(&broken) == "release/y",
        "both roots are on the target branch"
    );
}

/// The deliberate opposite of the sync policy: an unupdatable root is SKIPPED
/// and the run carries on. A skip is disclosed by the preflight matrix before
/// anything runs and leaves that root untouched by choice, so it is not the
/// accident this offer exists to undo.
#[test]
fn a_run_that_skipped_a_root_by_preflight_arms_no_offer() {
    let (_tmp, project, alpha, broken) = diverging_project("diverge-skip");
    // A MODIFICATION to a tracked file, so the branch preflight reads the root
    // as dirty (`status.changes` counts it).
    std::fs::write(broken.join("base.txt"), "uncommitted edit\n").unwrap();
    let mut state = app_two(&project, &alpha, &broken);

    // The matrix's will-run set is `alpha` alone: a root the preflight refused
    // is refused BEFORE anything runs, which is the whole difference between a
    // skip and a failure. Putting `broken` in the plan's roots instead would
    // force the attempt — the monitor's `Retry skipped` semantics — and turn the
    // skip into the failure this offer is meant to answer, testing nothing.
    let mut plan = cascade(&alpha, &broken, "feature/z");
    plan.roots = vec![rid(&alpha)];
    state.run_bulk_confirmed(plan);
    wait_for_summary(&mut state, "1 ok · 1 skipped (dirty worktree)");

    assert!(
        state.ui.branch_divergence.is_none(),
        "a preflight skip is a disclosed refusal, not a partial failure"
    );
    assert_eq!(
        current(&broken),
        "main",
        "the skipped root was never touched"
    );
    assert!(
        !has_branch(&broken, "feature/z"),
        "the skipped root gained no branch to disagree about"
    );
}

// -- accepting ---------------------------------------------------------------

#[test]
fn accepting_rolls_back_the_changed_root_only_and_leaves_the_failed_root_alone() {
    let (_tmp, project, alpha, broken) = diverging_project("diverge-accept");
    let mut state = app_two(&project, &alpha, &broken);

    state.run_bulk_confirmed(cascade(&alpha, &broken, "feature/x"));
    wait_for_summary(&mut state, "1 ok · 1 failed");
    assert_eq!(current(&alpha), "feature/x", "the run landed in alpha");
    assert_eq!(current(&broken), "main", "the run never landed in broken");

    let outcome = state.rollback_divergent_roots();

    assert_eq!(
        outcome,
        RollbackOutcome::Applied {
            branch: "feature/x".to_string(),
            roots: 1
        },
        "one root had been changed, so one root is rolled back"
    );

    // The succeeded root is back where it started.
    assert_eq!(current(&alpha), "main", "alpha's checkout is restored");
    assert!(
        !has_branch(&alpha, "feature/x"),
        "the branch the run created in alpha is deleted again"
    );
    // The failed root is untouched: it has its own collision ref, gained no
    // `feature/x`, and was never on anything but `main`.
    assert_eq!(current(&broken), "main");
    assert!(
        has_branch(&broken, "feature"),
        "the failed root's own refs are not the rollback's business"
    );
    assert!(
        !has_branch(&broken, "feature/x"),
        "a root the run failed in has nothing to roll back"
    );

    // The offer is spent, and the outcome is durable.
    assert!(
        state.ui.branch_divergence.is_none(),
        "a acted-on offer does not stay on screen"
    );
    let done = entry(&state, "Rolled back").expect("the rollback outcome is in the feed");
    assert_eq!(done.kind, turbogit_app::activity::ActivityKind::Success);
    assert!(done.message.contains("feature/x"), "{}", done.message);

    // The history record carries the fact, so a second accept cannot re-run it.
    assert!(
        state
            .ui
            .bulk_history
            .iter()
            .any(|r| r.rolled_back && r.branch == "feature/x"),
        "the run record remembers the rollback"
    );
    assert!(
        matches!(state.rollback_divergent_roots(), RollbackOutcome::Refused),
        "a spent offer rolls nothing back a second time"
    );
}

/// A pre-existing branch the run only checked out is not a branch the rollback
/// may delete — the undo differs per root, which is why the record carries a
/// `created` flag per row rather than one answer for the run.
#[test]
fn accepting_keeps_a_branch_that_already_existed_in_the_changed_root() {
    let (_tmp, project, alpha, broken) = diverging_project("diverge-existing");
    // alpha already carries `feature/x`, so the run only checks it out there —
    // and `broken` keeps the collision ref, so it is still the root that fails.
    git(&alpha, &["branch", "feature/x"]);
    let mut state = app_two(&project, &alpha, &broken);

    state.run_bulk_confirmed(cascade(&alpha, &broken, "feature/x"));
    wait_for_summary(&mut state, "1 ok · 1 failed");
    assert_eq!(current(&alpha), "feature/x");

    let outcome = state.rollback_divergent_roots();
    assert_eq!(
        outcome,
        RollbackOutcome::Applied {
            branch: "feature/x".to_string(),
            roots: 1
        }
    );

    assert_eq!(current(&alpha), "main", "only the checkout moves back");
    assert!(
        has_branch(&alpha, "feature/x"),
        "a branch the run did not create survives the rollback"
    );
}

// -- declining ---------------------------------------------------------------

#[test]
fn declining_keeps_the_branches_divergent_records_it_and_leaves_nothing_on_screen() {
    let (_tmp, project, alpha, broken) = diverging_project("diverge-decline");
    let mut state = app_two(&project, &alpha, &broken);

    state.run_bulk_confirmed(cascade(&alpha, &broken, "feature/x"));
    wait_for_summary(&mut state, "1 ok · 1 failed");

    state.keep_divergent_branches();

    assert!(
        state.ui.branch_divergence.is_none(),
        "the offer is dismissed"
    );
    assert_eq!(
        current(&alpha),
        "feature/x",
        "declining changes nothing on disk: alpha stays on the branch the run moved it to"
    );
    assert!(
        has_branch(&alpha, "feature/x"),
        "declining deletes no branch"
    );
    assert!(
        state.ui.bulk_history.iter().all(|r| !r.rolled_back),
        "no rollback was applied"
    );
    let kept = entry(&state, "kept").expect("the decline is itself durable");
    assert_eq!(
        kept.kind,
        turbogit_app::activity::ActivityKind::Warning,
        "a kept divergence is unresolved, not a success: {}",
        kept.message
    );
    assert!(kept.message.contains("feature/x"), "{}", kept.message);
}

/// The monitor is the run's truth: the offer is built from the same per-root
/// rows the cascade run monitor paints and the preflight scoped, so a rollback
/// cannot reach a root the run never covered.
#[test]
fn the_offer_is_seeded_from_the_monitored_rows_and_names_no_other_root() {
    let (_tmp, project, alpha, broken) = diverging_project("diverge-rows");
    let unselected = temp_repo(&project.join("frontend"), "unselected");
    let mut state = app_two(&project, &alpha, &broken);

    state.run_bulk_confirmed(cascade(&alpha, &broken, "feature/x"));
    wait_for_summary(&mut state, "1 ok · 1 failed");

    let offer = state.ui.branch_divergence.clone().unwrap();
    let named: Vec<String> = offer
        .changed
        .iter()
        .chain(&offer.failed)
        .map(|r| r.name.clone())
        .collect();
    assert_eq!(
        named,
        vec!["alpha".to_string(), "broken".to_string()],
        "exactly the roots the run covered are in the offer"
    );
    assert!(
        !named.iter().any(|n| n == "unselected"),
        "a root outside the run's scope is never offered"
    );
    let _ = unselected;

    let view_rows: Vec<&RowState> = state
        .ui
        .bulk_run
        .as_ref()
        .map(|v| {
            v.rows
                .iter()
                .map(|r| &r.state)
                .filter(|s| matches!(s, RowState::Done | RowState::Failed { .. }))
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(
        view_rows.len(),
        2,
        "the monitor itself still reports one terminal outcome per covered root"
    );
}

// -- the offer never strands ---------------------------------------------------

/// A later run that lands everywhere replaces the outstanding offer rather than
/// stacking a second one on it: the offer answers the NEWEST divergence, and an
/// old one whose roots have since moved is a promise about a state that no
/// longer exists.
#[test]
fn a_later_run_that_lands_everywhere_takes_the_outstanding_offer_away() {
    let (_tmp, project, alpha, broken) = diverging_project("diverge-replaced");
    let mut state = app_two(&project, &alpha, &broken);

    state.run_bulk_confirmed(cascade(&alpha, &broken, "feature/x"));
    wait_for_summary(&mut state, "1 ok · 1 failed");
    assert!(
        state.ui.branch_divergence.is_some(),
        "the first run diverged"
    );

    // `broken`'s collision ref goes, so the second run lands in both roots.
    git(&broken, &["branch", "-D", "feature"]);
    let mut second = cascade(&alpha, &broken, "release/q");
    second.roots = vec![rid(&alpha), rid(&broken)];
    state.run_bulk_confirmed(second);
    wait_for_summary(&mut state, "2 ok");

    assert!(
        state.ui.branch_divergence.is_none(),
        "a run that left nothing divergent must not keep an offer painted"
    );
}

/// The rollback reads the run's history record, and that list is capped — so an
/// offer whose record has been evicted must say it refused rather than pretend
/// the roots were restored.
#[test]
fn an_offer_whose_record_is_gone_refuses_and_says_so() {
    let (_tmp, project, alpha, broken) = diverging_project("diverge-evicted");
    let mut state = app_two(&project, &alpha, &broken);

    state.run_bulk_confirmed(cascade(&alpha, &broken, "feature/x"));
    wait_for_summary(&mut state, "1 ok · 1 failed");
    assert_eq!(current(&alpha), "feature/x");

    state.ui.bulk_history.clear();
    let outcome = state.rollback_divergent_roots();

    assert_eq!(outcome, RollbackOutcome::Refused, "no record, no rollback");
    assert_eq!(
        current(&alpha),
        "feature/x",
        "a refused rollback moves nothing, so it must not claim to have"
    );
    assert!(
        state.ui.branch_divergence.is_none(),
        "and the offer is spent"
    );
    let refused = entry(&state, "Rollback refused").expect("the refusal is durable");
    assert!(refused.message.contains("feature/x"), "{}", refused.message);
}

// -- the record arms even when the offer cannot -------------------------------

/// The offer and the record are two different promises, and `Custom` is where
/// they part. An arbitrary git cascade handed its roots directly (as Retry
/// re-dispatches skipped rows) can leave some roots moved and others
/// failed; but `reversible()` is
/// `op == CreateBranch` — the app did not arrange the change and cannot undo
/// it, so offering a rollback would be a false promise. The divergence is
/// still a fact the durable feed must record, because root divergence is
/// unrecoverable without hand repair and silence is the worst outcome.
#[test]
fn a_custom_run_that_moved_one_root_and_failed_in_another_records_without_offering() {
    let (_tmp, project, alpha, broken) = diverging_project("custom-diverge");
    let mut state = app_two(&project, &alpha, &broken);
    // Both roots sit on `main`, which the shipped default protects; this
    // suite is about the divergence bookkeeping, so the policy is off here.
    state.settings.protected_branch_patterns.clear();

    // `git branch feature/x` lands in `alpha` and hits `broken`'s ref-directory
    // collision at run time — both roots are handed to the run directly, so
    // the failure is a Failed row, not a skip.
    state.run_bulk_confirmed(BulkPlan {
        op: BulkOp::Custom,
        roots: vec![rid(&alpha), rid(&broken)],
        rebase: false,
        branch: String::new(),
        command: "branch feature/x".to_string(),
    });
    wait_for_summary(&mut state, "1 ok · 1 failed");

    assert!(
        has_branch(&alpha, "feature/x"),
        "the command really did move a root, which is what diverged"
    );

    // The offer stays closed: rolling back arbitrary git is a promise the app
    // cannot honour, which is exactly what `reversible()` exists to prevent.
    assert!(
        state.ui.branch_divergence.is_none(),
        "no rollback is offered for a run the app cannot undo"
    );

    // Yet the divergence is recorded durably — the feed names both halves.
    let record = entry(&state, "diverged")
        .expect("a diverged custom run is recorded even though no rollback is offered");
    assert_eq!(
        record.kind,
        turbogit_app::activity::ActivityKind::Warning,
        "a divergence needs attention, so it is not filed as a success"
    );
    assert!(
        record.message.contains("alpha") && record.message.contains("broken"),
        "the durable record names both halves: {}",
        record.message
    );
}
