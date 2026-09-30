//! What a completed operation settles *as* is decided by the operation's
//! identity, never by the wording of its label (ADR-0020).
//!
//! Every claim here crosses one seam: the Headless harness over the app state —
//! `for_roots`, `dispatch`, `drain_events` — and asserts observable state after
//! settlement: which surface opened, what the activity feed gained, which report
//! text is present.
//!
//! The three rebase flavours and the all-roots fetch are the two defects this
//! design exists to make unrepresentable; each is paired with the flavour that
//! already settled, so a fixture problem cannot masquerade as the bug.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;
use test_support::git_seed::{self, commit};
use turbogit_app::activity::ActivityKind;
use turbogit_app::operation::Operation;
use turbogit_app::root_caches::Affected;
use turbogit_app::state::AppState;
use turbogit_domain::model::{MergeOpts, RebaseAction, RebaseOpts, RebasePlanEntry, RootId};

// --- git fixture ---------------------------------------------------------------

/// Run `git <args>` in `dir`, asserting success and discarding stdout.
///
/// Kept local because `GIT_EDITOR=true` is load-bearing — every rebase flavour
/// here is a rewrite, and a rewrite opens an editor unless one is suppressed —
/// and the per-call identity env is this suite's only identity source.
fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .env("GIT_EDITOR", "true")
        .output()
        .expect("git must be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `git rev-parse <rev>` — a full commit id.
fn rev(dir: &Path, r: &str) -> String {
    git_seed::git(dir, &["rev-parse", r]).trim().to_string()
}

/// A repository whose history offers a pair of commits touching the same lines
/// of one file, so replaying one in the other's place conflicts:
///
/// ```text
/// main     A ─── M   ("base" then "main side")
/// feature   ╲─── F   ("base" then "feature side")
/// ```
///
/// Ends checked out on `feature`.
///
/// Kept local, not `git_seed::repo_with_conflict`: that recipe leaves a merge
/// *in progress* with the conflict staged, and the tests below are about a
/// *rebase* that stops on conflicts — starting post-merge would make the rebase
/// a no-op.
fn conflicting_repo() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git_seed::repo_with_one_commit(tmp.path(), "alpha");
    commit(&repo, "README.md", "base\n", "A: base");
    let base = rev(&repo, "HEAD");
    commit(&repo, "README.md", "main side\n", "M: main side");
    git(&repo, &["checkout", "-q", "-b", "feature", &base]);
    commit(&repo, "README.md", "feature side\n", "F: feature side");
    (tmp, repo)
}

/// Replay `M` then `F`: `M` lands, `F` contradicts it, and the rebase stops with
/// `README.md` conflicted.
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

/// The repository left mid-rebase with one conflicted file.
fn assert_repo_is_conflicted(repo: &Path) {
    let text = git_seed::git(repo, &["status", "--porcelain"]);
    assert!(
        text.contains("UU"),
        "the rebase must leave a conflicted file, got:\n{text}"
    );
}

/// A repository with a same-content `origin`, so a fetch is a real operation
/// that changes nothing and ahead/behind is a known zero. The recipe is
/// `test_support::git_seed`'s, promoted there so the UI suites rewrite the same
/// history this one does rather than a lookalike; this wrapper keeps the
/// suite's own tempdir.
fn project_with_origin(name: &str) -> (TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let work = test_support::git_seed::repo_with_origin(&project, name);
    (tmp, project, work)
}

/// One bare `origin` plus one local repo `name` on `main`, pushed to it.
fn repo_with_origin(project: &Path, name: &str) -> PathBuf {
    test_support::git_seed::repo_with_origin(project, name)
}

