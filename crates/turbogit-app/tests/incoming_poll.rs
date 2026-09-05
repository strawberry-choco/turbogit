//! Issue #27 — background incoming-check polling.
//!
//! Everything goes through the public surface over the headless harness
//! (`AppState::for_roots`, CONTEXT.md "Headless harness"): the scheduler is
//! driven by [`AppState::tick_incoming_poll`] with an injected `Instant` so
//! the clock is fully deterministic, repos with real upstreams are built
//! with the system `git`, and dispatch is asserted at the executor boundary
//! through [`RecordingExecutor`]'s fetch recording.
//!
//! Covered:
//! - an enabled, due poll fetches upstream roots and lands incoming commits
//!   in the ahead/behind caches and the activity log
//! - the setting off means nothing ever dispatches
//! - the interval gates re-polls (not due → no dispatch)
//! - roots without an upstream are never fetched
//! - an in-flight operation (`ui.busy`) parks the poll until the worker
//!   pool is free

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use test_support::RecordingExecutor;
use turbogit_app::root_caches::Affected;
use turbogit_app::state::AppState;
use turbogit_domain::model::{RootId, VcsSettings};
use turbogit_engine_api::GitExecutor;

// ---------------------------------------------------------------- helpers --

/// Run `git` in `dir`, asserting success.
fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git should be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A temp repo on `main` tracking its own local bare remote
/// (`<parent>/<name>-remote.git`), plus the bare's path.
fn repo_with_upstream(parent: &Path, name: &str) -> (PathBuf, PathBuf) {
    let path = parent.join(name);
    let remote = parent.join(format!("{name}-remote.git"));
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q", "-b", "main"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("base.txt"), "base\n").unwrap();
    git(&path, &["add", "."]);
    git(&path, &["commit", "-q", "-m", "base"]);
    git(
        &path,
        &["init", "--bare", "-b", "main", remote.to_str().unwrap()],
    );
    git(
        &path,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&path, &["push", "-q", "-u", "origin", "main"]);
    (path, remote)
}

/// Push an extra commit to the bare remote via a throwaway clone, so the
/// original repo has an incoming commit it cannot see until it fetches.
fn commit_on_remote(parent: &Path, bare: &Path, name: &str, file: &str) {
    let other = parent.join(format!("{name}-other"));
    if !other.exists() {
        git(
            parent,
            &[
                "clone",
                "-q",
                bare.to_str().unwrap(),
                other.to_str().unwrap(),
            ],
        );
        git(&other, &["config", "user.email", "test@example.com"]);
        git(&other, &["config", "user.name", "Test"]);
    }
    std::fs::write(other.join(file), "incoming\n").unwrap();
    git(&other, &["add", "."]);
    git(&other, &["commit", "-q", "-m", "incoming"]);
    git(&other, &["push", "-q", "origin", "main"]);
}

/// A plain temp repo with no remote at all.
fn repo_without_upstream(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q", "-b", "main"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("solo.txt"), "solo\n").unwrap();
    git(&path, &["add", "."]);
    git(&path, &["commit", "-q", "-m", "solo"]);
    path
}

/// `for_roots` with the executor/settings swap AFTER synchronous
/// registration, so setup reads never reach the recorder.
fn app_state_with(
    project_dir: &Path,
    roots: &[PathBuf],
    executor: Arc<dyn GitExecutor>,
    settings: VcsSettings,
) -> AppState {
    AppState::for_roots(project_dir, roots)
        .with_executor(executor)
        .with_settings(settings)
}

/// Settings with the background incoming check enabled at the default
/// interval (every 15 minutes).
fn poll_settings() -> VcsSettings {
    VcsSettings {
        incoming_poll: true,
        ..VcsSettings::default()
    }
}

// ------------------------------------------------------------------- tests --

