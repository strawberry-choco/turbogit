//! Issue 15 — Log commit actions, app seam: revert through the confirmation
//! gate (`PendingConfirm::RevertCommit` → `run_confirmed`) and cherry-pick
//! onto a chosen branch (`AppState::cherry_pick_to`), both dispatched through
//! the production `run_git` worker path with events pumped back in.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use turbogit_app::state::{AppState, PendingConfirm};

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

/// A project root with one repo checked out on `work` (non-protected) with
/// two commits, plus a `feature` branch off the first commit carrying one
/// extra commit. Returns (project, repo, work c2, feature commit).
fn seeded_project() -> (tempfile::TempDir, PathBuf, String, String) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let repo = project.join("alpha");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    let c1 = commit_file(&repo, "a.txt", "one\n", "c1");
    let c2 = commit_file(&repo, "b.txt", "two\n", "c2");
    git(&repo, &["checkout", "-q", "-b", "work"]);
    commit_file(&repo, "w.txt", "work\n", "work commit");
    git(&repo, &["checkout", "-q", "-b", "feature", &c1]);
    let fc = commit_file(&repo, "f.txt", "feature\n", "feature work");
    git(&repo, &["checkout", "-q", "work"]);
    (tmp, project, c2, fc)
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
        "condition not met within 10s; toast={:?} last_error={:?}",
        state.ui.toast, state.last_error
    );
}

#[test]
fn revert_through_the_confirm_gate_creates_and_commits_the_revert() {
    let (_tmp, project, c2, _fc) = seeded_project();
    let mut state = AppState::for_roots(&project, &[project.join("alpha")]);
    let root_id = state.multi.roots[0].id.clone();

    state.run_confirmed(PendingConfirm::RevertCommit { commit: c2.clone() });
    wait_for(&mut state, |s| {
        s.caches.log(&root_id).is_some_and(|cs| {
            cs.iter()
                .any(|c| c.message.to_lowercase().starts_with("revert"))
        })
    });

    let subject = git(&project.join("alpha"), &["log", "-1", "--format=%s"]);
    assert!(
        subject.to_lowercase().starts_with("revert"),
        "HEAD must be the revert commit; got {subject:?}"
    );
    assert!(
        !project.join("alpha").join("b.txt").exists(),
        "the reverted change must be undone in the tree"
    );
    // The success toast names the op (demo: feedback for the revert).
    assert!(
        state.ui.toast.is_some(),
        "a completed revert surfaces feedback through the toast"
    );
}

#[test]
fn cherry_pick_to_applies_the_commit_onto_the_target_branch() {
    let (_tmp, project, _c2, fc) = seeded_project();
    let repo = project.join("alpha");
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&repo));
    // The default patterns protect main/master; this test exercises the
    // happy path (protection itself is pinned at the service seam).
    state.settings.protected_branch_patterns.clear();
    let root_id = state.multi.roots[0].id.clone();

    state.cherry_pick_to(fc, "main".to_string());
    wait_for(&mut state, |s| {
        s.ui.toast
            .as_ref()
            .is_some_and(|t| t.kind == turbogit_app::state::ToastKind::Success)
    });

    // The commit landed on main exactly once…
    let count = git(&repo, &["rev-list", "--count", "main"])
        .trim()
        .parse::<usize>()
        .unwrap();
    assert_eq!(count, 3, "main must gain exactly one cherry-picked commit");
    // …and the original checkout was restored (the root snapshot refreshed).
    let snapshot = state.multi.by_id(&root_id).unwrap();
    assert_eq!(
        snapshot.current_branch.as_deref(),
        Some("work"),
        "cherry-picking must return the repo to the branch it was on"
    );
}
