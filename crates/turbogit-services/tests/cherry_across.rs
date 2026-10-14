//! Issue 16 — Cherry-pick across repositories, service seam: per-target
//! application (`apply_to_root`) and prediction (`forecast`) over real git
//! in temporary repositories.
//!
//! Cross-repo picks transport the source commits by fetching the source
//! repository's objects into the target (a local path fetch) and running a
//! real cherry-pick there, so a conflict leaves a genuine held cherry-pick
//! state — conflicted files and `CHERRY_PICK_HEAD` — and is never
//! auto-resolved.

use std::path::{Path, PathBuf};

use test_support::git_seed::{GitEnv, git_with_env};
use turbogit_domain::model::VcsSettings;
use turbogit_engine::cli::CliExecutor;
use turbogit_engine_api::GitExecutor;
use turbogit_services::cherry_across;

// The two repos this suite compares must agree on identity for a forecast to be
// about content rather than authorship, so every call is pinned per call through
// the fixture crate's runner: a headless machine has no global identity to
// borrow, and two repos that borrowed different ones would disagree about
// authorship instead of about content.

fn commit_file(dir: &Path, name: &str, body: &str, msg: &str) -> String {
    std::fs::write(dir.join(name), body).unwrap();
    git_with_env(dir, &["add", "."], &GitEnv::fixture());
    git_with_env(dir, &["commit", "-q", "-m", msg], &GitEnv::fixture());
    git_with_env(dir, &["rev-parse", "HEAD"], &GitEnv::fixture())
        .trim()
        .to_string()
}

fn init_repo(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    git_with_env(dir, &["init", "-q", "-b", "main"], &GitEnv::fixture());
    git_with_env(dir, &["config", "user.email", "t@t"], &GitEnv::fixture());
    git_with_env(dir, &["config", "user.name", "t"], &GitEnv::fixture());
    // Keep worktree line endings identical to the committed blobs so
    // checkout/apply comparisons are byte-for-byte (defaults on Git-for-
    // Windows turn on core.autocrlf and rewrite LF to CRLF).
    git_with_env(
        dir,
        &["config", "core.autocrlf", "false"],
        &GitEnv::fixture(),
    );
}

/// A project with a source repo `alpha` (commits c1..c3 on main) and a
/// target repo `beta` with one unrelated commit.
fn two_repos() -> (tempfile::TempDir, PathBuf, PathBuf, Vec<String>) {
    let tmp = tempfile::tempdir().unwrap();
    let alpha = tmp.path().join("alpha");
    let beta = tmp.path().join("beta");
    init_repo(&alpha);
    let c1 = commit_file(&alpha, "a.txt", "one\n", "c1");
    let c2 = commit_file(&alpha, "b.txt", "two\n", "c2");
    let c3 = commit_file(&alpha, "c.txt", "three\n", "c3");
    init_repo(&beta);
    commit_file(&beta, "base.txt", "base\n", "beta base");
    (tmp, alpha, beta, vec![c1, c2, c3])
}

fn engine() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

fn subjects(root: &Path) -> Vec<String> {
    git_with_env(
        root,
        &["log", "--format=%s", "--reverse"],
        &GitEnv::fixture(),
    )
    .lines()
    .map(str::to_string)
    .collect()
}

// --- apply_to_root --------------------------------------------------------------

#[test]
fn apply_to_root_applies_commits_in_order_onto_a_target_repo() {
    let (_tmp, alpha, beta, commits) = two_repos();
    let picks = vec![commits[1].clone(), commits[2].clone()];

    cherry_across::apply_to_root(&engine(), &alpha, &beta, &picks, true)
        .expect("both commits should apply cleanly");

    // Applied oldest-first as one chain: c3's copy sits on c2's copy.
    assert_eq!(
        subjects(&beta),
        vec!["beta base".to_string(), "c2".to_string(), "c3".to_string()],
        "the target gains the picked commits in order, on top of its own history"
    );
    assert_eq!(
        std::fs::read_to_string(beta.join("b.txt")).unwrap(),
        "two\n",
        "the picked change is present in the target's tree"
    );
    assert_eq!(
        std::fs::read_to_string(beta.join("c.txt")).unwrap(),
        "three\n"
    );
    // Nothing was left in progress.
    assert!(
        !turbogit_services::integrate_service::in_progress(&beta),
        "a clean run must not leave a cherry-pick in progress"
    );
}

