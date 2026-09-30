//! Issue 18 — Blame view, app seam: a blame target opened through the public
//! state (`ui.blame`) fetches per-line attribution through the production
//! worker path (`read` → `BlameReady` → `drain_events`), and a completed
//! operation's refresh (`refresh(All)`) drops the cached lines so the surface
//! refetches on its next read.
//!
//! The verdict is what is asserted, not the cache field: the key is the read's
//! business, and a test that spelled it out would only pin the coupling this
//! branch exists to remove.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use turbogit_app::keyed_read::Read;
use turbogit_app::root_caches::Affected;
use turbogit_app::state::{AppState, BlameTarget};
use turbogit_domain::model::BlameLine;

/// A `git` runner pinning the commit identity, not `git_seed::git`, which takes
/// no per-call env. It covers the `rev-parse` and `add` calls too, and is what
/// keeps a blame line's author the same on every machine.
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

fn target(state: &AppState, repo: &Path, rev: &str) -> BlameTarget {
    BlameTarget {
        root: state
            .multi
            .roots
            .iter()
            .find(|r| r.path == repo)
            .map(|r| r.id.clone())
            .unwrap_or_else(|| panic!("{} registered", repo.display())),
        path: PathBuf::from("a.txt"),
        rev: rev.to_owned(),
    }
}

/// Ask the read for a target's lines, settling whatever it dispatches. Answers
/// the blamed lines, or panics with the verdict it gave instead.
fn blamed(state: &mut AppState, target: BlameTarget) -> Arc<[BlameLine]> {
    if matches!(state.read(target.clone()), Read::Waiting) {
        state.drain_events();
    }
    match state.read(target) {
        Read::Fresh(lines) => lines,
        Read::Waiting => panic!(
            "the blame answer settles; last_error={:?}",
            state.last_error
        ),
        Read::Empty => panic!("blame of a two-line file is not an empty answer"),
        Read::Failed(message) => panic!("the blame fetch failed: {message}"),
    }
}

#[test]
fn blame_target_fetches_per_line_attribution_through_the_event_pump() {
    let (tmp, repo, c1, c2) = seeded_repo();
    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&repo));

    // The transition every entry point (footer link, context menu) makes:
    // record the blame target, then let the surface read its data.
    let open = target(&state, &repo, &c2);
    state.ui.blame = Some(open.clone());
    let lines = blamed(&mut state, open);

    assert_eq!(lines.len(), 2, "both lines of a.txt are blamed");
    assert_eq!(lines[0].commit, c1, "line 1 was introduced by c1");
    assert_eq!(lines[1].commit, c2, "line 2 was introduced by c2");
    assert_eq!(lines[1].author, "t");
    assert_eq!(lines[1].content, "two");
    assert_eq!(lines[1].line_no, 2);
}

#[test]
fn refresh_after_an_operation_drops_the_blame_lines_for_refetch() {
    let (tmp, repo, _c1, c2) = seeded_repo();
    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&repo));
    let open = target(&state, &repo, &c2);
    state.ui.blame = Some(open.clone());
    assert_eq!(blamed(&mut state, open.clone()).len(), 2);

    // A completed operation refreshes the affected roots; the cached lines
    // must be dropped with it (the diff cache's wholesale rule) so the next
    // read refetches — and does so successfully.
    state.refresh(Affected::All);
    assert!(
        matches!(state.read(open.clone()), Read::Waiting),
        "a refresh must leave the blame target needing a refetch"
    );
    assert_eq!(blamed(&mut state, open).len(), 2, "refetch repopulates");
}

#[test]
fn blaming_an_earlier_revision_is_its_own_answer() {
    let (tmp, repo, c1, c2) = seeded_repo();
    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&repo));

    let at_c2 = target(&state, &repo, &c2);
    assert_eq!(blamed(&mut state, at_c2.clone()).len(), 2);

    // The same file at the revision before its second line existed: a
    // different target, so a different answer — never the cached one, and the
    // read says so with `Waiting` rather than painting what it has.
    let at_c1 = target(&state, &repo, &c1);
    assert!(
        matches!(state.read(at_c1.clone()), Read::Waiting),
        "c2's lines must not answer a blame of c1"
    );
    let lines = blamed(&mut state, at_c1);
    assert_eq!(
        lines.len(),
        1,
        "blamed at c1, the file has only the line c1 introduced"
    );
    assert_eq!(lines[0].commit, c1);

    // Blame stores one entry, so switching back refetches — which is the
    // single-entry policy, and the one the keyed pane-bytes map deliberately
    // does not share.
    assert!(matches!(state.read(at_c2), Read::Waiting));
}
