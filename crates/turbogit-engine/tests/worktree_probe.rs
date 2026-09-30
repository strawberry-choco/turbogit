//! Ticket 01 — engine: the worktree list is decoupled from the dirty probe.
//!
//! `worktree_list` answers path + checked-out branch without ever running a
//! working-tree scan; dirtiness is a per-worktree probe (`worktree_dirty`)
//! invocable on demand with the same semantics `worktree_list` once computed
//! inline. Every test builds a real fixture repository with `git` on PATH and
//! pins behavior through the `GitExecutor` seam for both adapters.

use std::path::{Path, PathBuf};
use std::process::Command;

use test_support::git_seed::git;
use turbogit_domain::model::VcsSettings;
use turbogit_engine::GitExecutor;
use turbogit_engine::cli::CliExecutor;
use turbogit_engine::git2_exec::Git2Executor;

/// The CLI adapter under test with default settings (git resolved from PATH).
fn engine_cli() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

/// The libgit2 adapter under test, composed over the same CLI settings.
fn engine_git2() -> Git2Executor {
    Git2Executor::new(engine_cli())
}

// `temp_repo` stays local, not `repo_with_one_commit`: every case here writes
// to `base.txt` and then probes the linked worktree for it, so the tracked path
// is load-bearing and the recipe's `README.md` is not. `temp_repo_ignored`
// stays local because it commits a `.gitignore` excluding `build/`.

/// `git <args>` expected to CONFLICT: the non-zero exit IS the state under
/// test, so this inverts `git_seed::git`, which asserts success. It surfaces
/// stderr because here the failure is silent unless asked for.
fn git_merge_that_must_conflict(worktree: &Path, args: &[&str]) {
    let merged = Command::new("git")
        .args(args)
        .current_dir(worktree)
        .output()
        .expect("git in worktree");
    assert!(
        !merged.status.success(),
        "the divergent merge should conflict: {}",
        String::from_utf8_lossy(&merged.stderr)
    );
}

/// An initialized temp repository with one base commit on `main`. The
/// returned path is canonicalized so the engine's main-worktree filtering —
/// a path comparison — matches across adapters.
fn temp_repo(tag: &str) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join(tag);
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    std::fs::write(repo.join("base.txt"), "base\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    (tmp, repo.canonicalize().unwrap())
}

