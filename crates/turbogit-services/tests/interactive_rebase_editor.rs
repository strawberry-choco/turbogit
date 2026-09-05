//! Issue 30 — interactive rebase editor rework (screen 17).
//!
//! The editor's domain logic is tested at the [`history_editor`] seam: the
//! REBASE-TODO text round-trip, the post-plan result preview, the computed
//! cautions, and the backup-ref safety net. Pure parts run on plain data;
//! git-touching parts run against real repositories (tempdir + system `git`).

#![allow(dead_code)]

use std::path::Path;
use std::process::Command;

use turbogit_domain::error::TgError;
use turbogit_domain::model::{RebaseAction, RebasePlanEntry, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_services::history_editor;

// ---------------------------------------------------------------- helpers --

/// Run `git <args>` in `dir`, asserting success; returns stdout.
fn run_git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git").args(args).current_dir(dir).output();
    let output = output.expect("spawning git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// Append `text` to `<dir>/<name>`, stage, commit, return HEAD SHA.
fn commit(dir: &Path, name: &str, text: &str) -> String {
    let file = dir.join(name);
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
        .expect("opening work file");
    use std::io::Write;
    writeln!(f, "{text}").expect("appending work file");
    drop(f);
    run_git(dir, &["add", "."]);
    run_git(dir, &["commit", "-m", text]);
    run_git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

fn engine() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

/// Repo on `main` (one base commit) with a `feature` branch three commits
/// ahead, checked out — the interactive-editing subject.
fn plan_repo(tmp: &Path, name: &str) -> std::path::PathBuf {
    let repo = tmp.join(name);
    std::fs::create_dir_all(&repo).unwrap();
    run_git(&repo, &["init", "-q", "-b", "main"]);
    run_git(&repo, &["config", "user.email", "test@example.com"]);
    run_git(&repo, &["config", "user.name", "Test"]);
    commit(&repo, "base.txt", "base");
    run_git(&repo, &["checkout", "-q", "-b", "feature"]);
    commit(&repo, "a.txt", "feature-1");
    commit(&repo, "b.txt", "feature-2");
    commit(&repo, "c.txt", "feature-3");
    repo
}

fn entry(action: RebaseAction, commit: &str, subject: &str) -> RebasePlanEntry {
    RebasePlanEntry {
        action,
        commit: commit.to_string(),
        subject: subject.to_string(),
    }
}

// ------------------------------------------------------------ rebase-todo --

#[test]
fn a_plan_round_trips_through_the_todo_text() {
    let plan = vec![
        entry(RebaseAction::Pick, "9b2e440aaa", "docs: cascade runbook"),
        entry(RebaseAction::Fixup, "f7c1d08bbb", "wip: trim checks"),
        entry(RebaseAction::Squash, "a91c3f7ccc", "feat: dry-run mode"),
        entry(RebaseAction::Drop, "7d10b4eddd", "test: redundant case"),
    ];

    let text = history_editor::render_todo(&plan);
    let parsed =
        history_editor::parse_todo(&text).expect("the rendered todo must parse back cleanly");
    assert_eq!(parsed, plan, "parse(render(plan)) == plan");
}

#[test]
fn the_todo_text_names_each_verb_sha_and_subject() {
    let plan = vec![entry(
        RebaseAction::Pick,
        "9b2e440aaa",
        "docs: cascade runbook",
    )];
    let text = history_editor::render_todo(&plan);
    assert_eq!(text.trim(), "pick 9b2e440aaa docs: cascade runbook");
}

#[test]
fn parsing_accepts_the_verbs_the_plan_rows_offer() {
    let text = "\
pick 9b2e440a docs: runbook
reword a91c3f7c feat: dry-run
edit f4e2a91b refactor: split
squash f7c1d08b wip: trim
fixup 3a90be2a wip: rename
drop 7d10b4ec test: redundant
";
    let plan = history_editor::parse_todo(text).expect("every verb parses");
    let actions: Vec<RebaseAction> = plan.iter().map(|e| e.action.clone()).collect();
    assert_eq!(
        actions,
        vec![
            RebaseAction::Pick,
            RebaseAction::Reword,
            RebaseAction::Edit,
            RebaseAction::Squash,
            RebaseAction::Fixup,
            RebaseAction::Drop,
        ]
    );
}

#[test]
fn parsing_ignores_comments_and_blank_lines() {
    let text = "\
# Rebase todo edited by hand

pick 9b2e440a docs: runbook
# reword a91c3f7c commented out
";
    let plan = history_editor::parse_todo(text).expect("comments are skipped");
    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].commit, "9b2e440a");
}

#[test]
fn parsing_rejects_an_unknown_verb() {
    let err = history_editor::parse_todo("rebase 9b2e440a docs: runbook")
        .expect_err("an unknown verb cannot become a plan");
    assert!(matches!(err, TgError::Other(_)), "{err}");
}

// ---------------------------------------------------------------- preview --

#[test]
fn an_all_pick_plan_keeps_every_commit() {
    let plan = vec![
        entry(RebaseAction::Pick, "aaaa1111", "feature-1"),
        entry(RebaseAction::Pick, "aaaa2222", "feature-2"),
    ];
    let preview = history_editor::plan_preview(&plan);
    assert_eq!(preview.before, 2);
    assert_eq!(preview.after, 2);
    assert_eq!(preview.kept, plan);
    assert!(
        preview.folds.is_empty(),
        "nothing folds without a fold verb"
    );
    assert_eq!(preview.dropped, 0);
}

#[test]
fn folds_group_into_the_nearest_kept_commit_and_drops_are_counted() {
    // pick A · fixup B · squash C · drop D · pick E — screen 17's
    // "4 → 3 COMMITS" shape: A absorbs B and C, D is dropped, E survives.
    let plan = vec![
        entry(RebaseAction::Pick, "a91c3f7aaa", "feat: dry-run mode"),
        entry(RebaseAction::Fixup, "f7c1d08bbb", "wip: trim checks"),
        entry(RebaseAction::Squash, "3a90be2ccc", "wip: rename hooks"),
        entry(RebaseAction::Drop, "7d10b4eddd", "test: redundant case"),
        entry(RebaseAction::Pick, "f4e2a91eee", "refactor: split"),
    ];
    let preview = history_editor::plan_preview(&plan);
    assert_eq!(preview.before, 5);
    assert_eq!(preview.after, 2);
    assert_eq!(
        preview
            .kept
            .iter()
            .map(|e| e.commit.as_str())
            .collect::<Vec<_>>(),
        vec!["a91c3f7aaa", "f4e2a91eee"],
        "the fold target and the commits after the drop survive"
    );
    assert_eq!(preview.folds.len(), 1, "consecutive folds group into one");
    assert_eq!(preview.folds[0].folded, vec!["f7c1d08bbb", "3a90be2ccc"]);
    assert_eq!(preview.folds[0].into, "a91c3f7aaa");
    assert_eq!(preview.dropped, 1);
}

#[test]
fn a_fold_starts_a_new_summary_after_an_intervening_pick() {
    let plan = vec![
        entry(RebaseAction::Pick, "a91c3f7aaa", "feat: dry-run mode"),
        entry(RebaseAction::Fixup, "f7c1d08bbb", "wip: trim checks"),
        entry(RebaseAction::Pick, "f4e2a91eee", "refactor: split"),
        entry(RebaseAction::Squash, "3a90be2ccc", "wip: rename hooks"),
    ];
    let preview = history_editor::plan_preview(&plan);
    assert_eq!(preview.after, 2);
    assert_eq!(
        preview.folds.len(),
        2,
        "each fold target gets its own summary"
    );
    assert_eq!(preview.folds[0].into, "a91c3f7aaa");
    assert_eq!(preview.folds[1].into, "f4e2a91eee");
    assert_eq!(preview.dropped, 0);
}

// ---------------------------------------------------------------- cautions --

use turbogit_services::history_editor::RebaseCaution;

#[test]
fn commits_touching_the_same_file_raise_the_conflict_caution() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = plan_repo(tmp.path(), "conflict");
    // A fourth commit re-edits the file feature-1 touched.
    let head = commit(&repo, "a.txt", "feature-1 again");

    let plan = history_editor::build_plan(&engine(), &repo, "main").expect("plan");
    let _ = head;
    let cautions = history_editor::cautions(&engine(), &repo, &plan);
    assert!(
        cautions.contains(&RebaseCaution::ConflictRisk { files: 1 }),
        "a.txt is touched by two planned commits: {cautions:?}"
    );
}

