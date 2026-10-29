use std::path::{Path, PathBuf};

use test_support::git_seed::{GitEnv, git_with_env};
use turbogit_app::state::AppState;
use turbogit_domain::model::BranchKind;

fn repo_with_remote_branch() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let bare = project.join("origin.git");
    git_with_env(
        &project,
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
        &GitEnv::fixture(),
    );
    let work = project.join("work");
    git_with_env(
        &project,
        &["init", "-q", "-b", "main", work.to_str().unwrap()],
        &GitEnv::fixture(),
    );
    std::fs::write(work.join("README.md"), "x\n").unwrap();
    git_with_env(&work, &["add", "."], &GitEnv::fixture());
    git_with_env(&work, &["commit", "-q", "-m", "init"], &GitEnv::fixture());
    git_with_env(
        &work,
        &["remote", "add", "origin", bare.to_str().unwrap()],
        &GitEnv::fixture(),
    );
    git_with_env(
        &work,
        &["push", "-q", "-u", "origin", "main"],
        &GitEnv::fixture(),
    );
    git_with_env(&work, &["branch", "feat", "HEAD"], &GitEnv::fixture());
    git_with_env(&work, &["push", "-q", "origin", "feat"], &GitEnv::fixture());
    git_with_env(&work, &["branch", "-D", "feat"], &GitEnv::fixture());
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

fn git_stdout(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git must be on PATH");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}
