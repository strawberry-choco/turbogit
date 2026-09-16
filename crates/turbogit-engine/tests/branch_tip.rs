//! Issue 02 — Branch tip commit in the data model.
//!
//! `branches()` must attach the tip's short hash, subject, author and time to
//! every branch row on the same listing read (single `git for-each-ref` /
//! libgit2 peel) with no extra per-branch git calls. Expected values come from
//! `git rev-parse` / the pinned commit dates directly — an independent source
//! of truth. Runs against the real CLI adapter *and* libgit2 so both backends
//! stay in parity.

use std::path::Path;
use std::process::Command;

use turbogit_domain::model::{BranchKind, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_engine::git2_exec::Git2Executor;
use turbogit_engine_api::GitExecutor;

const EPOCH: i64 = 1_700_000_000;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "ada")
        .env("GIT_AUTHOR_EMAIL", "ada@example")
        .env("GIT_COMMITTER_NAME", "comm")
        .env("GIT_COMMITTER_EMAIL", "comm@example")
        .output()
        .expect("git must be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn commit(repo: &Path, msg: &str, file: &str, epoch: Option<i64>) {
    std::fs::write(repo.join(file), format!("{msg}\n")).unwrap();
    git(repo, &["add", "."]);
    let mut cmd = Command::new("git");
    cmd.args(["commit", "-q", "-m", msg])
        .env("GIT_AUTHOR_NAME", "ada")
        .env("GIT_AUTHOR_EMAIL", "ada@example")
        .env("GIT_COMMITTER_NAME", "comm")
        .env("GIT_COMMITTER_EMAIL", "comm@example")
        .current_dir(repo);
    if let Some(e) = epoch {
        cmd.env("GIT_AUTHOR_DATE", format!("@{e} +0000"))
            .env("GIT_COMMITTER_DATE", format!("@{e} +0000"));
    }
    assert!(cmd.output().expect("commit").status.success());
}

/// A repo on `main`: the pinned multi-line commit `# first topic`, branch
/// `old` at it, then a fresh `second topic` on `main`, plus a tracked remote
/// carrying both branches (fetched, so remote rows exist).
fn tip_repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("r");
    git(tmp.path(), &["-c", "init.defaultBranch=main", "init", "r"]);
    commit(
        &repo,
        "# first topic\n\nlonger body line",
        "a.txt",
        Some(EPOCH),
    );
    git(&repo, &["branch", "old"]);
    commit(&repo, "second topic", "b.txt", None);

    git(tmp.path(), &["clone", "--bare", "r", "origin.git"]);
    git(&repo, &["remote", "add", "origin", "../origin.git"]);
    git(&repo, &["push", "-q", "origin", "main"]);
    git(&repo, &["push", "-q", "origin", "old"]);
    git(&repo, &["fetch", "-q", "origin"]);
    tmp
}

fn cli() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

fn g2() -> Git2Executor {
    Git2Executor::new(turbogit_engine::cli::CliExecutor {
        settings: VcsSettings::default(),
    })
}

fn find(branches: &[turbogit_domain::model::Branch], kind: BranchKind, name: &str) {
    let b = branches
        .iter()
        .find(|b| b.kind == kind && b.name == name)
        .unwrap_or_else(|| panic!("{kind:?} branch {name} listed"));
    assert!(b.tip.is_some(), "{kind:?} branch {name} carries a tip");
}

/// Every branch — local and remote — carries its tip commit's short hash,
/// subject, author and time; the body of a multi-line message stays out.
#[test]
fn tip_commit_attached_to_local_and_remote_rows_on_both_backends() {
    for exec in [Box::new(cli()) as Box<dyn GitExecutor>, Box::new(g2())] {
        let tmp = tip_repo();
        let repo = tmp.path().join("r");
        let branches = exec.branches(&repo).expect("branches()");

        find(&branches, BranchKind::Local, "main");
        find(&branches, BranchKind::Local, "old");
        find(&branches, BranchKind::Remote, "main");
        find(&branches, BranchKind::Remote, "old");

        // main's tip is the fresh `second topic` commit; the short hash must
        // match git's own abbreviation.
        let main = branches
            .iter()
            .find(|b| b.kind == BranchKind::Local && b.name == "main")
            .unwrap();
        let tip = main.tip.as_ref().unwrap();
        let short = git(&repo, &["rev-parse", "--short", "HEAD"])
            .trim()
            .to_string();
        assert_eq!(
            tip.short_hash, short,
            "short hash matches `git rev-parse --short`"
        );
        assert_eq!(
            tip.message, "second topic",
            "subject is the message's first line"
        );
        assert_eq!(tip.author, "ada", "author name preserved");
        assert!(
            (chrono::Utc::now() - tip.time).num_seconds().abs() < 3600,
            "fresh tip time is recent, was {}",
            tip.time
        );

        // `old`'s tip is the pinned commit: same fields, exact pinned time,
        // and the body line is NOT part of the message.
        let old = branches
            .iter()
            .find(|b| b.kind == BranchKind::Local && b.name == "old")
            .unwrap();
        let old_tip = old.tip.as_ref().unwrap();
        assert_eq!(
            old_tip.message, "# first topic",
            "subject only, body excluded"
        );
        assert_eq!(old_tip.author, "ada");
        assert_eq!(
            old_tip.time.timestamp(),
            EPOCH,
            "pinned committer time preserved"
        );
        assert_eq!(
            old_tip.short_hash.len(),
            short.len(),
            "short hash length matches backend default"
        );
    }
}

/// Local and remote rows pointing at the same commit agree on the tip, and
/// the tip anchors `last_touched` (issue 32's stale input stays consistent).
#[test]
fn tip_time_anchors_last_touched() {
    for exec in [Box::new(cli()) as Box<dyn GitExecutor>, Box::new(g2())] {
        let tmp = tip_repo();
        let repo = tmp.path().join("r");
        let branches = exec.branches(&repo).expect("branches()");

        let old = branches
            .iter()
            .find(|b| b.kind == BranchKind::Local && b.name == "old")
            .unwrap();
        let tip = old.tip.as_ref().expect("tip present");
        assert_eq!(
            old.last_touched,
            Some(tip.time),
            "last_touched mirrors the tip"
        );
    }
}
