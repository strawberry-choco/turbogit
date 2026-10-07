//! The Log surface's named state machine at the app seam: which of the ten
//! states each independently-loaded thing reads as, what recovery it carries,
//! and that a failure names the operation that failed rather than the surface
//! it happened to be painted in.
//!
//! The machine's copy rule is the ticket's whole argument: four distinct error
//! states, two distinct empty states with different actions, and every
//! non-default state carrying its own recovery. Testing it here — with no
//! renderer — is what lets the four error states be checked *against each
//! other* rather than one at a time from a painted frame.

use std::path::{Path, PathBuf};

use turbogit_app::events::{AppEvent, LogBatchMode};
use turbogit_app::log_states::{self, LogRecovery, LogState, named_states};
use turbogit_app::root_caches::{Affected, LogRead, LogScope};
use turbogit_app::state::AppState;
use turbogit_domain::error::TgError;
use turbogit_domain::model::RootId;

// --- fixtures -------------------------------------------------------------------

fn git(dir: &Path, args: &[&str]) {
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
}

/// A repository with `n` commits, each writing `token<i>` into `f.txt`, so a
/// path scope and a pickaxe search both have real work to do.
fn seeded(project: &Path, n: usize) -> PathBuf {
    let repo = project.join("alpha");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    for i in 0..n {
        std::fs::write(repo.join("f.txt"), format!("token{i}\n")).unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", &format!("c{i}")]);
    }
    repo
}

/// A repository with an unborn HEAD: initialised, configured, never committed
/// in. The listing it answers with is empty, and that is an answer.
fn unborn(project: &Path) -> PathBuf {
    let repo = project.join("alpha");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    repo
}

/// One root behind the real CLI, with the log window's first batch settled.
fn loaded(project: &Path, commits: usize) -> (AppState, RootId) {
    let repo = seeded(project, commits);
    let mut state = AppState::for_roots(project, std::slice::from_ref(&repo));
    let root = state.multi.roots[0].id.clone();
    state.selected_root = Some(root.clone());
    state.fetch_log(root.clone());
    state.drain_events();
    assert!(
        state.caches.log(&root).is_some(),
        "the fixture's first batch must settle"
    );
    (state, root)
}

/// Post one failing answer on the production event channel, exactly as a worker
/// thread posts it, and settle it.
#[track_caller]
fn fail(state: &mut AppState, read: &LogRead, why: &str) {
    let err = || TgError::Other(why.to_owned());
    match read {
        LogRead::Commits(root) => state
            .tx
            .send(AppEvent::LogLoaded {
                root: root.clone(),
                commits: Err(err()),
                mode: LogBatchMode::Replace,
            })
            .expect("send failing LogLoaded"),
        LogRead::Scope(root, scope) => state
            .tx
            .send(AppEvent::LogBatchLoaded {
                root: root.clone(),
                scope: scope.clone(),
                commits: Err(err()),
                mode: LogBatchMode::Replace,
            })
            .expect("send failing LogBatchLoaded"),
        LogRead::Details(root, cid) => state
            .tx
            .send(AppEvent::FileStatsLoaded {
                root: root.clone(),
                commit: cid.clone(),
                stats: Err(err()),
            })
            .expect("send failing FileStatsLoaded"),
        LogRead::Changes(root, cid) => {
            // The changed-file list is a compute-on-miss read inside the caches
            // rather than a worker answer, so its failure is recorded through
            // the same public writer the cache itself uses.
            state.caches.record_log_failure(read.clone(), why);
            let _ = (root, cid);
        }
    }
    state.drain_events();
}

/// HEAD's id, from the settled window.
fn head(state: &AppState, root: &RootId) -> String {
    state
        .caches
        .log(root)
        .and_then(|rows| rows.first().map(|c| c.id.clone()))
        .expect("the fixture's log is loaded")
}

// --- the machine's own invariants ------------------------------------------------

