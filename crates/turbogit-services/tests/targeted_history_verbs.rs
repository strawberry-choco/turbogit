//! Targeted history verbs: [`history_editor::drop_commit`] and
//! [`history_editor::reword_commit`].
//!
//! One seam — the services function over a real temporary repository, with git
//! on `PATH` — because that is the seam where a history rewrite is either true
//! or a lie. Every assertion reads the repository back afterwards: which
//! subjects exist, what a path contains, whether the backup ref is there. None
//! asserts on the plan that was built to get there, even though building
//! exactly one action on exactly one row is the whole design, because that is
//! the implementation's business and not the developer's.
//!
//! Both verbs go through the plan the app's interactive-rebase editor already
//! executes (ADR-0025). That shared path is the point: a second rewrite
//! machinery would be the thing this change exists to prevent.

use std::path::{Path, PathBuf};

use test_support::git_seed::git;
use turbogit_domain::model::{RebaseAction, RebasePlanEntry, VcsSettings};
use turbogit_engine::GitExecutor;
use turbogit_engine::cli::CliExecutor;
use turbogit_services::history_editor;

// ---------------------------------------------------------------- helpers --

/// Subject lines, newest first — the log a developer would read.
fn subjects(dir: &Path) -> Vec<String> {
    git(dir, &["log", "--format=%s"])
        .lines()
        .map(str::to_string)
        .collect()
}

fn rev(dir: &Path, r: &str) -> String {
    git(dir, &["rev-parse", r]).trim().to_string()
}

fn engine() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

/// Commit `text` touching `name`, returning the new HEAD.
fn commit(dir: &Path, name: &str, text: &str) -> String {
    std::fs::write(dir.join(name), format!("{text}\n")).expect("writing work file");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", text]);
    rev(dir, "HEAD")
}

/// A repo on `main` with one base commit, then a `feature` branch carrying
/// three: the middle one is the subject both verbs act on.
fn three_deep(dir: &Path) -> PathBuf {
    let repo = dir.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    commit(&repo, "base.txt", "base");
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    commit(&repo, "a.txt", "feature-1");
    commit(&repo, "b.txt", "feature-2");
    commit(&repo, "c.txt", "feature-3");
    repo
}

// ------------------------------------------------------------- drop commit --

/// Dropping the middle commit removes exactly it: the work built on top is
/// replayed, not discarded, and the commit below never moved.
#[test]
fn dropping_a_commit_removes_that_one_and_keeps_its_descendants() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = three_deep(tmp.path());
    let dropped = rev(&repo, "HEAD~1");

    history_editor::drop_commit(
        &engine(),
        &repo,
        &dropped,
        &VcsSettings::default(),
        "feature",
    )
    .expect("the targeted drop runs");

    assert_eq!(
        subjects(&repo),
        vec!["feature-3", "feature-1", "base"],
        "the dropped commit is gone and both its neighbours survived"
    );
    assert!(
        !repo.join("b.txt").exists(),
        "the dropped commit's own change is undone"
    );
    assert!(
        repo.join("c.txt").exists(),
        "the descendant's change is replayed on top"
    );
    assert_eq!(
        rev(&repo, "HEAD~2"),
        rev(&repo, "main"),
        "the base under the dropped commit never moved"
    );
}

/// The rewrite is recoverable: the backup ref exists before anything moves, so
/// the existing recovery path returns the branch to the pre-drop state.
#[test]
fn a_drop_leaves_a_backup_the_recovery_path_can_restore() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = three_deep(tmp.path());
    let dropped = rev(&repo, "HEAD~1");
    let pre_drop = rev(&repo, "HEAD");

    history_editor::drop_commit(
        &engine(),
        &repo,
        &dropped,
        &VcsSettings::default(),
        "feature",
    )
    .expect("the targeted drop runs");

    let backup = rev(&repo, engine().rewrite_backup_ref());
    assert_eq!(
        backup, pre_drop,
        "the backup names the state before the rewrite"
    );

    history_editor::abort_to_backup(&engine(), &repo).expect("recovery runs");
    assert_eq!(
        subjects(&repo),
        vec!["feature-3", "feature-2", "feature-1", "base"],
        "and the recovery path puts the dropped commit back"
    );
}

/// The rewrite is recoverable for a reword too, and this is the verb where it
/// matters most: a message the developer regrets is a mistake one command
/// rewrites, and one they cannot get back. Same path as the drop — the backup ref
/// is written before the replay — and the recovery path puts the original message
/// back.
#[test]
fn a_reword_leaves_a_backup_the_recovery_path_can_restore() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = three_deep(tmp.path());
    let target = rev(&repo, "HEAD~1");
    let pre_rewrite = rev(&repo, "HEAD");

    history_editor::reword_commit(
        &engine(),
        &repo,
        &target,
        "a message that will be regretted",
        &VcsSettings::default(),
        "feature",
    )
    .expect("the targeted reword runs");
    assert!(
        subjects(&repo).contains(&"a message that will be regretted".to_string()),
        "the new message is in the history to be regretted"
    );

    let backup = rev(&repo, engine().rewrite_backup_ref());
    assert_eq!(
        backup, pre_rewrite,
        "the backup names the state before the rewrite"
    );

    history_editor::abort_to_backup(&engine(), &repo).expect("recovery runs");
    assert_eq!(
        subjects(&repo),
        vec!["feature-3", "feature-2", "feature-1", "base"],
        "and the recovery path puts the original message back"
    );
}

