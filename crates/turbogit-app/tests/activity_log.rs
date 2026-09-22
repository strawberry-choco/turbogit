//! Issue #04 — Activity log: every dispatched git operation and its outcome
//! appends an entry to the session-durable feed.
//!
//! Headless over a real temp repository (git on PATH): operations are
//! dispatched through the production [`AppState::dispatch`] path and events are
//! pumped through `drain_events`. Assertions are on the public
//! `state.ui.activity` surface — never on internals.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;
use turbogit_app::activity::ActivityKind;
use turbogit_app::operation::Operation;
use turbogit_app::root_caches::Affected;
use turbogit_app::state::AppState;

// --- git fixture ---------------------------------------------------------------

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_AUTHOR_COMMITTER_NAME", "t")
        .env("GIT_AUTHOR_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("git must be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn commit_readme(repo: &Path) {
    std::fs::write(repo.join("README.md"), "x\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-m", "init"]);
}

/// One bare local remote (`origin`) plus one local repo `alpha` on `main`
/// with `origin` configured, so a fetch is a real, valid operation.
fn repo_project() -> (TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let remote = project.join("origin.git");
    git(
        &project,
        &["init", "--bare", "-b", "main", remote.to_str().unwrap()],
    );
    git(
        &project,
        &["-c", "init.defaultBranch=main", "init", "alpha"],
    );
    let alpha = project.join("alpha");
    commit_readme(&alpha);
    git(
        &alpha,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&alpha, &["push", "-u", "origin", "main"]);
    (tmp, project, alpha)
}

// --- tests ---------------------------------------------------------------------

/// Contract: a dispatched git operation that completes successfully appends
/// exactly one activity entry naming the repo and the human-readable outcome.
#[test]
fn successful_op_appends_one_activity_entry() {
    let (_tmp, project, alpha) = repo_project();
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    let root = state.multi.roots[0].id.clone();
    let alpha_path = alpha.clone();
    state.dispatch(Operation::custom(
        "Fetch from origin",
        Affected::Root(root),
        move |v| v.fetch(&alpha_path, Some("origin")),
    ));

    // The harness runs the operation inline, so one drain settles it and the
    // activity entry is already there — no thread, no deadline.
    state.drain_events();
    assert!(
        !state.ui.activity.entries.is_empty(),
        "the op produced an activity entry; last_error={:?}",
        state.last_error
    );

    assert_eq!(
        state.ui.activity.entries.len(),
        1,
        "one op, one entry: {:?}",
        state.ui.activity.entries
    );
    let e = &state.ui.activity.entries[0];
    assert_eq!(e.repo.as_deref(), Some("alpha"));
    assert_eq!(e.kind, ActivityKind::Success);
    assert_eq!(e.message, "Fetch from origin");
}

/// Contract: a dispatched git operation that fails appends an Error entry
/// whose message carries the op label and the git error.
#[test]
fn failed_op_appends_error_entry() {
    let (_tmp, project, alpha) = repo_project();
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    let root = state.multi.roots[0].id.clone();
    let missing = PathBuf::from("/nonexistent/root/never/existed");
    state.dispatch(Operation::custom(
        "Fetch from origin",
        Affected::Root(root),
        move |v| v.fetch(&missing, Some("origin")),
    ));

    // The harness runs the operation inline, so one drain settles it and the
    // activity entry is already there — no thread, no deadline.
    state.drain_events();
    assert!(
        !state.ui.activity.entries.is_empty(),
        "the op produced an activity entry; last_error={:?}",
        state.last_error
    );

    let e = &state.ui.activity.entries[0];
    assert_eq!(e.kind, ActivityKind::Error);
    assert_eq!(e.repo.as_deref(), Some("alpha"));
    assert!(
        e.message.starts_with("Fetch from origin:"),
        "error entry must name the op and the failure: {:?}",
        e.message
    );
}

/// `git` that tolerates failure — used to plant a merge conflict, where the
/// non-zero exit is the point.
fn git_ok_or_fail(dir: &Path, args: &[&str]) {
    let _ = Command::new("git").args(args).current_dir(dir).output();
}

