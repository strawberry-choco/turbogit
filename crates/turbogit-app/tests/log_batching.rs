//! Commit-log batching at the app seam: the fetch requests ONE batch for one
//! root — `skip` supplies the position, `max_count` the batch size — and the
//! window it extends is derived from the cache, never from a stored batch
//! index. Plan: `.scratch/log-paging-redesign/execution-plan.md`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use test_support::RecordingExecutor;
use turbogit_app::events::{AppEvent, LogBatchMode};
use turbogit_app::operation::OpKind;
use turbogit_app::root_caches::{Affected, LogScope};
use turbogit_app::state::{AppState, LOG_BATCH_SIZE};
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

/// A repo whose every commit touches `file`, so that file's history spans the
/// whole log — the long scoped listing a scoped window has to page
/// (log-view-scaling 01). Each commit also ADDS a `token<i>` line, so the
/// number of `token` occurrences grows every commit and a pickaxe search for
/// `token` matches all of them, giving a search scope batches to page too.
fn seeded_file_repo(project: &Path, n: usize, file: &str) -> PathBuf {
    let repo = project.join("alpha");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    let mut body = String::new();
    for i in 0..n {
        body.push_str(&format!("token{i}\n"));
        std::fs::write(repo.join(file), &body).unwrap();
        git(&repo, &["add", file]);
        git(&repo, &["commit", "-q", "-m", &format!("c{i}")]);
    }
    repo
}

/// A repo whose pickaxe term matches only every SECOND commit: every commit
/// touches `tracked.txt`, an even one ADDS a `token{i}` line (so the number of
/// `token` occurrences grows and `-Stoken` matches it) and an odd one adds a
/// `plain{i}` line instead (so the count is unchanged and it does not).
///
/// This is the shape that makes a search listing and the traversal behind it two
/// different lengths, which is the whole reason a search cannot be positioned
/// with `--skip` — `--skip` counts the traversal, and the pickaxe filter is
/// applied after it. `seeded_file_repo` cannot show that: every one of its
/// commits matches, so the two streams coincide and a `--skip` that is off by
/// the number of non-matching commits still lands on the anchor.
///
/// Called with `2 * (2 * LOG_BATCH_SIZE)` commits, it holds exactly
/// `2 * LOG_BATCH_SIZE` matches: two full batches of hits.
fn seeded_interleaved_repo(project: &Path, commits: usize) -> PathBuf {
    let repo = project.join("alpha");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    let mut body = String::new();
    let mut stream = String::new();
    let data = |stream: &mut String, s: &str| {
        stream.push_str(&format!("data {}\n{s}", s.len()));
    };
    for i in 0..commits {
        if i % 2 == 0 {
            body.push_str(&format!("token{i}\n"));
        } else {
            body.push_str(&format!("plain{i}\n"));
        }
        // Marks are the stream's own chain, so seeding one repo costs one
        // subprocess rather than one per commit.
        stream.push_str(&format!(
            "commit refs/heads/main\nmark :{}\n\
             author t <t@t> {} +0000\n\
             committer t <t@t> {} +0000\n",
            i + 1,
            1_000_000_000 + i as i64,
            1_000_000_000 + i as i64
        ));
        data(&mut stream, &format!("c{i}\n"));
        if i > 0 {
            stream.push_str(&format!("from :{i}\n"));
        }
        stream.push_str("M 644 inline tracked.txt\n");
        data(&mut stream, &body);
    }
    let mut child = std::process::Command::new("git")
        .arg("fast-import")
        .current_dir(&repo)
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn git fast-import");
    use std::io::Write as _;
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stream.as_bytes())
        .expect("write fast-import stream");
    let out = child.wait_with_output().expect("wait fast-import");
    assert!(
        out.status.success(),
        "git fast-import failed with {:?}; stderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr),
    );
    git(&repo, &["reset", "-q", "--hard", "main"]);
    repo
}

/// The same repo, seeded once for the whole test binary.
fn interleaved_search_project() -> &'static (PathBuf, PathBuf) {
    static PROJECT: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();
    PROJECT.get_or_init(|| {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("project");
        let repo = seeded_interleaved_repo(&project, 2 * (2 * LOG_BATCH_SIZE));
        std::mem::forget(dir);
        (project, repo)
    })
}

// --- Batches fold into one window (P3/P4) ---------------------------------------

/// git's own answer for `repo` — the independent oracle a batched window is
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

/// git's own answer for one scoped listing in `repo` — the independent oracle
/// a scope that has paged is checked against, uncapped and newest-first.
fn scoped_engine_ids(repo: &Path, term: LogOpts) -> Vec<String> {
    CliExecutor {
        settings: VcsSettings::default(),
    }
    .log(repo, &term)
    .expect("engine log")
    .iter()
    .map(|c| c.id.clone())
    .collect()
}

/// Ids of one scope's held window, newest first.
fn scoped_ids(state: &AppState, root: &RootId, scope: &LogScope) -> Vec<String> {
    state
        .caches
        .scoped_log(root, scope)
        .unwrap_or_default()
        .iter()
        .map(|c| c.id.clone())
        .collect()
}

/// A repo deeper than two batches, seeded once for the whole test binary —
/// one `git commit` subprocess per commit is the slow part of this suite, and
/// nothing here writes to it.
fn deep_project() -> &'static (PathBuf, PathBuf) {
    static PROJECT: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();
    PROJECT.get_or_init(|| {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("project");
        let repo = seeded_repo(&project, 2 * LOG_BATCH_SIZE + 3);
        // The temp dir outlives every test in this binary.
        std::mem::forget(dir);
        (project, repo)
    })
}

/// The same deep repo, but every commit touches `file` — so a path scope on it
/// has the whole history to page through.
fn deep_file_project() -> &'static (PathBuf, PathBuf) {
    static PROJECT: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();
    PROJECT.get_or_init(|| {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("project");
        let repo = seeded_file_repo(&project, 2 * LOG_BATCH_SIZE + 3, "tracked.txt");
        std::mem::forget(dir);
        (project, repo)
    })
}

/// A repo holding exactly one batch of history.
fn one_batch_project() -> &'static (PathBuf, PathBuf) {
    static PROJECT: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();
    PROJECT.get_or_init(|| {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("project");
        let repo = seeded_repo(&project, LOG_BATCH_SIZE);
        std::mem::forget(dir);
        (project, repo)
    })
}

/// Two batches fetched one after the other through the production worker path
/// have to ADD up: the second batch extends the window, and the row both
/// requests overlap on is held once.
#[test]
fn consecutive_batches_append_into_one_window() {
    let (project, repo) = deep_project();
    let (mut state, recorder) = recording_state(project, repo);
    let root = state.multi.roots[0].id.clone();
    let total = 2 * LOG_BATCH_SIZE + 3;

    state.fetch_log(root.clone());
    state.drain_events();
    assert!(
        cached_ids(&state, &root).len() == LOG_BATCH_SIZE,
        "the inline pump settles this in one drain"
    );
    let first = cached_ids(&state, &root);
    assert!(
        state.caches.log_has_more(&root),
        "a full batch cannot be the end of the history"
    );

    state.load_more_log();
    state.drain_events();
    assert!(
        { cached_ids(&state, &root).len() == 2 * LOG_BATCH_SIZE },
        "the inline pump settles this in one drain"
    );
    let second = cached_ids(&state, &root);
    assert_eq!(
        &second[..LOG_BATCH_SIZE],
        &first[..],
        "the rows already held keep their place, newest first"
    );
    assert_eq!(
        second,
        engine_ids(repo)[..2 * LOG_BATCH_SIZE].to_vec(),
        "two appended batches are exactly git's first two batches, no row twice"
    );
    assert!(
        state.caches.log_has_more(&root),
        "another full batch, so still not the end"
    );

    state.load_more_log();
    state.drain_events();
    assert!(
        cached_ids(&state, &root).len() == total,
        "the inline pump settles this in one drain"
    );
    assert_eq!(
        cached_ids(&state, &root),
        engine_ids(repo),
        "the whole window is git's own listing, in git's own order"
    );
    assert!(
        !state.caches.log_has_more(&root),
        "a short batch is the end of history"
    );

    let settled = recorder.log_call_count();
    state.load_more_log();
    assert_eq!(
        recorder.log_call_count(),
        settled,
        "past the end of history, load more must not fetch again"
    );
}

