#![allow(dead_code)]

use std::path::Path;
use std::process::Command;

use test_support::git_seed::git;
use turbogit_domain::model::{
    Commit, RebaseAction, RebasePlanEntry, RootId, Signature, SignatureState, VcsSettings,
};
use turbogit_engine::GitExecutor;
use turbogit_engine::cli::CliExecutor;
use turbogit_services::history_editor;
use turbogit_services::reselection::{self, RewrittenAnchor};

fn append_commit(dir: &Path, name: &str, text: &str) -> String {
    let file = dir.join(name);
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
        .expect("opening work file");
    use std::io::Write;
    writeln!(f, "{text}").expect("appending work file");
    drop(f);
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", text]);
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

fn engine() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

fn plan_repo(tmp: &Path, name: &str) -> std::path::PathBuf {
    let repo = tmp.join(name);
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    append_commit(&repo, "base.txt", "base");
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    append_commit(&repo, "a.txt", "feature-1");
    append_commit(&repo, "b.txt", "feature-2");
    append_commit(&repo, "c.txt", "feature-3");
    repo
}

fn entry(action: RebaseAction, commit: &str, subject: &str) -> RebasePlanEntry {
    RebasePlanEntry {
        action,
        commit: commit.to_string(),
        subject: subject.to_string(),
        message: None,
    }
}

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

/// Git's rebase-todo format has no slot for a replacement message, so a plan
/// that carries one does NOT round-trip through the editor's buffer: the verb
/// survives, the message is dropped rather than erroring. This is the contract
/// ADR-0025 accepts to keep history editing on one path, the editor's buffer
/// stays a list of verbs, never a message store.
#[test]
fn a_reword_message_survives_as_a_verb_and_not_as_text() {
    let plan = vec![reword("aaaa1111", "new subject\n\nand a body")];

    let text = history_editor::render_todo(&plan);
    assert_eq!(
        text.trim(),
        "reword aaaa1111 new subject",
        "the rendered todo carries the verb, the sha and the subject — nothing else"
    );

    let parsed = history_editor::parse_todo(&text).expect("a rendered plan always parses back");
    assert_eq!(parsed[0].action, RebaseAction::Reword);
    assert_eq!(
        parsed[0].message, None,
        "the replacement message is deliberately lost, not an error"
    );
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
fn an_unrecognised_verb_falls_back_to_unknown_and_survives_the_round_trip() {
    let plan = history_editor::parse_todo("rebase 9b2e440a docs: runbook")
        .expect("the row survives with an Unknown action");
    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].action, RebaseAction::Unknown);
    assert_eq!(plan[0].commit, "9b2e440a");

    let round_tripped = history_editor::parse_todo(&history_editor::render_todo(&plan))
        .expect("the rendered fallback re-parses");
    assert_eq!(round_tripped, plan);
}

#[test]
fn update_ref_parses_and_renders_like_any_verb() {
    let plan =
        history_editor::parse_todo("update-ref 9b2e440a docs: runbook").expect("update-ref parses");
    assert_eq!(plan[0].action, RebaseAction::UpdateRef);
    assert_eq!(
        history_editor::render_todo(&plan).trim(),
        "update-ref 9b2e440a docs: runbook",
        "the rendered todo round-trips"
    );
    assert_eq!(
        history_editor::parse_todo("u 9b2e440a docs: runbook").unwrap()[0].action,
        RebaseAction::UpdateRef,
        "git's own single-letter abbreviation parses too"
    );
    let preview = history_editor::plan_preview(&plan);
    assert_eq!(preview.after, 1);
    assert_eq!(preview.kept, plan);
}

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

use turbogit_services::history_editor::RebaseCaution;

#[test]
fn commits_touching_the_same_file_raise_the_conflict_caution() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = plan_repo(tmp.path(), "conflict");
    let head = append_commit(&repo, "a.txt", "feature-1 again");

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
    git(&repo, &["config", "user.name", "Other"]);
    append_commit(&repo, "d.txt", "feature-4");

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

fn backup_ref() -> String {
    engine().rewrite_backup_ref().to_string()
}

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
    let pre_head = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    let plan = all_drop_plan(&repo);

    history_editor::execute_with_backup(
        &engine(),
        &repo,
        &plan,
        &VcsSettings::default(),
        "feature",
    )
    .expect("the guarded replay runs");

    let backup = git(&repo, &["rev-parse", &backup_ref()]).trim().to_string();
    assert_eq!(
        backup, pre_head,
        "the backup ref names the pre-rebase state"
    );
}

