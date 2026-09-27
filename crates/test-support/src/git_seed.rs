//! Seeded git repositories, for the suites that need real history to rewrite.
//!
//! The recipes live here rather than in one suite so a second suite that needs
//! the same history gets the same one: a targeted history rewrite's test
//! measures the *operation*, and a fixture that differed by one commit's path
//! would quietly measure git's merge machinery instead.
//!
//! Every helper takes the project directory to build in and returns the
//! repository's path, so the caller owns the `TempDir` and its lifetime.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Run `git <args>` in `dir`, asserting success; returns stdout.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git must be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} in {} failed: {}",
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn commit(repo: &Path, path: &str, body: &str, message: &str) {
    std::fs::write(repo.join(path), body).unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", message]);
}

/// One bare `origin` plus one local repo `name` on `main` with a single commit,
/// pushed to it — so `main` is its own upstream and ahead/behind is a known
/// zero. Returns the working repository.
pub fn repo_with_origin(project: &Path, name: &str) -> PathBuf {
    let bare = project.join(format!("{name}.git"));
    git(
        project,
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    let work = project.join(name);
    git(
        project,
        &["init", "-q", "-b", "main", work.to_str().unwrap()],
    );
    commit(&work, "README.md", "x\n", "init");
    git(&work, &["config", "user.email", "test@example.com"]);
    git(&work, &["config", "user.name", "Test"]);
    git(&work, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(&work, &["push", "-q", "-u", "origin", "main"]);
    work
}

/// A repository named `name` with a `feature` branch three commits deep and
/// checked out — history a targeted rewrite may act on without tripping the
/// protected-branch guard, and without tripping git's conflict machinery
/// either: each commit touches its OWN path, so replaying a descendant over a
/// dropped neighbour cannot conflict. A test that needs a conflict must say so
/// in its own fixture rather than inherit one here.
pub fn repo_with_history(project: &Path, name: &str) -> PathBuf {
    let work = repo_with_origin(project, name);
    git(&work, &["checkout", "-q", "-b", "feature"]);
    for n in 1..=3 {
        commit(
            &work,
            &format!("feature-{n}.txt"),
            &format!("{n}\n"),
            &format!("feature-{n}"),
        );
    }
    work
}
