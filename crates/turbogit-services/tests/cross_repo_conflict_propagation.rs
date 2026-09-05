//! Issue #23 — Cross-repo conflict propagation (screen 07 "SAME CONFLICT
//! ELSEWHERE" panel).
//!
//! Behavior under test, exercised through the public service seam
//! `turbogit_services::conflict_propagation`:
//!
//! - `find_matches` scans every other registered root's conflicted files and
//!   returns, per root, the paths whose conflict hunks are byte-identical
//!   to the source root's hunks (same `(ours, theirs)` pairs, in the same
//!   order, with the same non-conflict surroundings).
//! - `apply_to_matches` writes the source's resolution to each matched file
//!   in every root that has at least one full match, staging the resolved
//!   content; roots with no matching files are left completely untouched.
//! - Repos whose conflict hunks differ in any way are not touched, even when
//!   they share a path name with the source.
//!
//! All tests drive a real CLI engine against temporary repositories — same
//! integration style the rest of `turbogit-services`' cross-repo suites
//! use (`commit_across`, `cherry_across`). Assertions are on the on-disk
//! working tree + the index (`git status` / `git diff --cached`), which is
//! the observable contract a multi-root user cares about.

use std::path::{Path, PathBuf};

use turbogit_domain::model::{MultiRootManager, Root, RootId, RootStatus};
use turbogit_engine::build_executor;
use turbogit_engine_api::GitExecutor;
use turbogit_services::conflict_propagation::{apply_to_matches, find_matches};
/// Run `git` in `repo`, asserting success; returns stdout.
fn git(repo: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .expect("git must run");
    assert!(
        out.status.success(),
        "git {args:?} failed in {}: {}",
        repo.display(),
        String::from_utf8_lossy(&out.stderr),
    );
    String::from_utf8(out.stdout).expect("utf-8 stdout")
}

/// Run `git` without asserting success (a merge that conflicts exits non-zero).
fn git_unchecked(repo: &Path, args: &[&str]) {
    let _ = std::process::Command::new("git")
        .current_dir(repo)
        .args(args)
        .output();
}

struct Repo {
    path: PathBuf,
}

impl Repo {
    fn init(parent: &Path, name: &str) -> Self {
        let path = parent.join(name);
        std::fs::create_dir_all(&path).unwrap();
        git(&path, &["init", "-q", "-b", "main"]);
        git(&path, &["config", "user.email", "t@x"]);
        git(&path, &["config", "user.name", "t"]);
        Self { path }
    }

    fn write(&self, rel: &str, content: &str) {
        let p = self.path.join(rel);
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(p, content).unwrap();
    }
}

/// Snapshot a `Root` from the live engine (so `MultiRootManager` reflects
/// what the production app would see after `discover_roots` + `register_all`).
fn snapshot_root(executor: &dyn GitExecutor, path: &Path) -> Root {
    turbogit_services::multi_root::build_root(executor, path).expect("snapshot root")
}
/// Build a manager with `paths` registered as fresh roots.
fn manager_with(executor: &dyn GitExecutor, paths: &[PathBuf]) -> MultiRootManager {
    let mut mgr = MultiRootManager::default();
    for p in paths {
        mgr.register_root(snapshot_root(executor, p));
    }
    mgr
}

/// Register a path as a `Root` snapshot without snapshotting through the
/// engine (used for paths that are not git repositories on purpose, to
/// exercise per-root failure isolation).
fn manager_with_ghost(
    real_executor: &dyn GitExecutor,
    real_paths: &[PathBuf],
    ghost_paths: &[PathBuf],
) -> MultiRootManager {
    let mut mgr = manager_with(real_executor, real_paths);
    for ghost in ghost_paths {
        mgr.register_root(Root {
            id: RootId(ghost.clone().into()),
            path: ghost.clone(),
            remotes: Vec::new(),
            branches: Vec::new(),
            current_branch: None,
            head: None,
            status: RootStatus::default(),
        });
    }
    mgr
}