/// Five error states — the four the ticket enumerates plus the file-history
/// variant — no two sharing a label, each naming the operation that failed
/// rather than the pane it is painted in. A shared "Error" string is the
/// collapse this ticket exists to prevent.
#[test]
fn every_error_state_names_the_operation_that_failed() {
    let census = named_states();
    let errors: Vec<&LogState> = census.iter().filter(|s| s.is_error()).collect();
    let labels: Vec<&str> = errors.iter().map(|s| s.label()).collect();
    assert_eq!(
        labels,
        [
            "Error loading commits",
            "Error loading commit details",
            "Error loading changes",
            "Error filtering commits",
            "Error calculating file history",
        ],
        "the five failures are five named states, each saying which read died"
    );
    for label in &labels {
        assert!(
            label.starts_with("Error "),
            "an error state must open by naming what failed, saw {label:?}"
        );
    }
    // No error state's string is contained in another's. This is the property
    // that makes the four (five) states tellable apart at all — and tellable
    // apart *from painted output*, where a test finds a string by asking
    // whether a painted galley contains it. A state whose label were a
    // substring of another's could never be asserted absent.
    for a in &labels {
        for b in &labels {
            if a != b {
                assert!(
                    !b.contains(a),
                    "the error state {a:?} is contained in {b:?}, so no painted \
                     frame can tell them apart"
                );
            }
        }
    }
}

/// Every error state and every empty state carries its own recovery — the
/// machine's second governing rule. The four loading states and the default
/// carry none: they have nothing to escape from.
#[test]
fn every_error_and_empty_state_carries_its_own_recovery() {
    let mut escaped = 0;
    for state in &named_states() {
        let needs_one = state.is_error() || state.is_empty_state();
        assert_eq!(
            state.recovery().is_some(),
            needs_one,
            "{state:?} must {}carry its own recovery",
            if needs_one { "" } else { "NOT " }
        );
        escaped += usize::from(needs_one);
    }
    assert_eq!(
        escaped, 8,
        "the recovery census: five errors, three empties"
    );
    assert_eq!(
        LogState::NothingCommitted.recovery(),
        Some(LogRecovery::CommitLocalChanges),
        "'no history at all' is a call to action, not a filter problem"
    );
    assert_eq!(
        LogState::FilteredOut.recovery(),
        Some(LogRecovery::ResetFilters),
        "'history exists but is excluded' clears the filters, it does not commit"
    );
    assert_eq!(
        LogState::FilteringFailed("x".into()).recovery(),
        Some(LogRecovery::ResetFilters),
        "a filter that cannot be evaluated is escaped by resetting it, not by \
         re-running it"
    );
    for state in &named_states() {
        assert!(
            !state.is_error() || state.reason().is_some(),
            "an error state carries the message its read answered with: {state:?}"
        );
    }
}

/// The two empty states are two states with two different actions — the
/// mislabelling this ticket names as its central bug is one standing in for
/// the other.
#[test]
fn the_two_empty_states_are_distinct_states_with_distinct_actions() {
    assert_ne!(LogState::NothingCommitted, LogState::FilteredOut);
    assert_ne!(
        LogState::NothingCommitted.label(),
        LogState::FilteredOut.label(),
        "one string cannot say both 'the repository has no history' and 'your \
         filters excluded its history'"
    );
    assert_ne!(
        LogState::NothingCommitted.recovery(),
        LogState::FilteredOut.recovery()
    );
}

// --- the listing the graph pane shows --------------------------------------------

/// Nothing has been read yet: the log is on its first paint, not empty and not
/// failed.
#[test]
fn a_cold_log_reads_as_the_first_paint() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let repo = seeded(&project, 3);
    let state = AppState::for_roots(&project, std::slice::from_ref(&repo));
    assert_eq!(log_states::commit_list(&state, true), LogState::LoadingLog);
}

