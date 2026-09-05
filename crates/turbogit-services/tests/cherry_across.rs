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

use turbogit_domain::model::VcsSettings;
use turbogit_engine::cli::CliExecutor;
use turbogit_engine_api::GitExecutor;
use turbogit_services::cherry_across;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("git must be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn commit_file(dir: &Path, name: &str, body: &str, msg: &str) -> String {
    std::fs::write(dir.join(name), body).unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", msg]);
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

fn init_repo(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["config", "user.email", "t@t"]);
    git(dir, &["config", "user.name", "t"]);
    // Keep worktree line endings identical to the committed blobs so
    // checkout/apply comparisons are byte-for-byte (defaults on Git-for-
    // Windows turn on core.autocrlf and rewrite LF to CRLF).
    git(dir, &["config", "core.autocrlf", "false"]);
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
    git(root, &["log", "--format=%s", "--reverse"])
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

#[test]
fn failed_hunks_counts_the_distinct_failing_hunks() {
    // Two hunks, the second reported failing by git's stderr.
    let patch_text = "\
diff --git a/f.txt b/f.txt
--- a/f.txt
+++ b/f.txt
@@ -1,3 +1,4 @@
 one
+ins
 two
 three
@@ -10,3 +11,4 @@
 four
 five
+ins2
 six
";
    let stderr = "error: patch failed: f.txt:10\nerror: f.txt: patch does not apply\n";
    assert_eq!(cherry_across::failed_hunks(patch_text, stderr), 1);
    let both = "error: patch failed: f.txt:1\nerror: patch failed: f.txt:10\n";
    assert_eq!(cherry_across::failed_hunks(patch_text, both), 2);
    // A stderr without parseable locations still reports at least one.
    assert_eq!(cherry_across::failed_hunks(patch_text, "boom"), 1);
    // A clean check has no stderr at all.
    assert_eq!(cherry_across::failed_hunks(patch_text, ""), 0);
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