#[test]
fn disjoint_commits_raise_no_conflict_caution() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = plan_repo(tmp.path(), "disjoint");
    let plan = history_editor::build_plan(&engine(), &repo, "main").expect("plan");
    let cautions = history_editor::cautions(&engine(), &repo, &plan);
    assert!(
        !cautions
            .iter()
            .any(|c| matches!(c, RebaseCaution::ConflictRisk { .. })),
        "every commit touches its own file: {cautions:?}"
    );
}

#[test]
fn mixed_author_identities_raise_the_identity_caution() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = plan_repo(tmp.path(), "mixed");
    run_git(&repo, &["config", "user.name", "Other"]);
    commit(&repo, "d.txt", "feature-4");

    let plan = history_editor::build_plan(&engine(), &repo, "main").expect("plan");
    let cautions = history_editor::cautions(&engine(), &repo, &plan);
    assert!(
        cautions.contains(&RebaseCaution::MixedIdentities { authors: 2 }),
        "Test and Other both authored planned commits: {cautions:?}"
    );
}

#[test]
fn a_single_author_raises_no_identity_caution() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = plan_repo(tmp.path(), "single");
    let plan = history_editor::build_plan(&engine(), &repo, "main").expect("plan");
    let cautions = history_editor::cautions(&engine(), &repo, &plan);
    assert!(cautions.is_empty(), "{cautions:?}");
}