#[test]
fn apply_to_root_skips_a_commit_already_present_in_the_target_history() {
    let (_tmp, alpha, beta, commits) = two_repos();
    let picks = vec![commits[1].clone(), commits[2].clone()];
    // Pick c2 once…
    cherry_across::apply_to_root(&engine(), &alpha, &beta, &picks, true)
        .expect("the first pass applies cleanly");
    // …then replay the whole selection: c2 is already present and must be
    // skipped, not duplicated; c3 still applies.
    cherry_across::apply_to_root(&engine(), &alpha, &beta, &picks, true)
        .expect("the replay should succeed by skipping the present commit");

    let count = subjects(&beta).iter().filter(|s| *s == "c2").count();
    assert_eq!(count, 1, "an already-present commit is never duplicated");
    let count = subjects(&beta).iter().filter(|s| *s == "c3").count();
    assert_eq!(count, 1, "the not-yet-present commit still applies");
}

/// `beta` shares one file with `alpha`'s picks: `b.txt` at conflicting
/// content, so picking `c2` (which writes "two\n") conflicts.
fn two_repos_with_conflicting_target() -> (tempfile::TempDir, PathBuf, PathBuf, Vec<String>) {
    let tmp = tempfile::tempdir().unwrap();
    let alpha = tmp.path().join("alpha");
    let beta = tmp.path().join("beta");
    init_repo(&alpha);
    let c1 = commit_file(&alpha, "a.txt", "one\n", "c1");
    let c2 = commit_file(&alpha, "b.txt", "two\n", "c2");
    let c3 = commit_file(&alpha, "c.txt", "three\n", "c3");
    init_repo(&beta);
    commit_file(&beta, "b.txt", "conflicting\n", "beta diverges b.txt");
    (tmp, alpha, beta, vec![c1, c2, c3])
}

#[test]
fn a_conflicted_pick_is_held_for_manual_resolution_and_halts_the_repo() {
    let (_tmp, alpha, beta, commits) = two_repos_with_conflicting_target();
    let picks = vec![commits[1].clone(), commits[2].clone()];

    let err = cherry_across::apply_to_root(&engine(), &alpha, &beta, &picks, true)
        .expect_err("the conflicting pick must fail");

    // Held, not auto-resolved: the cherry-pick is still in progress and the
    // conflict is visible in the status (the Resolve deep link's input).
    assert!(
        turbogit_services::integrate_service::in_progress(&beta),
        "the conflicted pick must stay held: {err}"
    );
    let status = engine().status(&beta).expect("status");
    assert!(
        !status.conflicted.is_empty(),
        "the conflicted file must show up as awaiting resolution"
    );
    assert!(
        !beta.join("c.txt").exists(),
        "with the stop policy on, commits after the conflict are not attempted"
    );
}

#[test]
fn with_the_policy_off_a_conflict_is_released_and_the_run_continues() {
    let (_tmp, alpha, beta, commits) = two_repos_with_conflicting_target();
    let picks = vec![commits[1].clone(), commits[2].clone()];

    let err = cherry_across::apply_to_root(&engine(), &alpha, &beta, &picks, false)
        .expect_err("the conflicted commit is a failure even when the run continues");

    // The held pick was released — nothing is left in progress.
    assert!(
        !turbogit_services::integrate_service::in_progress(&beta),
        "the policy-off run aborts the conflicted pick: {err}"
    );
    // The remaining commit still applied.
    assert!(
        beta.join("c.txt").exists(),
        "with the policy off, commits after the conflict are still attempted"
    );
    assert!(
        err.to_string().contains("1/2"),
        "the summary reports how much applied: {err}"
    );
}

// --- forecast -------------------------------------------------------------------

use turbogit_domain::model::{Commit, RootId};
use turbogit_services::cherry_across::{PickForecast, TargetRisk};

/// The picked source commits in application order (oldest first; the log is
/// newest-first).
fn source_picks(exec: &CliExecutor, alpha: &Path, shas: &[String]) -> Vec<Commit> {
    let mut picks: Vec<Commit> = exec
        .log(alpha, &Default::default())
        .expect("source log")
        .into_iter()
        .filter(|c| shas.contains(&c.id))
        .collect();
    picks.reverse();
    picks
}

#[test]
fn forecast_predicts_a_clean_low_risk_apply_for_untouched_targets() {
    let (_tmp, alpha, beta, commits) = two_repos();
    let exec = engine();
    let picks = source_picks(&exec, &alpha, &commits[1..3]);
    let targets = vec![(
        RootId(beta.clone().into()),
        "beta".to_string(),
        beta.clone(),
    )];

    let fc = cherry_across::forecast(&exec, &alpha, &picks, &targets);
    assert_eq!(fc.len(), 1);
    let t = &fc[0];
    assert_eq!((t.total, t.applies), (2, 2), "both picks will be attempted");
    assert_eq!(t.risk, TargetRisk::Low);
    assert!(t.blocked.is_none());
    assert!(
        t.picks
            .iter()
            .all(|p| matches!(p, PickForecast::CleanApply)),
        "new files into an unrelated repo apply cleanly: {:?}",
        t.picks
    );
}

