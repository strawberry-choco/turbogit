//! Commit-log paging at the app seam: the fetch requests ONE page for one
//! root — `skip` supplies the position, `max_count` the page size — and the
//! window it extends is derived from the cache, never from a stored page
//! index. Plan: `.scratch/log-paging-redesign/execution-plan.md`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use test_support::RecordingExecutor;
use turbogit_app::events::{AppEvent, LogPageMode};
use turbogit_app::root_caches::Affected;
use turbogit_app::state::{AppState, LOG_PAGE_SIZE};
use turbogit_domain::error::TgError;
use turbogit_domain::model::{Commit, LogOpts, RootId, Signature, SignatureState, VcsSettings};
use turbogit_engine::GitExecutor;
use turbogit_engine::cli::CliExecutor;
use turbogit_engine::fake::FakeExecutor;

fn git(dir: &Path, args: &[&str]) -> String {
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
    String::from_utf8_lossy(&out.stdout).into_owned()
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

// --- Pages fold into one window (P3/P4) ---------------------------------------

/// git's own answer for `repo` — the independent oracle a paged window is
/// checked against, uncapped and newest-first.
fn engine_ids(repo: &Path) -> Vec<String> {
    CliExecutor {
        settings: VcsSettings::default(),
    }
    .log(repo, &LogOpts::default())
    .expect("engine log")
    .iter()
    .map(|c| c.id.clone())
    .collect()
}

/// Ids of `root`'s cached window, newest first.
fn cached_ids(state: &AppState, root: &RootId) -> Vec<String> {
    state
        .caches
        .log(root)
        .unwrap_or_default()
        .iter()
        .map(|c| c.id.clone())
        .collect()
}

/// A repo deeper than two pages, seeded once for the whole test binary —
/// one `git commit` subprocess per commit is the slow part of this suite, and
/// nothing here writes to it.
fn deep_project() -> &'static (PathBuf, PathBuf) {
    static PROJECT: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();
    PROJECT.get_or_init(|| {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("project");
        let repo = seeded_repo(&project, 2 * LOG_PAGE_SIZE + 3);
        // The temp dir outlives every test in this binary.
        std::mem::forget(dir);
        (project, repo)
    })
}

/// A repo holding exactly one page of history.
fn one_page_project() -> &'static (PathBuf, PathBuf) {
    static PROJECT: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();
    PROJECT.get_or_init(|| {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("project");
        let repo = seeded_repo(&project, LOG_PAGE_SIZE);
        std::mem::forget(dir);
        (project, repo)
    })
}

/// Two pages fetched one after the other through the production worker path
/// have to ADD up: the second page extends the window, and the row both
/// requests overlap on is held once.
#[test]
fn consecutive_pages_append_into_one_window() {
    let (project, repo) = deep_project();
    let (mut state, recorder) = recording_state(project, repo);
    let root = state.multi.roots[0].id.clone();
    let total = 2 * LOG_PAGE_SIZE + 3;

    state.fetch_log(root.clone());
    wait_for(&mut state, |s| cached_ids(s, &root).len() == LOG_PAGE_SIZE);
    let first = cached_ids(&state, &root);
    assert!(
        state.caches.log_has_more(&root),
        "a full page cannot be the end of the history"
    );

    state.load_more_log();
    wait_for(&mut state, |s| {
        cached_ids(s, &root).len() == 2 * LOG_PAGE_SIZE
    });
    let second = cached_ids(&state, &root);
    assert_eq!(
        &second[..LOG_PAGE_SIZE],
        &first[..],
        "the rows already held keep their place, newest first"
    );
    assert_eq!(
        second,
        engine_ids(repo)[..2 * LOG_PAGE_SIZE].to_vec(),
        "two appended pages are exactly git's first two pages, no row twice"
    );
    assert!(
        state.caches.log_has_more(&root),
        "another full page, so still not the end"
    );

    state.load_more_log();
    wait_for(&mut state, |s| cached_ids(s, &root).len() == total);
    assert_eq!(
        cached_ids(&state, &root),
        engine_ids(repo),
        "the whole window is git's own listing, in git's own order"
    );
    assert!(
        !state.caches.log_has_more(&root),
        "a short page is the end of history"
    );

    let settled = recorder.log_call_count();
    state.load_more_log();
    assert_eq!(
        recorder.log_call_count(),
        settled,
        "past the end of history, load more must not fetch again"
    );
}

