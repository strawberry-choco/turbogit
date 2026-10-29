use std::path::{Path, PathBuf};

use tempfile::TempDir;
use test_support::git_seed::{self, GitEnv, commit, git_with_env};
use turbogit_app::activity::ActivityKind;
use turbogit_app::operation::Operation;
use turbogit_app::root_caches::Affected;
use turbogit_app::shell_writes::commit as commit_writes;
use turbogit_app::state::{AppState, Dialog, UnmergedCommit, UnmergedCommits};
use turbogit_domain::model::{
    MergeOpts, RebaseAction, RebaseOpts, RebasePlanEntry, RootId, Upstream,
};

fn rev(dir: &Path, r: &str) -> String {
    git_seed::git(dir, &["rev-parse", r]).trim().to_string()
}

fn conflicting_repo() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git_seed::repo_with_one_commit(tmp.path(), "alpha");
    commit(&repo, "README.md", "base\n", "A: base");
    let base = rev(&repo, "HEAD");
    commit(&repo, "README.md", "main side\n", "M: main side");
    git_with_env(
        &repo,
        &["checkout", "-q", "-b", "feature", &base],
        &GitEnv::fixture(),
    );
    commit(&repo, "README.md", "feature side\n", "F: feature side");
    (tmp, repo)
}

fn interactive_plan_conflicting(repo: &Path) -> Vec<RebasePlanEntry> {
    let shas = [rev(repo, "main"), rev(repo, "HEAD")];
    shas.into_iter()
        .map(|commit| RebasePlanEntry {
            action: RebaseAction::Pick,
            commit,
            subject: String::new(),
            message: None,
        })
        .collect()
}

fn assert_repo_is_conflicted(repo: &Path) {
    let text = git_seed::git(repo, &["status", "--porcelain"]);
    assert!(
        text.contains("UU"),
        "the rebase must leave a conflicted file, got:\n{text}"
    );
}

fn project_with_origin(name: &str) -> (TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let work = test_support::git_seed::repo_with_origin(&project, name);
    (tmp, project, work)
}

fn repo_with_origin(project: &Path, name: &str) -> PathBuf {
    test_support::git_seed::repo_with_origin(project, name)
}

fn push_branch_to_origin(project: &Path, name: &str, branch: &str) {
    let work = project.join(name);
    git_with_env(&work, &["branch", branch, "HEAD"], &GitEnv::fixture());
    git_with_env(&work, &["push", "-q", "origin", branch], &GitEnv::fixture());
    git_with_env(&work, &["branch", "-D", branch], &GitEnv::fixture());
    git_with_env(
        &work,
        &["update-ref", "-d", &format!("refs/remotes/origin/{branch}")],
        &GitEnv::fixture(),
    );
}

fn registered_root(state: &AppState, path: &Path) -> RootId {
    state
        .multi
        .roots
        .iter()
        .find(|r| r.path == path)
        .unwrap_or_else(|| panic!("{} registered", path.display()))
        .id
        .clone()
}

fn dispatch_and_settle(state: &mut AppState, op: Operation) {
    state.dispatch(op);
    state.drain_events();
    assert!(
        !state.ui.busy,
        "the dispatched operation settled its busy flag; feed {:?}",
        feed(state)
    );
}

fn feed(state: &AppState) -> Vec<String> {
    state
        .ui
        .activity
        .entries
        .iter()
        .map(|e| e.message.clone())
        .collect()
}

#[test]
fn every_rebase_flavour_that_stops_on_conflicts_opens_the_resolver() {
    for kind in [
        "standard",
        "autosquash",
        "interactive",
        "interactive with backup",
    ] {
        let (_tmp, repo) = conflicting_repo();
        let mut state = AppState::for_roots(repo.parent().unwrap(), std::slice::from_ref(&repo));
        let root = registered_root(&state, &repo);
        let settings = state.settings.clone();
        let plan = interactive_plan_conflicting(&repo);

        let op = match kind {
            "standard" => {
                Operation::rebase_onto(&root, "feature", "main", &RebaseOpts::default(), &settings)
            }
            "autosquash" => Operation::rebase_autosquash(
                &root,
                "feature",
                "main",
                &RebaseOpts {
                    autosquash: true,
                    ..Default::default()
                },
                &settings,
            ),
            "interactive" => {
                Operation::rebase_interactive(&root, "feature", plan, &settings, false)
            }
            _ => Operation::rebase_interactive(&root, "feature", plan, &settings, true),
        };
        dispatch_and_settle(&mut state, op);

        assert_repo_is_conflicted(&repo);
        assert!(
            state.ui.conflict_resolver_open,
            "{kind} rebase stopped on conflicts and settled as {kind:?} — the feed read {:?}",
            feed(&state)
        );
        assert!(
            state.ui.merge_in_progress,
            "{kind} rebase left an explicit mid-operation state"
        );
    }
}