#[test]
fn forecast_marks_a_commit_already_present_in_the_target_history() {
    let (_tmp, alpha, beta, commits) = two_repos();
    let exec = engine();
    // beta already carries a commit with c2's subject.
    commit_file(&beta, "b.txt", "independent work\n", "c2");
    let picks = source_picks(&exec, &alpha, &commits[1..3]);
    let targets = vec![(
        RootId(beta.clone().into()),
        "beta".to_string(),
        beta.clone(),
    )];

    let t = &cherry_across::forecast(&exec, &alpha, &picks, &targets)[0];
    assert_eq!((t.applies, t.total), (1, 2), "c2 is skipped, c3 applies");
    assert!(matches!(t.picks[0], PickForecast::AlreadyPresent));
    assert!(matches!(t.picks[1], PickForecast::CleanApply));
}

#[test]
fn forecast_predicts_the_conflict_and_counts_the_overlapping_hunks() {
    let (_tmp, alpha, beta, commits) = two_repos_with_conflicting_target();
    let exec = engine();
    // c2 rewrites b.txt; beta's b.txt diverged → the whole (single) hunk fails.
    let picks = source_picks(&exec, &alpha, &commits[1..2]);
    let targets = vec![(
        RootId(beta.clone().into()),
        "beta".to_string(),
        beta.clone(),
    )];

    let t = &cherry_across::forecast(&exec, &alpha, &picks, &targets)[0];
    assert_eq!(t.risk, TargetRisk::High);
    match &t.picks[0] {
        PickForecast::Conflict { failed_hunks } => {
            assert_eq!(*failed_hunks, 1, "one hunk overlaps");
        }
        other => panic!("expected a conflict forecast, got {other:?}"),
    }
}

#[test]
fn forecast_rates_a_clean_apply_over_drifted_files_as_medium_risk() {
    let tmp = tempfile::tempdir().unwrap();
    let alpha = tmp.path().join("alpha");
    let beta = tmp.path().join("beta");
    init_repo(&alpha);
    let five = "l1\nl2\nl3\nl4\nl5\n";
    commit_file(&alpha, "a.txt", five, "c1");
    let c2 = commit_file(
        &alpha,
        "a.txt",
        "l1\nl2\nl3 changed\nl4\nl5\n",
        "c2 edits l3",
    );
    // beta carries the same file with one extra trailing line: the hunk
    // (mid-file, not EOF-anchored) still applies cleanly, but the touched
    // file drifted from the source's parent version.
    init_repo(&beta);
    commit_file(&beta, "a.txt", &format!("{five}l6\n"), "beta base");
    let exec = engine();
    let picks = source_picks(&exec, &alpha, &[c2]);
    let targets = vec![(
        RootId(beta.clone().into()),
        "beta".to_string(),
        beta.clone(),
    )];

    let t = &cherry_across::forecast(&exec, &alpha, &picks, &targets)[0];
    assert_eq!(t.risk, TargetRisk::Medium, "drift ⇒ medium: {:?}", t.picks);
    assert!(matches!(t.picks[0], PickForecast::CleanApplyDrifted));
}

#[test]
fn forecast_blocks_a_dirty_target() {
    let (_tmp, alpha, beta, commits) = two_repos();
    let exec = engine();
    std::fs::write(beta.join("base.txt"), "dirty\n").unwrap();
    let picks = source_picks(&exec, &alpha, &commits[1..3]);
    let targets = vec![(
        RootId(beta.clone().into()),
        "beta".to_string(),
        beta.clone(),
    )];

    let t = &cherry_across::forecast(&exec, &alpha, &picks, &targets)[0];
    assert_eq!(
        t.blocked.as_deref(),
        Some("working tree is dirty"),
        "a dirty target is skipped before any pick is attempted"
    );
    assert_eq!(t.applies, 0);
}

/// The count of failing hunks is the **Git engine**'s answer hunk by hunk, not
/// A patch value, read from git's syntax. That reader belongs to the adapters,
/// so a service's test reaches into the engine only to build a fixture.
fn value(text: &str) -> turbogit_domain::model::Patch {
    turbogit_engine_api::patch::parse_patch(text)
}