/// A history that is an exact multiple of the batch size ends on the row
/// limit, so the pager cannot know it has arrived until it asks once more.
/// That is the design's accepted cost — exactly one extra fetch, never a
/// repeating one.
#[test]
fn an_exact_multiple_of_the_batch_size_ends_after_one_extra_fetch() {
    let (project, repo) = one_batch_project();
    let (mut state, recorder) = recording_state(project, repo);
    let root = state.multi.roots[0].id.clone();

    state.fetch_log(root.clone());
    state.drain_events();
    assert!(
        cached_ids(&state, &root).len() == LOG_BATCH_SIZE,
        "the inline pump settles this in one drain"
    );
    assert!(
        state.caches.log_has_more(&root),
        "a batch that fills the request offers one more"
    );

    state.load_more_log();
    state.drain_events();
    assert!(
        !state.caches.log_has_more(&root),
        "the inline pump settles this in one drain"
    );
    assert_eq!(
        recorder.log_call_count(),
        2,
        "exactly one extra fetch ends a history of exactly one batch"
    );
    assert_eq!(
        cached_ids(&state, &root).len(),
        LOG_BATCH_SIZE,
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

/// The checksum half of P3: an append batch must lead with the row the window
/// was measured against. A commit landing mid-paging moves HEAD, so the batch
/// leads with something else — and a window that keeps growing from there is
/// torn. The window restarts at batch 0 instead.
#[test]
fn a_commit_landed_mid_paging_restarts_the_window_instead_of_tearing_it() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let repo = seeded_repo(&project, LOG_BATCH_SIZE + 1);
    let (mut state, recorder) = recording_state(&project, &repo);
    let root = state.multi.roots[0].id.clone();

    state.fetch_log(root.clone());
    state.drain_events();
    assert!(
        cached_ids(&state, &root).len() == LOG_BATCH_SIZE,
        "the inline pump settles this in one drain"
    );
    let held_before = cached_ids(&state, &root);

    // HEAD moves while the next batch is in flight.
    git(
        &repo,
        &["commit", "-q", "--allow-empty", "-m", "landed mid-paging"],
    );
    let late = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    state.load_more_log();

    // The rejected batch is never appended: the window is a clean batch 0 that
    // leads with the commit that moved it.
    state.drain_events();
    assert!(
        {
            let ids = cached_ids(&state, &root);
            ids.first().map(String::as_str) == Some(late.as_str())
        },
        "the inline pump settles this in one drain"
    );
    let after = cached_ids(&state, &root);
    assert_eq!(
        after.len(),
        LOG_BATCH_SIZE,
        "a restart is batch 0, not the torn remainder of a window"
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
        LOG_BATCH_SIZE,
        "and holds no row twice"
    );
    assert!(
        state.caches.log_has_more(&root),
        "51 commits, 50 shown — there is still one more"
    );
    assert!(
        recorder.log_call_count() >= 3,
        "the restart refetches batch 0 through the same worker path"
    );
}

/// The same walk with no git in the loop: [`FakeExecutor`] slices its seeded
/// listing by `(skip, max_count)`, so the window can be checked against a
/// literal, hand-written listing rather than one read back from the engine.
#[test]
fn the_batch_walk_never_repeats_or_skips_a_row_of_the_listing() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let repo = seeded_repo(&project, 1);
    let state0 = AppState::for_roots(&project, std::slice::from_ref(&repo));
    let root = state0.multi.roots[0].id.clone();
    let seeded: Vec<Commit> = (0..2 * LOG_BATCH_SIZE + 3)
        .map(|i| row(&root, &format!("r{i}")))
        .collect();

    let mut fake = FakeExecutor::default();
    fake.logs.insert(repo.clone(), seeded.clone());
    let mut state = state0.with_executor(Arc::new(fake));

    state.fetch_log(root.clone());
    state.drain_events();
    assert!(
        cached_ids(&state, &root).len() == LOG_BATCH_SIZE,
        "the inline pump settles this in one drain"
    );
    assert!(state.caches.log_has_more(&root));

    // Batch 2 and batch 3: a full batch, then the short one that ends it.
    for expected in [2 * LOG_BATCH_SIZE, 2 * LOG_BATCH_SIZE + 3] {
        state.load_more_log();
        state.drain_events();
        assert!(
            cached_ids(&state, &root).len() == expected,
            "the inline pump settles this in one drain"
        );
    }
    assert_eq!(
        cached_ids(&state, &root),
        seeded.iter().map(|c| c.id.clone()).collect::<Vec<_>>(),
        "the window is exactly the seeded listing, in order, once each"
    );
    assert!(
        !state.caches.log_has_more(&root),
        "the short batch ended the history"
    );
}

// --- A scoped batch is dispatched, not applied (log-view-scaling 02) -----------
/// A scoped batch leaves the frame that asked for it: the engine has been
/// called by the time the ask returns, but the window only grows when the
/// batch's event drains — the same seam the unscoped listing's batches arrive on
/// (log-view-scaling 02).
#[test]
fn a_scoped_batch_is_admitted_only_when_its_event_drains() {
    let (project, repo) = deep_file_project();
    let (mut state, recorder) = recording_state(project, repo);
    let root = state.multi.roots[0].id.clone();
    let scope = LogScope::Path(PathBuf::from("tracked.txt"));

    state.ensure_log(&root, scope.clone());
    assert_eq!(
        scoped_ids(&state, &root, &scope).len(),
        LOG_BATCH_SIZE,
        "one batch held"
    );

    state.load_more_scoped_log(&root, &scope);
    assert_eq!(
        scoped_ids(&state, &root, &scope).len(),
        LOG_BATCH_SIZE,
        "the batch is in flight, not in the window: the frame that asked does not apply it"
    );
    assert_eq!(
        recorder.log_call_count(),
        2,
        "and the engine has been asked for it"
    );

    state.drain_events();
    assert_eq!(
        scoped_ids(&state, &root, &scope).len(),
        2 * LOG_BATCH_SIZE,
        "the drain admits it, exactly as it admits an unscoped batch"
    );
    assert!(
        state.caches.scoped_log_has_more(&root, &scope),
        "and the flag says the scope still has more"
    );
}

