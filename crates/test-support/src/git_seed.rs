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
//!
//! ## Where a commit identity comes from
//!
//! Two routes, and a suite picks one deliberately:
//!
//! * **configure the repository** — `git config user.email` / `user.name` on the
//!   fixture, then commit through [`git`] or [`commit`]. The majority route, and
//!   the one to take when nothing reads an identity back out of git.
//! * **pin one for a call** — fill in a [`GitEnv`], run through [`git_with_env`],
//!   and the call commits as exactly those bytes on every machine.
//!
//! Both serve the same rule — a headless machine has no global identity for a
//! first commit to borrow — and only the second can carry the two answers no
//! repository config can express: an author that differs from its committer, and
//! a pinned commit date.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// One identity: a name and an email.
///
/// Fields rather than an env slice, and that is the whole point of the shape: a
/// caller fills in values and never writes an environment variable name, so
/// `GIT_AUTHOR_COMMITTER_NAME` — the misspelling two suites in this tree carried,
/// of a variable git does not have — is not expressible here at all.
#[derive(Clone, Debug)]
pub struct GitIdent {
    pub name: &'static str,
    pub email: &'static str,
}

/// A commit identity pinned for one git invocation, plus the dates a fixture
/// that asserts on commit times needs. Every field is public so a caller adds a
/// date by assigning it; nothing else is configurable, by design.
#[derive(Clone, Debug)]
pub struct GitEnv {
    pub author: GitIdent,
    pub committer: GitIdent,
    /// Pinned only where a suite asserts on a commit's date; `None` leaves the
    /// wall clock alone.
    pub author_date: Option<String>,
    pub committer_date: Option<String>,
}

impl GitEnv {
    /// The shared pin: author and committer both `t <t@t>`, no dates. A named
    /// value rather than one copy of the same four literals per suite.
    pub fn fixture() -> Self {
        Self {
            author: GitIdent {
                name: "t",
                email: "t@t",
            },
            committer: GitIdent {
                name: "t",
                email: "t@t",
            },
            author_date: None,
            committer_date: None,
        }
    }

    /// `git <args>` in `dir` under this identity.
    ///
    /// The ONE place the four `GIT_AUTHOR_*`/`GIT_COMMITTER_*` names and the two
    /// date names are spelled. A fixture that needs the spawn itself rather than
    /// the outcome — to expect a failure — takes its environment from here.
    pub fn command(&self, dir: &Path, args: &[&str]) -> Command {
        let mut cmd = git_command(dir, args);
        cmd.env("GIT_AUTHOR_NAME", self.author.name)
            .env("GIT_AUTHOR_EMAIL", self.author.email)
            .env("GIT_COMMITTER_NAME", self.committer.name)
            .env("GIT_COMMITTER_EMAIL", self.committer.email);
        if let Some(date) = &self.author_date {
            cmd.env("GIT_AUTHOR_DATE", date);
        }
        if let Some(date) = &self.committer_date {
            cmd.env("GIT_COMMITTER_DATE", date);
        }
        cmd
    }
}

/// `git <args>`, cwd `dir`, inheriting this process's environment: the one
/// command both doors build, so the two differ only in what they pin onto it.
fn git_command(dir: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(dir);
    cmd
}

/// The one spawn step both doors go through: run `git <args>` in `dir`, assert
/// success, return stdout. `pin` is `None` for [`git`]'s inherited environment.
fn spawn_git(dir: &Path, args: &[&str], pin: Option<&GitEnv>) -> String {
    let mut cmd = git_command(dir, args);
    if let Some(pin) = pin {
        cmd = pin.command(dir, args);
    }
    let out = cmd.output().expect("git must be on PATH");
    assert_success(out, args, dir)
}

/// git's own stderr in the panic, so a failed invocation is reported as the rule
/// git broke rather than as a claim about `PATH` — `git must be on PATH` belongs
/// to the spawn above, which is the only failure that is about `PATH`.
fn assert_success(out: Output, args: &[&str], dir: &Path) -> String {
    assert!(
        out.status.success(),
        "git {args:?} in {} failed: {}",
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Run `git <args>` in `dir`, asserting success; returns stdout.
///
/// The repository-configure door: the invocation inherits this process's
/// environment, so it commits as whatever the repository or the machine carries.
/// Use [`git_with_env`] when the call has to commit as specific bytes.
pub fn git(dir: &Path, args: &[&str]) -> String {
    spawn_git(dir, args, None)
}

/// Run `git <args>` in `dir` under `env`, asserting success; returns stdout.
///
/// The pin-one-call door: the call commits as `env`'s identity and dates
/// whatever the repository and the machine happen to be configured with.
pub fn git_with_env(dir: &Path, args: &[&str], env: &GitEnv) -> String {
    spawn_git(dir, args, Some(env))
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

/// Pin the eol policy in `repo`'s own config, so its text checks out as LF on any
/// machine.
///
/// Persisted in the repository rather than passed as a per-invocation
/// `-c core.autocrlf=false` flag: the engine adapter spawns its own git in the
/// same repository, and only repository config reaches that second spawn.
pub fn pin_eol_lf(repo: &Path) {
    git(repo, &["config", "core.autocrlf", "false"]);
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
    pin_eol_lf(&work);
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
