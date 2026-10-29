use std::path::Path;

use test_support::git_seed::{git, repo_with_one_commit};
use turbogit_domain::model::{MergeStrategy, MultiRootManager};
use turbogit_engine::cli::CliExecutor;
use turbogit_services::integrate_service;
use turbogit_services::multi_root::{build_root, register};

#[test]
fn no_commit_strategy_maps_to_no_commit() {
    let opts = integrate_service::merge_flags(MergeStrategy::NoCommit, false, false, false);
    assert!(opts.no_commit);
    // --no-commit alone cannot stop a fast-forward (git would still move
    // the branch), so the strategy always carries --no-ff with it.
    assert!(opts.no_ff);
    assert!(!opts.squash);
    assert!(!opts.ff_only);
}

#[test]
fn commit_strategy_maps_to_a_plain_merge() {
    let opts = integrate_service::merge_flags(MergeStrategy::Commit, false, false, false);
    assert!(!opts.no_commit);
    assert!(!opts.squash);
    assert!(!opts.ff_only);
}

#[test]
fn squash_strategy_maps_to_squash() {
    let opts = integrate_service::merge_flags(MergeStrategy::Squash, false, false, false);
    assert!(opts.squash);
    assert!(!opts.no_commit);
    assert!(!opts.ff_only);
}

#[test]
fn fast_forward_strategy_maps_to_ff_only() {
    let opts = integrate_service::merge_flags(MergeStrategy::FastForward, false, false, false);
    assert!(opts.ff_only);
    assert!(!opts.squash);
    assert!(!opts.no_commit);
}

#[test]
fn options_pass_through_on_top_of_the_strategy() {
    let opts = integrate_service::merge_flags(MergeStrategy::Commit, true, true, true);
    assert!(opts.no_ff);
    assert!(opts.verify_signatures);
    assert!(opts.allow_unrelated);
}

#[test]
fn fast_forward_wins_over_no_ff() {
    let opts = integrate_service::merge_flags(MergeStrategy::FastForward, true, false, false);
    assert!(opts.ff_only);
    assert!(!opts.no_ff);
}

fn append_commit(dir: &Path, name: &str, text: &str) -> String {
    let file = dir.join(name);
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
        .expect("opening work file");
    use std::io::Write;
    writeln!(f, "{text}").expect("appending work file");
    drop(f);
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", text]);
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

fn divergent_repo() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = repo_with_one_commit(tmp.path(), "repo");
    append_commit(&repo, "base.txt", "base");
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    append_commit(&repo, "feature.txt", "feature-1");
    append_commit(&repo, "feature.txt", "feature-2");
    git(&repo, &["checkout", "-q", "main"]);
    append_commit(&repo, "main.txt", "main-1");
    (tmp, repo)
}

fn engine() -> CliExecutor {
    CliExecutor {
        settings: Default::default(),
    }
}

#[test]
fn preview_counts_what_merging_brings_in() {
    let (_tmp, repo) = divergent_repo();
    let preview =
        integrate_service::merge_preview(&engine(), &repo, "feature", MergeStrategy::Commit)
            .expect("preview");

    assert_eq!(preview.merge_commits, 1);
    assert_eq!(preview.files, 1);
    assert_eq!(preview.insertions, 2);
    assert_eq!(preview.deletions, 0);
}

#[test]
fn preview_of_an_already_merged_target_reports_no_commit_and_no_changes() {
    let (_tmp, repo) = divergent_repo();
    git(
        &repo,
        &["merge", "-q", "--no-ff", "-m", "merge feature", "feature"],
    );
    let preview =
        integrate_service::merge_preview(&engine(), &repo, "feature", MergeStrategy::Commit)
            .expect("preview");
    assert_eq!(preview.merge_commits, 0);
    assert_eq!(preview.files, 0);
    assert_eq!(preview.insertions, 0);
    assert_eq!(preview.deletions, 0);
}

#[test]
fn fast_forward_strategy_previews_no_merge_commit() {
    let (_tmp, repo) = divergent_repo();
    git(&repo, &["reset", "-q", "--hard", "main~1"]);
    let preview =
        integrate_service::merge_preview(&engine(), &repo, "feature", MergeStrategy::FastForward)
            .expect("preview");
    assert_eq!(preview.merge_commits, 0);
    assert_eq!(preview.files, 1);
    assert_eq!(preview.insertions, 2);
}

#[test]
fn squash_strategy_previews_one_non_merge_commit() {
    let (_tmp, repo) = divergent_repo();
    let preview =
        integrate_service::merge_preview(&engine(), &repo, "feature", MergeStrategy::Squash)
            .expect("preview");
    assert_eq!(preview.merge_commits, 1);
    assert_eq!(preview.files, 1);
    assert_eq!(preview.insertions, 2);
}

#[test]
fn siblings_are_other_roots_on_the_same_branch() {
    use turbogit_domain::model::RootId;
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut mgr = MultiRootManager::default();
    for name in ["alpha", "beta", "gamma"] {
        let dir = repo_with_one_commit(tmp.path(), name);
        let root = build_root(&engine(), &dir).expect("root snapshot");
        register(&mut mgr, root);
    }
    let gamma = tmp.path().join("gamma");
    git(&gamma, &["checkout", "-q", "-b", "release"]);
    let root = build_root(&engine(), &gamma).expect("root snapshot");
    register(&mut mgr, root);

    let alpha_id = RootId(tmp.path().join("alpha").into());
    let roots: Vec<_> = mgr.roots.clone();
    let siblings = integrate_service::cascade_siblings(&roots, &alpha_id);
    assert_eq!(siblings, vec![RootId(tmp.path().join("beta").into())]);
}

#[test]
fn detached_roots_are_not_cascade_siblings() {
    use turbogit_domain::model::RootId;
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut mgr = MultiRootManager::default();
    for name in ["alpha", "beta"] {
        let dir = repo_with_one_commit(tmp.path(), name);
        let root = build_root(&engine(), &dir).expect("root snapshot");
        register(&mut mgr, root);
    }
    let beta = tmp.path().join("beta");
    git(&beta, &["checkout", "-q", "--detach"]);
    let root = build_root(&engine(), &beta).expect("root snapshot");
    register(&mut mgr, root);

    let alpha_id = RootId(tmp.path().join("alpha").into());
    let roots: Vec<_> = mgr.roots.clone();
    assert!(integrate_service::cascade_siblings(&roots, &alpha_id).is_empty());
}
