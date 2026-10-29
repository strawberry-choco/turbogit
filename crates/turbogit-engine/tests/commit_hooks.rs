use std::path::{Path, PathBuf};

use test_support::git_seed::git;
use turbogit_domain::model::VcsSettings;
use turbogit_engine::GitExecutor;
use turbogit_engine::build_executor;

fn exec() -> std::sync::Arc<dyn GitExecutor> {
    build_executor(&VcsSettings::default())
}

fn hooked_repo() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    std::fs::write(repo.join("base.txt"), "base\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    std::fs::write(repo.join("work.txt"), "staged work\n").unwrap();
    git(&repo, &["add", "work.txt"]);

    let hook = repo.join(".git").join("hooks").join("pre-commit");
    std::fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    (tmp, repo)
}

fn head_subject(repo: &Path) -> String {
    git(repo, &["log", "-1", "--format=%s"]).trim().to_string()
}

#[test]
fn commit_index_runs_hooks_and_no_verify_skips_them() {
    let (_tmp, repo) = hooked_repo();
    let engine = exec();

    let refused = engine.commit_index(&repo, "refused subject", false, false);
    assert!(
        refused.is_err(),
        "a failing pre-commit hook must refuse the commit"
    );
    assert_eq!(
        head_subject(&repo),
        "init",
        "the refused commit left HEAD where it was"
    );

    engine
        .commit_index(&repo, "no-verify subject", false, true)
        .expect("the --no-verify commit must land");
    assert_eq!(head_subject(&repo), "no-verify subject");
}

#[test]
fn commit_runs_hooks_and_no_verify_skips_them() {
    let (_tmp, repo) = hooked_repo();
    let engine = exec();

    let refused = engine.commit(&repo, "refused subject", false, false);
    assert!(
        refused.is_err(),
        "a failing pre-commit hook must refuse the -a commit"
    );
    assert_eq!(head_subject(&repo), "init", "HEAD is unmoved");

    engine
        .commit(&repo, "no-verify subject", false, true)
        .expect("the --no-verify -a commit must land");
    assert_eq!(head_subject(&repo), "no-verify subject");
}
