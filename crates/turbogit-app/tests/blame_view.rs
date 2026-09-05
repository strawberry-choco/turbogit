//! Issue 18 — Blame view, app seam: a blame target opened through the public
//! state (`ui.blame`) fetches per-line attribution through the production
//! worker path (`ensure_blame` → `BlameReady` → `drain_events`), and a
//! completed operation's refresh (`refresh(All)`) drops the cache so the
//! surface refetches on its next ensure.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use turbogit_app::root_caches::Affected;
use turbogit_app::state::{AppState, BlameTarget};
use turbogit_domain::model::BlameLine;

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

fn commit_file(dir: &Path, name: &str, body: &str, msg: &str) -> String {
    std::fs::write(dir.join(name), body).unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", msg]);
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

/// One repo, two commits on `main`, both touching `a.txt` so its blame
/// attributes line 1 to the first commit and line 2 to the second.
fn seeded_repo() -> (tempfile::TempDir, PathBuf, String, String) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("alpha");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    let c1 = commit_file(&repo, "a.txt", "one\n", "c1");
    let c2 = commit_file(&repo, "a.txt", "one\ntwo\n", "c2");
    (tmp, repo, c1, c2)
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
    panic!(
        "condition not met within 10s; blame_error={:?} last_error={:?}",
        state.ui.blame_error, state.last_error
    );
}

fn cached_lines(state: &AppState) -> &[BlameLine] {
    state
        .ui
        .blame_cache
        .as_ref()
        .map(|(_, lines)| lines.as_slice())
        .expect("blame cache must be populated")
}

#[test]
fn blame_target_fetches_per_line_attribution_through_the_event_pump() {
    let (tmp, repo, c1, c2) = seeded_repo();
    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&repo));

    // The transition every entry point (footer link, context menu) makes:
    // record the blame target, then let the surface ensure its data.
    state.ui.blame = Some(BlameTarget {
        root: state.selected_root.clone().unwrap(),
        path: PathBuf::from("a.txt"),
        rev: c2.clone(),
    });
    state.ensure_blame();
    wait_for(&mut state, |s| s.ui.blame_cache.is_some());

    let lines = cached_lines(&state);
    assert_eq!(lines.len(), 2, "both lines of a.txt are blamed");
    assert_eq!(lines[0].commit, c1, "line 1 was introduced by c1");
    assert_eq!(lines[1].commit, c2, "line 2 was introduced by c2");
    assert_eq!(lines[1].author, "t");
    assert_eq!(lines[1].content, "two");
    assert_eq!(lines[1].line_no, 2);
}

#[test]
fn refresh_after_an_operation_drops_the_blame_cache_for_refetch() {
    let (tmp, repo, _c1, c2) = seeded_repo();
    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&repo));
    state.ui.blame = Some(BlameTarget {
        root: state.selected_root.clone().unwrap(),
        path: PathBuf::from("a.txt"),
        rev: c2.clone(),
    });
    state.ensure_blame();
    wait_for(&mut state, |s| s.ui.blame_cache.is_some());

    // A completed operation refreshes the affected roots; the blame cache
    // must be dropped with it (the diff cache's wholesale rule) so the next
    // ensure refetches — and does so successfully.
    state.refresh(Affected::All);
    assert!(
        state.ui.blame_cache.is_none(),
        "refresh must drop the blame cache"
    );
    state.ensure_blame();
    wait_for(&mut state, |s| s.ui.blame_cache.is_some());
    assert_eq!(cached_lines(&state).len(), 2, "refetch repopulates blame");
}