#[test]
fn due_poll_fetches_upstream_roots_and_records_incoming() {
    let project = tempfile::tempdir().unwrap();
    let (repo, bare) = repo_with_upstream(project.path(), "alpha");
    commit_on_remote(project.path(), &bare, "alpha", "incoming.txt");
    let rec = Arc::new(RecordingExecutor::new(Arc::new(
        turbogit_engine::cli::CliExecutor {
            settings: VcsSettings::default(),
        },
    )));
    let mut state = app_state_with(
        project.path(),
        std::slice::from_ref(&repo),
        rec.clone(),
        poll_settings(),
    );

    let root = RootId(repo.into());
    assert!(
        state.caches.ahead_behind(&root).is_none(),
        "no ahead/behind knowledge before the first poll"
    );

    state.tick_incoming_poll(Instant::now());

    // The poll fetched the root's remotes and its finding landed in the
    // ahead/behind cache: 0 outgoing, 1 incoming.
    assert_eq!(rec.fetches().len(), 1, "exactly one fetch dispatched");
    assert_eq!(
        state.caches.ahead_behind(&root),
        Some((0, 1)),
        "the incoming commit must show up without a manual refresh"
    );

    // The finding surfaced in the activity log too.
    let entries: Vec<_> = state
        .ui
        .activity
        .entries
        .iter()
        .filter(|e| e.message.contains("incoming"))
        .collect();
    assert_eq!(entries.len(), 1, "one activity entry for the finding");
    assert_eq!(entries[0].repo.as_deref(), Some("alpha"));
}

#[test]
fn disabled_poll_never_fetches() {
    let project = tempfile::tempdir().unwrap();
    let (repo, _bare) = repo_with_upstream(project.path(), "alpha");
    let rec = Arc::new(RecordingExecutor::new(Arc::new(
        turbogit_engine::cli::CliExecutor {
            settings: VcsSettings::default(),
        },
    )));
    let mut state = app_state_with(project.path(), &[repo], rec.clone(), VcsSettings::default());

    state.tick_incoming_poll(Instant::now());
    state.tick_incoming_poll(
        Instant::now()
            .checked_add(Duration::from_secs(3600))
            .unwrap(),
    );

    assert!(
        rec.fetches().is_empty(),
        "the setting off must keep the scheduler idle"
    );
}

#[test]
fn poll_respects_the_interval() {
    let project = tempfile::tempdir().unwrap();
    let (repo, bare) = repo_with_upstream(project.path(), "alpha");
    commit_on_remote(project.path(), &bare, "alpha", "incoming1.txt");
    let rec = Arc::new(RecordingExecutor::new(Arc::new(
        turbogit_engine::cli::CliExecutor {
            settings: VcsSettings::default(),
        },
    )));
    let mut state = app_state_with(
        project.path(),
        std::slice::from_ref(&repo),
        rec.clone(),
        poll_settings(),
    );
    let root = RootId(repo.into());
    let t0 = Instant::now();

    state.tick_incoming_poll(t0);
    assert_eq!(rec.fetches().len(), 1);

    // A second incoming commit arrives, but the interval has not elapsed.
    commit_on_remote(project.path(), &bare, "alpha", "incoming2.txt");
    state.tick_incoming_poll(t0 + Duration::from_secs(60));
    assert_eq!(rec.fetches().len(), 1, "an early tick must not re-poll");
    assert_eq!(
        state.caches.ahead_behind(&root),
        Some((0, 1)),
        "the cache keeps the last poll's finding"
    );

    // Once the interval (15 minutes) has elapsed, the poll runs again and
    // both incoming commits show up.
    state.tick_incoming_poll(t0 + Duration::from_secs(15 * 60));
    assert_eq!(rec.fetches().len(), 2);
    assert_eq!(state.caches.ahead_behind(&root), Some((0, 2)));
}

#[test]
fn poll_skips_roots_without_upstream() {
    let project = tempfile::tempdir().unwrap();
    let (repo, _bare) = repo_with_upstream(project.path(), "alpha");
    let solo = repo_without_upstream(project.path(), "solo");
    let rec = Arc::new(RecordingExecutor::new(Arc::new(
        turbogit_engine::cli::CliExecutor {
            settings: VcsSettings::default(),
        },
    )));
    let mut state = app_state_with(project.path(), &[repo, solo], rec.clone(), poll_settings());

    state.tick_incoming_poll(Instant::now());

    let fetched: Vec<_> = rec
        .fetches()
        .into_iter()
        .filter(|(root, _)| root.ends_with("solo"))
        .collect();
    assert!(
        fetched.is_empty(),
        "roots without an upstream must never be fetched"
    );
    assert_eq!(rec.fetches().len(), 1, "only the upstream root polls");
}