/// A history that is an exact multiple of the page size ends on the row
/// limit, so the pager cannot know it has arrived until it asks once more.
/// That is the design's accepted cost — exactly one extra fetch, never a
/// repeating one.
#[test]
fn an_exact_multiple_of_the_page_size_ends_after_one_extra_fetch() {
    let (project, repo) = one_page_project();
    let (mut state, recorder) = recording_state(project, repo);
    let root = state.multi.roots[0].id.clone();

    state.fetch_log(root.clone());
    wait_for(&mut state, |s| cached_ids(s, &root).len() == LOG_PAGE_SIZE);
    assert!(
        state.caches.log_has_more(&root),
        "a page that fills the request offers one more"
    );

    state.load_more_log();
    wait_for(&mut state, |s| !s.caches.log_has_more(&root));
    assert_eq!(
        recorder.log_call_count(),
        2,
        "exactly one extra fetch ends a history of exactly one page"
    );
    assert_eq!(
        cached_ids(&state, &root).len(),
        LOG_PAGE_SIZE,
        "the extra fetch carried only the anchor row, which is not new"
    );

    let settled = recorder.log_call_count();
    state.load_more_log();
    assert_eq!(
        recorder.log_call_count(),
        settled,
        "and it does not ask again"
    );
}

/// The checksum half of P3: an append page must lead with the row the window
/// was measured against. A commit landing mid-paging moves HEAD, so the page
/// leads with something else — and a window that keeps growing from there is
/// torn. The window restarts at page 0 instead.
#[test]
fn a_commit_landed_mid_paging_restarts_the_window_instead_of_tearing_it() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let repo = seeded_repo(&project, LOG_PAGE_SIZE + 1);
    let (mut state, recorder) = recording_state(&project, &repo);
    let root = state.multi.roots[0].id.clone();

    state.fetch_log(root.clone());
    wait_for(&mut state, |s| cached_ids(s, &root).len() == LOG_PAGE_SIZE);
    let held_before = cached_ids(&state, &root);

    // HEAD moves while the next page is in flight.
    git(
        &repo,
        &["commit", "-q", "--allow-empty", "-m", "landed mid-paging"],
    );
    let late = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    state.load_more_log();

    // The rejected page is never appended: the window is a clean page 0 that
    // leads with the commit that moved it.
    wait_for(&mut state, |s| {
        let ids = cached_ids(s, &root);
        ids.first().map(String::as_str) == Some(late.as_str())
    });
    let after = cached_ids(&state, &root);
    assert_eq!(
        after.len(),
        LOG_PAGE_SIZE,
        "a restart is page 0, not the torn remainder of a window"
    );
    assert_ne!(
        after, held_before,
        "the refetched window carries the new commit"
    );
    assert_eq!(
        after
            .iter()
            .collect::<std::collections::HashSet<&String>>()
            .len(),
        LOG_PAGE_SIZE,
        "and holds no row twice"
    );
    assert!(
        state.caches.log_has_more(&root),
        "51 commits, 50 shown — there is still one more"
    );
    assert!(
        recorder.log_call_count() >= 3,
        "the restart refetches page 0 through the same worker path"
    );
}

/// The same walk with no git in the loop: [`FakeExecutor`] slices its seeded
/// listing by `(skip, max_count)`, so the window can be checked against a
/// literal, hand-written listing rather than one read back from the engine.
#[test]
fn the_page_walk_never_repeats_or_skips_a_row_of_the_listing() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let repo = seeded_repo(&project, 1);
    let state0 = AppState::for_roots(&project, std::slice::from_ref(&repo));
    let root = state0.multi.roots[0].id.clone();
    let seeded: Vec<Commit> = (0..2 * LOG_PAGE_SIZE + 3)
        .map(|i| row(&root, &format!("r{i}")))
        .collect();

    let mut fake = FakeExecutor::default();
    fake.logs.insert(repo.clone(), seeded.clone());
    let mut state = state0.with_executor(Arc::new(fake));

    state.fetch_log(root.clone());
    wait_for(&mut state, |s| cached_ids(s, &root).len() == LOG_PAGE_SIZE);
    assert!(state.caches.log_has_more(&root));

    // Page 2 and page 3: a full page, then the short one that ends it.
    for expected in [2 * LOG_PAGE_SIZE, 2 * LOG_PAGE_SIZE + 3] {
        state.load_more_log();
        wait_for(&mut state, |s| cached_ids(s, &root).len() == expected);
    }
    assert_eq!(
        cached_ids(&state, &root),
        seeded.iter().map(|c| c.id.clone()).collect::<Vec<_>>(),
        "the window is exactly the seeded listing, in order, once each"
    );
    assert!(
        !state.caches.log_has_more(&root),
        "the short page ended the history"
    );
}