/// A commit-listing read that failed says so about *commits*, and offers the
/// one thing that can still work: ask again.
#[test]
fn a_failed_commit_read_names_commits_and_offers_refresh() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let (mut state, root) = loaded(&project, 3);
    state
        .caches
        .forget_log_read(&LogRead::Commits(root.clone()));
    fail(&mut state, &LogRead::Commits(root.clone()), "offline");
    let list = log_states::commit_list(&state, true);
    assert_eq!(
        list,
        LogState::CommitsFailed("offline".to_owned()),
        "the error state carries the reason its read gave"
    );
    assert_eq!(list.label(), "Error loading commits");
    assert_eq!(list.recovery(), Some(LogRecovery::Refresh));
    assert_eq!(list.reason(), Some("offline"));
}

/// An empty repository is an empty listing, never a failed one. `git log` on an
/// unborn HEAD exits non-zero, so the engine has to answer it as the empty list
/// it is — otherwise a fresh repository is reported as a crash, and a UI that
/// says "Error loading commits" to someone who has just made one sends them
/// somewhere that is not their problem.
#[test]
fn an_unborn_repository_reads_as_nothing_committed_rather_than_as_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let repo = unborn(&project);
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&repo));
    let root = state.multi.roots[0].id.clone();
    state.selected_root = Some(root.clone());
    state.fetch_log(root.clone());
    state.drain_events();
    let window = state
        .caches
        .log(&root)
        .expect("an unborn HEAD answers with an empty listing, not a failure");
    assert!(window.is_empty(), "a fresh repository has no commits");
    let list = log_states::commit_list(&state, true);
    assert_eq!(list, LogState::NothingCommitted);
    assert_eq!(list.label(), "No changes committed.");
    assert_eq!(list.recovery(), Some(LogRecovery::CommitLocalChanges));
    assert!(
        !state
            .caches
            .has_log_failure(&LogRead::Commits(root.clone())),
        "a legitimate empty must not also be recorded as a failure"
    );
}

/// A live filter that excludes every loaded row is a FILTER problem, on a
/// repository that plainly has history — the two empty states must not be
/// confusable, and the action each offers proves which one was meant.
#[test]
fn a_filter_that_excludes_everything_is_not_an_empty_history() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let (mut state, _root) = loaded(&project, 3);
    state.ui.log_filter = "zzz-no-such-subject".to_owned();
    let list = log_states::commit_list(&state, true);
    assert_eq!(
        list,
        LogState::FilteredOut,
        "rows are loaded and hidden: the history exists"
    );
    assert_eq!(list.label(), "No commits matching filters");
    assert_eq!(list.recovery(), Some(LogRecovery::ResetFilters));
    assert_ne!(
        list,
        LogState::NothingCommitted,
        "a UI that reports an empty history here sends the user to commit \
         instead of to their filters"
    );
}

/// A ref scope narrows the listing with no search text at all, so an empty
/// result under it is still a filter outcome and never "nothing has been
/// committed" — and a scope whose read has not landed is still loading.
#[test]
fn an_empty_ref_scoped_listing_reads_as_filters_not_as_no_history() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let (mut state, root) = loaded(&project, 3);
    let scope = LogScope::Ref("main".to_owned());
    state.ui.log_ref_scope = Some((root.clone(), "main".to_owned()));
    assert_eq!(
        log_states::commit_list(&state, true),
        LogState::LoadingCommits,
        "the ref's own listing has never been read"
    );
    state.ensure_log(&root, scope.clone());
    state.ui.log_filter = "zzz-no-such-subject".to_owned();
    assert_eq!(
        log_states::commit_list(&state, true),
        LogState::FilteredOut,
        "a scope is one of the filters the reset action escapes"
    );
}

// --- file history: the scoped pair ------------------------------------------------

