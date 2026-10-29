use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use test_support::RecordingExecutor;
use test_support::git_seed::{git, repo_with_one_commit, repo_with_origin};
use turbogit_app::operation::Operation;
use turbogit_app::root_caches::Affected;
use turbogit_app::state::AppState;
use turbogit_domain::model::{IncomingCheckInterval, RootId, VcsSettings};
use turbogit_engine_api::GitExecutor;

fn repo_with_upstream(parent: &Path, name: &str) -> (PathBuf, PathBuf) {
    let work = repo_with_origin(parent, name);
    let remote = parent.join(format!("{name}.git"));
    (work, remote)
}

fn repo_without_upstream(parent: &Path, name: &str) -> PathBuf {
    repo_with_one_commit(parent, name)
}

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

fn tick_and_settle(state: &mut AppState, at: Instant) {
    state.tick_incoming_poll(at);
    state.drain_events();
}

fn poll_settings() -> VcsSettings {
    VcsSettings {
        incoming_poll: true,
        incoming_interval: IncomingCheckInterval::Min15,
        ..VcsSettings::default()
    }
}

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

    tick_and_settle(&mut state, Instant::now());

    assert_eq!(rec.fetches().len(), 1, "exactly one fetch dispatched");
    assert_eq!(
        state.caches.ahead_behind(&root),
        Some((0, 1)),
        "the incoming commit must show up without a manual refresh"
    );

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

    tick_and_settle(&mut state, Instant::now());
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

    tick_and_settle(&mut state, t0);
    assert_eq!(rec.fetches().len(), 1);

    commit_on_remote(project.path(), &bare, "alpha", "incoming2.txt");
    tick_and_settle(&mut state, t0 + Duration::from_secs(60));
    assert_eq!(rec.fetches().len(), 1, "an early tick must not re-poll");
    assert_eq!(
        state.caches.ahead_behind(&root),
        Some((0, 1)),
        "the cache keeps the last poll's finding"
    );

    tick_and_settle(&mut state, t0 + Duration::from_secs(15 * 60));
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

    state.dispatch(Operation::custom("No-op", Affected::All, |_| Ok(())));
    state.tick_incoming_poll(Instant::now());
    state.drain_events();
    assert!(
        rec.fetches().is_empty(),
        "a busy worker pool must defer the poll"
    );

    while state.ui.busy {
        state.drain_events();
    }
    tick_and_settle(&mut state, Instant::now());
    assert_eq!(rec.fetches().len(), 1);
    assert_eq!(state.caches.ahead_behind(&root), Some((0, 1)));
}

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
    )
    .with_executor(rec.clone() as Arc<dyn GitExecutor>);
    state.settings = poll_settings();
    let root = RootId(repo.into());
    let t0 = Instant::now();

    assert!(
        wait_until(5_000, || {
            while state.drain_events() > 0 {}
            state.caches.ahead_behind(&root).is_some()
        }),
        "the launch scan must record its ahead/behind reading"
    );
    assert_eq!(state.caches.ahead_behind(&root), Some((0, 0)));

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