/// Seed a one-file conflict (`conf.txt`) and leave the merge mid-flight.
/// `main_content` is what HEAD on `main` writes; `side_content` is what
/// `side` writes. Seven unchanged lines between the edits keep git from
/// coalescing into a single conflict region if both branches diverge
/// identically.
fn seed_conflict(repo: &Repo, main_content: &str, side_content: &str) {
    let base = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n";
    repo.write("conf.txt", base);
    git(&repo.path, &["add", "conf.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "base"]);
    git(&repo.path, &["checkout", "-q", "-b", "side"]);
    repo.write("conf.txt", side_content);
    git(&repo.path, &["add", "conf.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "side"]);
    git(&repo.path, &["checkout", "-q", "main"]);
    repo.write("conf.txt", main_content);
    git(&repo.path, &["add", "conf.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "main"]);
    git_unchecked(&repo.path, &["merge", "--no-edit", "side"]);
}

/// The source's resolution for the two-hunk conflict: ours on hunk 1,
/// theirs on hunk 2. This is the resolution the propagation service must
/// replay verbatim into every matching target file.
fn source_resolutions() -> Vec<Option<u8>> {
    vec![Some(0), Some(1)] // (CHOICE_OURS, CHOICE_THEIRS)
}

/// The composed source content the service must replay into matching
/// targets. Computed from the same segments+resolutions the UI uses:
/// `MAIN-*` for hunk 1, `SIDE-*` for hunk 2.
fn composed_source() -> &'static str {
    "MAIN-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nSIDE-nine\nten\n"
}

// -------------------------------------------------------------- tests ----

/// `find_matches` returns the OTHER roots whose conflicted files contain
/// the source hunk; the source root itself is excluded.
#[test]
fn find_matches_lists_other_roots_with_the_identical_hunk() {
    let parent = tempfile::tempdir().unwrap();
    let src = Repo::init(parent.path(), "src");
    let peer_match = Repo::init(parent.path(), "peer_match");
    let peer_miss = Repo::init(parent.path(), "peer_miss");

    let main = "MAIN-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nMAIN-nine\nten\n";
    let side = "SIDE-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nSIDE-nine\nten\n";
    let other_side = "OTHER-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nOTHER-nine\nten\n";
    seed_conflict(&src, main, side);
    seed_conflict(&peer_match, main, side);
    seed_conflict(&peer_miss, main, other_side);

    let settings = turbogit_domain::model::VcsSettings::default();
    let executor = build_executor(&settings);
    let mgr = manager_with(
        executor.as_ref(),
        &[
            src.path.clone(),
            peer_match.path.clone(),
            peer_miss.path.clone(),
        ],
    );

    let matches = find_matches(
        executor.as_ref(),
        &mgr,
        &RootId(src.path.clone().into()),
        Path::new("conf.txt"),
        &source_resolutions(),
    );

    let matched_ids: Vec<&RootId> = matches.iter().map(|m| &m.root).collect();
    assert!(
        matched_ids.contains(&&RootId(peer_match.path.clone().into())),
        "peer_match must be listed as having the identical hunk; got {matched_ids:?}",
    );
    assert!(
        !matched_ids.contains(&&RootId(src.path.clone().into())),
        "the source root must be excluded from its own propagation results",
    );
    assert!(
        !matched_ids.contains(&&RootId(peer_miss.path.clone().into())),
        "peer_miss (different ours/theirs) must not be listed",
    );
    assert_eq!(
        matches.len(),
        1,
        "exactly one other root has the identical hunk; got {matches:?}",
    );
    assert_eq!(matches[0].paths, vec![PathBuf::from("conf.txt")]);
    assert_eq!(
        matches[0].matching_hunk_count, 2,
        "both source hunks match in peer_match",
    );
}

/// `apply_to_matches` writes the source's composed resolution into every
/// matched file in every matched root, stages the file, and reports per-root
/// success — all in one call.
#[test]
fn apply_to_matches_resolves_and_stages_every_matched_file() {
    let parent = tempfile::tempdir().unwrap();
    let src = Repo::init(parent.path(), "src");
    let peer_match = Repo::init(parent.path(), "peer_match");
    let peer_miss = Repo::init(parent.path(), "peer_miss");

    let main = "MAIN-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nMAIN-nine\nten\n";
    let side = "SIDE-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nSIDE-nine\nten\n";
    let other_side = "OTHER-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nOTHER-nine\nten\n";
    seed_conflict(&src, main, side);
    seed_conflict(&peer_match, main, side);
    seed_conflict(&peer_miss, main, other_side);

    let settings = turbogit_domain::model::VcsSettings::default();
    let executor = build_executor(&settings);
    let mgr = manager_with(
        executor.as_ref(),
        &[
            src.path.clone(),
            peer_match.path.clone(),
            peer_miss.path.clone(),
        ],
    );

    let outcomes = apply_to_matches(
        executor.as_ref(),
        &mgr,
        &RootId(src.path.clone().into()),
        Path::new("conf.txt"),
        &source_resolutions(),
    );

    // peer_match is the only resolved target — peer_miss is untouched.
    assert_eq!(
        outcomes.len(),
        1,
        "only peer_match gets propagated; outcomes = {outcomes:?}",
    );
    assert_eq!(outcomes[0].root, RootId(peer_match.path.clone().into()));
    assert_eq!(outcomes[0].path, PathBuf::from("conf.txt"));
    assert!(
        outcomes[0].result.is_ok(),
        "peer_match resolution must succeed; got error {:?}",
        outcomes[0].result,
    );

    // The on-disk file is exactly the source's composed resolution.
    let written = std::fs::read_to_string(peer_match.path.join("conf.txt")).unwrap();
    assert_eq!(
        written,
        composed_source(),
        "peer_match's conf.txt must equal the source's composed resolution",
    );

    // The resolved file is staged (no longer reported as conflicted).
    let status = executor.status(&peer_match.path).unwrap();
    assert!(
        !status.conflicted.contains(&PathBuf::from("conf.txt")),
        "conf.txt must be staged and out of the conflicted list; conflicted = {:?}",
        status.conflicted,
    );

    // peer_miss is untouched: same conflicted path, same on-disk content.
    let written_miss = std::fs::read_to_string(peer_miss.path.join("conf.txt")).unwrap();
    assert!(
        written_miss.contains("<<<<<<<"),
        "peer_miss must still carry conflict markers; got:\n{written_miss}",
    );
    let status_miss = executor.status(&peer_miss.path).unwrap();
    assert!(
        status_miss.conflicted.contains(&PathBuf::from("conf.txt")),
        "peer_miss must still be conflicted",
    );
}

/// A root that has one matching file and one non-matching file gets the
/// matching file resolved while the non-matching file is left exactly as-is
/// (no partial writes, no stage). This is the "Repos where the conflict
/// differs in any way are not touched" guarantee at the per-file level.
#[test]
fn root_with_one_matching_and_one_different_file_is_only_partially_touched() {
    let parent = tempfile::tempdir().unwrap();
    let src = Repo::init(parent.path(), "src");
    let peer = Repo::init(parent.path(), "peer");

    // Base: both files exist on both branches with no edits.
    let conf_base = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n";
    let conf2_base = "alpha\nbeta\ngamma\ndelta\nepsilon\nzeta\neta\ntheta\niota\nkappa\n";
    for repo in [&src, &peer] {
        repo.write("conf.txt", conf_base);
        repo.write("conf2.txt", conf2_base);
        git(&repo.path, &["add", "."]);
        git(&repo.path, &["commit", "-q", "-m", "base"]);

        git(&repo.path, &["checkout", "-q", "-b", "side"]);
        repo.write(
            "conf.txt",
            "SIDE-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nSIDE-nine\nten\n",
        );
        repo.write(
            "conf2.txt",
            "SIDE-alpha\nbeta\ngamma\ndelta\nepsilon\nzeta\neta\ntheta\nSIDE-iota\nkappa\n",
        );
        git(&repo.path, &["add", "."]);
        git(&repo.path, &["commit", "-q", "-m", "side"]);
        git(&repo.path, &["checkout", "-q", "main"]);
        repo.write(
            "conf.txt",
            "MAIN-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nMAIN-nine\nten\n",
        );
        repo.write(
            "conf2.txt",
            "MAIN-alpha\nbeta\ngamma\ndelta\nepsilon\nzeta\neta\ntheta\nMAIN-iota\nkappa\n",
        );
        git(&repo.path, &["add", "."]);
        git(&repo.path, &["commit", "-q", "-m", "main"]);
        git_unchecked(&repo.path, &["merge", "--no-edit", "side"]);
    }

    let settings = turbogit_domain::model::VcsSettings::default();
    let executor = build_executor(&settings);
    let mgr = manager_with(executor.as_ref(), &[src.path.clone(), peer.path.clone()]);

    let outcomes = apply_to_matches(
        executor.as_ref(),
        &mgr,
        &RootId(src.path.clone().into()),
        Path::new("conf.txt"),
        &source_resolutions(),
    );

    assert_eq!(
        outcomes.len(),
        1,
        "exactly one root (peer) has matching conflicts; outcomes = {outcomes:?}",
    );
    assert!(outcomes[0].result.is_ok(), "peer resolution must succeed");

    // conf.txt is resolved; conf2.txt (different hunks) is untouched.
    let conf1 = std::fs::read_to_string(peer.path.join("conf.txt")).unwrap();
    assert_eq!(
        conf1,
        composed_source(),
        "conf.txt gets the source resolution"
    );
    let conf2 = std::fs::read_to_string(peer.path.join("conf2.txt")).unwrap();
    assert!(
        conf2.contains("<<<<<<<"),
        "conf2.txt must keep its conflict markers — its hunks don't match the source; got:\n{conf2}",
    );

    // The index reflects the partial apply: conf.txt staged, conf2.txt
    // still conflicted.
    let status = executor.status(&peer.path).unwrap();
    assert!(
        !status.conflicted.contains(&PathBuf::from("conf.txt")),
        "conf.txt must be staged after propagation",
    );
    assert!(
        status.conflicted.contains(&PathBuf::from("conf2.txt")),
        "conf2.txt must still be conflicted (its hunks differ)",
    );
}

/// When no other root has matching hunks, `apply_to_matches` is a no-op:
/// it returns zero outcomes and mutates nothing — neither stages nor writes
/// — anywhere outside the source root.
#[test]
fn apply_to_matches_is_a_noop_when_no_other_root_has_matching_hunks() {
    let parent = tempfile::tempdir().unwrap();
    let src = Repo::init(parent.path(), "src");
    let peer_miss = Repo::init(parent.path(), "peer_miss");

    let main = "MAIN-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nMAIN-nine\nten\n";
    let side = "SIDE-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nSIDE-nine\nten\n";
    let other_side = "OTHER-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nOTHER-nine\nten\n";
    seed_conflict(&src, main, side);
    seed_conflict(&peer_miss, main, other_side);

    let settings = turbogit_domain::model::VcsSettings::default();
    let executor = build_executor(&settings);
    let mgr = manager_with(
        executor.as_ref(),
        &[src.path.clone(), peer_miss.path.clone()],
    );

    let outcomes = apply_to_matches(
        executor.as_ref(),
        &mgr,
        &RootId(src.path.clone().into()),
        Path::new("conf.txt"),
        &source_resolutions(),
    );
    assert!(
        outcomes.is_empty(),
        "no matching hunks anywhere → no propagation; got {outcomes:?}",
    );

    // Source untouched: still conflicted, original markers still present.
    let written_src = std::fs::read_to_string(src.path.join("conf.txt")).unwrap();
    assert!(
        written_src.contains("<<<<<<<"),
        "source must remain conflicted; got:\n{written_src}",
    );
    let status_src = executor.status(&src.path).unwrap();
    assert!(
        status_src.conflicted.contains(&PathBuf::from("conf.txt")),
        "source must remain conflicted",
    );

    // The miss root is also untouched.
    let written_miss = std::fs::read_to_string(peer_miss.path.join("conf.txt")).unwrap();
    assert!(
        written_miss.contains("<<<<<<<"),
        "peer_miss must remain conflicted",
    );
}

/// Per-repo failure is isolated: when one target root's resolution fails,
/// other targets still succeed and the failure is reported in the per-root
/// outcome. We simulate the failure by registering a "ghost" root — a path
/// that exists on disk but was never `git init`-ed — so any git operation
/// on it surfaces as a real engine error.
#[test]
fn per_root_failures_are_isolated_and_reported() {
    let parent = tempfile::tempdir().unwrap();
    let src = Repo::init(parent.path(), "src");
    let ok = Repo::init(parent.path(), "ok");
    let ghost_dir = parent.path().join("ghost");
    std::fs::create_dir_all(&ghost_dir).unwrap();
    let main = "MAIN-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nMAIN-nine\nten\n";
    let side = "SIDE-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nSIDE-nine\nten\n";
    seed_conflict(&src, main, side);
    seed_conflict(&ok, main, side);
    let settings = turbogit_domain::model::VcsSettings::default();
    let executor = build_executor(&settings);
    let mgr = manager_with_ghost(
        executor.as_ref(),
        &[src.path.clone(), ok.path.clone()],
        std::slice::from_ref(&ghost_dir),
    );

    let outcomes = apply_to_matches(
        executor.as_ref(),
        &mgr,
        &RootId(src.path.clone().into()),
        Path::new("conf.txt"),
        &source_resolutions(),
    );

    let ok_outcome = outcomes
        .iter()
        .find(|o| o.root == RootId(ok.path.clone().into()))
        .expect("ok is a matched root and must appear in outcomes");
    let ghost_outcome = outcomes
        .iter()
        .find(|o| o.root == RootId(ghost_dir.clone().into()))
        .expect("ghost root is in the manager and must surface in outcomes");
    assert!(
        ok_outcome.result.is_ok(),
        "ok's resolution must succeed independently of the ghost root's failure",
    );
    assert!(
        ghost_outcome.result.is_err(),
        "ghost root (no git repo) must surface as a failure",
    );

    // ok actually resolved: file contents match the source's resolution.
    let ok_conf = std::fs::read_to_string(ok.path.join("conf.txt")).unwrap();
    assert_eq!(
        ok_conf,
        composed_source(),
        "ok must carry the source's composed resolution after propagation",
    );
}

/// Sanity: a single-root workspace has nothing to propagate to — the source
/// is the only root, so `find_matches` is empty and `apply_to_matches` is a
/// no-op (and the source itself is never touched).
#[test]
fn single_root_workspace_has_nothing_to_propagate_to() {
    let parent = tempfile::tempdir().unwrap();
    let src = Repo::init(parent.path(), "src");
    let main = "MAIN-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nMAIN-nine\nten\n";
    let side = "SIDE-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nSIDE-nine\nten\n";
    seed_conflict(&src, main, side);

    let settings = turbogit_domain::model::VcsSettings::default();
    let executor = build_executor(&settings);
    let mgr = manager_with(executor.as_ref(), std::slice::from_ref(&src.path));

    let matches = find_matches(
        executor.as_ref(),
        &mgr,
        &RootId(src.path.clone().into()),
        Path::new("conf.txt"),
        &source_resolutions(),
    );
    assert!(
        matches.is_empty(),
        "single-root workspace → no propagation targets; got {matches:?}",
    );

    let outcomes = apply_to_matches(
        executor.as_ref(),
        &mgr,
        &RootId(src.path.clone().into()),
        Path::new("conf.txt"),
        &source_resolutions(),
    );
    assert!(
        outcomes.is_empty(),
        "single-root workspace → no apply attempts; got {outcomes:?}",
    );
}