/// A file whose history is empty says `File history` and escapes through the
/// filters, which is the only thing that can have produced the empty list.
#[test]
fn an_empty_path_scope_reads_as_file_history() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let (mut state, root) = loaded(&project, 3);
    let path = PathBuf::from("never-touched.txt");
    state.ui.log_path_scope = Some(path.clone());
    state.ensure_log(&root, LogScope::Path(path));
    let list = log_states::commit_list(&state, true);
    assert_eq!(list, LogState::FileHistory);
    assert_eq!(list.label(), "File history");
    assert_eq!(list.recovery(), Some(LogRecovery::ResetFilters));
}

/// The same scope with a read that failed names *file history*, not commits:
/// the failure belongs to the operation that produced it.
#[test]
fn a_failed_path_scope_read_names_file_history() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let (mut state, root) = loaded(&project, 3);
    let path = PathBuf::from("f.txt");
    state.ui.log_path_scope = Some(path.clone());
    fail(
        &mut state,
        &LogRead::Scope(root.clone(), LogScope::Path(path)),
        "bad path",
    );
    let list = log_states::commit_list(&state, true);
    assert_eq!(list, LogState::FileHistoryFailed("bad path".to_owned()));
    assert_eq!(list.label(), "Error calculating file history");
    assert_eq!(list.recovery(), Some(LogRecovery::ResetFilters));
}

// --- filtering: the pickaxe's own slot --------------------------------------------

/// The code-change search is a listing of its own and its failure is a FILTER
/// state — with the commit rows still on screen. A search that quietly returns
/// nothing looks exactly like a search that found nothing, so the state naming
/// the failed search is what keeps content search honest.
#[test]
fn a_failed_search_read_is_a_filtering_state_while_the_list_stands() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let (mut state, root) = loaded(&project, 3);
    state.ui.log_filter = "token".to_owned();
    fail(
        &mut state,
        &LogRead::Scope(root.clone(), LogScope::Search("token".to_owned())),
        "pickaxe unsupported",
    );
    let filtering = log_states::filtering(&state);
    assert_eq!(
        filtering,
        LogState::FilteringFailed("pickaxe unsupported".to_owned())
    );
    assert_eq!(filtering.label(), "Error filtering commits");
    assert_eq!(filtering.recovery(), Some(LogRecovery::ResetFilters));
    assert_eq!(
        log_states::commit_list(&state, false),
        LogState::Ready,
        "the listing that loaded is not the search that failed"
    );
}

/// A search with no settled window yet is filters being applied, and offers
/// nothing: the answer is coming.
#[test]
fn a_search_on_the_pump_reads_as_applying_filters() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let (mut state, _root) = loaded(&project, 3);
    state.ui.log_filter = "token".to_owned();
    let filtering = log_states::filtering(&state);
    assert_eq!(filtering, LogState::LoadingFilters);
    assert_eq!(filtering.label(), "Applying Filters…");
    assert!(
        filtering.recovery().is_none(),
        "a loading state has nothing to escape"
    );
}

/// No query typed is no filter read: the slot is silent rather than empty. And
/// inside a path scope the pickaxe never runs, so it can own no state there.
#[test]
fn an_absent_search_term_owns_no_filtering_state() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let (mut state, root) = loaded(&project, 3);
    assert_eq!(log_states::filtering(&state), LogState::Ready);
    state.ui.log_filter = "token".to_owned();
    state.ui.log_path_scope = Some(PathBuf::from("f.txt"));
    state.ensure_log(&root, LogScope::Path(PathBuf::from("f.txt")));
    assert_eq!(
        log_states::filtering(&state),
        LogState::Ready,
        "a path scope never mixes in pickaxe hits, so it owns no filter state"
    );
}

// --- the selected commit's two panes ------------------------------------------------