// ------------------------------------------------- backup ref & recovery --

use turbogit_services::history_editor::BACKUP_REF;

/// The all-drop plan rewrites `feature` to nothing, so a successful replay
/// provably moves HEAD away from the pre-rebase tip.
fn all_drop_plan(repo: &Path) -> Vec<RebasePlanEntry> {
    history_editor::build_plan(&engine(), repo, "main")
        .expect("plan")
        .into_iter()
        .map(|e| entry(RebaseAction::Drop, &e.commit, &e.subject))
        .collect()
}

#[test]
fn executing_writes_the_backup_ref_at_the_pre_rebase_head() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = plan_repo(tmp.path(), "backup");
    let pre_head = run_git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    let plan = all_drop_plan(&repo);

    history_editor::execute_with_backup(
        &engine(),
        &repo,
        &plan,
        &VcsSettings::default(),
        "feature",
    )
    .expect("the guarded replay runs");

    let backup = run_git(&repo, &["rev-parse", BACKUP_REF])
        .trim()
        .to_string();
    assert_eq!(
        backup, pre_head,
        "the backup ref names the pre-rebase state"
    );
}

#[test]
fn abort_to_backup_restores_the_pre_rebase_state() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = plan_repo(tmp.path(), "restore");
    let pre_head = run_git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    let plan = all_drop_plan(&repo);
    history_editor::execute_with_backup(
        &engine(),
        &repo,
        &plan,
        &VcsSettings::default(),
        "feature",
    )
    .expect("the guarded replay runs");
    assert_ne!(
        run_git(&repo, &["rev-parse", "HEAD"]).trim(),
        pre_head,
        "the all-drop replay must actually have moved HEAD"
    );

    history_editor::abort_to_backup(&engine(), &repo).expect("abort restores");
    assert_eq!(
        run_git(&repo, &["rev-parse", "HEAD"]).trim(),
        pre_head,
        "HEAD is back at the pre-rebase tip"
    );
    // The safety net is spent after it restores: the ref goes with it.
    assert!(
        !Command::new("git")
            .args(["rev-parse", "--verify", "--quiet", BACKUP_REF])
            .current_dir(&repo)
            .output()
            .expect("spawning git")
            .status
            .success(),
        "the backup ref is deleted after a restore"
    );
}

#[test]
fn a_protected_branch_refuses_before_any_backup_ref_is_written() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = plan_repo(tmp.path(), "protected");
    let settings = VcsSettings {
        protected_branch_patterns: vec!["feature".to_string()],
        ..Default::default()
    };
    let plan = all_drop_plan(&repo);

    let err = history_editor::execute_with_backup(&engine(), &repo, &plan, &settings, "feature")
        .expect_err("a protected branch must refuse");
    assert!(err.to_string().contains("protected"), "{err}");
    assert!(
        !Command::new("git")
            .args(["rev-parse", "--verify", "--quiet", BACKUP_REF])
            .current_dir(&repo)
            .output()
            .expect("spawning git")
            .status
            .success(),
        "no backup ref may exist for a refused rewrite"
    );
}

// ------------------------------------------------ open helpers & estimate --

#[test]
fn base_of_is_the_selected_commit_s_first_parent() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = plan_repo(tmp.path(), "base-of");
    let head = run_git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    let parent = run_git(&repo, &["rev-parse", "HEAD~1"]).trim().to_string();

    let base = history_editor::base_of(&engine(), &repo, &head).expect("HEAD has a parent");
    assert_eq!(
        base, parent,
        "the default base is the commit's first parent"
    );
    assert!(
        history_editor::base_of(&engine(), &repo, "0123456789abcdef").is_none(),
        "an unknown commit has no base"
    );
}

#[test]
fn the_estimate_counts_the_replayed_commits_at_three_seconds_each() {
    let plan = vec![
        entry(RebaseAction::Pick, "aaaa1111", "feature-1"),
        entry(RebaseAction::Drop, "aaaa2222", "feature-2"),
        entry(RebaseAction::Squash, "aaaa3333", "feature-3"),
    ];
    // Two commits actually replay (the drop never runs) — ~6s.
    assert_eq!(history_editor::estimate(&plan), "~6s estimated");
}