#[test]
fn poll_waits_while_an_operation_is_in_flight() {
    let project = tempfile::tempdir().unwrap();
    let (repo, bare) = repo_with_upstream(project.path(), "alpha");
    commit_on_remote(project.path(), &bare, "alpha", "incoming.txt");
    let rec = Arc::new(RecordingExecutor::new(Arc::new(
        turbogit_engine::cli::CliExecutor {
            settings: VcsSettings::default(),
        },
    )));
    let mut state = app_state_with(
        project.path(),
        std::slice::from_ref(&repo),
        rec.clone(),
        poll_settings(),
    );
    let root = RootId(repo.into());

    // An op occupies the worker pool; the due poll must park.
    state.run_git("No-op".into(), Affected::All, |_| Ok(()));
    state.tick_incoming_poll(Instant::now());
    assert!(
        rec.fetches().is_empty(),
        "a busy worker pool must defer the poll"
    );

    // Once the op completes, the parked poll fires on the next tick.
    while state.ui.busy {
        state.drain_events();
    }
    state.tick_incoming_poll(Instant::now());
    assert_eq!(rec.fetches().len(), 1);
    assert_eq!(state.caches.ahead_behind(&root), Some((0, 1)));
}

// ------------------------------------------------- async path (production) --

/// Init `repo` on `main` tracking `bare` (which may live anywhere on disk),
/// so a project scan can discover the repo without picking up the remote.
fn init_repo_with_upstream(repo: &Path, bare: &Path) {
    std::fs::create_dir_all(repo).unwrap();
    git(repo, &["init", "-q", "-b", "main"]);
    git(repo, &["config", "user.email", "test@example.com"]);
    git(repo, &["config", "user.name", "Test"]);
    std::fs::write(repo.join("base.txt"), "base\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "base"]);
    git(
        repo,
        &["init", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    git(repo, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(repo, &["push", "-q", "-u", "origin", "main"]);
}

/// Poll `f` until true or the deadline elapses (production-mode workers run
/// asynchronously, so completion is observed by polling).
fn wait_until<F: FnMut() -> bool>(ms: u64, mut f: F) -> bool {
    let start = Instant::now();
    while !f() {
        if start.elapsed() >= Duration::from_millis(ms) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    true
}

#[test]
fn in_flight_poll_is_not_redispatched_until_it_settles() {
    let project = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let repo = project.path().join("alpha");
    let bare = scratch.path().join("alpha-remote.git");
    init_repo_with_upstream(&repo, &bare);
    commit_on_remote(scratch.path(), &bare, "alpha", "incoming.txt");

    let rec = Arc::new(RecordingExecutor::new(Arc::new(
        turbogit_engine::cli::CliExecutor {
            settings: VcsSettings::default(),
        },
    )));
    let cfg = tempfile::tempdir().unwrap();
    let mut state = AppState::launch_in(
        Some(project.path().to_path_buf()),
        Some(cfg.path().to_path_buf()),
    );
    // The executor/settings swap happens after the launch scan, so setup
    // reads never reach the recorder.
    state.executor = rec.clone() as Arc<dyn GitExecutor>;
    state.settings = poll_settings();
    let root = RootId(repo.into());
    let t0 = Instant::now();

    // Let the launch scan's status/ahead-behind threads finish and drain
    // their events, so they cannot race the poll's settlement below.
    assert!(
        wait_until(5_000, || {
            while state.drain_events() > 0 {}
            state.caches.ahead_behind(&root).is_some()
        }),
        "the launch scan must record its ahead/behind reading"
    );
    assert_eq!(state.caches.ahead_behind(&root), Some((0, 0)));

    // First tick dispatches a background poll; an immediate second tick one
    // interval later must not stack a second fetch on the same repo — the
    // in-flight slot only frees with the poll's settlement (a drain).
    state.tick_incoming_poll(t0);
    state.tick_incoming_poll(t0 + Duration::from_secs(15 * 60));
    assert!(
        wait_until(5_000, || rec.fetches().len() == 1),
        "the first poll must fetch exactly once"
    );
    assert!(
        wait_until(5_000, || {
            while state.drain_events() > 0 {}
            state.caches.ahead_behind(&root) == Some((0, 1))
        }),
        "the settled poll must land its finding in the cache"
    );

    // Settled → the next interval tick re-polls. The skipped tick still
    // consumed its schedule slot, so the re-poll is due one full interval
    // after that tick.
    state.tick_incoming_poll(t0 + Duration::from_secs(31 * 60));
    assert!(
        wait_until(5_000, || rec.fetches().len() == 2),
        "after settlement the root is pollable again"
    );
    assert!(
        wait_until(5_000, || {
            while state.drain_events() > 0 {}
            state.caches.ahead_behind(&root) == Some((0, 1))
        }),
        "the second poll's finding lands too"
    );
}
