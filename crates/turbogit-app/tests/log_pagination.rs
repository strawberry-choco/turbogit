//! Issue 17 — Log UX upgrades, app seam: the commit list paginates. The
//! fetch is page-sized (`(ui.log_page + 1) * LOG_PAGE_SIZE` commits) and
//! [`AppState::load_more_log`] widens the fetch through the production
//! worker path, replacing the previously uncapped log load.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use test_support::RecordingExecutor;
use turbogit_app::events::AppEvent;
use turbogit_app::state::{AppState, LOG_PAGE_SIZE};
use turbogit_domain::error::TgError;
use turbogit_domain::model::VcsSettings;
use turbogit_engine::cli::CliExecutor;

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

/// A repo with `n` commits (empty commits are enough — pagination only
/// counts log entries).
fn seeded_repo(project: &Path, n: usize) -> PathBuf {
    let repo = project.join("alpha");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    for i in 0..n {
        git(
            &repo,
            &["commit", "-q", "--allow-empty", "-m", &format!("c{i}")],
        );
    }
    repo
}

/// Pump worker events until `pred` holds or the deadline passes.
fn wait_for(state: &mut AppState, pred: impl Fn(&AppState) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        state.drain_events();
        if pred(state) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("condition not met within 10s");
}

#[test]
fn fetch_loads_one_page_and_load_more_widens_the_window() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let total = LOG_PAGE_SIZE + 1;
    let repo = seeded_repo(&project, total);
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&repo));
    let root_id = state.multi.roots[0].id.clone();

    // One page: the initial fetch must NOT load the whole log.
    state.fetch_log(root_id.clone());
    wait_for(&mut state, |s| {
        s.caches
            .log(&root_id)
            .is_some_and(|c| c.len() == LOG_PAGE_SIZE)
    });

    // Load more widens the fetch to two pages: the whole log lands.
    state.load_more_log();
    wait_for(&mut state, |s| {
        s.caches.log(&root_id).is_some_and(|c| c.len() == total)
    });

    // Loading more past the end is safe: the log stays at `total` entries,
    // with no duplicates — the wider fetch replaces, never appends.
    state.load_more_log();
    wait_for(&mut state, |s| {
        s.caches.log(&root_id).is_some_and(|c| c.len() == total)
    });
    let ids: Vec<&str> = state
        .caches
        .log(&root_id)
        .unwrap()
        .iter()
        .map(|c| c.id.as_str())
        .collect();
    assert_eq!(ids.len(), total);
    assert_eq!(
        ids.iter().collect::<std::collections::HashSet<_>>().len(),
        total,
        "load-more refetch must replace, not append duplicates"
    );
}

// --- In-flight log fetch guard (log-open perf, D2) ------------------------------

/// An [`AppState`] over one seeded repo whose engine is a recording wrapper,
/// so `log` invocations are countable.
fn recording_state(project: &Path, repo: &PathBuf) -> (AppState, Arc<RecordingExecutor>) {
    let recorder: Arc<RecordingExecutor> =
        Arc::new(RecordingExecutor::new(Arc::new(CliExecutor {
            settings: VcsSettings::default(),
        })));
    let state =
        AppState::for_roots(project, std::slice::from_ref(repo)).with_executor(recorder.clone());
    (state, recorder)
}

/// Drain events until the recording executor has seen at least `n` log calls.
fn wait_log_calls(state: &mut AppState, recorder: &RecordingExecutor, n: usize) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        state.drain_events();
        if recorder.log_call_count() >= n {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("log call count never reached {n}");
}

#[test]
fn two_fetches_for_the_same_root_in_one_frame_spawn_one_worker() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let repo = seeded_repo(&project, 3);
    let (mut state, recorder) = recording_state(&project, &repo);
    let root_id = state.multi.roots[0].id.clone();

    // The tool-window body asks for the log every frame while the cache is
    // cold; the second call lands before the first event drains.
    state.fetch_log(root_id.clone());
    state.fetch_log(root_id.clone());
    wait_for(&mut state, |s| s.caches.log(&root_id).is_some());
    state.drain_events();

    assert_eq!(
        recorder.log_call_count(),
        1,
        "two same-frame fetches must produce exactly one worker / one log call"
    );
}

#[test]
fn inflight_guard_releases_when_the_log_event_drains_ok_and_err() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let repo = seeded_repo(&project, 3);
    let (mut state, recorder) = recording_state(&project, &repo);
    let root_id = state.multi.roots[0].id.clone();

    // Ok path: the drained LogLoaded frees the guard, so a later fetch runs.
    state.fetch_log(root_id.clone());
    wait_for(&mut state, |s| s.caches.log(&root_id).is_some());
    state.drain_events();
    state.fetch_log(root_id.clone());
    wait_log_calls(&mut state, &recorder, 2);
    assert!(
        recorder.log_call_count() >= 2,
        "a later fetch must run again after the Ok drain"
    );

    // Err path: a failing log-load event frees the guard too — a later fetch
    // must run again, and the failure surfaces through last_error.
    state
        .tx
        .send(AppEvent::LogLoaded {
            root: root_id.clone(),
            commits: Err(TgError::Other("offline".to_string())),
        })
        .expect("send failing LogLoaded");
    state.drain_events();
    assert!(
        state.last_error.is_some(),
        "a failing log load must surface through last_error"
    );
    state.fetch_log(root_id.clone());
    wait_log_calls(&mut state, &recorder, 3);
    assert!(
        recorder.log_call_count() >= 3,
        "a later fetch must run again after the Err drain"
    );
}