/// a reading of git's stderr prose. Real repository, real `git apply --check`.
#[test]
fn failed_hunks_asks_the_engine_per_hunk() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("target");
    init_repo(&repo);
    commit_file(&repo, "f.txt", "one\ntwo\nthree\n", "base");
    let exec = CliExecutor {
        settings: VcsSettings::default(),
    };

    // Two hunks: the first describes this repository, the tenth-line one does
    // not — its context lines do not exist here.
    let patch = value(
        "diff --git a/f.txt b/f.txt\n\
         --- a/f.txt\n\
         +++ b/f.txt\n\
         @@ -1,3 +1,4 @@\n\
          one\n\
         +ins\n\
          two\n\
          three\n\
         @@ -10,3 +11,4 @@\n\
          four\n\
          five\n\
         +ins2\n\
          six\n",
    );
    assert_eq!(
        cherry_across::failed_hunks(&exec, &repo, &patch),
        1,
        "one hunk's context is not in this repository"
    );

    // Nothing fails when the patch describes the tree it is checked against.
    let clean = value(
        "diff --git a/f.txt b/f.txt\n\
         --- a/f.txt\n\
         +++ b/f.txt\n\
         @@ -1,3 +1,4 @@\n\
          one\n\
         +ins\n\
          two\n\
          three\n",
    );
    assert_eq!(cherry_across::failed_hunks(&exec, &repo, &clean), 0);

    // Every hunk of a patch against an unrelated tree fails.
    let wrong = value(
        "diff --git a/f.txt b/f.txt\n\
         --- a/f.txt\n\
         +++ b/f.txt\n\
         @@ -1,3 +1,4 @@\n\
          alpha\n\
         +ins\n\
          beta\n\
          gamma\n",
    );
    assert_eq!(cherry_across::failed_hunks(&exec, &repo, &wrong), 1);
}
#[test]
fn apply_to_root_refuses_a_dirty_target_before_touching_git() {
    let (_tmp, alpha, beta, commits) = two_repos();
    std::fs::write(beta.join("base.txt"), "dirty\n").unwrap();
    let err = cherry_across::apply_to_root(&engine(), &alpha, &beta, &commits[1..3], true)
        .expect_err("a dirty target must be refused");
    assert!(
        err.to_string().contains("dirty"),
        "the refusal names the dirty tree: {err}"
    );
    assert!(
        !subjects(&beta).contains(&"c2".to_string()),
        "nothing was applied to the dirty target"
    );
}

/// A conflicted **change entry** — with no conflicted path beside it — blocks
/// the pick, and that is this rule disagreeing with the repository-state ladder
/// on purpose.
///
/// The two dirty rules differ in **both directions**, on three inputs, and
/// neither is reconciled without a ruling:
///
/// * **unversioned-only** — `Dirty` to the owner's ladder
///   (`modified() + unversioned() > 0`), **clean** to this rule;
/// * **conflicted-change-entry-only** — **blocking** to this rule, and not
///   `Conflict` to the ladder, which reads the separate `conflicted` path list
///   (the ladder's first rung) and not the change entry;
/// * **ignored-only** — **blocking** to this rule, which counts anything that
///   is not unversioned or ignored, and **clean** to the ladder.
///
/// The fixture is a seeded status rather than a real conflicted repository
/// because a real one fills the conflicted *path* list at the same time, which
/// is the ladder's `Conflict` rung — the disagreement only exists on the entry
/// alone. Reconciling it is a product question (should an untracked file block
/// a cherry-pick?), so the answer ships as it stands and this is the pin that
/// fails the moment somebody "fixes" it without ruling.
#[test]
fn apply_to_root_refuses_a_target_whose_only_change_is_a_conflicted_entry() {
    use turbogit_domain::model::{Change, ChangeStatus, RootStatus};
    use turbogit_engine::fake::FakeExecutor;

    let target = PathBuf::from("/w/target");
    let source = PathBuf::from("/w/source");
    let mut exec = FakeExecutor::new();
    exec.status.insert(
        target.clone(),
        RootStatus {
            changes: vec![Change {
                path: PathBuf::from("c.txt"),
                status: ChangeStatus::Conflicted,
                chunks: vec![],
                staged: false,
                unstaged: false,
                orig_path: None,
            }],
            // No conflicted *path*: the ladder reads this list, not the entry.
            conflicted: vec![],
            conflict_kinds: vec![],
        },
    );

    let err = cherry_across::apply_to_root(&exec, &source, &target, &["abc123".to_string()], true)
        .expect_err("a conflicted change entry blocks the pick");
    assert!(
        err.to_string().contains("dirty"),
        "the refusal is the local-changes one: {err}"
    );
    assert!(
        exec.calls.lock().unwrap().is_empty(),
        "the refusal happens before any git call — the target is never fetched, \
         never picked into, and the owner's rule is never consulted"
    );
}
