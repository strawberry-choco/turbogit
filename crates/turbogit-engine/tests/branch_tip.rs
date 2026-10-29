use std::path::Path;

use test_support::git_seed::{GitEnv, GitIdent, git_with_env};
use turbogit_domain::model::{BranchKind, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_engine::git2_exec::Git2Executor;
use turbogit_engine_api::GitExecutor;

const EPOCH: i64 = 1_700_000_000;

fn ident() -> GitEnv {
    GitEnv {
        author: GitIdent {
            name: "ada",
            email: "ada@example",
        },
        committer: GitIdent {
            name: "comm",
            email: "comm@example",
        },
        author_date: None,
        committer_date: None,
    }
}

fn commit(repo: &Path, msg: &str, file: &str, epoch: Option<i64>) {
    std::fs::write(repo.join(file), format!("{msg}\n")).unwrap();
    git_with_env(repo, &["add", "."], &ident());
    let mut env = ident();
    if let Some(e) = epoch {
        env.author_date = Some(format!("@{e} +0000"));
        env.committer_date = Some(format!("@{e} +0000"));
    }
    git_with_env(repo, &["commit", "-q", "-m", msg], &env);
}

fn tip_repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("r");
    git_with_env(
        tmp.path(),
        &["-c", "init.defaultBranch=main", "init", "r"],
        &ident(),
    );
    commit(
        &repo,
        "# first topic\n\nlonger body line",
        "a.txt",
        Some(EPOCH),
    );
    git_with_env(&repo, &["branch", "old"], &ident());
    commit(&repo, "second topic", "b.txt", None);

    git_with_env(
        tmp.path(),
        &["clone", "--bare", "r", "origin.git"],
        &ident(),
    );
    git_with_env(
        &repo,
        &["remote", "add", "origin", "../origin.git"],
        &ident(),
    );
    git_with_env(&repo, &["push", "-q", "origin", "main"], &ident());
    git_with_env(&repo, &["push", "-q", "origin", "old"], &ident());
    git_with_env(&repo, &["fetch", "-q", "origin"], &ident());
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

        let main = branches
            .iter()
            .find(|b| b.kind == BranchKind::Local && b.name == "main")
            .unwrap();
        let tip = main.tip.as_ref().unwrap();
        let short = git_with_env(&repo, &["rev-parse", "--short", "HEAD"], &ident())
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