/// A branch that exists only on `origin`, so the next fetch discovers it.
fn push_branch_to_origin(project: &Path, name: &str, branch: &str) {
    let work = project.join(name);
    git(&work, &["branch", branch, "HEAD"]);
    git(&work, &["push", "-q", "origin", branch]);
    git(&work, &["branch", "-D", branch]);
    git(
        &work,
        &["update-ref", "-d", &format!("refs/remotes/origin/{branch}")],
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

// --- pumping -------------------------------------------------------------------

/// Dispatch an operation and settle it, the way the shell's next frame does.
///
/// The headless harness runs git work inline, so the completion is already in
/// the channel by the time `dispatch` returns, and one drain applies it —
/// including anything that settlement itself queues.
fn dispatch_and_settle(state: &mut AppState, op: Operation) {
    state.dispatch(op);
    state.drain_events();
    assert!(
        !state.ui.busy,
        "the dispatched operation settled its busy flag; feed {:?}",
        feed(state)
    );
}

/// The activity entries the feed gained, oldest first.
fn feed(state: &AppState) -> Vec<String> {
    state
        .ui
        .activity
        .entries
        .iter()
        .map(|e| e.message.clone())
        .collect()
}

// --- the rebase conflict hand-off ----------------------------------------------

/// Every rebase flavour that stops on conflicts hands off to the conflict
/// resolver — including the two whose labels used to miss the settlement test.
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

/// A fourth flavour added later cannot miss the hand-off: settlement matched
/// `Rebase`, not a kind. Asserted by driving the branch-rewriting flavour the
/// branch tree uses, whose label names both sides.
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

/// A merge that stops on conflicts hands off too, and its warning-toned report
/// names the conflict count.
#[test]
fn a_merge_that_stops_on_conflicts_opens_the_resolver() {
    let (_tmp, repo) = conflicting_repo();
    git(&repo, &["checkout", "-q", "main"]);
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

// --- the fetch report ----------------------------------------------------------

/// A fetch reports what it brought in whether it covered one root or several —
/// the multi-root case used to report nothing while the single-root one
/// reported.
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

/// A fetch that brought nothing in still says so, rather than going quiet.
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

// --- settlement as a table -----------------------------------------------------

/// One row per named operation: what a *successful* completion leaves in the
/// feed. The exhaustive settlement match is what lets a row be added here
/// without adding machinery.
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
            // Nothing came in, and the branch is level with its upstream.
            expect: "Merge merged-branch · nothing to push",
        },
        Row {
            // Rebased from a branch that is not `main`: the default settings
            // protect `main`, and a protected branch refuses before git is
            // touched, which would settle the row as a failure.
            what: "rebase",
            prepare: |repo| git(repo, &["checkout", "-q", "merged-branch"]),
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
                git(repo, &["add", "."]);
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
        // A branch pointing at HEAD: merging or replaying it changes nothing,
        // so every row's expected report is a literal rather than a count that
        // depends on how the fixture happened to land.
        git(&alpha, &["branch", "merged-branch"]);
        git(&alpha, &["branch", "spare"]);
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

/// A row of the settlement table.
struct Row {
    what: &'static str,
    /// Extra worktree state the operation needs. Only Shelve dirties the tree:
    /// a staged change there is enough to make a rebase refuse to run at all,
    /// which would settle the row as a failure rather than a report.
    prepare: fn(&Path),
    op: fn(&RootId, &turbogit_domain::model::VcsSettings) -> Operation,
    expect: &'static str,
}

fn nothing_prepared(_: &Path) {}

/// Worktree-mutating operations invalidate the cached list *after* the work
/// lands, which is the ordering ADR-0019 requires. A completion that never
/// emits the second event would leave a stale list cached forever.
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
    // Priming the cache with a pre-mutation list is what an eager per-frame
    // fill does; invalidating at dispatch time instead of after the work would
    // leave exactly this stale entry cached forever (ADR-0019).
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

/// A one-row worktree list that no longer describes the repository.
fn stale_worktree(repo: &Path, root: &RootId) -> turbogit_domain::model::Worktree {
    turbogit_domain::model::Worktree {
        path: repo.join("gone"),
        branch: "stale".into(),
        dirty: None,
        root: root.clone(),
    }
}

// --- the undo arm --------------------------------------------------------------

/// A confirmed local-branch deletion arms the short-window undo with the tip it
/// captured. The arm used to read `Delete branch ` off the label.
#[test]
fn a_deleted_branch_arms_the_undo() {
    let (_tmp, project, alpha) = project_with_origin("alpha");
    git(&alpha, &["branch", "doomed"]);
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    let root = registered_root(&state, &alpha);

    state.ui.branches_delete_pending = Some(turbogit_app::state::BranchDeletePending {
        root: root.clone(),
        name: "doomed".into(),
        tip_sha: rev(&alpha, "doomed"),
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
}

/// An unrelated operation must not consume the pending deletion without arming
/// anything — the shape the label test used to get wrong in both directions.
#[test]
fn an_unrelated_operation_does_not_arm_the_undo() {
    let (_tmp, project, alpha) = project_with_origin("alpha");
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    let root = registered_root(&state, &alpha);

    state.ui.branches_delete_pending = Some(turbogit_app::state::BranchDeletePending {
        root: root.clone(),
        name: "doomed".into(),
        tip_sha: rev(&alpha, "main"),
    });

    dispatch_and_settle(&mut state, Operation::Fetch { roots: vec![root] });

    assert!(
        state.ui.branches_undo.is_none(),
        "a fetch cannot arm a branch-deletion undo"
    );
}

// --- the two targeted history rewrites ----------------------------------------------

/// `alpha` plus a `feature` branch three commits deep and checked out — history
/// a targeted rewrite may act on without tripping the protected-branch guard.
/// Each commit touches its OWN path: replaying a descendant over a dropped
/// neighbour must not conflict, or the test measures git's merge machinery
/// rather than the operation's settlement.
fn project_with_history(name: &str) -> (TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let work = test_support::git_seed::repo_with_history(&project, name);
    (tmp, project, work)
}

fn short(sha: &str) -> String {
    sha.chars().take(7).collect()
}

/// The subjects a repository's log holds, newest first.
fn subjects(dir: &Path) -> Vec<String> {
    git_seed::git(dir, &["log", "--format=%s"])
        .lines()
        .map(str::to_string)
        .collect()
}

/// A dropped commit settles as its own kind of work: one feed entry naming the
/// short reference and the root it moved, and the completion — not the log
/// surface — dropping the cached history so the next read answers from git.
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
    // The cached history the Git Log reads is the refreshed one. Nothing in this
    // test asks the log surface for anything: completion invalidated the root
    // and the ordinary rescan refilled it.
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

/// A reworded commit settles the same way, and the feed never carries the new
/// message — it records the work, not the text.
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

/// A rewrite the settings refuse is reported as a failure, so the log is never
/// left believing a commit is gone. It is still its own kind failing, and the
/// error entry names it.
#[test]
fn a_refused_rewrite_is_reported_as_a_failure_of_its_own_kind() {
    let (_tmp, project, alpha) = project_with_origin("alpha");
    // A second commit, so the target has a first parent and a plan can be built
    // for it at all: a root commit is refused before any plan exists.
    commit(&alpha, "README.md", "y\n", "second");
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    let root = registered_root(&state, &alpha);
    // `main` is protected by the default settings, and HEAD is on it.
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
    // Armed the way the preflight arms it, so the refusal has something to clear:
    // a rewrite that did not happen must leave nothing waiting to move a
    // selection, and the commit the developer was looking at still selected.
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

// --- a failed operation --------------------------------------------------------

/// A failure reports the operation and the git error, and leaves the conflict
/// resolver alone.
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
