use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[derive(Clone, Debug)]
pub struct GitIdent {
    pub name: &'static str,
    pub email: &'static str,
}

#[derive(Clone, Debug)]
pub struct GitEnv {
    pub author: GitIdent,
    pub committer: GitIdent,
    pub author_date: Option<String>,
    pub committer_date: Option<String>,
}

impl GitEnv {
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

fn git_command(dir: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(dir);
    cmd
}

fn spawn_git(dir: &Path, args: &[&str], pin: Option<&GitEnv>) -> String {
    let mut cmd = git_command(dir, args);
    if let Some(pin) = pin {
        cmd = pin.command(dir, args);
    }
    let out = cmd.output().expect("git must be on PATH");
    assert_success(out, args, dir)
}

fn assert_success(out: Output, args: &[&str], dir: &Path) -> String {
    assert!(
        out.status.success(),
        "git {args:?} in {} failed: {}",
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

pub fn git(dir: &Path, args: &[&str]) -> String {
    spawn_git(dir, args, None)
}

pub fn git_with_env(dir: &Path, args: &[&str], env: &GitEnv) -> String {
    spawn_git(dir, args, Some(env))
}

/// Write `body` to `<repo>/<path>`, stage everything, commit it as `message`.
/// It **overwrites** `path` rather than appending, and configures no identity:
/// set `user.email`/`user.name` on the repository first, because a headless
/// machine has no global identity for the first commit to borrow.
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
/// [`repo_with_origin`], which always carries a bare `origin` plus an upstream, a
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
/// conflict, a test that needs a conflict must say so in its own fixture.
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

pub fn two_repos(project: &Path, first: &str, second: &str) -> (PathBuf, PathBuf) {
    (
        repo_with_one_commit(project, first),
        repo_with_one_commit(project, second),
    )
}

/// A repository `name` with a bare `origin`, `main` published to it, and a
/// LOCAL `feature` branch one commit ahead. HEAD left on `main`.
///
/// Distinct from [`repo_with_history`], which checks `feature` out and buries it three
/// commits deep: a suite that wants to *check out* `feature`, or wants the branch list to
/// be exactly two entries, wants this. `feature` is deliberately not pushed, publishing
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
/// can never conflict. The guarantee is the state, not the branch names, both-unmerged
/// in `git status`, a `MERGE_HEAD`, all three index stages staged, and git's own two-way
/// `<<<<<<< HEAD / ======= / >>>>>>> side` markers in the working copy. No remote.
pub fn repo_with_conflict(project: &Path, name: &str) -> PathBuf {
    let work = repo_with_one_commit(project, name);
    commit(&work, "conf.txt", "base\n", "conf: base");
    git(&work, &["checkout", "-q", "-b", "side"]);
    commit(&work, "conf.txt", "side\n", "conf: side");
    git(&work, &["checkout", "-q", "main"]);
    commit(&work, "conf.txt", "main\n", "conf: main");
    let _ = Command::new("git")
        .args(["merge", "--no-edit", "side"])
        .current_dir(&work)
        .output();
    work
}