/// The same two phases under the pump the desktop app runs.
/// [`AppState::new`] is the production constructor — its reads are spawned on a
/// worker thread — so here the scoped batch is genuinely read off the asking
/// frame: the ask returns with the window it already had, and the batch lands
/// on a later frame's drain (log-view-scaling 02).
#[test]
fn a_scoped_batch_lands_on_a_later_frame_under_the_spawned_pump() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let repo = seeded_file_repo(&project, 1, "tracked.txt");
    let mut fake = FakeExecutor::default();
    let row_root = RootId(std::sync::Arc::from(repo.clone()));
    fake.logs.insert(
        repo.clone(),
        (0..2 * LOG_BATCH_SIZE)
            .map(|i| row(&row_root, &format!("r{i}")))
            .collect(),
    );
    let recorder = Arc::new(RecordingExecutor::new(Arc::new(fake)));
    let mut state = AppState::new(project.clone()).with_executor(recorder.clone());
    let root = state.multi.roots[0].id.clone();
    let scope = LogScope::Path(PathBuf::from("tracked.txt"));

    state.ensure_log(&root, scope.clone());
    assert_eq!(
        scoped_ids(&state, &root, &scope).len(),
        LOG_BATCH_SIZE,
        "one batch held before anything is asked of a worker"
    );

    state.load_more_scoped_log(&root, &scope);
    assert_eq!(
        scoped_ids(&state, &root, &scope).len(),
        LOG_BATCH_SIZE,
        "the ask returned with the window it already had: the frame that asked for a \
         scoped batch did not wait for it"
    );

    // The worker's answer arrives on the channel, so a later frame's drain is
    // what admits it. The wait is bounded: a worker that never answered would
    // fail this test rather than hang it.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        state.drain_events();
        if scoped_ids(&state, &root, &scope).len() == 2 * LOG_BATCH_SIZE {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the spawned read never answered"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        state.caches.scoped_log_has_more(&root, &scope),
        "and the flag came with it"
    );
}

// --- One in-flight read per (root, scope) (log-view-scaling 02) --------------

/// The tool-window body asks for the scope's next batch whenever it draws, so
/// two asks in one frame — before the first batch has landed — must produce
/// exactly one engine call, the same guard the unscoped listing has
/// (log-view-scaling 02).
#[test]
fn two_scoped_asks_in_one_frame_ask_the_engine_once() {
    let (project, repo) = deep_file_project();
    let (mut state, recorder) = recording_state(project, repo);
    let root = state.multi.roots[0].id.clone();
    let scope = LogScope::Path(PathBuf::from("tracked.txt"));

    state.ensure_log(&root, scope.clone());
    // The first ask is already in flight when the second one is made.
    state.load_more_scoped_log(&root, &scope);
    state.load_more_scoped_log(&root, &scope);

    assert_eq!(
        recorder.log_call_count(),
        2,
        "the cold read plus ONE batched read: the second ask is the same read"
    );
    assert_eq!(
        recorder.log_opts().last().map(|o| o.skip),
        Some(Some(LOG_BATCH_SIZE - 1)),
        "and the one call is the batch after the window the scope already holds"
    );
    assert_eq!(
        scoped_ids(&state, &root, &scope).len(),
        LOG_BATCH_SIZE,
        "neither ask has been admitted yet"
    );

    state.drain_events();
    state.drain_events();
    assert_eq!(
        scoped_ids(&state, &root, &scope).len(),
        2 * LOG_BATCH_SIZE,
        "one batch, admitted once — the window never grows by two batches at once"
    );
    assert_eq!(
        recorder.log_call_count(),
        2,
        "and no second read was smuggled in behind the guard"
    );

    // The drained batch freed the guard, so asking again goes out.
    state.load_more_scoped_log(&root, &scope);
    assert_eq!(
        recorder.log_call_count(),
        3,
        "the Ok drain freed the guard for this scope"
    );
}

// --- The in-flight guard releases on both outcomes (log-view-scaling 02) -----

/// A failing scoped read must not wedge the scope: the guard releases when the
/// failed batch drains, so the next ask goes out and the window the earlier
/// batches filled survives the failure (log-view-scaling 02).
///
/// The failure is a real one — the ref this scope reads stops existing, so git
/// refuses the read — which is the only way to hold the guard while a genuine
/// failure drains it.
#[test]
fn a_failing_scoped_read_leaves_the_scope_able_to_load_another_batch() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let repo = seeded_repo(&project, LOG_BATCH_SIZE + 1);
    let (mut state, recorder) = recording_state(&project, &repo);
    let root = state.multi.roots[0].id.clone();
    let scope = LogScope::Ref("main".to_string());

    state.ensure_log(&root, scope.clone());
    assert_eq!(
        scoped_ids(&state, &root, &scope).len(),
        LOG_BATCH_SIZE,
        "one batch of the ref's history"
    );
    assert!(
        state.caches.scoped_log_has_more(&root, &scope),
        "and more of it to ask for"
    );

    // The ref goes away, so the next read of this scope fails for real.
    git(&repo, &["checkout", "-q", "-b", "other"]);
    git(&repo, &["branch", "-D", "main"]);
    state.load_more_scoped_log(&root, &scope);
    assert_eq!(recorder.log_call_count(), 2, "the read was dispatched");

    state.drain_events();
    assert!(
        state.last_error.is_some(),
        "a failing scoped read must surface through last_error"
    );
    assert_eq!(
        scoped_ids(&state, &root, &scope).len(),
        LOG_BATCH_SIZE,
        "a failed batch must not touch the window it was extending"
    );
    assert!(
        state.caches.scoped_log_has_more(&root, &scope),
        "and the scope still knows it has history to page for"
    );

    state.load_more_scoped_log(&root, &scope);
    assert_eq!(
        recorder.log_call_count(),
        3,
        "the Err drain freed the guard, so a scope whose read failed can page again"
    );
}

/// A repo whose every commit touches `tracked.txt` and every other one touches
/// `sparse.txt` as well — so a path scope on the second file and a ref scope on
/// `main` are two listings of clearly different lengths, which is what makes
/// "this batch landed in the wrong window" visible (log-view-scaling 02).
fn two_file_project() -> &'static (PathBuf, PathBuf) {
    static PROJECT: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();
    PROJECT.get_or_init(|| {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("project");
        let repo = project.join("alpha");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "t@t"]);
        git(&repo, &["config", "user.name", "t"]);
        for i in 0..3 * LOG_BATCH_SIZE + 1 {
            // A commit that does not touch `sparse.txt` is an empty one: it is
            // in the ref's history and not in the file's, which is the whole
            // point of the fixture.
            if i % 2 == 0 {
                std::fs::write(repo.join("sparse.txt"), format!("sparse{i}\n")).unwrap();
                git(&repo, &["add", "sparse.txt"]);
                git(&repo, &["commit", "-q", "-m", &format!("c{i}")]);
            } else {
                git(
                    &repo,
                    &["commit", "-q", "--allow-empty", "-m", &format!("c{i}")],
                );
            }
        }
        std::mem::forget(dir);
        (project, repo)
    })
}

// --- A batch lands in the scope it was read for (log-view-scaling 02) --------