#[test]
fn a_rebase_whose_label_names_both_sides_still_hands_off() {
    let (_tmp, repo) = conflicting_repo();
    let mut state = AppState::for_roots(repo.parent().unwrap(), std::slice::from_ref(&repo));
    let root = registered_root(&state, &repo);

    dispatch_and_settle(
        &mut state,
        Operation::rebase_branch_onto(&root, "feature", "main"),
    );

    assert_repo_is_conflicted(&repo);
    assert!(
        state.ui.conflict_resolver_open,
        "\"Rebase feature onto main\" hands off like any other rebase; the feed \
         read {:?}",
        feed(&state)
    );
}

#[test]
fn a_merge_that_stops_on_conflicts_opens_the_resolver() {
    let (_tmp, repo) = conflicting_repo();
    git_with_env(&repo, &["checkout", "-q", "main"], &GitEnv::fixture());
    let mut state = AppState::for_roots(repo.parent().unwrap(), std::slice::from_ref(&repo));
    let root = registered_root(&state, &repo);

    dispatch_and_settle(
        &mut state,
        Operation::Merge {
            root,
            target: "feature".into(),
            opts: MergeOpts::default(),
            clean: turbogit_domain::model::CleanTreeMethod::Stash,
            commits: 1,
        },
    );

    assert!(
        state.ui.conflict_resolver_open,
        "a conflicted merge hands off; the feed read {:?}",
        feed(&state)
    );
    assert!(
        state
            .ui
            .activity
            .entries
            .iter()
            .any(|e| e.kind == ActivityKind::Warning && e.message.contains("unresolved conflicts")),
        "the merge reports the conflicts it left; the feed read {:?}",
        feed(&state)
    );
}

#[test]
fn a_fetch_reports_what_it_brought_in_whatever_its_scope() {
    for scope in ["one root", "every root"] {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().to_path_buf();
        let alpha = repo_with_origin(&project, "alpha");
        push_branch_to_origin(&project, "alpha", "fresh-alpha");
        let roots = match scope {
            "one root" => vec![alpha.clone()],
            _ => {
                let beta = repo_with_origin(&project, "beta");
                push_branch_to_origin(&project, "beta", "fresh-beta");
                vec![alpha.clone(), beta]
            }
        };
        let mut state = AppState::for_roots(&project, &roots);

        let roots = roots
            .iter()
            .map(|p| registered_root(&state, p))
            .collect::<Vec<_>>();
        dispatch_and_settle(&mut state, Operation::Fetch { roots });

        let reported = feed(&state)
            .into_iter()
            .filter(|m| m.starts_with("Fetch"))
            .collect::<Vec<_>>();
        assert!(
            reported.iter().any(|m| m.contains("new remote branches")),
            "a fetch of {scope} reports the branches it brought in; the feed \
             read {reported:?}"
        );
    }
}

#[test]
fn a_fetch_that_changed_nothing_says_so() {
    let (_tmp, project, alpha) = project_with_origin("alpha");
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    let root = registered_root(&state, &alpha);

    dispatch_and_settle(&mut state, Operation::Fetch { roots: vec![root] });

    assert!(
        feed(&state).contains(&"Fetch · nothing changed".to_string()),
        "never a bare success, never silence; the feed read {:?}",
        feed(&state)
    );
}

