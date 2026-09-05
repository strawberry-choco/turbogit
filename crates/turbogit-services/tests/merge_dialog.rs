//! Issue 28 — merge dialog upgrade: the strategy → invocation mapping, the
//! pre-merge preview totals, and the cascade-sibling detection.
//!
//! The strategy mapping and sibling detection are pure functions over the
//! domain model; the preview runs against real git repositories (tempdir +
//! system `git`) because its totals are git's own three-dot diff.

use std::path::Path;
use std::process::Command;

use turbogit_domain::model::{MergeOpts, MergeStrategy, MultiRootManager};
use turbogit_engine::cli::CliExecutor;
use turbogit_services::integrate_service;
use turbogit_services::multi_root::{build_root, register};

// ------------------------------------------------------------- strategy ----

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
    // The strategy is the segmented control: a Fast-forward merge cannot
    // carry --no-ff even if the option row was toggled on earlier.
    let opts = integrate_service::merge_flags(MergeStrategy::FastForward, true, false, false);
    assert!(opts.ff_only);
    assert!(!opts.no_ff);
}

// --------------------------------------------------------------- preview ----

/// Run `git <args>` in `dir`, asserting success; returns stdout.
fn run_git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git").args(args).current_dir(dir).output();
    let output = output.expect("spawning git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// Append `text` to `<dir>/<name>`, stage, commit, return HEAD SHA.
fn commit(dir: &Path, name: &str, text: &str) -> String {
    let file = dir.join(name);
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
        .expect("opening work file");
    use std::io::Write;
    writeln!(f, "{text}").expect("appending work file");
    drop(f);
    run_git(dir, &["add", "."]);
    run_git(dir, &["commit", "-m", text]);
    run_git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

/// Fresh repo on `main` with one base commit; a `feature` branch forked at
/// base carrying two commits (2 lines each), and one more commit on `main`
/// after the fork so the branches are truly divergent.
fn divergent_repo() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repo dir");
    run_git(&repo, &["init", "-q", "-b", "main"]);
    run_git(&repo, &["config", "user.email", "test@example.com"]);
    run_git(&repo, &["config", "user.name", "Test"]);
    commit(&repo, "base.txt", "base");
    run_git(&repo, &["checkout", "-q", "-b", "feature"]);
    commit(&repo, "feature.txt", "feature-1");
    commit(&repo, "feature.txt", "feature-2");
    run_git(&repo, &["checkout", "-q", "main"]);
    commit(&repo, "main.txt", "main-1");
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

    // feature carries 2 commits × 1 added line each after the fork point.
    assert_eq!(preview.merge_commits, 1);
    assert_eq!(preview.files, 1);
    assert_eq!(preview.insertions, 2);
    assert_eq!(preview.deletions, 0);
}

#[test]
fn preview_of_an_already_merged_target_reports_no_commit_and_no_changes() {
    let (_tmp, repo) = divergent_repo();
    run_git(
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
    // A repo where feature is strictly ahead of main: ff-able, so the
    // Fast-forward strategy creates no merge commit while still bringing
    // the changes in.
    let (_tmp, repo) = divergent_repo();
    run_git(&repo, &["reset", "-q", "--hard", "main~1"]); // drop main-1
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

// ------------------------------------------------------ cascade siblings ----

#[test]
fn siblings_are_other_roots_on_the_same_branch() {
    use turbogit_domain::model::RootId;
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut mgr = MultiRootManager::default();
    for name in ["alpha", "beta", "gamma"] {
        let dir = tmp.path().join(name);
        std::fs::create_dir_all(&dir).unwrap();
        run_git(&dir, &["init", "-q", "-b", "main"]);
        run_git(&dir, &["config", "user.email", "test@example.com"]);
        run_git(&dir, &["config", "user.name", "Test"]);
        commit(&dir, "base.txt", "base");
        let root = build_root(&engine(), &dir).expect("root snapshot");
        register(&mut mgr, root);
    }
    // Put gamma on another branch: it does not share alpha's branch.
    let gamma = tmp.path().join("gamma");
    run_git(&gamma, &["checkout", "-q", "-b", "release"]);
    // Re-register gamma so its snapshot carries the new current branch.
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
        let dir = tmp.path().join(name);
        std::fs::create_dir_all(&dir).unwrap();
        run_git(&dir, &["init", "-q", "-b", "main"]);
        run_git(&dir, &["config", "user.email", "test@example.com"]);
        run_git(&dir, &["config", "user.name", "Test"]);
        commit(&dir, "base.txt", "base");
        let root = build_root(&engine(), &dir).expect("root snapshot");
        register(&mut mgr, root);
    }
    let beta = tmp.path().join("beta");
    run_git(&beta, &["checkout", "-q", "--detach"]);
    let root = build_root(&engine(), &beta).expect("root snapshot");
    register(&mut mgr, root);

    let alpha_id = RootId(tmp.path().join("alpha").into());
    let roots: Vec<_> = mgr.roots.clone();
    assert!(integrate_service::cascade_siblings(&roots, &alpha_id).is_empty());
}

// Keep MergeOpts import used even if assertions above shift.
#[allow(dead_code)]
fn _opts_shape(_o: MergeOpts) {}