/// A batch that arrives off-thread is admitted against the scope it was READ
/// for, whatever order the answers come back in: here the ref's batch is
/// injected first, so a slow ref read cannot land in the path window that is
/// waiting for its own batch (log-view-scaling 02).
#[test]
fn a_batch_arriving_off_thread_lands_in_its_own_scopes_window() {
    let (project, repo) = two_file_project();
    let (mut state, _recorder) = recording_state(project, repo);
    let root = state.multi.roots[0].id.clone();
    let sparse = LogScope::Path(PathBuf::from("sparse.txt"));
    let by_ref = LogScope::Ref("main".to_string());

    state.ensure_log(&root, sparse.clone());
    state.ensure_log(&root, by_ref.clone());
    let path_before = scoped_ids(&state, &root, &sparse);
    let ref_before = scoped_ids(&state, &root, &by_ref);

    // A batch is the anchor row the window already holds plus what came after
    // it, so each batch is a batch of the listing it was read for — and says so
    // in its row ids.
    let batch_for = |state: &AppState, root: &RootId, scope: &LogScope, next: &str| {
        let anchor = state
            .caches
            .scoped_log(root, scope)
            .and_then(|w| w.last())
            .expect("a held window")
            .clone();
        let mode = LogBatchMode::Append {
            anchor: anchor.id.clone(),
        };
        (vec![anchor, row(root, next)], mode)
    };
    let (ref_batch, ref_mode) = batch_for(&state, &root, &by_ref, "ref-only-next");
    let (path_batch, path_mode) = batch_for(&state, &root, &sparse, "path-only-next");

    // The ref's answer lands first — the slow read overtaking the path's.
    for (scope, commits, mode) in [
        (by_ref.clone(), Ok(ref_batch), ref_mode),
        (sparse.clone(), Ok(path_batch), path_mode),
    ] {
        state
            .tx
            .send(AppEvent::LogBatchLoaded {
                root: root.clone(),
                scope,
                commits,
                mode,
            })
            .expect("send LogBatchLoaded");
    }
    state.drain_events();

    let mut ref_after = ref_before;
    ref_after.push(String::from("ref-only-next"));
    let mut path_after = path_before;
    path_after.push(String::from("path-only-next"));
    assert_eq!(
        scoped_ids(&state, &root, &by_ref),
        ref_after,
        "the ref window grew by the ref's own row and nothing else"
    );
    assert_eq!(
        scoped_ids(&state, &root, &sparse),
        path_after,
        "the path window grew by the path's own row and nothing else"
    );
}

/// Two scopes of ONE root asked in the same frame each get their own answer:
/// the in-flight key is the pair, not the root, so neither read swallows the
/// other and neither window can grow by the other's rows
/// (log-view-scaling 02).
#[test]
fn two_scopes_of_one_root_paging_at_once_each_get_their_own_answer() {
    let (project, repo) = two_file_project();
    let (mut state, recorder) = recording_state(project, repo);
    let root = state.multi.roots[0].id.clone();
    let sparse = LogScope::Path(PathBuf::from("sparse.txt"));
    let by_ref = LogScope::Ref("main".to_string());
    let sparse_listing = scoped_engine_ids(
        repo,
        LogOpts {
            path: Some(PathBuf::from("sparse.txt")),
            ..Default::default()
        },
    );
    let ref_listing = scoped_engine_ids(
        repo,
        LogOpts {
            branch: Some("main".to_string()),
            ..Default::default()
        },
    );
    assert!(
        sparse_listing.len() > LOG_BATCH_SIZE && ref_listing.len() > 2 * LOG_BATCH_SIZE,
        "both scopes have to have more than a batch, and the two listings must differ: \
         {} and {}",
        sparse_listing.len(),
        ref_listing.len()
    );

    state.ensure_log(&root, sparse.clone());
    state.ensure_log(&root, by_ref.clone());
    // Both batches are dispatched in the same frame, before either has landed.
    state.load_more_scoped_log(&root, &sparse);
    state.load_more_scoped_log(&root, &by_ref);
    assert_eq!(
        recorder.log_call_count(),
        4,
        "two cold reads plus one batched read each: neither scope was held back by the other"
    );

    state.drain_events();
    assert_eq!(
        scoped_ids(&state, &root, &sparse),
        sparse_listing,
        "the file scope paged to the end of ITS listing, and no row of anyone else's"
    );
    assert_eq!(
        scoped_ids(&state, &root, &by_ref),
        ref_listing[..2 * LOG_BATCH_SIZE],
        "the ref scope holds two batches of its own, longer listing"
    );
    assert!(
        !state.caches.scoped_log_has_more(&root, &sparse),
        "the file scope's own batch said it was the end"
    );
    assert!(
        state.caches.scoped_log_has_more(&root, &by_ref),
        "while the ref scope's own batch said there is more — two answers, two flags"
    );
}

/// A completed operation moves HEAD, which invalidates the offsets the window
/// was built on. The refetch restarts it at batch 0 — it must neither continue
/// the grown window nor smuggle a whole-history write past the pager.
#[test]
fn a_completed_op_resets_the_window_to_one_batch() {
    let (project, repo) = deep_project();
    let (mut state, recorder) = recording_state(project, repo);
    let root = state.multi.roots[0].id.clone();

    state.fetch_log(root.clone());
    state.drain_events();
    assert!(
        cached_ids(&state, &root).len() == LOG_BATCH_SIZE,
        "the inline pump settles this in one drain"
    );
    state.load_more_log();
    state.drain_events();
    assert!(
        { cached_ids(&state, &root).len() == 2 * LOG_BATCH_SIZE },
        "the inline pump settles this in one drain"
    );

    state
        .tx
        .send(AppEvent::OpCompleted {
            kind: OpKind::Other,
            label: "op".to_string(),
            affected: Affected::Root(root.clone()),
            result: Ok(()),
        })
        .expect("send OpCompleted");
    state.drain_events();

    assert_eq!(
        cached_ids(&state, &root),
        engine_ids(repo)[..LOG_BATCH_SIZE].to_vec(),
        "the window is one batch again, from the front of the listing"
    );
    assert!(
        state.caches.log_has_more(&root),
        "and it knows the history continues past it"
    );
    assert_eq!(
        recorder.log_opts().last(),
        Some(&LogOpts {
            max_count: Some(LOG_BATCH_SIZE),
            ..Default::default()
        }),
        "the refresh asks for a batch, not for the whole history"
    );
}

// --- A search listing pages in the MATCH stream (log-view-scaling 10) -----------