/// Put `alpha` into an in-progress merge with one conflicted file, as if a
/// merge just stopped mid-resolution.
fn plant_merge_conflict(alpha: &Path) {
    git(alpha, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(alpha.join("README.md"), "feature side\n").unwrap();
    git(alpha, &["commit", "-qam", "feature change"]);
    git(alpha, &["checkout", "-q", "main"]);
    std::fs::write(alpha.join("README.md"), "main side\n").unwrap();
    git(alpha, &["commit", "-qam", "main change"]);
    git_ok_or_fail(alpha, &["merge", "feature", "--no-edit"]);
    let status = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(alpha)
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&status.stdout).to_string();
    assert!(
        out.contains("UU"),
        "fixture must leave a conflicted file, got:\n{out}"
    );
}

/// Contract: an op that completes but leaves conflicts behind logs a
/// Warning entry — success coloring would hide the unresolved state.
#[test]
fn op_leaving_conflicts_logs_warning_entry() {
    let (_tmp, project, alpha) = repo_project();
    plant_merge_conflict(&alpha);
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    let root = state.multi.roots[0].id.clone();
    let alpha_path = alpha.clone();
    state.dispatch(Operation::custom(
        "Fetch from origin",
        Affected::Root(root),
        move |v| v.fetch(&alpha_path, Some("origin")),
    ));

    // The harness runs the operation inline, so one drain settles it and the
    // activity entry is already there — no thread, no deadline.
    state.drain_events();
    assert!(
        !state.ui.activity.entries.is_empty(),
        "the op produced an activity entry; last_error={:?}",
        state.last_error
    );

    let e = &state.ui.activity.entries[0];
    assert_eq!(e.kind, ActivityKind::Warning, "entry: {:?}", e);
    assert_eq!(
        e.message, "Fetch from origin · 1 unresolved conflicts",
        "entry: {:?}",
        e
    );
}

// --- pure feed semantics (repo / time filters, clear) ---------------------------

use chrono::{Duration as ChronoDuration, Local};
use turbogit_app::activity::{ActivityEntry, ActivityLog, TimeWindow};

fn entry(minutes_ago: i64, repo: &str, message: &str) -> ActivityEntry {
    ActivityEntry {
        at: Local::now() - ChronoDuration::minutes(minutes_ago),
        repo: Some(repo.to_string()),
        message: message.to_string(),
        kind: ActivityKind::Success,
    }
}

/// Contract: the repo filter narrows the visible feed to exactly that
/// repo's entries; `None` shows everything.
#[test]
fn repo_filter_narrows_visible_entries() {
    let mut log = ActivityLog::default();
    log.push(entry(0, "alpha", "Fetch from origin"));
    log.push(entry(0, "beta", "Pull"));

    let all = log.visible(Local::now());
    assert_eq!(all.len(), 2, "no filter → every entry");

    log.repo_filter = Some("alpha".to_string());
    let only_alpha = log.visible(Local::now());
    assert_eq!(only_alpha.len(), 1);
    assert_eq!(only_alpha[0].repo.as_deref(), Some("alpha"));
}

/// Contract: the 30-minute window hides older entries but keeps fresh ones;
/// `All` keeps everything regardless of age.
#[test]
fn time_window_excludes_old_entries() {
    let mut log = ActivityLog::default();
    log.push(entry(0, "alpha", "fresh"));
    log.push(entry(40, "alpha", "stale"));

    log.window = TimeWindow::Last30Min;
    let recent = log.visible(Local::now());
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].message, "fresh");

    log.window = TimeWindow::All;
    assert_eq!(log.visible(Local::now()).len(), 2);
}

/// Contract: `clear` empties the feed for good.
#[test]
fn clear_empties_the_feed() {
    let mut log = ActivityLog::default();
    log.push(entry(0, "alpha", "Fetch from origin"));
    log.clear();
    assert!(log.entries.is_empty());
    assert!(log.visible(Local::now()).is_empty());
}