#[test]
fn abort_to_backup_restores_the_pre_rebase_state() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = plan_repo(tmp.path(), "restore");
    let pre_head = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
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
        git(&repo, &["rev-parse", "HEAD"]).trim(),
        pre_head,
        "the all-drop replay must actually have moved HEAD"
    );

    history_editor::abort_to_backup(&engine(), &repo).expect("abort restores");
    assert_eq!(
        git(&repo, &["rev-parse", "HEAD"]).trim(),
        pre_head,
        "HEAD is back at the pre-rebase tip"
    );
    assert!(
        !Command::new("git")
            .args(["rev-parse", "--verify", "--quiet", &backup_ref()])
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
            .args(["rev-parse", "--verify", "--quiet", &backup_ref()])
            .current_dir(&repo)
            .output()
            .expect("spawning git")
            .status
            .success(),
        "no backup ref may exist for a refused rewrite"
    );
}

#[test]
fn base_of_is_the_selected_commit_s_first_parent() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = plan_repo(tmp.path(), "base-of");
    let head = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    let parent = git(&repo, &["rev-parse", "HEAD~1"]).trim().to_string();

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
    assert_eq!(history_editor::estimate(&plan), "~6s estimated");
}

#[test]
fn setting_pick_resets_a_modified_row_and_other_verbs_do_not() {
    let mut reset = vec![reword("aaaa1111", "new subject\n\nand a body")];
    history_editor::set_action(&mut reset, 0, RebaseAction::Pick);
    assert_eq!(reset[0].action, RebaseAction::Pick);
    assert_eq!(
        reset[0].message, None,
        "the reset drops the replacement message: the commit is applied as is"
    );

    let mut retargeted = vec![reword("aaaa1111", "new subject\n\nand a body")];
    history_editor::set_action(&mut retargeted, 0, RebaseAction::Squash);
    assert_eq!(
        retargeted[0].message.as_deref(),
        Some("new subject\n\nand a body"),
        "a retarget is not a reset: the row keeps what it carried"
    );
}

fn subject_of(dir: &Path, rev: &str) -> String {
    git(dir, &["log", "-1", "--format=%s", rev])
        .trim()
        .to_string()
}

fn message_of(dir: &Path, rev: &str) -> String {
    git(dir, &["log", "-1", "--format=%B", rev])
}

fn author_of(dir: &Path, rev: &str) -> String {
    git(dir, &["log", "-1", "--format=%an <%ae> %aI", rev])
        .trim()
        .to_string()
}

fn rev_parse(dir: &Path, rev: &str) -> String {
    git(dir, &["rev-parse", rev]).trim().to_string()
}

fn reword(commit: &str, message: &str) -> RebasePlanEntry {
    RebasePlanEntry {
        action: RebaseAction::Reword,
        commit: commit.to_string(),
        subject: message.lines().next().unwrap_or("").to_string(),
        message: Some(message.to_string()),
    }
}

#[test]
fn a_reword_in_the_plan_rewrites_the_message_and_nothing_else() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = plan_repo(tmp.path(), "reword-message");
    let target = rev_parse(&repo, "HEAD~1");
    let descendant = rev_parse(&repo, "HEAD");
    let tree = rev_parse(&repo, "HEAD~1^{tree}");
    let author = author_of(&repo, "HEAD~1");

    let plan = vec![
        reword(
            &target,
            "feature-2: corrected\n\nAnd a body while we are here.",
        ),
        entry(RebaseAction::Pick, &descendant, "feature-3"),
    ];
    history_editor::execute(&engine(), &repo, &plan).expect("the replay runs");

    assert_eq!(
        subject_of(&repo, "HEAD~1"),
        "feature-2: corrected",
        "the subject the plan carried is the subject git records"
    );
    assert!(
        message_of(&repo, "HEAD~1").contains("And a body while we are here."),
        "the body travels with the subject: {:?}",
        message_of(&repo, "HEAD~1")
    );
    let reworded = rev_parse(&repo, "HEAD~1");
    assert_ne!(reworded, target, "a reworded commit has a new hash");
    assert_eq!(
        rev_parse(&repo, "HEAD~1^{tree}"),
        tree,
        "correcting a message changes what the commit SAYS, not what it did"
    );
    assert_eq!(
        author_of(&repo, "HEAD~1"),
        author,
        "the author and the author date are not the reword's to change"
    );
    assert_eq!(
        subject_of(&repo, "HEAD"),
        "feature-3",
        "and the descendant is replayed on top of it"
    );
}