/// A protected branch refuses before git is touched at all.
#[test]
fn a_drop_refuses_a_protected_branch_before_anything_runs() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = three_deep(tmp.path());
    let dropped = rev(&repo, "HEAD~1");
    let settings = VcsSettings {
        protected_branch_patterns: vec!["feature".into()],
        ..Default::default()
    };

    let refused = history_editor::drop_commit(&engine(), &repo, &dropped, &settings, "feature")
        .expect_err("a protected branch must refuse");
    assert!(
        refused.to_string().contains("protected"),
        "the refusal says why: {refused}"
    );
    assert_eq!(
        subjects(&repo),
        vec!["feature-3", "feature-2", "feature-1", "base"],
        "and nothing moved"
    );
}

// ------------------------------------------------------------ reword commit --

/// Rewording changes the message and nothing else: the same content, the same
/// descendants, a different hash.
#[test]
fn rewording_a_commit_changes_its_message_and_not_its_work() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = three_deep(tmp.path());
    let target = rev(&repo, "HEAD~1");
    let target_tree = rev(&repo, "HEAD~1^{tree}");
    let pre_rewrite = rev(&repo, "HEAD");

    history_editor::reword_commit(
        &engine(),
        &repo,
        &target,
        "feature-2: the message as it should have read",
        &VcsSettings::default(),
        "feature",
    )
    .expect("the targeted reword runs");

    assert_eq!(
        rev(&repo, engine().rewrite_backup_ref()),
        pre_rewrite,
        "a reword is the same rewrite, so it leaves the same safety net"
    );

    assert_eq!(
        subjects(&repo),
        vec![
            "feature-3",
            "feature-2: the message as it should have read",
            "feature-1",
            "base"
        ]
    );
    assert_ne!(rev(&repo, "HEAD~1"), target, "and the old hash is dead");
    assert_eq!(
        rev(&repo, "HEAD~1^{tree}"),
        target_tree,
        "correcting a message does not change what the commit did"
    );
}

/// A protected branch refuses a reword too — it is the same rewrite, so it is
/// the same guard.
#[test]
fn a_reword_refuses_a_protected_branch_before_anything_runs() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = three_deep(tmp.path());
    let target = rev(&repo, "HEAD~1");
    let settings = VcsSettings {
        protected_branch_patterns: vec!["feature".into()],
        ..Default::default()
    };

    let refused = history_editor::reword_commit(
        &engine(),
        &repo,
        &target,
        "anything at all",
        &settings,
        "feature",
    )
    .expect_err("a protected branch must refuse");
    assert!(refused.to_string().contains("protected"));
    assert_eq!(
        subjects(&repo),
        vec!["feature-3", "feature-2", "feature-1", "base"],
        "and nothing moved"
    );
}

// ------------------------------------------------------- one shared path ----

/// Both verbs are one plan with one row set, so the interactive editor's own
/// machinery — its result preview and its cautions — answers for a targeted
/// rewrite exactly as it does for a hand-edited one. A reword counts as
/// surviving verbatim, and the preview cannot show the replacement text because
/// git's todo has nowhere to put it (ADR-0025).
#[test]
fn a_targeted_plan_spans_first_parent_to_tip_and_sets_one_row() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = three_deep(tmp.path());
    let target = rev(&repo, "HEAD~1");
    let first_parent = rev(&repo, "HEAD~2");

    let plan = history_editor::targeted_plan(&engine(), &repo, &target, RebaseAction::Drop, None)
        .expect("the plan builds");

    assert_eq!(
        (
            plan.first().map(|e| e.commit.as_str()),
            plan.last().map(|e| e.commit.as_str())
        ),
        (Some(target.as_str()), Some(rev(&repo, "HEAD").as_str())),
        "the plan runs from the target commit through to the tip"
    );
    assert_eq!(
        plan.iter()
            .filter(|e| e.action == RebaseAction::Drop)
            .count(),
        1,
        "exactly one row carries the verb"
    );
    assert_eq!(
        history_editor::base_of(&engine(), &repo, &target).as_deref(),
        Some(first_parent.as_str()),
        "and the base it is built over is the target's first parent"
    );

    let preview = history_editor::plan_preview(&plan);
    assert_eq!(
        (preview.before, preview.after, preview.dropped),
        (2, 1, 1),
        "the target plus its one descendant, less the target"
    );
    assert!(
        history_editor::cautions(&engine(), &repo, &plan).is_empty(),
        "a linear three-commit plan by one author over disjoint files has no cautions"
    );
}

/// A reword counts as surviving verbatim in the preview, and the preview cannot
/// show the replacement text — the todo format has nowhere to put it, so the
/// message is confirmed in its own editor before the plan is ever built.
#[test]
fn a_reword_survives_the_preview_without_showing_its_new_text() {
    let plan = vec![
        RebasePlanEntry {
            action: RebaseAction::Pick,
            commit: "aaaa1111".into(),
            subject: "feature-1".into(),
            message: None,
        },
        RebasePlanEntry {
            action: RebaseAction::Reword,
            commit: "bbbb2222".into(),
            subject: "feature-2".into(),
            message: Some("feature-2 as it should have read".into()),
        },
    ];

    let preview = history_editor::plan_preview(&plan);
    assert_eq!(preview.after, 2, "a reword removes nothing");
    assert_eq!(preview.dropped, 0);
    assert_eq!(
        preview.kept[1].subject, "feature-2",
        "the preview carries the OLD subject, because the replacement text has \
         no slot in git's todo"
    );
}
