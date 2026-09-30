//! Checking out a remote branch leaves the new local branch tracking it.
//!
//! Written to license deleting the app's explicit
//! `branch --set-upstream-to` after `checkout -b <name> origin/<name>`, on the
//! theory that git's own DWIM already sets the upstream. That theory holds only
//! for the CLI adapter: the default `Auto` backend answers `branch_create` from
//! libgit2, which creates the branch and sets HEAD without ever recording a
//! tracking ref. The test failed there, so the write stayed and this is what
//! pins the outcome now.

use std::path::{Path, PathBuf};
use std::process::Command;

use turbogit_app::state::AppState;
use turbogit_domain::model::BranchKind;

/// A `git` runner that pins the commit identity on every invocation, and deliberately
/// NOT `test_support::git_seed::git`, which takes no per-call env. This fixture's
/// commits carry no repo-local `user.*` of their own, so without the pin they would
/// commit as whatever the machine's global git config names.
fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
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

/// One repo with a same-content `origin`, plus `feat` pushed there and deleted
/// locally — the state a remote branch row is clicked from.
fn repo_with_remote_branch() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let bare = project.join("origin.git");
    git(
        &project,
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    let work = project.join("work");
    git(
        &project,
        &["init", "-q", "-b", "main", work.to_str().unwrap()],
    );
    std::fs::write(work.join("README.md"), "x\n").unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "-q", "-m", "init"]);
    git(&work, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(&work, &["push", "-q", "-u", "origin", "main"]);
    git(&work, &["branch", "feat", "HEAD"]);
    git(&work, &["push", "-q", "origin", "feat"]);
    git(&work, &["branch", "-D", "feat"]);
    (tmp, work)
}

#[test]
fn checking_out_a_remote_branch_tracks_it() {
    let (_tmp, work) = repo_with_remote_branch();
    let mut state = AppState::for_roots(work.parent().unwrap(), std::slice::from_ref(&work));
    let root = state.selected_root.clone().unwrap();

    state.checkout_branch_op(&root, BranchKind::Remote, "feat");
    state.drain_events();

    let feat = state
        .multi
        .by_id(&root)
        .and_then(|r| {
            r.branches
                .iter()
                .find(|b| b.name == "feat" && b.kind == BranchKind::Local)
        })
        .expect("the checkout created a local `feat`");
    assert_eq!(
        feat.tracking,
        turbogit_domain::model::Upstream::from_git_ref("origin/feat"),
        "checking out a remote branch must leave the local branch tracking it"
    );
    assert_eq!(
        git_stdout(&work, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "feat",
        "the checkout is what moved HEAD"
    );
}

/// `git` for the one call whose OUTPUT is the assertion: it trims stdout and makes no
/// success check of its own, so it is deliberately not `test_support::git_seed::git`.
fn git_stdout(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git must be on PATH");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}