/// THE REGRESSION, and the test that would have caught the defect it names: a
/// search over a deep history returns MORE than one batch of matches, and the
/// batches concatenate into git's own match stream — no row twice, no row
/// skipped, newest first to the end.
///
/// It is deliberately the shape `seeded_file_repo` cannot express. There every
/// commit matches, so a `--skip` positioned in the traversal also positions the
/// match stream, and a search that is off by nothing looks right. Here only
/// every second commit matches, so the two streams are half as long as each
/// other, and the batch that a traversal position asks for leads with the wrong
/// row. That is the whole defect: `--skip` counts the traversal and `-S`
/// filters after it, so a search batch requested with a `skip` never leads with
/// the anchor the window is checked against. It is read as torn, the window is
/// dropped, the scope is read again from the front, and it lands on the same
/// first batch — a listing frozen at one batch, which is what capping a search at
/// `LOG_BATCH_SIZE` silently did to every broad search in a long history.
#[test]
fn a_search_over_a_deep_history_pages_past_one_batch_of_matches() {
    let (project, repo) = interleaved_search_project();
    let (mut state, _recorder) = recording_state(project, repo);
    let root = state.multi.roots[0].id.clone();
    let scope = LogScope::Search("token".to_string());
    let term = LogOpts {
        pickaxe: Some("token".to_string()),
        ..Default::default()
    };

    // git's own answer, uncapped: the oracle every batch is checked against.
    let whole = scoped_engine_ids(repo, term);
    assert_eq!(
        whole.len(),
        2 * LOG_BATCH_SIZE,
        "the fixture's premise: two full batches of hits, with twice as many commits behind them"
    );
    assert_eq!(
        engine_ids(repo).len(),
        2 * whole.len(),
        "and exactly twice as many commits sit behind them, so the traversal and the \
         match stream are different lengths — which is what makes them impossible to \
         position with one skip"
    );

    state.ensure_log(&root, scope.clone());
    assert_eq!(
        scoped_ids(&state, &root, &scope),
        whole[..LOG_BATCH_SIZE],
        "one batch of the matches, from the front, newest first"
    );
    assert!(
        state.caches.scoped_log_has_more(&root, &scope),
        "a full first batch cannot be the end of the match stream"
    );

    state.load_more_scoped_log(&root, &scope);
    state.drain_events();

    assert_eq!(
        scoped_ids(&state, &root, &scope),
        whole,
        "the second batch carries the other half: no row twice, no row skipped, \
         newest first, and the window is git's own match stream to the end"
    );
    // The match stream is an exact multiple of the batch size, so it ends ON the
    // row limit: a full batch cannot be told from the end, and the flag says
    // there is more. That is the same extra fetch every other listing costs, and
    // it is the rule the flag is set by — not a search-shaped one.
    assert!(
        state.caches.scoped_log_has_more(&root, &scope),
        "100 matches is 100 rows asked for, so the flag cannot know it has arrived"
    );

    state.load_more_scoped_log(&root, &scope);
    state.drain_events();
    assert_eq!(
        scoped_ids(&state, &root, &scope),
        whole,
        "the batch past the end brings the anchor row alone, so the window does not \
         move"
    );
    assert!(
        !state.caches.scoped_log_has_more(&root, &scope),
        "and now the match stream is over"
    );
}

/// The search plan itself, read off the requests the engine was actually asked
/// to make: one batch from the front when the scope is cold, and then from the
/// front again for everything it holds plus one batch, with NO skip at all.
///
/// A `skip` is the whole defect, so its absence is the point of the assertion —
/// and the growing `max_count` is what replaces it: `held + LOG_BATCH_SIZE` rows
/// of the match stream, whose tail is the batch.
#[test]
fn a_warm_search_scope_is_asked_for_what_it_holds_plus_one_batch_from_the_front() {
    let (project, repo) = interleaved_search_project();
    let (mut state, recorder) = recording_state(project, repo);
    let root = state.multi.roots[0].id.clone();
    let scope = LogScope::Search("token".to_string());
    let term = LogOpts {
        pickaxe: Some("token".to_string()),
        ..Default::default()
    };

    state.ensure_log(&root, scope.clone());
    state.load_more_scoped_log(&root, &scope);
    state.drain_events();

    assert_eq!(
        recorder.log_opts(),
        vec![
            LogOpts {
                max_count: Some(LOG_BATCH_SIZE),
                pickaxe: Some("token".to_string()),
                ..Default::default()
            },
            LogOpts {
                max_count: Some(2 * LOG_BATCH_SIZE),
                skip: None,
                pickaxe: Some("token".to_string()),
                ..Default::default()
            },
        ],
        "a search asks from the front of its match stream: a cold scope for one \
         batch, a held one for what it holds plus one more, and never with a skip"
    );
    assert_eq!(
        term.pickaxe.as_deref(),
        Some("token"),
        "the term rides along"
    );
}

/// A search whose whole match stream is shorter than one batch has said its
/// history is over: the window reports no more, and asking again issues no read
/// at all — one cold read for the listing, and never a second. The short batch
/// is an answer, not a reason to ask twice (log-view-scaling 02), and that rule
/// is the admission decision every scope shares.
#[test]
fn a_search_shorter_than_one_batch_reports_no_more_history_and_is_asked_once() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    // Six commits, every second one a match: three hits, and a traversal twice
    // as long — so this is a short batch in a real match stream, not a stub.
    let repo = seeded_interleaved_repo(&project, 6);
    let (mut state, recorder) = recording_state(&project, &repo);
    let root = state.multi.roots[0].id.clone();
    let scope = LogScope::Search("token".to_string());

    state.ensure_log(&root, scope.clone());
    let held = scoped_ids(&state, &root, &scope);
    assert_eq!(
        held.len(),
        3,
        "the whole match stream, and the premise: three is fewer than a batch"
    );
    assert!(
        !state.caches.scoped_log_has_more(&root, &scope),
        "three rows is a short batch, so the match stream is over"
    );

    state.load_more_scoped_log(&root, &scope);
    assert_eq!(
        recorder.log_call_count(),
        1,
        "past the end of its match stream a search must not be asked again"
    );
    assert_eq!(
        scoped_ids(&state, &root, &scope),
        held,
        "and the held rows are left alone"
    );
}

/// The other two scopes are untouched by any of this, and the way to know is to
/// pin the requests: for the same window length, a path scope and a ref scope
/// still ask for one row more than a batch, `held - 1` deep, so the row the
/// window already holds comes back as the boundary checksum.
#[test]
fn a_path_and_a_ref_scope_are_still_asked_with_the_skip_they_always_were() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let repo = seeded_repo(&project, 1);
    let state0 = AppState::for_roots(&project, std::slice::from_ref(&repo));
    let root = state0.multi.roots[0].id.clone();
    let mut fake = FakeExecutor::default();
    fake.logs.insert(
        repo.clone(),
        (0..2 * LOG_BATCH_SIZE)
            .map(|i| row(&root, &format!("r{i}")))
            .collect(),
    );
    let recorder = Arc::new(RecordingExecutor::new(Arc::new(fake)));
    let mut state = state0.with_executor(recorder.clone());

    for (scope, term) in [
        (
            LogScope::Path(PathBuf::from("tracked.txt")),
            LogOpts {
                path: Some(PathBuf::from("tracked.txt")),
                ..Default::default()
            },
        ),
        (
            LogScope::Ref("main".to_string()),
            LogOpts {
                branch: Some("main".to_string()),
                ..Default::default()
            },
        ),
    ] {
        // Each scope's own cold request, then the request for its next batch:
        // the second one is what carries the position.
        let before = recorder.log_opts().len();
        state.ensure_log(&root, scope.clone());
        state.load_more_scoped_log(&root, &scope);
        assert_eq!(
            recorder.log_opts()[before + 1],
            LogOpts {
                max_count: Some(LOG_BATCH_SIZE + 1),
                skip: Some(LOG_BATCH_SIZE - 1),
                ..term
            },
            "{scope:?} pages by skip and by the overlap row, exactly as it always has"
        );
    }
}

