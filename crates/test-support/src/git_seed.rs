//! Seeded git repositories, for the suites that need real history to rewrite.
//!
//! The recipes live here so a second suite gets the same history: a targeted history
//! rewrite's test measures the *operation*, and a fixture differing by one commit's
//! path would quietly measure git's merge machinery instead.
//!
//! Every helper takes the project directory to build in and returns the
//! repository path (or paths, for the two-repository shape), so the caller owns
//! the `TempDir` and its lifetime. One named function per shape is the ceiling: a
//! recipe that owned its own `TempDir` could not be composed by a suite that needs
//! two repositories, which is the majority case.

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

/// Write `body` to `<repo>/<path>`, stage everything, commit it as `message`.
///
/// The one commit primitive every fixture in the tree reimplements. It **overwrites**
/// `path` rather than appending, and configures no identity: set `user.email`/
/// `user.name` on the repository first, because a headless machine has no global
/// identity for the first commit to borrow.
pub fn commit(repo: &Path, path: &str, body: &str, message: &str) {
    std::fs::write(repo.join(path), body).unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", message]);
}

/// A repository `name` on `main` carrying exactly one commit and **no remote**.
///
/// The floor fixture: a suite measuring one git operation wants a repository whose
/// only facts are the ones the operation introduces. Distinct from
/// [`repo_with_origin`], which always carries a bare `origin` plus an upstream — a
/// suite asserting "no remote anywhere" cannot reach for that one.
pub fn repo_with_one_commit(project: &Path, name: &str) -> PathBuf {
    let work = project.join(name);
    git(
        project,
        &["init", "-q", "-b", "main", work.to_str().unwrap()],
    );
    git(&work, &["config", "user.email", "test@example.com"]);
    git(&work, &["config", "user.name", "Test"]);
    commit(&work, "README.md", "x\n", "init");
    work
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
    let work = repo_with_one_commit(project, name);
    git(&work, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(&work, &["push", "-q", "-u", "origin", "main"]);
    work
}

/// A repository with a `feature` branch three commits deep and checked out. Each commit
/// touches its OWN path, so replaying a descendant over a dropped neighbour cannot
/// conflict — a test that needs a conflict must say so in its own fixture.
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

/// Two sibling repositories, each the [`repo_with_one_commit`] shape. Both paths come
/// back so a suite can drive a cross-repo operation without either repository
/// owning the other's lifetime.
pub fn two_repos(project: &Path, first: &str, second: &str) -> (PathBuf, PathBuf) {
    (
        repo_with_one_commit(project, first),
        repo_with_one_commit(project, second),
    )
}

/// A repository `name` with a bare `origin`, `main` published to it, and a
/// LOCAL `feature` branch one commit ahead — HEAD left on `main`.
///
/// Distinct from [`repo_with_history`], which checks `feature` out and buries it three
/// commits deep: a suite that wants to *check out* `feature`, or wants the branch list to
/// be exactly two entries, wants this. `feature` is deliberately not pushed — publishing
/// it is the operation under test.
pub fn repo_with_feature_branch(project: &Path, name: &str) -> PathBuf {
    let work = repo_with_origin(project, name);
    git(&work, &["checkout", "-q", "-b", "feature"]);
    commit(
        &work,
        "feature.txt",
        "feature work\n",
        "feature: work in progress",
    );
    git(&work, &["checkout", "-q", "main"]);
    work
}

/// A repository `name` carrying a **real, unresolved merge conflict**: `main`
/// and a `side` branch each rewrote the same line of `conf.txt`, and
/// `git merge side` was started from `main` and left in progress.
///
/// Named rather than inherited: [`repo_with_history`] commits to disjoint paths so it
/// can never conflict. The guarantee is the state, not the branch names — both-unmerged
/// in `git status`, a `MERGE_HEAD`, all three index stages staged, and git's own two-way
/// `<<<<<<< HEAD / ======= / >>>>>>> side` markers in the working copy. No remote.
pub fn repo_with_conflict(project: &Path, name: &str) -> PathBuf {
    let work = repo_with_one_commit(project, name);
    commit(&work, "conf.txt", "base\n", "conf: base");
    git(&work, &["checkout", "-q", "-b", "side"]);
    commit(&work, "conf.txt", "side\n", "conf: side");
    git(&work, &["checkout", "-q", "main"]);
    commit(&work, "conf.txt", "main\n", "conf: main");
    // Expected to fail: that refusal IS the fixture, so this one call is the
    // only place in the module that may not assert success.
    let _ = Command::new("git")
        .args(["merge", "--no-edit", "side"])
        .current_dir(&work)
        .output();
    work
}
