//! Issue 17 — Log UX upgrades, app seam: the commit list paginates. The
//! fetch is page-sized (`(ui.log_page + 1) * LOG_PAGE_SIZE` commits) and
//! [`AppState::load_more_log`] widens the fetch through the production
//! worker path, replacing the previously uncapped log load.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use turbogit_app::state::{AppState, LOG_PAGE_SIZE};

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
