use std::path::{Path, PathBuf};

use test_support::git_seed::{GitEnv, git_with_env};
use turbogit_app::events::{AppEvent, LogBatchMode};
use turbogit_app::log_states::{self, LogRecovery, LogState, named_states};
use turbogit_app::root_caches::{Affected, LogRead, LogScope};
use turbogit_app::state::AppState;
use turbogit_domain::error::TgError;
use turbogit_domain::model::RootId;

fn seeded(project: &Path, n: usize) -> PathBuf {
    let repo = project.join("alpha");
    std::fs::create_dir_all(&repo).unwrap();
    git_with_env(&repo, &["init", "-q", "-b", "main"], &GitEnv::fixture());
    git_with_env(&repo, &["config", "user.email", "t@t"], &GitEnv::fixture());
    git_with_env(&repo, &["config", "user.name", "t"], &GitEnv::fixture());
    for i in 0..n {
        std::fs::write(repo.join("f.txt"), format!("token{i}\n")).unwrap();
        git_with_env(&repo, &["add", "."], &GitEnv::fixture());
        git_with_env(
            &repo,
            &["commit", "-q", "-m", &format!("c{i}")],
            &GitEnv::fixture(),
        );
    }
    repo
}

fn unborn(project: &Path) -> PathBuf {
    let repo = project.join("alpha");
    std::fs::create_dir_all(&repo).unwrap();
    git_with_env(&repo, &["init", "-q", "-b", "main"], &GitEnv::fixture());
    git_with_env(&repo, &["config", "user.email", "t@t"], &GitEnv::fixture());
    git_with_env(&repo, &["config", "user.name", "t"], &GitEnv::fixture());
    repo
}

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
            state.caches.record_log_failure(read.clone(), why);
            let _ = (root, cid);
        }
    }
    state.drain_events();
}

fn head(state: &AppState, root: &RootId) -> String {
    state
        .caches
        .log(root)
        .and_then(|rows| rows.first().map(|c| c.id.clone()))
        .expect("the fixture's log is loaded")
}

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

#[test]
fn a_cold_log_reads_as_the_first_paint() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let repo = seeded(&project, 3);
    let state = AppState::for_roots(&project, std::slice::from_ref(&repo));
    assert_eq!(log_states::commit_list(&state, true), LogState::LoadingLog);
}

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

#[test]
fn an_unselected_pane_has_no_state_to_report() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let (state, _root) = loaded(&project, 3);
    assert_eq!(log_states::commit_details(&state), LogState::Ready);
    assert_eq!(log_states::changed_files(&state), LogState::Ready);
}

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

#[test]
fn a_multi_root_listing_never_collapses_to_one_root() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let alpha = seeded(&project, 3);
    let beta = project.join("beta");
    std::fs::create_dir_all(&beta).unwrap();
    git_with_env(&beta, &["init", "-q", "-b", "main"], &GitEnv::fixture());
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
    state.ui.log_root_filter = Some(RootId(beta.into()));
    assert_eq!(log_states::commit_list(&state, true), LogState::LoadingLog);
    state.ui.log_root_filter = None;
    fail(&mut state, &LogRead::Commits(alpha_id), "offline");
    assert_eq!(
        log_states::commit_list(&state, false),
        LogState::Ready,
        "rows are on screen, so a failed re-read does not claim the message slot"
    );
}
