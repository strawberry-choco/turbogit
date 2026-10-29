use std::path::{Path, PathBuf};

use test_support::git_seed::git;
use turbogit_domain::model::{RebaseAction, RebasePlanEntry, VcsSettings};
use turbogit_engine::GitExecutor;
use turbogit_engine::cli::CliExecutor;
use turbogit_services::history_editor;

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

fn commit(dir: &Path, name: &str, text: &str) -> String {
    std::fs::write(dir.join(name), format!("{text}\n")).expect("writing work file");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", text]);
    rev(dir, "HEAD")
}

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

fn ref_absent(dir: &Path, r: &str) -> bool {
    !std::process::Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", r])
        .current_dir(dir)
        .output()
        .expect("git must be on PATH")
        .status
        .success()
}

#[test]
fn undoing_a_reword_restores_the_pre_rewrite_history_from_the_backup() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = three_deep(tmp.path());
    let target = rev(&repo, "HEAD~1");

    history_editor::reword_commit(
        &engine(),
        &repo,
        &target,
        "a message that will be regretted",
        &VcsSettings::default(),
        "feature",
    )
    .expect("the targeted reword runs");

    let after_reword = rev(&repo, "HEAD");
    history_editor::undo_last_rewrite(&engine(), &repo, &after_reword)
        .expect("the undo runs while the repository stands where the rewrite left it");

    assert_eq!(
        subjects(&repo),
        vec!["feature-3", "feature-2", "feature-1", "base"],
        "the branch is back on the pre-rewrite history, from the backup ref"
    );
    assert!(
        ref_absent(&repo, engine().rewrite_backup_ref()),
        "and the spent backup is discarded, not left dangling"
    );
}

#[test]
fn an_undo_refuses_when_the_repository_has_moved_since_the_rewrite() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = three_deep(tmp.path());
    let target = rev(&repo, "HEAD~1");

    history_editor::reword_commit(
        &engine(),
        &repo,
        &target,
        "a message that will be regretted",
        &VcsSettings::default(),
        "feature",
    )
    .expect("the targeted reword runs");

    let after_reword = rev(&repo, "HEAD");
    commit(&repo, "d.txt", "feature-4");

    let refused = history_editor::undo_last_rewrite(&engine(), &repo, &after_reword)
        .expect_err("a moved repository must refuse the undo");
    assert!(
        refused.to_string().contains("changed"),
        "the refusal says why: {refused}"
    );
    assert_eq!(
        subjects(&repo),
        vec![
            "feature-4",
            "feature-3",
            "a message that will be regretted",
            "feature-1",
            "base"
        ],
        "and nothing was restored: the newer commit survives"
    );
    assert!(
        !ref_absent(&repo, engine().rewrite_backup_ref()),
        "and the backup is kept — the refusal is not a discard"
    );
}