#[test]
fn each_operation_kind_settles_to_its_own_report() {
    for row in [
        Row {
            what: "fetch",
            prepare: nothing_prepared,
            op: |root, _| Operation::Fetch {
                roots: vec![root.clone()],
            },
            expect: "Fetch · nothing changed",
        },
        Row {
            what: "merge",
            prepare: nothing_prepared,
            op: |root, settings| Operation::Merge {
                root: root.clone(),
                target: "merged-branch".into(),
                opts: MergeOpts::default(),
                clean: settings.clean_tree_method,
                commits: 0,
            },
            expect: "Merge merged-branch · nothing to push",
        },
        Row {
            what: "rebase",
            prepare: |repo| {
                git_with_env(
                    repo,
                    &["checkout", "-q", "merged-branch"],
                    &GitEnv::fixture(),
                );
            },
            op: |root, settings| {
                Operation::rebase_onto(
                    root,
                    "merged-branch",
                    "main",
                    &RebaseOpts::default(),
                    settings,
                )
            },
            expect: "Rebase onto main · nothing to push",
        },
        Row {
            what: "branch delete",
            prepare: nothing_prepared,
            op: |root, _| Operation::DeleteBranch {
                root: root.clone(),
                name: "spare".into(),
            },
            expect: "Delete branch spare",
        },
        Row {
            what: "shelve",
            prepare: |repo| {
                std::fs::write(repo.join("NOTES.md"), "to shelve\n").unwrap();
                git_with_env(repo, &["add", "."], &GitEnv::fixture());
            },
            op: |root, _| Operation::shelve(root.clone(), Vec::new(), "Shelved before sweep"),
            expect: "Shelve",
        },
        Row {
            what: "worktree add",
            prepare: nothing_prepared,
            op: |root, settings| {
                let _ = settings;
                Operation::WorktreeAdd {
                    root: root.clone(),
                    path: root.as_path().parent().unwrap().join("wt-table"),
                    branch: "wt-table-branch".into(),
                }
            },
            expect: "Add worktree wt-table-branch",
        },
        Row {
            what: "display-only work",
            prepare: nothing_prepared,
            op: |root, _| {
                Operation::custom(
                    "Say something nobody inspects",
                    Affected::Root(root.clone()),
                    |_| Ok(()),
                )
            },
            expect: "Say something nobody inspects",
        },
    ] {
        let (_tmp, project, alpha) = project_with_origin("alpha");
        git_with_env(&alpha, &["branch", "merged-branch"], &GitEnv::fixture());
        git_with_env(&alpha, &["branch", "spare"], &GitEnv::fixture());
        (row.prepare)(&alpha);
        let mut state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
        let root = registered_root(&state, &alpha);
        let settings = state.settings.clone();

        dispatch_and_settle(&mut state, (row.op)(&root, &settings));

        let entries = &state.ui.activity.entries;
        assert_eq!(
            entries.len(),
            1,
            "one operation, one feed entry for {:?}",
            row.what
        );
        assert_eq!(
            entries[0].kind,
            ActivityKind::Success,
            "{} settled as: {:?}",
            row.what,
            entries[0].message
        );
        assert_eq!(
            entries[0].message, row.expect,
            "{} settled to the wrong report",
            row.what
        );
        assert_eq!(entries[0].repo.as_deref(), Some("alpha"));
    }
}

struct Row {
    what: &'static str,
    prepare: fn(&Path),
    op: fn(&RootId, &turbogit_domain::model::VcsSettings) -> Operation,
    expect: &'static str,
}

fn nothing_prepared(_: &Path) {}

#[test]
fn a_worktree_mutation_invalidates_after_it_lands() {
    let (_tmp, project, alpha) = project_with_origin("alpha");
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    let root = registered_root(&state, &alpha);

    dispatch_and_settle(
        &mut state,
        Operation::WorktreeAdd {
            root: root.clone(),
            path: project.join("wt-ordering"),
            branch: "wt-ordering-branch".into(),
        },
    );

    assert!(
        project.join("wt-ordering").exists(),
        "the worktree was created"
    );
    state
        .caches
        .store_worktrees(root.clone(), vec![stale_worktree(&alpha, &root)]);
    assert!(
        state.caches.worktrees(&root).is_some(),
        "the pre-mutation list is cached before the add"
    );

    dispatch_and_settle(
        &mut state,
        Operation::WorktreeRemove {
            root: root.clone(),
            path: project.join("wt-ordering"),
        },
    );
    assert!(
        state.caches.worktrees(&root).is_none(),
        "the mutation dropped the cached list rather than refreshing under it"
    );
}

