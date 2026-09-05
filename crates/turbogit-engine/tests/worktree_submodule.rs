//! Issue 14 — Worktrees & Submodules tool tabs: the engine operations the
//! tabs read through.
//!
//! Every test builds a real fixture repository with `git` on PATH and pins
//! the CLI adapter's behavior for one operation: `worktree_list`'s dirty
//! state, `worktree_remove`, and the submodule read/update/deinit trio.

use std::path::{Path, PathBuf};
use std::process::Command;

use turbogit_domain::model::{SubmoduleState, VcsSettings};
use turbogit_engine::GitExecutor;
use turbogit_engine::cli::CliExecutor;

/// The executor under test with default settings (git resolved from PATH).
fn engine() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

/// Run `git <args>` in `dir`, asserting success; returns stdout.
fn run_git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git should be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// An initialized temp repository with one base commit on `main`. The
/// returned path is canonicalized (macOS tempdirs are symlinked) so the
/// engine's main-worktree filtering — a path comparison — matches.
fn temp_repo(tag: &str) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join(tag);
    std::fs::create_dir_all(&repo).unwrap();
    run_git(&repo, &["init", "-q", "-b", "main"]);
    run_git(&repo, &["config", "user.email", "test@example.com"]);
    run_git(&repo, &["config", "user.name", "Test"]);
    std::fs::write(repo.join("base.txt"), "base\n").unwrap();
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-q", "-m", "init"]);
    (tmp, repo.canonicalize().unwrap())
}

// ------------------------------------------------------- worktree reads --

/// `worktree_list` reports each linked worktree with its checked-out branch
/// and a dirty flag: clean on a fresh checkout, true once the worktree's
/// files change (issue 14 Worktrees tab status column).
#[test]
fn worktree_list_reports_branch_and_dirty_state() {
    let (_tmp, repo) = temp_repo("wt-dirty");
    run_git(&repo, &["branch", "feature"]);
    let wt_path = _tmp.path().join("wt-feature");
    run_git(
        &repo,
        &["worktree", "add", wt_path.to_str().unwrap(), "feature"],
    );
    let wt_path = wt_path.canonicalize().unwrap();

    let wts = engine().worktree_list(&repo).expect("worktree_list");
    assert_eq!(wts.len(), 1, "only the linked worktree is listed");
    assert_eq!(wts[0].path, wt_path);
    assert_eq!(wts[0].branch, "feature");
    assert!(!wts[0].dirty, "a fresh checkout is clean");

    std::fs::write(wt_path.join("base.txt"), "changed\n").unwrap();
    let wts = engine().worktree_list(&repo).expect("worktree_list");
    assert!(wts[0].dirty, "a modified worktree reports dirty");
}

/// `worktree_remove` deletes a linked worktree: it refuses a dirty one
/// without `force`, and `force` removes it — the directory is gone and the
/// listing comes back empty.
#[test]
fn worktree_remove_refuses_dirty_without_force_and_removes_with_force() {
    let (_tmp, repo) = temp_repo("wt-remove");
    run_git(&repo, &["branch", "feature"]);
    let wt_path = _tmp.path().join("wt-feature");
    run_git(
        &repo,
        &["worktree", "add", wt_path.to_str().unwrap(), "feature"],
    );
    let wt_path = wt_path.canonicalize().unwrap();
    std::fs::write(wt_path.join("base.txt"), "changed\n").unwrap();

    let refused = engine().worktree_remove(&repo, &wt_path, false);
    assert!(refused.is_err(), "a dirty worktree needs force");
    assert!(wt_path.exists(), "refused removal leaves the tree alone");

    engine()
        .worktree_remove(&repo, &wt_path, true)
        .expect("forced removal of a dirty worktree");
    assert!(!wt_path.exists(), "removed worktree directory is gone");
    assert!(
        engine().worktree_list(&repo).unwrap().is_empty(),
        "removed worktree leaves the listing"
    );
}

// ------------------------------------------------------ submodule reads --

/// A superproject with one locally-added submodule. The child repo lives
/// outside the superproject (`../child-src` relative to it) so the clone
/// has a real source.
fn temp_super_with_submodule(tag: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let child = tmp.path().join("child-src");
    std::fs::create_dir_all(&child).unwrap();
    run_git(&child, &["init", "-q", "-b", "main"]);
    run_git(&child, &["config", "user.email", "test@example.com"]);
    run_git(&child, &["config", "user.name", "Test"]);
    std::fs::write(child.join("c.txt"), "one\n").unwrap();
    run_git(&child, &["add", "."]);
    run_git(&child, &["commit", "-q", "-m", "c1"]);

    let superproject = tmp.path().join(tag);
    std::fs::create_dir_all(&superproject).unwrap();
    run_git(&superproject, &["init", "-q", "-b", "main"]);
    run_git(&superproject, &["config", "user.email", "test@example.com"]);
    run_git(&superproject, &["config", "user.name", "Test"]);
    // Newer git refuses the file transport for submodule clones by default;
    // `-c` travels through GIT_CONFIG_PARAMETERS to the child clone.
    run_git(
        &superproject,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            "../child-src",
            "child",
        ],
    );
    run_git(&superproject, &["commit", "-q", "-m", "add child"]);
    (tmp, superproject, child)
}