/// The checksum half, for a search: a commit that MATCHES landing on the listing
/// moves the match stream's head, so the batch fetched against the window comes
/// back with a different row where the anchor is. A search window that grew from
/// there would be torn, so it restarts at batch 0 instead — the same boundary
/// every other listing batches on, and the reason the batch is cut by position
/// rather than by hunting the anchor down in the page.
#[test]
fn a_commit_landing_on_a_search_listing_restarts_the_search_window() {
    // A repo of its own: this test LANDS a commit in it, and the shared fixture
    // other tests measure against has to stay exactly as seeded.
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let repo = seeded_interleaved_repo(&project, 2 * (2 * LOG_BATCH_SIZE));
    let (mut state, recorder) = recording_state(&project, &repo);
    let root = state.multi.roots[0].id.clone();
    let scope = LogScope::Search("token".to_string());

    state.ensure_log(&root, scope.clone());
    assert_eq!(scoped_ids(&state, &root, &scope).len(), LOG_BATCH_SIZE);
    let held_before = scoped_ids(&state, &root, &scope);

    // A commit that changes the number of `token` occurrences lands, so the match
    // stream — not just the traversal — grows by one at its head.
    let mut body = git(&repo, &["show", "HEAD:tracked.txt"]);
    body.push_str("token-landed\n");
    std::fs::write(repo.join("tracked.txt"), &body).unwrap();
    git(&repo, &["add", "tracked.txt"]);
    git(&repo, &["commit", "-q", "-m", "landed mid-search"]);
    let late = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();

    state.load_more_scoped_log(&root, &scope);
    state.drain_events();

    let after = scoped_ids(&state, &root, &scope);
    assert_eq!(
        after.len(),
        LOG_BATCH_SIZE,
        "a restart is batch 0, not the torn remainder of a window"
    );
    assert_eq!(
        after.first().map(String::as_str),
        Some(late.as_str()),
        "the refetched window leads with the match that moved it"
    );
    assert_ne!(
        after, held_before,
        "and it is not the window it was asked to extend"
    );
    assert_eq!(
        after,
        scoped_engine_ids(
            &repo,
            LogOpts {
                pickaxe: Some("token".to_string()),
                ..Default::default()
            }
        )[..LOG_BATCH_SIZE]
            .to_vec(),
        "it is exactly git's first batch of matches, newest first, no row twice"
    );
    assert!(
        state.caches.scoped_log_has_more(&root, &scope),
        "and the match stream is still longer than one batch"
    );
    assert_eq!(
        recorder.log_call_count(),
        3,
        "the cold read, the torn batch, and the restart from the front"
    );
}

// --- A scoped listing pages too (log-view-scaling 01) --------------------------

/// A path scope is one batch of that file's history, asked for exactly the way
/// the unscoped window asks for a batch: one batch from the front, with the
/// file's path riding alongside as the filter (log-view-scaling 01). The
/// engine composes the two into a single `git log`, so the scope is still one
/// call.
#[test]
fn a_path_scope_is_asked_for_one_batch_from_the_front() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let repo = seeded_repo(&project, 3);
    let (mut state, recorder) = recording_state(&project, &repo);
    let root = state.multi.roots[0].id.clone();

    state.ensure_log(&root, LogScope::Path(PathBuf::from("file.txt")));

    assert_eq!(
        recorder.log_opts(),
        vec![LogOpts {
            max_count: Some(LOG_BATCH_SIZE),
            path: Some(PathBuf::from("file.txt")),
            ..Default::default()
        }],
        "a scoped read is a batch of the scoped listing, not the whole of it"
    );
}

/// A scope that already holds a batch is asked for the batch AFTER it — and for
/// one row more than a batch, so the row it already holds comes back as the
/// boundary checksum. The scoped window pages by the same rule as the
/// unscoped one (log-view-scaling 01).
#[test]
fn a_warm_path_scope_is_asked_for_the_next_batch_with_one_row_of_overlap() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let repo = seeded_repo(&project, 1);
    let state0 = AppState::for_roots(&project, std::slice::from_ref(&repo));
    let root = state0.multi.roots[0].id.clone();
    let mut fake = FakeExecutor::default();
    fake.logs.insert(
        repo.clone(),
        (0..2 * LOG_BATCH_SIZE)
            .map(|i| row(&root, &format!("r{i}")))
            .collect(),
    );
    let recorder = Arc::new(RecordingExecutor::new(Arc::new(fake)));
    let mut state = state0.with_executor(recorder.clone());

    let scope = LogScope::Path(PathBuf::from("tracked.txt"));
    state.ensure_log(&root, scope.clone());
    state.load_more_scoped_log(&root, &scope);

    assert_eq!(
        recorder.log_opts(),
        vec![
            LogOpts {
                max_count: Some(LOG_BATCH_SIZE),
                path: Some(PathBuf::from("tracked.txt")),
                ..Default::default()
            },
            LogOpts {
                max_count: Some(LOG_BATCH_SIZE + 1),
                skip: Some(LOG_BATCH_SIZE - 1),
                path: Some(PathBuf::from("tracked.txt")),
                ..Default::default()
            },
        ],
        "a held scope is asked for the next batch, overlapping it by its oldest row"
    );
}

/// A path scope's batches have to ADD UP: the second batch extends the window,
/// the row both batches overlap on is held once, and the whole walk is git's own
/// listing of that file in git's own order (log-view-scaling 01).
#[test]
fn a_path_scope_concatenates_into_one_newest_first_window() {
    let (project, repo) = deep_file_project();
    let (mut state, _recorder) = recording_state(project, repo);
    let root = state.multi.roots[0].id.clone();
    let scope = LogScope::Path(PathBuf::from("tracked.txt"));
    let whole = scoped_engine_ids(
        repo,
        LogOpts {
            path: Some(PathBuf::from("tracked.txt")),
            ..Default::default()
        },
    );
    assert_eq!(
        whole.len(),
        2 * LOG_BATCH_SIZE + 3,
        "the scoped listing has to span batches for this to mean anything"
    );

    state.ensure_log(&root, scope.clone());
    let first = scoped_ids(&state, &root, &scope);
    assert_eq!(
        first.len(),
        LOG_BATCH_SIZE,
        "one batch of the file's history"
    );
    assert_eq!(
        first,
        whole[..LOG_BATCH_SIZE],
        "the first read is the head of that file's listing, newest first"
    );

    state.load_more_scoped_log(&root, &scope);
    state.drain_events();
    let second = scoped_ids(&state, &root, &scope);
    assert_eq!(
        &second[..LOG_BATCH_SIZE],
        &first[..],
        "the rows already held keep their place, newest first"
    );
    assert_eq!(
        second.len(),
        2 * LOG_BATCH_SIZE,
        "the second batch is appended to the window, not substituted for it"
    );
    assert_eq!(
        second,
        whole[..2 * LOG_BATCH_SIZE],
        "two appended batches are exactly git's first two batches of that file"
    );

    state.load_more_scoped_log(&root, &scope);
    state.drain_events();
    assert_eq!(
        scoped_ids(&state, &root, &scope),
        whole,
        "no row twice, no row skipped, newest first to the end"
    );
    assert!(
        !state.caches.scoped_log_has_more(&root, &scope),
        "the short batch that completed the listing is the end of it"
    );
}

/// The other two scopes page by the same rules, each with its filter in its
/// own `LogOpts` field (log-view-scaling 01).
#[test]
fn a_ref_scope_and_a_pickaxe_search_load_batches_the_same_way() {
    let (project, repo) = deep_file_project();
    let (mut state, _recorder) = recording_state(project, repo);
    let root = state.multi.roots[0].id.clone();

    for (scope, term) in [
        (
            LogScope::Ref("main".to_string()),
            LogOpts {
                branch: Some("main".to_string()),
                ..Default::default()
            },
        ),
        (
            LogScope::Search("token".to_string()),
            LogOpts {
                pickaxe: Some("token".to_string()),
                ..Default::default()
            },
        ),
    ] {
        let whole = scoped_engine_ids(repo, term);
        assert!(
            whole.len() > 2 * LOG_BATCH_SIZE,
            "{scope:?} must span batches for this to mean anything ({} entries)",
            whole.len()
        );

        state.ensure_log(&root, scope.clone());
        assert_eq!(
            scoped_ids(&state, &root, &scope),
            whole[..LOG_BATCH_SIZE],
            "{scope:?} reads one batch from the front of its listing"
        );

        state.load_more_scoped_log(&root, &scope);
        state.drain_events();
        assert_eq!(
            scoped_ids(&state, &root, &scope),
            whole[..2 * LOG_BATCH_SIZE],
            "{scope:?} appends the batch after it, whole and in order"
        );
        assert!(
            state.caches.scoped_log_has_more(&root, &scope),
            "{scope:?} carries its own has-more flag"
        );
    }
}