fn stale_worktree(repo: &Path, root: &RootId) -> turbogit_domain::model::Worktree {
    turbogit_domain::model::Worktree {
        path: repo.join("gone"),
        branch: "stale".into(),
        dirty: None,
        root: root.clone(),
    }
}

#[test]
fn a_deleted_branch_arms_the_undo() {
    let (_tmp, project, alpha) = project_with_origin("alpha");
    git_with_env(&alpha, &["branch", "doomed"], &GitEnv::fixture());
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    let root = registered_root(&state, &alpha);

    state.ui.branches_delete_pending = Some(turbogit_app::state::BranchDeletePending {
        root: root.clone(),
        name: "doomed".into(),
        tip_sha: rev(&alpha, "doomed"),
        upstream: Some(Upstream {
            remote: "origin".into(),
            branch: "doomed".into(),
        }),
        unmerged: Some(UnmergedCommits {
            root: root.clone(),
            branch: "doomed".into(),
            merged_target: "main".into(),
            tip_sha: Some(rev(&alpha, "doomed")),
            commits: vec![UnmergedCommit {
                id: "abc".into(),
                subject: Some("the one that was lost".into()),
            }],
        }),
    });

    dispatch_and_settle(
        &mut state,
        Operation::DeleteBranch {
            root,
            name: "doomed".into(),
        },
    );

    let undo = state
        .ui
        .branches_undo
        .clone()
        .expect("the deletion armed the undo window");
    assert_eq!(undo.name, "doomed");
    assert_eq!(undo.root, registered_root(&state, &alpha));
    assert_eq!(
        undo.upstream.map(|u| u.branch),
        Some("doomed".to_string()),
        "the offer to remove the tracking remote branch survives the delete"
    );
    assert_eq!(
        undo.unmerged.as_ref().map(|u| u.commits.len()),
        Some(1),
        "the list of what the delete discarded survives the delete"
    );
}

#[test]
fn an_unrelated_operation_does_not_arm_the_undo() {
    let (_tmp, project, alpha) = project_with_origin("alpha");
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    let root = registered_root(&state, &alpha);

    state.ui.branches_delete_pending = Some(turbogit_app::state::BranchDeletePending {
        root: root.clone(),
        name: "doomed".into(),
        tip_sha: rev(&alpha, "main"),
        upstream: None,
        unmerged: None,
    });

    dispatch_and_settle(&mut state, Operation::Fetch { roots: vec![root] });

    assert!(
        state.ui.branches_undo.is_none(),
        "a fetch cannot arm a branch-deletion undo"
    );
}

fn project_with_history(name: &str) -> (TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let work = test_support::git_seed::repo_with_history(&project, name);
    (tmp, project, work)
}

fn short(sha: &str) -> String {
    sha.chars().take(7).collect()
}