// --- The window resets wherever HEAD moves --------------------------------

/// A completed operation moves HEAD, which invalidates the offsets the window
/// was built on. The refetch restarts it at page 0 — it must neither continue
/// the grown window nor smuggle a whole-history write past the pager.
#[test]
fn a_completed_op_resets_the_window_to_one_page() {
    let (project, repo) = deep_project();
    let (mut state, recorder) = recording_state(project, repo);
    let root = state.multi.roots[0].id.clone();

    state.fetch_log(root.clone());
    wait_for(&mut state, |s| cached_ids(s, &root).len() == LOG_PAGE_SIZE);
    state.load_more_log();
    wait_for(&mut state, |s| {
        cached_ids(s, &root).len() == 2 * LOG_PAGE_SIZE
    });

    state
        .tx
        .send(AppEvent::OpCompleted {
            label: "op".to_string(),
            affected: Affected::Root(root.clone()),
            result: Ok(()),
            retry: None,
        })
        .expect("send OpCompleted");
    state.drain_events();

    assert_eq!(
        cached_ids(&state, &root),
        engine_ids(repo)[..LOG_PAGE_SIZE].to_vec(),
        "the window is one page again, from the front of the listing"
    );
    assert!(
        state.caches.log_has_more(&root),
        "and it knows the history continues past it"
    );
    assert_eq!(
        recorder.log_opts().last(),
        Some(&LogOpts {
            max_count: Some(LOG_PAGE_SIZE),
            ..Default::default()
        }),
        "the refresh asks for a page, not for the whole history"
    );
}

/// Paging belongs to the root's log window only. A path- or ref-scoped listing
/// is fetched whole and never pages (P7), so the scoped caches stay exactly as
/// they were.
#[test]
fn scoped_listings_are_still_fetched_whole() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let repo = seeded_repo(&project, 3);
    let (mut state, recorder) = recording_state(&project, &repo);
    let root = state.multi.roots[0].id.clone();

    state.caches.ensure_ref_log(&*recorder, &root, "main");
    state
        .caches
        .ensure_path_log(&*recorder, &root, Path::new("file.txt"));

    assert_eq!(
        recorder.log_opts(),
        vec![
            LogOpts {
                branch: Some("main".to_string()),
                ..Default::default()
            },
            LogOpts {
                path: Some(PathBuf::from("file.txt")),
                ..Default::default()
            },
        ],
        "a scoped listing carries no page window"
    );
}

/// A cached row with a chosen id — paging turns on identity.
fn row(root: &RootId, id: &str) -> Commit {
    let sig = Signature {
        name: "t".to_string(),
        email: "t@t".to_string(),
        time: 0,
    };
    Commit {
        id: id.to_string(),
        parents: Vec::new(),
        author: sig.clone(),
        committer: sig,
        message: format!("commit {id}"),
        time: 0,
        root: root.clone(),
        signature: SignatureState::Unsigned,
    }
}

/// Put a whole page into `root`'s window through the cache's own writer,
/// newest first, ids `row0` (newest) … `row{n-1}` (oldest), and state whether
/// history continues past it.
fn prime_window(state: &mut AppState, root: &RootId, n: usize, has_more: bool) {
    let commits: Vec<Commit> = (0..n).map(|i| row(root, &format!("row{i}"))).collect();
    state.caches.store_log(root.clone(), commits);
    state.caches.set_log_has_more(root, has_more);
}

/// A cold root gets one page from the front of the listing: no position, no
/// anchor to checksum against.
#[test]
fn a_cold_root_is_asked_for_one_page_from_the_front() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let repo = seeded_repo(&project, 3);
    let (mut state, recorder) = recording_state(&project, &repo);
    let root_id = state.multi.roots[0].id.clone();

    state.fetch_log(root_id.clone());
    wait_log_calls(&mut state, &recorder, 1);

    assert_eq!(
        recorder.log_opts(),
        vec![LogOpts {
            max_count: Some(LOG_PAGE_SIZE),
            ..Default::default()
        }],
        "the first request is a page of the listing, not a widened prefix"
    );
}