/// A scope shorter than one batch has already said its history is over: the
/// window reports no more, and asking again issues no fetch at all.
#[test]
fn a_scope_shorter_than_a_batch_reports_no_more_history() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let repo = seeded_repo(&project, 1);
    let state0 = AppState::for_roots(&project, std::slice::from_ref(&repo));
    let root = state0.multi.roots[0].id.clone();
    let mut fake = FakeExecutor::default();
    fake.logs.insert(
        repo.clone(),
        (0..3).map(|i| row(&root, &format!("r{i}"))).collect(),
    );
    let recorder = Arc::new(RecordingExecutor::new(Arc::new(fake)));
    let mut state = state0.with_executor(recorder.clone());

    let scope = LogScope::Search("token".to_string());
    state.ensure_log(&root, scope.clone());
    assert_eq!(
        scoped_ids(&state, &root, &scope).len(),
        3,
        "the whole scope"
    );
    assert!(
        !state.caches.scoped_log_has_more(&root, &scope),
        "three rows is a short batch, so the listing is over"
    );

    state.load_more_scoped_log(&root, &scope);
    assert_eq!(
        recorder.log_call_count(),
        1,
        "past the end of its history, a scope must not be asked again"
    );
    assert_eq!(
        scoped_ids(&state, &root, &scope).len(),
        3,
        "and the held rows are left alone"
    );
}

/// Each scope carries its own paging flag, set by the same rule the unscoped
/// window uses — a full batch means more, a full batch plus the overlap row
/// means more, a short batch is the end — and the flag is public app state
/// (log-view-scaling 01). Exhausting one scope leaves every other flag
/// exactly where it was.
#[test]
fn each_scope_carries_its_own_has_more_flag() {
    let (project, repo) = deep_file_project();
    let (mut state, _recorder) = recording_state(project, repo);
    let root = state.multi.roots[0].id.clone();
    let path = LogScope::Path(PathBuf::from("tracked.txt"));
    let search = LogScope::Search("token".to_string());

    assert!(
        !state.caches.scoped_log_has_more(&root, &path),
        "a scope nobody has read has no opinion about its history"
    );

    state.fetch_log(root.clone());
    state.drain_events();
    state.ensure_log(&root, path.clone());
    assert!(
        state.caches.scoped_log_has_more(&root, &path),
        "a full first batch cannot be the end of the listing"
    );
    state.load_more_scoped_log(&root, &path);
    state.drain_events();
    assert!(
        state.caches.scoped_log_has_more(&root, &path),
        "a full batch plus its overlap row cannot be the end either"
    );
    state.load_more_scoped_log(&root, &path);
    state.drain_events();
    assert_eq!(
        scoped_ids(&state, &root, &path).len(),
        2 * LOG_BATCH_SIZE + 3,
        "and this scope has now paged to the end of its own listing"
    );
    assert!(
        !state.caches.scoped_log_has_more(&root, &path),
        "the short batch that finished it is the end"
    );
    assert!(
        state.caches.log_has_more(&root),
        "the unscoped window keeps its own flag — it is a different listing"
    );

    state.ensure_log(&root, search.clone());
    assert!(
        state.caches.scoped_log_has_more(&root, &search),
        "and so does every other scope: this one is still mid-history"
    );
    assert_eq!(
        scoped_ids(&state, &root, &search).len(),
        LOG_BATCH_SIZE,
        "one batch of it, whatever the other scope has paged to"
    );
}

/// The checksum half, for a scope: a commit landing on the scoped listing
/// moves its head, so the batch fetched against the window comes back leading
/// with a different row. A scoped window that grew from there would be torn,
/// so it restarts at batch 0 instead — the same boundary the unscoped window
/// batches on (log-view-scaling 01).
#[test]
fn a_commit_landing_on_a_scoped_listing_restarts_the_scope_instead_of_tearing_it() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let repo = seeded_file_repo(&project, LOG_BATCH_SIZE + 1, "tracked.txt");
    let (mut state, recorder) = recording_state(&project, &repo);
    let root = state.multi.roots[0].id.clone();
    let scope = LogScope::Path(PathBuf::from("tracked.txt"));
    let term = LogOpts {
        path: Some(PathBuf::from("tracked.txt")),
        ..Default::default()
    };

    state.ensure_log(&root, scope.clone());
    assert_eq!(
        scoped_ids(&state, &root, &scope).len(),
        LOG_BATCH_SIZE,
        "one batch of the file's history"
    );
    let held_before = scoped_ids(&state, &root, &scope);

    // A commit lands on the scoped listing while its next batch is in flight.
    std::fs::write(repo.join("tracked.txt"), "token0\nlanded\n").unwrap();
    git(&repo, &["add", "tracked.txt"]);
    git(&repo, &["commit", "-q", "-m", "landed mid-paging"]);
    let late = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();

    state.load_more_scoped_log(&root, &scope);
    state.drain_events();

    // The torn batch is never held: the window is a clean batch 0 that leads
    // with the commit that moved the listing.
    let after = scoped_ids(&state, &root, &scope);
    assert_eq!(
        after.len(),
        LOG_BATCH_SIZE,
        "a restart is batch 0, not the torn remainder of a window"
    );
    assert_eq!(
        after.first().map(String::as_str),
        Some(late.as_str()),
        "the refetched window leads with the commit that moved it"
    );
    assert_ne!(
        after, held_before,
        "and it is not the window it was asked to extend"
    );
    assert_eq!(
        after,
        scoped_engine_ids(&repo, term)[..LOG_BATCH_SIZE].to_vec(),
        "it is exactly git's first batch of that file, newest first, no row twice"
    );
    assert!(
        state.caches.scoped_log_has_more(&root, &scope),
        "52 commits in this file, 50 shown — there is still one more"
    );
    assert_eq!(
        recorder.log_call_count(),
        3,
        "the cold read, the torn batch, and the restart from the front"
    );
}