fn subjects(dir: &Path) -> Vec<String> {
    git_seed::git(dir, &["log", "--format=%s"])
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn a_dropped_commit_settles_as_a_history_rewrite_and_refreshes_the_log() {
    let (_tmp, project, alpha) = project_with_history("alpha");
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    let root = registered_root(&state, &alpha);
    let settings = state.settings.clone();
    let target = rev(&alpha, "HEAD~1");

    dispatch_and_settle(
        &mut state,
        Operation::drop_commit(&root, "feature", &target, &settings),
    );

    let entries = &state.ui.activity.entries;
    assert_eq!(entries.len(), 1, "the feed read {:?}", feed(&state));
    assert_eq!(
        entries[0].kind,
        ActivityKind::Success,
        "the feed read {:?}",
        feed(&state)
    );
    assert_eq!(
        entries[0].message,
        format!("Drop commit {}", short(&target)),
        "the activity entry names the short reference the item showed"
    );
    assert_eq!(entries[0].repo.as_deref(), Some("alpha"));
    let cached = state
        .caches
        .log(&root)
        .expect("the completion left the root's log cached afresh");
    let held: Vec<&str> = cached
        .iter()
        .map(|c| c.message.lines().next().unwrap_or_default())
        .collect();
    assert!(
        !held.contains(&"feature-2") && held.contains(&"feature-3"),
        "the log the shell is reading matches the repository: {held:?}"
    );
    assert_eq!(
        subjects(&alpha),
        held.iter().map(|s| s.to_string()).collect::<Vec<String>>(),
        "and that cached history is the repository's own history"
    );
}

#[test]
fn a_reworded_commit_settles_without_leaking_its_new_message() {
    let (_tmp, project, alpha) = project_with_history("alpha");
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    let root = registered_root(&state, &alpha);
    let settings = state.settings.clone();
    let target = rev(&alpha, "HEAD");

    dispatch_and_settle(
        &mut state,
        Operation::reword_commit(&root, "feature", &target, "a corrected subject", &settings),
    );

    let entries = &state.ui.activity.entries;
    assert_eq!(entries[0].kind, ActivityKind::Success);
    assert_eq!(
        entries[0].message,
        format!("Reword commit {}", short(&target))
    );
    assert!(
        !entries[0].message.contains("corrected"),
        "the message is content, not part of the record: {:?}",
        entries[0].message
    );
    let text = git_seed::git(&alpha, &["log", "--format=%s", "-1"]);
    assert_eq!(text.trim(), "a corrected subject");
}

#[test]
fn a_refused_rewrite_is_reported_as_a_failure_of_its_own_kind() {
    let (_tmp, project, alpha) = project_with_origin("alpha");
    commit(&alpha, "README.md", "y\n", "second");
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    let root = registered_root(&state, &alpha);
    let target = rev(&alpha, "HEAD");
    let settings = state.settings.clone();
    let history_before = subjects(&alpha);

    dispatch_and_settle(
        &mut state,
        Operation::drop_commit(&root, "main", &target, &settings),
    );

    let entries = &state.ui.activity.entries;
    assert_eq!(entries.len(), 1, "the feed read {:?}", feed(&state));
    assert_eq!(entries[0].kind, ActivityKind::Error);
    assert!(
        entries[0]
            .message
            .starts_with(&format!("Drop commit {}:", short(&target))),
        "the failure names the rewrite it belongs to: {:?}",
        entries[0].message
    );
    assert_eq!(
        subjects(&alpha),
        history_before,
        "and the log the developer is reading still describes the repository"
    );
    let plan = state
        .history_verb_plan(&root, &target, RebaseAction::Drop, None)
        .expect("a plan for the target");
    state.arm_reselect_after_rewrite(
        root.clone(),
        plan,
        target.clone(),
        turbogit_services::reselection::RewrittenAnchor::Dropped,
    );
    assert!(
        state.ui.pending_reselect.is_some(),
        "the reselect is armed before the dispatch, never after"
    );
    dispatch_and_settle(
        &mut state,
        Operation::drop_commit(&root, "main", &target, &settings),
    );
    assert!(
        state.ui.pending_reselect.is_none(),
        "a refused rewrite moved nothing, so nothing follows it"
    );
    assert_eq!(
        state.ui.selected_commit.as_deref(),
        None,
        "and the selection is whatever the developer chose, not a rewrite's aftermath"
    );
}

#[test]
fn a_failed_operation_names_the_operation_and_the_failure() {
    let (_tmp, project, alpha) = project_with_origin("alpha");
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    let root = registered_root(&state, &alpha);

    dispatch_and_settle(
        &mut state,
        Operation::Fetch {
            roots: vec![RootId(
                PathBuf::from("/nonexistent/root/never/existed").into(),
            )],
        },
    );

    let entries = &state.ui.activity.entries;
    assert_eq!(entries.len(), 1, "the feed read {:?}", feed(&state));
    assert_eq!(entries[0].kind, ActivityKind::Error);
    assert!(
        entries[0].message.starts_with("Fetch:"),
        "the failure names the operation: {:?}",
        entries[0].message
    );
    assert!(
        !state.ui.conflict_resolver_open,
        "a failed fetch is not a conflict hand-off"
    );
    let _ = root;
}

#[test]
fn a_landed_commit_that_owed_an_open_opens_the_push_dialog() {
    let (_tmp, project, alpha) = project_with_origin("alpha");
    std::fs::write(alpha.join("chained.txt"), "chained\n").unwrap();
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    let root = registered_root(&state, &alpha);

    dispatch_and_settle(
        &mut state,
        commit_writes::set_staged(&root, Path::new("chained.txt"), true),
    );

    state.dispatch_commit(&root, "the chained subject", false, false, true);
    state.drain_events();
    assert!(
        !state.ui.busy,
        "the commit settled; the feed read {:?}",
        feed(&state)
    );

    assert_eq!(
        state.ui.dialog,
        Some(Dialog::Push),
        "the chain opens its dialog on the commit's completion; the feed read {:?}",
        feed(&state)
    );
    assert!(
        state.ui.commit_push_dialog_pending.is_none(),
        "the completion spent the ask rather than leaving it armed"
    );
    assert!(
        subjects(&alpha).contains(&"the chained subject".to_string()),
        "and the commit it chained from is really in the repository: {:?}",
        subjects(&alpha)
    );
}

#[test]
fn a_commit_that_fails_opens_nothing_and_drops_the_ask() {
    let (_tmp, project, alpha) = project_with_origin("alpha");
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    let root = registered_root(&state, &alpha);
    let history_before = subjects(&alpha);

    state.dispatch_commit(&root, "nothing is staged", false, false, true);
    state.drain_events();

    let entries = &state.ui.activity.entries;
    assert_eq!(entries.len(), 1, "the feed read {:?}", feed(&state));
    assert_eq!(entries[0].kind, ActivityKind::Error);
    assert!(
        entries[0].message.starts_with("Commit:"),
        "the failure names the commit it belongs to: {:?}",
        entries[0].message
    );
    assert_eq!(
        state.ui.dialog, None,
        "a commit that did not land owes no push dialog"
    );
    assert!(
        state.ui.commit_push_dialog_pending.is_none(),
        "and the ask goes with the failure instead of standing over the next verb"
    );
    assert_eq!(
        subjects(&alpha),
        history_before,
        "nothing landed on the refused commit"
    );
}

#[test]
fn a_completion_of_another_kind_cannot_answer_the_owed_open() {
    for kind in ["a stage", "a fetch"] {
        let (_tmp, project, alpha) = project_with_origin("alpha");
        std::fs::write(alpha.join("other.txt"), "other\n").unwrap();
        let mut state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
        let root = registered_root(&state, &alpha);
        state.ui.commit_push_dialog_pending = Some(root.clone());

        let op = match kind {
            "a stage" => commit_writes::set_staged(&root, Path::new("other.txt"), true),
            _ => Operation::Fetch {
                roots: vec![root.clone()],
            },
        };
        dispatch_and_settle(&mut state, op);

        assert_eq!(
            state.ui.dialog,
            None,
            "{kind} cannot open the dialog a commit owed; the feed read {:?}",
            feed(&state)
        );
        assert_eq!(
            state.ui.commit_push_dialog_pending.as_ref(),
            Some(&root),
            "{kind} cannot spend it either; the feed read {:?}",
            feed(&state)
        );
    }
}

#[test]
fn a_plain_commit_clears_an_owed_open_instead_of_stranding_it() {
    let (_tmp, project, alpha) = project_with_origin("alpha");
    std::fs::write(alpha.join("plain.txt"), "plain\n").unwrap();
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    let root = registered_root(&state, &alpha);
    dispatch_and_settle(
        &mut state,
        commit_writes::set_staged(&root, Path::new("plain.txt"), true),
    );
    state.ui.commit_push_dialog_pending = Some(root.clone());

    state.dispatch_commit(&root, "a plain subject", false, false, false);
    state.drain_events();

    assert!(
        state.ui.commit_push_dialog_pending.is_none(),
        "a plain commit drops an ask it was not asked to answer"
    );
    assert_eq!(
        state.ui.dialog,
        None,
        "and its own completion must not answer the ask the plain click replaced: {:?}",
        feed(&state)
    );
    assert!(
        subjects(&alpha).contains(&"a plain subject".to_string()),
        "the plain commit still landed: {:?}",
        subjects(&alpha)
    );
}