/// A root that already holds a page is asked for the page AFTER it — and for
/// one row more than a page, so the row it already holds comes back as the
/// boundary checksum (P3). `skip` is the position; the walk still starts at
/// HEAD.
#[test]
fn a_warm_root_is_asked_for_the_next_page_with_one_row_of_overlap() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let repo = seeded_repo(&project, 3);
    let (mut state, recorder) = recording_state(&project, &repo);
    let root_id = state.multi.roots[0].id.clone();
    prime_window(&mut state, &root_id, LOG_PAGE_SIZE, true);

    state.fetch_log(root_id.clone());
    wait_log_calls(&mut state, &recorder, 1);

    assert_eq!(
        recorder.log_opts(),
        vec![LogOpts {
            max_count: Some(LOG_PAGE_SIZE + 1),
            skip: Some(LOG_PAGE_SIZE - 1),
            ..Default::default()
        }],
        "one page past a held window, overlapping it by exactly its oldest row"
    );
}

/// Load more is per root: only a window that claims more gets a next page, and
/// a window that does not is left alone.
#[test]
fn load_more_pages_only_the_roots_that_still_have_more() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let alpha = seeded_repo(&project, 3);
    let beta = seeded_repo(&project.join("second"), 3);
    let (mut state, recorder) = recording_state_over(&project, &[alpha.clone(), beta.clone()]);
    let alpha_id = state.multi.roots[0].id.clone();
    let beta_id = state.multi.roots[1].id.clone();
    prime_window(&mut state, &alpha_id, LOG_PAGE_SIZE, true);
    prime_window(&mut state, &beta_id, 3, false);

    state.load_more_log();
    wait_log_calls(&mut state, &recorder, 1);
    state.drain_events();
    assert_eq!(
        recorder.log_call_count(),
        1,
        "one fetch per root that has more — beta's short window is the end"
    );
    assert_eq!(recorder.log_opts()[0].skip, Some(LOG_PAGE_SIZE - 1));

    // With nothing left to load, load more is silent. The explicit fetch below
    // proves the counter is live rather than merely slow.
    state.caches.set_log_has_more(&alpha_id, false);
    state.load_more_log();
    state.fetch_log(beta_id.clone());
    wait_log_calls(&mut state, &recorder, 2);
    state.drain_events();
    assert_eq!(
        recorder.log_call_count(),
        2,
        "a window with nothing left to load must issue no fetch"
    );
}

// --- In-flight log fetch guard (log-open perf, D2) ------------------------------

/// An [`AppState`] over one seeded repo whose engine is a recording wrapper,
/// so `log` invocations are countable.
fn recording_state(project: &Path, repo: &PathBuf) -> (AppState, Arc<RecordingExecutor>) {
    recording_state_over(project, std::slice::from_ref(repo))
}

/// [`AppState::for_roots`] over several seeded repos, behind a recording
/// wrapper.
fn recording_state_over(project: &Path, repos: &[PathBuf]) -> (AppState, Arc<RecordingExecutor>) {
    let recorder: Arc<RecordingExecutor> =
        Arc::new(RecordingExecutor::new(Arc::new(CliExecutor {
            settings: VcsSettings::default(),
        })));
    let state = AppState::for_roots(project, repos).with_executor(recorder.clone());
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
    // must run again, the failure surfaces through last_error, and the window
    // the previous page filled is left alone.
    let held = cached_ids(&state, &root_id);
    state
        .tx
        .send(AppEvent::LogLoaded {
            root: root_id.clone(),
            commits: Err(TgError::Other("offline".to_string())),
            mode: LogPageMode::Replace,
        })
        .expect("send failing LogLoaded");
    state.drain_events();
    assert!(
        state.last_error.is_some(),
        "a failing log load must surface through last_error"
    );
    assert_eq!(
        cached_ids(&state, &root_id),
        held,
        "a failing page must not touch the window"
    );
    state.fetch_log(root_id.clone());
    wait_log_calls(&mut state, &recorder, 3);
    assert!(
        recorder.log_call_count() >= 3,
        "a later fetch must run again after the Err drain"
    );
}
