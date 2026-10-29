use std::path::{Path, PathBuf};

use test_support::git_seed::git;
use turbogit_domain::model::{MultiRootManager, Root, RootId, RootStatus};
use turbogit_engine::build_executor;
use turbogit_engine_api::GitExecutor;
use turbogit_services::conflict::{CHOICE_OURS, CHOICE_THEIRS};
use turbogit_services::conflict_propagation::{apply_to_matches, find_matches};
use turbogit_services::multi_root::register;

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

fn snapshot_root(executor: &dyn GitExecutor, path: &Path) -> Root {
    turbogit_services::multi_root::build_root(executor, path).expect("snapshot root")
}
fn manager_with(executor: &dyn GitExecutor, paths: &[PathBuf]) -> MultiRootManager {
    let mut mgr = MultiRootManager::default();
    for p in paths {
        register(&mut mgr, snapshot_root(executor, p));
    }
    mgr
}

fn manager_with_ghost(
    real_executor: &dyn GitExecutor,
    real_paths: &[PathBuf],
    ghost_paths: &[PathBuf],
) -> MultiRootManager {
    let mut mgr = manager_with(real_executor, real_paths);
    for ghost in ghost_paths {
        register(
            &mut mgr,
            Root {
                id: RootId(ghost.clone().into()),
                path: ghost.clone(),
                remotes: Vec::new(),
                branches: Vec::new(),
                current_branch: None,
                head: None,
                status: RootStatus::default(),
            },
        );
    }
    assert_eq!(
        mgr.roots.len(),
        real_paths.len() + ghost_paths.len(),
        "every ghost must land in the manager alongside the real roots"
    );
    mgr
}

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

fn source_resolutions() -> Vec<Option<u8>> {
    vec![Some(CHOICE_OURS), Some(CHOICE_THEIRS)]
}

fn composed_source() -> &'static str {
    "MAIN-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nSIDE-nine\nten\n"
}

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

    let written = std::fs::read_to_string(peer_match.path.join("conf.txt")).unwrap();
    assert_eq!(
        written,
        composed_source(),
        "peer_match's conf.txt must equal the source's composed resolution",
    );

    let status = executor.status(&peer_match.path).unwrap();
    assert!(
        !status.conflicted.contains(&PathBuf::from("conf.txt")),
        "conf.txt must be staged and out of the conflicted list; conflicted = {:?}",
        status.conflicted,
    );

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

#[test]
fn root_with_one_matching_and_one_different_file_is_only_partially_touched() {
    let parent = tempfile::tempdir().unwrap();
    let src = Repo::init(parent.path(), "src");
    let peer = Repo::init(parent.path(), "peer");

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

    let written_miss = std::fs::read_to_string(peer_miss.path.join("conf.txt")).unwrap();
    assert!(
        written_miss.contains("<<<<<<<"),
        "peer_miss must remain conflicted",
    );
}

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

    let ok_conf = std::fs::read_to_string(ok.path.join("conf.txt")).unwrap();
    assert_eq!(
        ok_conf,
        composed_source(),
        "ok must carry the source's composed resolution after propagation",
    );
}

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