/// Four windows over one repository — the unscoped listing and the three
/// scopes — stay four: reading or paging one never fills another, and one
/// invalidation of the root drops all four together (log-view-scaling 01).
#[test]
fn scoped_windows_stay_separate_from_each_other_and_invalidate_together() {
    let (project, repo) = deep_file_project();
    let (mut state, _recorder) = recording_state(project, repo);
    let root = state.multi.roots[0].id.clone();
    let path = LogScope::Path(PathBuf::from("tracked.txt"));
    let by_ref = LogScope::Ref("main".to_string());
    // Only the first commit ever added `token0`, so this search is a listing
    // of its own — one row, and nothing like the other three windows.
    let search = LogScope::Search("token0".to_string());

    state.fetch_log(root.clone());
    state.drain_events();
    state.ensure_log(&root, path.clone());
    state.load_more_scoped_log(&root, &path);
    state.drain_events();
    state.ensure_log(&root, by_ref.clone());
    state.ensure_log(&root, search.clone());

    assert_eq!(
        cached_ids(&state, &root).len(),
        LOG_BATCH_SIZE,
        "the unscoped window"
    );
    assert_eq!(
        scoped_ids(&state, &root, &path).len(),
        2 * LOG_BATCH_SIZE,
        "the path scope paged once and holds two batches of that file's history"
    );
    assert_eq!(
        scoped_ids(&state, &root, &by_ref).len(),
        LOG_BATCH_SIZE,
        "the ref scope is its own window, not the path scope's"
    );
    let searched = scoped_ids(&state, &root, &search);
    assert_eq!(
        searched,
        scoped_engine_ids(
            repo,
            LogOpts {
                pickaxe: Some("token0".to_string()),
                ..Default::default()
            }
        ),
        "the search scope holds only what its own term matches"
    );
    assert!(
        !searched.contains(&scoped_ids(&state, &root, &path)[0]),
        "and shares no row with the path scope's window"
    );
    assert!(
        state.caches.scoped_log_has_more(&root, &path)
            && state.caches.scoped_log_has_more(&root, &by_ref)
            && !state.caches.scoped_log_has_more(&root, &search),
        "three flags, each its own"
    );

    // One refresh of this root drops the unscoped window and all three scopes
    // with it — no cache outlives the invalidation that covers it.
    state.refresh(Affected::Root(root.clone()));
    assert!(state.caches.log(&root).is_none(), "the unscoped window");
    for (scope, which) in [
        (&path, "the path scope"),
        (&by_ref, "the ref scope"),
        (&search, "the search scope"),
    ] {
        assert!(
            state.caches.scoped_log(&root, scope).is_none(),
            "{which} goes with it"
        );
        assert!(
            !state.caches.scoped_log_has_more(&root, scope),
            "{which} takes its paging flag with it"
        );
    }
    assert!(state.caches.is_empty(), "and nothing is left behind");
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

/// Put a whole batch into `root`'s window through the cache's own writer,
/// newest first, ids `row0` (newest) … `row{n-1}` (oldest), and state whether
/// history continues past it.
fn prime_window(state: &mut AppState, root: &RootId, n: usize, has_more: bool) {
    let commits: Vec<Commit> = (0..n).map(|i| row(root, &format!("row{i}"))).collect();
    state.caches.store_log(root.clone(), commits);
    state.caches.set_log_has_more(root, has_more);
}

/// A cold root gets one batch from the front of the listing: no position, no
/// anchor to checksum against.
#[test]
fn a_cold_root_is_asked_for_one_page_from_the_front() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let repo = seeded_repo(&project, 3);
    let (mut state, recorder) = recording_state(&project, &repo);
    let root_id = state.multi.roots[0].id.clone();

    state.fetch_log(root_id.clone());
    state.drain_events();
    assert!(
        recorder.log_call_count() >= 1,
        "the inline pump already made the call; saw {}",
        recorder.log_call_count()
    );

    assert_eq!(
        recorder.log_opts(),
        vec![LogOpts {
            max_count: Some(LOG_BATCH_SIZE),
            ..Default::default()
        }],
        "the first request is a batch of the listing, not a widened prefix"
    );
}

/// A root that already holds a batch is asked for the batch AFTER it — and for
/// one row more than a batch, so the row it already holds comes back as the
/// boundary checksum (P3). `skip` is the position; the walk still starts at
/// HEAD.
#[test]
fn a_warm_root_is_asked_for_the_next_batch_with_one_row_of_overlap() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let repo = seeded_repo(&project, 3);
    let (mut state, recorder) = recording_state(&project, &repo);
    let root_id = state.multi.roots[0].id.clone();
    prime_window(&mut state, &root_id, LOG_BATCH_SIZE, true);

    state.fetch_log(root_id.clone());
    // The harness runs the fetch inline, so the request the test is about is
    // already recorded. Settling it is a later concern — and settling this one
    // would add a second request, because the primed rows are synthetic and the
    // answer tears the window (the restart test below covers that).
    assert_eq!(
        recorder.log_opts(),
        vec![LogOpts {
            max_count: Some(LOG_BATCH_SIZE + 1),
            skip: Some(LOG_BATCH_SIZE - 1),
            ..Default::default()
        }],
        "one batch past a held window, overlapping it by exactly its oldest row"
    );
}

/// Load more is per root: only a window that claims more gets a next batch, and
/// a window that does not is left alone.
#[test]
fn load_more_batches_only_the_roots_that_still_have_more() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let alpha = seeded_repo(&project, 3);
    let beta = seeded_repo(&project.join("second"), 3);
    let (mut state, recorder) = recording_state_over(&project, &[alpha.clone(), beta.clone()]);
    let alpha_id = state.multi.roots[0].id.clone();
    let beta_id = state.multi.roots[1].id.clone();
    prime_window(&mut state, &alpha_id, LOG_BATCH_SIZE, true);
    prime_window(&mut state, &beta_id, 3, false);

    state.load_more_log();
    let requests = recorder.log_opts();
    assert_eq!(
        requests.len(),
        1,
        "one fetch per root that has more — beta's short window is the end"
    );
    assert_eq!(requests[0].skip, Some(LOG_BATCH_SIZE - 1));

    // With nothing left to load, load more is silent. The explicit fetch below
    // proves the counter is live rather than merely slow.
    state.caches.set_log_has_more(&alpha_id, false);
    state.load_more_log();
    assert_eq!(
        recorder.log_opts().len(),
        1,
        "a window with nothing left to load must issue no fetch"
    );
    state.fetch_log(beta_id.clone());
    assert_eq!(
        recorder.log_opts().len(),
        2,
        "an explicit ask still goes out, and the counter is live"
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

#[test]
fn two_fetches_for_the_same_root_in_one_frame_ask_the_engine_once() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let repo = seeded_repo(&project, 3);
    let (mut state, recorder) = recording_state(&project, &repo);
    let root_id = state.multi.roots[0].id.clone();

    // The tool-window body asks for the log every frame while the cache is
    // cold; the second call lands before the first event drains.
    state.fetch_log(root_id.clone());
    state.fetch_log(root_id.clone());
    state.drain_events();
    assert!(
        state.caches.log(&root_id).is_some(),
        "the inline pump settles this in one drain"
    );
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
    state.drain_events();
    assert!(
        state.caches.log(&root_id).is_some(),
        "the inline pump settles this in one drain"
    );
    state.drain_events();
    state.fetch_log(root_id.clone());
    state.drain_events();
    assert!(
        recorder.log_call_count() >= 2,
        "the inline pump already made the call; saw {}",
        recorder.log_call_count()
    );
    assert!(
        recorder.log_call_count() >= 2,
        "a later fetch must run again after the Ok drain"
    );

    // Err path: a failing log-load event frees the guard too — a later fetch
    // must run again, the failure surfaces through last_error, and the window
    // the previous batch filled is left alone.
    let held = cached_ids(&state, &root_id);
    state
        .tx
        .send(AppEvent::LogLoaded {
            root: root_id.clone(),
            commits: Err(TgError::Other("offline".to_string())),
            mode: LogBatchMode::Replace,
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
        "a failing batch must not touch the window"
    );
    state.fetch_log(root_id.clone());
    state.drain_events();
    assert!(
        recorder.log_call_count() >= 3,
        "the inline pump already made the call; saw {}",
        recorder.log_call_count()
    );
    assert!(
        recorder.log_call_count() >= 3,
        "a later fetch must run again after the Err drain"
    );
}
