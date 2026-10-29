use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;
use test_support::RecordingExecutor;
use test_support::git_seed::{git, pin_eol_lf};
use turbogit_app::root_caches::{Affected, file_stat};
use turbogit_app::state::AppState;
use turbogit_domain::model::{RootId, VcsSettings};
use turbogit_engine::cli::CliExecutor;

fn seeded_root() -> (TempDir, RootId, PathBuf, String) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("stats");
    std::fs::create_dir_all(&repo).expect("repo dir");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "cache@example.com"]);
    git(&repo, &["config", "user.name", "Cache Author"]);
    pin_eol_lf(&repo);
    std::fs::write(repo.join("a.txt"), "1\n2\n3\n").expect("write a");
    std::fs::write(repo.join("b.txt"), "gone\n").expect("write b");
    git(&repo, &["add", "--", "."]);
    git(&repo, &["commit", "-q", "-m", "base"]);
    std::fs::write(repo.join("a.txt"), "1\n2\nX\nY\n").expect("edit a");
    std::fs::remove_file(repo.join("b.txt")).expect("remove b");
    git(&repo, &["add", "--", "."]);
    git(&repo, &["commit", "-q", "-m", "edit and delete"]);
    let head = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    (tmp, RootId(repo.clone().into()), repo, head)
}

#[test]
fn file_stats_are_requested_once_per_commit_and_served_from_the_cache() {
    let (_tmp, root, repo, head) = seeded_root();
    let recorder = Arc::new(RecordingExecutor::new(Arc::new(CliExecutor {
        settings: VcsSettings::default(),
    })));
    let mut state =
        AppState::for_roots(&repo, std::slice::from_ref(&repo)).with_executor(recorder.clone());

    state.fetch_file_stats(root.clone(), head.clone());
    state.fetch_file_stats(root.clone(), head.clone());
    state.fetch_file_stats(root.clone(), head.clone());
    state.drain_events();
    assert!(
        state.caches.file_stats_loaded(&root, &head),
        "the line counts settled into the cache"
    );
    assert_eq!(
        recorder.stats_call_count(),
        1,
        "repeated same-frame requests must produce one commit_file_stats call"
    );

    let counts = state
        .caches
        .file_stats_for(&root, &head)
        .expect("cached stats");
    let total_added: usize = counts.iter().filter_map(|f| f.insertions).sum();
    let total_removed: usize = counts.iter().filter_map(|f| f.deletions).sum();
    assert_eq!((total_added, total_removed), (2, 2), "churn totals");
    assert_eq!(file_stat(counts, Path::new("a.txt")), Some((2, 1)));
    assert_eq!(file_stat(counts, Path::new("b.txt")), Some((0, 1)));
    assert_eq!(file_stat(counts, Path::new("c.txt")), None);

    state.fetch_file_stats(root.clone(), head.clone());
    state.drain_events();
    assert_eq!(
        recorder.stats_call_count(),
        1,
        "a settled commit must never be refetched"
    );
}

#[test]
fn invalidating_a_root_drops_its_cached_file_stats() {
    let (_tmp, root, repo, head) = seeded_root();
    let recorder = Arc::new(RecordingExecutor::new(Arc::new(CliExecutor {
        settings: VcsSettings::default(),
    })));
    let mut state =
        AppState::for_roots(&repo, std::slice::from_ref(&repo)).with_executor(recorder.clone());

    state.fetch_file_stats(root.clone(), head.clone());
    state.drain_events();
    assert!(
        state.caches.file_stats_loaded(&root, &head),
        "the line counts settled into the cache"
    );

    state.caches.invalidate(&Affected::Root(root.clone()));
    assert!(
        !state.caches.file_stats_loaded(&root, &head),
        "invalidation must drop the stats with every other per-commit cache"
    );

    state.fetch_file_stats(root.clone(), head);
    state.drain_events();
    assert_eq!(recorder.stats_call_count(), 2, "the refetch ran again");
}
