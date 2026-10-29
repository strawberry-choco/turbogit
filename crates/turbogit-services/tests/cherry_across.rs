use std::path::{Path, PathBuf};

use test_support::git_seed::{GitEnv, git_with_env, pin_eol_lf};
use turbogit_domain::model::VcsSettings;
use turbogit_engine::cli::CliExecutor;
use turbogit_engine_api::GitExecutor;
use turbogit_services::cherry_across;

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
    pin_eol_lf(dir);
}

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

#[test]
fn apply_to_root_applies_commits_in_order_onto_a_target_repo() {
    let (_tmp, alpha, beta, commits) = two_repos();
    let picks = vec![commits[1].clone(), commits[2].clone()];

    cherry_across::apply_to_root(&engine(), &alpha, &beta, &picks, true)
        .expect("both commits should apply cleanly");

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
    assert!(
        !turbogit_services::integrate_service::in_progress(&beta),
        "a clean run must not leave a cherry-pick in progress"
    );
}

#[test]
fn apply_to_root_skips_a_commit_already_present_in_the_target_history() {
    let (_tmp, alpha, beta, commits) = two_repos();
    let picks = vec![commits[1].clone(), commits[2].clone()];
    cherry_across::apply_to_root(&engine(), &alpha, &beta, &picks, true)
        .expect("the first pass applies cleanly");
    cherry_across::apply_to_root(&engine(), &alpha, &beta, &picks, true)
        .expect("the replay should succeed by skipping the present commit");

    let count = subjects(&beta).iter().filter(|s| *s == "c2").count();
    assert_eq!(count, 1, "an already-present commit is never duplicated");
    let count = subjects(&beta).iter().filter(|s| *s == "c3").count();
    assert_eq!(count, 1, "the not-yet-present commit still applies");
}

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

    assert!(
        !turbogit_services::integrate_service::in_progress(&beta),
        "the policy-off run aborts the conflicted pick: {err}"
    );
    assert!(
        beta.join("c.txt").exists(),
        "with the policy off, commits after the conflict are still attempted"
    );
    assert!(
        err.to_string().contains("1/2"),
        "the summary reports how much applied: {err}"
    );
}

use turbogit_domain::model::{Commit, RootId};
use turbogit_services::cherry_across::{PickForecast, TargetRisk};

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

fn value(text: &str) -> turbogit_domain::model::Patch {
    turbogit_engine_api::patch::parse_patch(text)
}

#[test]
fn failed_hunks_asks_the_engine_per_hunk() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("target");
    init_repo(&repo);
    commit_file(&repo, "f.txt", "one\ntwo\nthree\n", "base");
    let exec = CliExecutor {
        settings: VcsSettings::default(),
    };

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