// Git's todo has no per-commit slot for "the refs touching this replay follow
// the rewrite" and no honest line for a verb the editor did not write, so both
// are settled where the plan meets git: an `UpdateRef` row replays as the pick
// it is and turns the replay into `--update-refs`, and an `Unknown` row is
// refused outright. Both assertions are read back from a real repository, the
// same discipline as the reword test above.

#[test]
fn an_update_ref_row_replays_with_update_refs_so_affected_branches_follow() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = plan_repo(tmp.path(), "update-refs");
    git(&repo, &["branch", "topic", "feature~1"]);

    let mut plan = history_editor::build_plan(&engine(), &repo, "main").expect("plan");
    plan[0].action = RebaseAction::UpdateRef;
    plan[1] = reword(&plan[1].commit, "feature-2: retitled\n\nAnd a body.");

    history_editor::execute(&engine(), &repo, &plan).expect("the replay runs");

    assert_eq!(subject_of(&repo, "HEAD~1"), "feature-2: retitled");
    assert_eq!(
        rev_parse(&repo, "topic"),
        rev_parse(&repo, "HEAD~1"),
        "the replay ran with --update-refs, so topic followed the reworded commit"
    );
}

#[test]
fn without_an_update_ref_row_the_replay_leaves_other_refs_alone() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = plan_repo(tmp.path(), "no-update-refs");
    git(&repo, &["branch", "topic", "feature~1"]);
    let old_first = rev_parse(&repo, "feature~1");

    let mut plan = history_editor::build_plan(&engine(), &repo, "main").expect("plan");
    plan[1] = reword(&plan[1].commit, "feature-2: retitled\n\nAnd a body.");

    history_editor::execute(&engine(), &repo, &plan).expect("the replay runs");

    assert_eq!(subject_of(&repo, "HEAD~1"), "feature-2: retitled");
    assert_eq!(
        rev_parse(&repo, "topic"),
        old_first,
        "no UpdateRef row, no --update-refs: the sibling branch keeps its own commit"
    );
    assert_ne!(
        rev_parse(&repo, "topic"),
        rev_parse(&repo, "HEAD~1"),
        "the reword really did move the commit, so the stale ref is genuinely left behind"
    );
}

#[test]
fn a_plan_carrying_an_unknown_row_is_refused_not_coerced() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = plan_repo(tmp.path(), "unknown-refused");
    let built = history_editor::build_plan(&engine(), &repo, "main").expect("plan");
    let plan = vec![entry(
        RebaseAction::Unknown,
        &built[1].commit,
        "docs: runbook",
    )];

    let err = history_editor::execute(&engine(), &repo, &plan)
        .expect_err("an unknown row must never reach git as another verb");
    assert!(err.to_string().contains("unknown"), "{err}");
    assert_eq!(
        subject_of(&repo, "HEAD"),
        "feature-3",
        "the refusal happens before git is touched"
    );
}

fn logged(id: &str, subject: &str) -> Commit {
    let who = Signature {
        name: "Author".into(),
        email: "author@example.com".into(),
        time: 1_700_000_000,
    };
    Commit {
        id: id.into(),
        parents: Vec::new(),
        author: who.clone(),
        committer: who,
        message: format!("{subject}\n\nThe body the pane still shows."),
        time: 1_700_000_000,
        root: RootId(std::sync::Arc::from(std::path::Path::new("/repo/alpha"))),
        signature: SignatureState::Unsigned,
    }
}

fn dropping(commit: &str, subject: &str) -> RebasePlanEntry {
    entry(RebaseAction::Drop, commit, subject)
}

