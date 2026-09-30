//! Issue 32 — Branches popup sync data, engine seam: the CLI adapter's
//! `branches()` parses each local branch's ahead/behind counts and the
//! `[gone]` marker from `git branch -vv`, plus the branch-tip committer
//! date (the popup's stale badge) from `git for-each-ref`. Real repos,
//! pinned dates — the migration oracle for replacement backends.

use std::path::Path;
use std::process::Command;

use chrono::DateTime;
use test_support::git_seed::git;
use turbogit_domain::model::{BranchKind, Upstream, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_engine_api::GitExecutor;

/// Fixed commit epochs (git internal `@<unix> <offset>` date format).
const EPOCH_1: i64 = 1_112_911_993;
const EPOCH_2: i64 = 1_234_567_890;
const EPOCH_3: i64 = 1_700_000_000;
const EPOCH_4: i64 = 1_750_000_000;

fn engine() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

/// Commit a new file with both author and committer dates pinned to `epoch`.
///
/// Stays local because it sets `GIT_AUTHOR_DATE` / `GIT_COMMITTER_DATE` per call
/// and `git_seed::git` takes no per-call environment. The pin is load-bearing:
/// without it those commits would carry the wall clock and the stale badge's
/// `EPOCH_1..=EPOCH_4` assertions would assert nothing.
fn commit_at_epoch(repo: &Path, msg: &str, epoch: i64) {
    std::fs::write(repo.join("f.txt"), format!("{msg}\n")).unwrap();
    git(repo, &["add", "."]);
    let out = Command::new("git")
        .args(["commit", "-q", "-m", msg])
        .env("GIT_AUTHOR_DATE", format!("@{epoch} +0000"))
        .env("GIT_COMMITTER_DATE", format!("@{epoch} +0000"))
        .current_dir(repo)
        .output()
        .expect("git commit");
    assert!(
        out.status.success(),
        "commit failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn local<'a>(
    branches: &'a [turbogit_domain::model::Branch],
    name: &str,
) -> &'a turbogit_domain::model::Branch {
    branches
        .iter()
        .find(|b| b.kind == BranchKind::Local && b.name == name)
        .unwrap_or_else(|| panic!("local branch {name} listed"))
}

fn remote<'a>(
    branches: &'a [turbogit_domain::model::Branch],
    name: &str,
) -> &'a turbogit_domain::model::Branch {
    branches
        .iter()
        .find(|b| b.kind == BranchKind::Remote && b.name == name)
        .unwrap_or_else(|| panic!("remote branch {name} listed"))
}

#[test]
fn branches_parse_ahead_behind_gone_and_last_touched() {
    // Inline rather than a `git_seed` recipe: this needs a bare `origin` pushed
    // to twice, a `ghost` deleted INSIDE the bare and then pruned, and a `feat`
    // tracking `origin/main`.
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let origin = tmp.path().join("origin.git");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);

    // c1 on main is the fork point; the branch is pushed once before any
    // divergence so `feat` can track `origin/main`.
    commit_at_epoch(&repo, "c1", EPOCH_1);
    git(
        &repo,
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            origin.to_str().unwrap(),
        ],
    );
    git(
        &repo,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(&repo, &["push", "-q", "origin", "main"]);

    // ghost: tracks origin/ghost. It is pushed and fetched while the remote
    // branch exists, then the remote branch is deleted INSIDE the bare remote
    // and `git fetch --prune` drops the local remote-tracking ref — the
    // exact sequence that makes `git branch -vv` print `[origin/ghost: gone]`.
    git(&repo, &["branch", "ghost"]);
    git(&repo, &["push", "-q", "-u", "origin", "ghost"]);
    git(&repo, &["fetch", "-q", "origin"]);

    // feat: two commits past the fork; main advances one. Expect ahead=2,
    // behind=1 against origin/main.
    git(&repo, &["branch", "feat"]);
    git(&repo, &["branch", "--set-upstream-to=origin/main", "feat"]);
    git(&repo, &["checkout", "-q", "feat"]);
    commit_at_epoch(&repo, "c2", EPOCH_2);
    commit_at_epoch(&repo, "c3", EPOCH_3);
    git(&repo, &["checkout", "-q", "main"]);
    commit_at_epoch(&repo, "c4", EPOCH_4);
    git(&repo, &["push", "-q", "origin", "main"]);

    // Delete the remote ghost branch and prune its tracking ref.
    git(&origin, &["update-ref", "-d", "refs/heads/ghost"]);
    git(&repo, &["fetch", "-q", "--prune", "origin"]);

    let branches = engine().branches(&repo).expect("branches");

    // feat carries two unpushed commits and lags one pushed commit.
    let feat = local(&branches, "feat");
    assert_eq!(feat.tracking, Upstream::from_git_ref("origin/main"));
    assert_eq!(
        (feat.ahead, feat.behind),
        (2, 1),
        "ahead/behind must come from the -vv bracket"
    );
    assert!(!feat.gone);
    assert_eq!(
        feat.last_touched,
        Some(DateTime::from_timestamp(EPOCH_3, 0).unwrap()),
        "stale badge reads the branch-tip committer date"
    );

    // ghost's upstream was deleted on the remote.
    let ghost = local(&branches, "ghost");
    assert_eq!(ghost.tracking, Upstream::from_git_ref("origin/ghost"));
    assert!(ghost.gone, "deleted upstream must mark the branch gone");
    assert_eq!(
        ghost.last_touched,
        Some(DateTime::from_timestamp(EPOCH_1, 0).unwrap())
    );

    // Untracked branches carry no sync markers.
    let main = local(&branches, "main");
    assert_eq!(main.tracking, None);
    assert_eq!((main.ahead, main.behind), (0, 0));
    assert!(!main.gone);
    assert_eq!(
        main.last_touched,
        Some(DateTime::from_timestamp(EPOCH_4, 0).unwrap())
    );

    // Remote rows keep their short name and gain the same date.
    let origin_main = remote(&branches, "main");
    assert_eq!(origin_main.tracking, None);
    assert_eq!(
        origin_main.last_touched,
        Some(DateTime::from_timestamp(EPOCH_4, 0).unwrap())
    );

    // The pruned remote-tracking ref is gone from the list; the popup
    // derives the `origin/ghost` row from the local branch's tracking
    // and its `gone` flag instead.
    assert!(
        !branches
            .iter()
            .any(|b| b.kind == BranchKind::Remote && b.name == "ghost"),
        "a pruned remote ref must not be listed"
    );
}