/// `submodule_status` reports the pinned HEAD commit, the recorded gitlink,
/// and the up-to-date state for a fresh submodule (issue 14 pinned-vs-recorded).
#[test]
fn submodule_status_reports_pinned_and_recorded_for_fresh_submodule() {
    let (_tmp, repo, _child) = temp_super_with_submodule("sub-fresh");
    let subs = engine().submodule_status(&repo).expect("submodule_status");
    assert_eq!(subs.len(), 1);
    let sub = &subs[0];
    assert_eq!(sub.path, PathBuf::from("child"));
    assert_eq!(sub.state, SubmoduleState::UpToDate);
    assert_eq!(sub.head, sub.recorded, "fresh checkout matches the record");
    let sha = sub.recorded.as_deref().expect("recorded sha");
    assert_eq!(sha.len(), 40, "a full gitlink sha is reported");
}

/// Moving the submodule's HEAD to a newer commit (without recording it in
/// the superproject) flips the state to needs-update and separates the
/// pinned HEAD from the recorded gitlink.
#[test]
fn submodule_status_flags_needs_update_when_head_moves_off_record() {
    let (_tmp, repo, _child_src) = temp_super_with_submodule("sub-ahead");
    // Move the SUBMODULE's HEAD (the checked-out copy), not the source repo.
    let sub_wc = repo.join("child");
    std::fs::write(sub_wc.join("c.txt"), "two\n").unwrap();
    run_git(&sub_wc, &["add", "."]);
    run_git(&sub_wc, &["commit", "-q", "-m", "c2"]);

    let subs = engine().submodule_status(&repo).expect("submodule_status");
    let sub = &subs[0];
    assert_eq!(sub.state, SubmoduleState::NeedsUpdate);
    assert_ne!(sub.head, sub.recorded);
    // The recorded side stays at the old commit.
    let recorded = sub.recorded.as_deref().unwrap();
    assert_eq!(recorded.len(), 40);
}

/// `submodule_deinit` un-initializes the submodule (the status flips to
/// uninitialized, HEAD reads None, the recorded gitlink survives), and
/// `submodule_update` with `init` re-checks it out up to date again.
#[test]
fn submodule_deinit_then_update_round_trips() {
    let (_tmp, repo, _child) = temp_super_with_submodule("sub-cycle");
    engine()
        .submodule_deinit(&repo, Path::new("child"), false)
        .expect("submodule_deinit");

    let subs = engine()
        .submodule_status(&repo)
        .expect("status after deinit");
    assert_eq!(subs[0].state, SubmoduleState::Uninitialized);
    assert_eq!(subs[0].head, None, "uninitialized has no HEAD");
    assert!(subs[0].recorded.is_some(), "the record survives deinit");

    engine()
        .submodule_update(&repo, Path::new("child"), true)
        .expect("submodule update --init");
    let subs = engine()
        .submodule_status(&repo)
        .expect("status after update");
    assert_eq!(subs[0].state, SubmoduleState::UpToDate);
    assert_eq!(subs[0].head, subs[0].recorded);
}

/// `submodule_update` on a needs-update submodule checks the recorded
/// commit back out (the update action's core contract, issue 14).
#[test]
fn submodule_update_checks_recorded_commit_back_out() {
    let (_tmp, repo, _child_src) = temp_super_with_submodule("sub-update");
    // Move the SUBMODULE's HEAD (the checked-out copy), not the source repo.
    let sub_wc = repo.join("child");
    std::fs::write(sub_wc.join("c.txt"), "two\n").unwrap();
    run_git(&sub_wc, &["add", "."]);
    run_git(&sub_wc, &["commit", "-q", "-m", "c2"]);
    assert_eq!(
        engine().submodule_status(&repo).unwrap()[0].state,
        SubmoduleState::NeedsUpdate
    );

    engine()
        .submodule_update(&repo, Path::new("child"), false)
        .expect("submodule update");

    let subs = engine()
        .submodule_status(&repo)
        .expect("status after update");
    assert_eq!(subs[0].state, SubmoduleState::UpToDate);
    assert_eq!(subs[0].head, subs[0].recorded);
}