#[test]
fn a_rewrite_moves_the_selection_to_the_commit_that_took_the_anchors_place() {
    struct Case {
        what: &'static str,
        plan: Vec<RebasePlanEntry>,
        after: Vec<Commit>,
        anchor: &'static str,
        fate: RewrittenAnchor,
        want: Option<&'static str>,
    }
    let cases = [
        Case {
            what: "a drop in the middle lands on its first descendant",
            plan: vec![
                entry(RebaseAction::Pick, "base0", "the base commit"),
                dropping("drop1", "the dropped commit"),
                entry(RebaseAction::Pick, "keep1", "the work on top"),
                entry(RebaseAction::Pick, "keep2", "more work"),
            ],
            after: vec![
                logged("new2", "more work"),
                logged("new1", "the work on top"),
                logged("base0", "the base commit"),
            ],
            anchor: "drop1",
            fate: RewrittenAnchor::Dropped,
            want: Some("new1"),
        },
        Case {
            what: "a drop of the tip lands on the commit that became the tip",
            plan: vec![
                entry(RebaseAction::Pick, "base0", "the base commit"),
                entry(RebaseAction::Pick, "keep1", "the work on top"),
                dropping("drop1", "the dropped tip"),
            ],
            after: vec![
                logged("new1", "the work on top"),
                logged("base0", "the base commit"),
            ],
            anchor: "drop1",
            fate: RewrittenAnchor::Dropped,
            want: Some("new1"),
        },
        Case {
            what: "a reword stays on the commit it rewrote",
            plan: vec![
                entry(RebaseAction::Pick, "base0", "the base commit"),
                reword("word1", "the corrected subject"),
                entry(RebaseAction::Pick, "keep1", "the work on top"),
            ],
            after: vec![
                logged("new1", "the work on top"),
                logged("new0", "the corrected subject"),
                logged("base0", "the base commit"),
            ],
            anchor: "word1",
            fate: RewrittenAnchor::Reworded {
                subject: "the corrected subject".into(),
            },
            want: Some("new0"),
        },
    ];
    for case in cases {
        assert_eq!(
            reselection::reselect_after_rewrite(&case.plan, &case.after, case.anchor, &case.fate),
            case.want.map(str::to_string),
            "{}",
            case.what
        );
    }
}

#[test]
fn a_repeated_subject_resolves_by_nearest_position_not_by_the_first_match() {
    let plan = vec![
        entry(RebaseAction::Pick, "base0", "the base commit"),
        dropping("drop1", "the dropped commit"),
        entry(RebaseAction::Pick, "twin1", "fix the flaky test"),
        entry(RebaseAction::Pick, "twin2", "fix the flaky test"),
        entry(RebaseAction::Pick, "tip0", "the tip commit"),
    ];
    let after = vec![
        logged("new0", "the tip commit"),
        logged("new1", "fix the flaky test"),
        logged("new2", "fix the flaky test"),
        logged("base0", "the base commit"),
    ];
    assert_eq!(
        reselection::reselect_after_rewrite(&plan, &after, "drop1", &RewrittenAnchor::Dropped),
        Some("new2".to_string()),
        "the twin nearest the anchor's own row took its place, not the first match"
    );
}

#[test]
fn a_reword_finds_its_own_commit_and_not_an_impostor_holding_the_old_subject() {
    let plan = vec![
        entry(RebaseAction::Pick, "base0", "the base commit"),
        entry(
            RebaseAction::Pick,
            "other0",
            "the subject the reword replaces",
        ),
        reword("word1", "the corrected subject"),
    ];
    let after = vec![
        logged("new0", "the corrected subject"),
        logged("other0", "the subject the reword replaces"),
        logged("base0", "the base commit"),
    ];
    assert_eq!(
        reselection::reselect_after_rewrite(
            &plan,
            &after,
            "word1",
            &RewrittenAnchor::Reworded {
                subject: "the corrected subject".into(),
            }
        ),
        Some("new0".to_string()),
        "the reworded commit itself, not the impostor holding its old subject"
    );
}

#[test]
fn a_rule_with_nothing_to_point_at_says_so() {
    let plan = vec![
        entry(RebaseAction::Pick, "base0", "the base commit"),
        dropping("drop1", "the dropped commit"),
    ];
    let after = vec![logged("base0", "the base commit")];

    assert_eq!(
        reselection::reselect_after_rewrite(
            &plan,
            &after,
            "some-other-commit",
            &RewrittenAnchor::Dropped
        ),
        None,
        "an unknown anchor has no place in this plan, so the rule declines"
    );
    let wide = vec![
        entry(RebaseAction::Pick, "base0", "the base commit"),
        dropping("drop1", "the dropped commit"),
        entry(RebaseAction::Pick, "keep1", "the work on top"),
    ];
    assert_eq!(
        reselection::reselect_after_rewrite(
            &wide,
            &[logged("unrelated", "some other repository\'s tip")],
            "drop1",
            &RewrittenAnchor::Dropped
        ),
        None,
        "a log that does not contain the row has no answer to give"
    );
}