/// A details read that fails names DETAILS and leaves the commit list ready:
/// the error states have to be genuinely distinct, and the list loading
/// cleanly beside the failure is what proves it.
#[test]
fn a_failed_details_read_names_details_and_not_commits() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let (mut state, root) = loaded(&project, 3);
    let cid = head(&state, &root);
    state.ui.selected_commit = Some(cid.clone());
    fail(
        &mut state,
        &LogRead::Details(root.clone(), cid.clone()),
        "no diff engine",
    );
    let details = log_states::commit_details(&state);
    assert_eq!(
        details,
        LogState::DetailsFailed("no diff engine".to_owned())
    );
    assert_eq!(details.label(), "Error loading commit details");
    assert_ne!(
        details.label(),
        log_states::commit_list(&state, false).label(),
        "a details failure must not wear the commit list's error"
    );
    assert_eq!(details.recovery(), Some(LogRecovery::Refresh));
    assert_eq!(
        log_states::commit_list(&state, false),
        LogState::Ready,
        "the commit list loaded, and says so"
    );
}

/// The details read is the pane's only off-thread content, so before it lands
/// the pane is loading rather than showing zeroes.
#[test]
fn a_selection_whose_details_are_not_in_yet_is_loading_details() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let (mut state, root) = loaded(&project, 3);
    state.ui.selected_commit = Some(head(&state, &root));
    let details = log_states::commit_details(&state);
    assert_eq!(details, LogState::LoadingDetails);
    assert_eq!(details.label(), "Loading selected details");
    assert!(details.recovery().is_none());
}

/// With nothing selected neither pane has a subject, which is the pane's own
/// answer and none of the machine's business.
#[test]
fn an_unselected_pane_has_no_state_to_report() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let (state, _root) = loaded(&project, 3);
    assert_eq!(log_states::commit_details(&state), LogState::Ready);
    assert_eq!(log_states::changed_files(&state), LogState::Ready);
}

/// The changed-file list is a third read, and its failure belongs to it alone.
#[test]
fn a_failed_changes_read_names_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let (mut state, root) = loaded(&project, 3);
    let cid = head(&state, &root);
    state.ui.selected_commit = Some(cid.clone());
    fail(&mut state, &LogRead::Changes(root, cid), "bad object");
    let changes = log_states::changed_files(&state);
    assert_eq!(changes, LogState::ChangesFailed("bad object".to_owned()));
    assert_eq!(changes.label(), "Error loading changes");
    assert_eq!(changes.recovery(), Some(LogRecovery::Refresh));
    assert_ne!(
        changes.label(),
        LogState::CommitsFailed(String::new()).label()
    );
    assert_eq!(log_states::commit_list(&state, false), LogState::Ready);
}

// --- settlement ---------------------------------------------------------------------

/// A failure is a fact about one read of one key. The same read answering
/// successfully forgets it: a stale failure must not outlive the state that
/// caused it and keep a working pane reporting an error.
#[test]
fn a_successful_read_clears_the_failure_it_follows() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let (mut state, root) = loaded(&project, 3);
    state
        .caches
        .forget_log_read(&LogRead::Commits(root.clone()));
    fail(&mut state, &LogRead::Commits(root.clone()), "offline");
    assert!(
        state
            .caches
            .has_log_failure(&LogRead::Commits(root.clone()))
    );
    state.fetch_log(root.clone());
    state.drain_events();
    assert!(
        !state
            .caches
            .has_log_failure(&LogRead::Commits(root.clone())),
        "the settled window answers, so the failure is history"
    );
    assert_eq!(log_states::commit_list(&state, false), LogState::Ready);
}

/// Invalidation drops the failure records with the windows: an operation that
/// changed the repository must not leave a pane still reporting the read the
/// operation made stale.
#[test]
fn invalidating_a_root_drops_its_failures_with_its_windows() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let (mut state, root) = loaded(&project, 3);
    state
        .caches
        .forget_log_read(&LogRead::Commits(root.clone()));
    fail(&mut state, &LogRead::Commits(root.clone()), "offline");
    assert!(
        state
            .caches
            .has_log_failure(&LogRead::Commits(root.clone()))
    );
    state.caches.invalidate(&Affected::Root(root.clone()));
    assert!(
        !state
            .caches
            .has_log_failure(&LogRead::Commits(root.clone())),
        "a failure must not outlive the state that caused it"
    );
}