/// Like [`temp_repo`], but the base commit also carries a `.gitignore`
/// excluding a `build/` directory — so linked worktrees checkout the ignore
/// rules to the tree under test.
fn temp_repo_ignored(tag: &str) -> (tempfile::TempDir, PathBuf) {
    let (_tmp, repo) = temp_repo(tag);
    std::fs::write(repo.join(".gitignore"), "build/\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "add gitignore"]);
    (_tmp, repo)
}

/// A linked worktree of `repo` on an existing branch, canonicalized like the
/// root; returns its directory.
fn add_worktree(repo: &Path, parent: &Path, name: &str, branch: &str) -> PathBuf {
    git(repo, &["branch", branch]);
    let wt = parent.join(name);
    git(repo, &["worktree", "add", wt.to_str().unwrap(), branch]);
    wt.canonicalize().unwrap()
}

// ------------------------------------------------------- list vs probe --

/// The list answers path + branch without ever running a working-tree scan:
/// a worktree with tracked modifications still lists with an untouched
/// (unknown) dirty flag.
fn assert_list_is_probe_free(engine: &dyn GitExecutor) {
    let (_tmp, repo) = temp_repo("wt-list-free");
    let wt_path = add_worktree(&repo, _tmp.path(), "wt-feature", "feature");
    std::fs::write(wt_path.join("base.txt"), "changed\n").unwrap();

    let wts = engine.worktree_list(&repo).expect("worktree_list");
    assert_eq!(wts.len(), 1, "only the linked worktree is listed");
    assert_eq!(wts[0].path, wt_path);
    assert_eq!(wts[0].branch, "feature");
    assert_eq!(
        wts[0].dirty, None,
        "the list never runs a dirty probe — dirtiness is unknown here"
    );
}

#[test]
fn cli_worktree_list_answers_probe_free() {
    assert_list_is_probe_free(&engine_cli());
}

#[test]
fn git2_worktree_list_answers_probe_free() {
    assert_list_is_probe_free(&engine_git2());
}

// ------------------------------------------------------ probe semantics --

/// Pin the dirty probe's semantics on independent fixtures (ticket 01): a
/// fresh checkout is clean; a modified tracked file is dirty; a non-ignored
/// untracked file is dirty; only ignored files present is clean; a
/// conflicted worktree is dirty.
fn assert_dirty_probe_semantics(engine: &dyn GitExecutor) {
    // Fresh checkout → clean.
    let (_tmp, repo) = temp_repo("probe-fresh");
    let wt = add_worktree(&repo, _tmp.path(), "wt-fresh", "feature");
    assert!(
        !engine.worktree_dirty(&wt).expect("probe fresh"),
        "a fresh checkout is clean"
    );

    // A modified tracked file → dirty.
    let (_tmp, repo) = temp_repo("probe-tracked");
    let wt = add_worktree(&repo, _tmp.path(), "wt-tracked", "feature");
    std::fs::write(wt.join("base.txt"), "changed\n").unwrap();
    assert!(
        engine.worktree_dirty(&wt).expect("probe tracked"),
        "a modified tracked file makes the worktree dirty"
    );

    // A non-ignored untracked file → dirty.
    let (_tmp, repo) = temp_repo("probe-untracked");
    let wt = add_worktree(&repo, _tmp.path(), "wt-untracked", "feature");
    std::fs::write(wt.join("new.txt"), "new\n").unwrap();
    assert!(
        engine.worktree_dirty(&wt).expect("probe untracked"),
        "a non-ignored untracked file makes the worktree dirty"
    );

    // Only ignored files present (a committed .gitignore excluding a build
    // dir) → clean.
    let (_tmp, repo) = temp_repo_ignored("probe-ignored");
    let wt = add_worktree(&repo, _tmp.path(), "wt-ignored", "feature");
    std::fs::create_dir_all(wt.join("build")).unwrap();
    std::fs::write(wt.join("build/out.txt"), "out\n").unwrap();
    assert!(
        !engine.worktree_dirty(&wt).expect("probe ignored"),
        "ignored-only files keep the worktree clean"
    );

    // A conflicted worktree → dirty. `feature` and `main` both edit the
    // same line of base.txt, so merging main into the worktree's feature
    // conflicts (the merge exits non-zero and leaves unmerged entries).
    let (_tmp, repo) = temp_repo("probe-conflict");
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(repo.join("base.txt"), "feature change\n").unwrap();
    git(&repo, &["commit", "-q", "-am", "feature edit"]);
    // Release `feature` from the main tree so the worktree can check it out.
    git(&repo, &["checkout", "-q", "main"]);
    let wt = _tmp.path().join("wt-conflict");
    git(&repo, &["worktree", "add", wt.to_str().unwrap(), "feature"]);
    let wt = wt.canonicalize().unwrap();
    std::fs::write(repo.join("base.txt"), "main change\n").unwrap();
    git(&repo, &["commit", "-q", "-am", "main edit"]);
    git_merge_that_must_conflict(&wt, &["merge", "--no-edit", "main"]);
    assert!(
        engine.worktree_dirty(&wt).expect("probe conflict"),
        "a conflicted worktree is dirty"
    );
}

#[test]
fn cli_dirty_probe_pins_semantics() {
    assert_dirty_probe_semantics(&engine_cli());
}

#[test]
fn git2_dirty_probe_pins_semantics() {
    assert_dirty_probe_semantics(&engine_git2());
}

// ------------------------------------------- parity, prunable, detached --

/// Both backends answer identically on the same fixture: the list (path +
/// branch + unknown dirty) and the per-worktree probe agree across the
/// adapter seam (ticket 01).
#[test]
fn adapters_answer_identically_on_shared_fixtures() {
    let (_tmp, repo) = temp_repo("parity");
    let fresh = add_worktree(&repo, _tmp.path(), "wt-fresh", "feature");
    let dirty = add_worktree(&repo, _tmp.path(), "wt-dirty", "other");
    std::fs::write(dirty.join("base.txt"), "changed\n").unwrap();

    let cli_list = engine_cli().worktree_list(&repo).expect("cli list");
    let git2_list = engine_git2().worktree_list(&repo).expect("git2 list");
    assert_eq!(cli_list.len(), git2_list.len(), "same entries listed");
    for (a, b) in cli_list.iter().zip(git2_list.iter()) {
        assert_eq!(a.path, b.path, "same path");
        assert_eq!(a.branch, b.branch, "same branch");
        assert_eq!(a.dirty, b.dirty, "both lists are equally probe-free");
    }

    for wt in [&fresh, &dirty] {
        let (c, g) = (
            engine_cli().worktree_dirty(wt).expect("cli probe"),
            engine_git2().worktree_dirty(wt).expect("git2 probe"),
        );
        assert_eq!(c, g, "backends agree on the dirty answer for {wt:?}");
    }
}

/// A prunable (deleted-directory) worktree stays surfaced in the list, with
/// its dirtiness unknown; its probe reads clean.
#[test]
fn prunable_worktrees_stay_listed_and_read_clean() {
    let (_tmp, repo) = temp_repo("prunable");
    let wt = add_worktree(&repo, _tmp.path(), "wt-prunable", "feature");
    std::fs::remove_dir_all(&wt).expect("remove worktree dir");

    let cli_list = engine_cli().worktree_list(&repo).expect("cli list");
    assert!(
        cli_list
            .iter()
            .any(|w| w.path.file_name().and_then(|s| s.to_str()) == Some("wt-prunable")),
        "the prunable worktree remains in the CLI list: {cli_list:?}"
    );
    assert!(
        cli_list.iter().all(|w| w.dirty.is_none()),
        "prunable entries list probe-free"
    );
    assert!(
        !engine_cli()
            .worktree_dirty(&wt)
            .expect("cli probe prunable"),
        "a prunable worktree reads clean"
    );

    let git2_list = engine_git2().worktree_list(&repo).expect("git2 list");
    assert!(
        git2_list.iter().any(|w| w.dirty.is_none()),
        "the prunable registration remains in the git2 list, probe-free"
    );
    let prunable_path = git2_list
        .iter()
        .find(|w| w.branch.is_empty())
        .map(|w| w.path.clone())
        .unwrap_or_else(|| wt.clone());
    assert!(
        !engine_git2()
            .worktree_dirty(&prunable_path)
            .expect("git2 probe prunable"),
        "a prunable worktree reads clean"
    );
}

/// Detached-HEAD worktrees list with no branch (unchanged from issue 14), and
/// their probe still answers.
#[test]
fn detached_head_worktrees_list_branchless_and_probe() {
    let (_tmp, repo) = temp_repo("detached");
    let head = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    let wt = _tmp.path().join("wt-detached");
    git(
        &repo,
        &["worktree", "add", "--detach", wt.to_str().unwrap(), &head],
    );
    let wt = wt.canonicalize().unwrap();
    std::fs::write(wt.join("base.txt"), "changed\n").unwrap();

    for engine in [&engine_cli() as &dyn GitExecutor, &engine_git2()] {
        let wts = engine.worktree_list(&repo).expect("list");
        assert_eq!(wts.len(), 1, "only the linked worktree is listed");
        assert_eq!(wts[0].branch, "", "detached HEAD reports no branch");
        assert!(
            engine.worktree_dirty(&wt).expect("probe"),
            "a modified detached worktree is dirty"
        );
    }
}