/// `Refresh` is a recovery that re-asks, not a label. Forgetting the read's
/// answer is what makes the pane's own ask reach the engine again, and the
/// listing that then settles is the error's disappearance.
#[test]
fn refreshing_a_failed_listing_re_asks_the_engine_and_clears_the_state() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let (mut state, root) = loaded(&project, 3);
    state
        .caches
        .forget_log_read(&LogRead::Commits(root.clone()));
    fail(&mut state, &LogRead::Commits(root.clone()), "offline");
    assert!(log_states::commit_list(&state, true).is_error());

    state.retry_log_read(&LogRead::Commits(root.clone()));
    state.drain_events();
    assert_eq!(
        log_states::commit_list(&state, false),
        LogState::Ready,
        "the retry asked, the answer landed, the error state is gone"
    );
}

/// A retry of a scoped listing forgets that scope's window and nobody else's,
/// which is what keeps a failed file-history read from being kept forever as a
/// permanently empty answer.
#[test]
fn refreshing_a_failed_scope_forgets_that_scope_and_not_the_whole_log() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let (mut state, root) = loaded(&project, 3);
    let scope = LogScope::Path(PathBuf::from("f.txt"));
    state.ensure_log(&root, scope.clone());
    fail(
        &mut state,
        &LogRead::Scope(root.clone(), scope.clone()),
        "gone",
    );
    assert!(
        state
            .caches
            .has_log_failure(&LogRead::Scope(root.clone(), scope.clone()))
    );

    state.retry_log_read(&LogRead::Scope(root.clone(), scope.clone()));
    state.drain_events();
    assert!(
        state.caches.log(&root).is_some(),
        "the unscoped window is nobody else's business on a scope retry"
    );
    assert!(
        !state
            .caches
            .has_log_failure(&LogRead::Scope(root.clone(), scope.clone())),
        "the retry cleared the scope's failure"
    );
    assert_eq!(
        state.caches.scoped_log(&root, &scope).map(|r| r.len()),
        Some(3),
        "and the retry re-read the scope from the engine"
    );
}

/// A multi-root listing is a listing over EVERY visible root: one root's
/// failure is the union's error only when the union has nothing to show, and
/// one root unread beside one settled root is settled rather than loading
/// forever.
#[test]
fn a_multi_root_listing_never_collapses_to_one_root() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let alpha = seeded(&project, 3);
    let beta = project.join("beta");
    std::fs::create_dir_all(&beta).unwrap();
    git(&beta, &["init", "-q", "-b", "main"]);
    let mut state = AppState::for_roots(&project, &[alpha.clone(), beta.clone()]);
    assert_eq!(
        state.multi.roots.len(),
        2,
        "both roots are registered — the union is over them"
    );
    let alpha_id = RootId(alpha.into());
    state.fetch_log(alpha_id.clone());
    state.drain_events();
    assert_eq!(
        log_states::commit_list(&state, false),
        LogState::Ready,
        "beta has never been read, and that must not leave the union 'loading' \
         while alpha's rows are on screen"
    );
    // Narrow the view to the unread root and the same machine reports the cold
    // first paint for it — the roots filter is one of the listing's inputs.
    state.ui.log_root_filter = Some(RootId(beta.into()));
    assert_eq!(log_states::commit_list(&state, true), LogState::LoadingLog);
    // And the roots filter keeps BOTH roots' failures in view: a union that
    // showed one root's error as the other's would be the collapse again.
    state.ui.log_root_filter = None;
    fail(&mut state, &LogRead::Commits(alpha_id), "offline");
    assert_eq!(
        log_states::commit_list(&state, false),
        LogState::Ready,
        "rows are on screen, so a failed re-read does not claim the message slot"
    );
}
