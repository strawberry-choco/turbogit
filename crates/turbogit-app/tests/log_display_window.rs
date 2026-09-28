//! The log pane's held display window at the app seam (log-view-scaling 04):
//! one derived value per change to the loaded window or the UI's inputs, and
//! the same value across the frames in between.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use turbogit_app::root_caches::LogScope;
use turbogit_app::state::AppState;
use turbogit_domain::model::{Commit, CommitId, RootId, Signature, SignatureState};

fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("git must be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Two one-commit repositories, seeded once for the whole test binary. The
/// history itself is settled into the caches by hand below — what is under test
/// is the seam between the caches and the derived window, not git's own log.
fn two_root_project() -> &'static (PathBuf, Vec<PathBuf>) {
    static PROJECT: OnceLock<(PathBuf, Vec<PathBuf>)> = OnceLock::new();
    PROJECT.get_or_init(|| {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("project");
        let mut repos = Vec::new();
        for name in ["alpha", "beta"] {
            let repo = project.join(name);
            std::fs::create_dir_all(&repo).unwrap();
            git(&repo, &["init", "-q", "-b", "main"]);
            git(&repo, &["config", "user.email", "t@t"]);
            git(&repo, &["config", "user.name", "t"]);
            git(&repo, &["commit", "-q", "--allow-empty", "-m", "seed"]);
            repos.push(repo);
        }
        // The temp dir outlives every test in this binary.
        std::mem::forget(dir);
        (project, repos)
    })
}

fn state_over_two_roots() -> (AppState, RootId, RootId) {
    let (project, repos) = two_root_project();
    let state = AppState::for_roots(project, repos);
    let roots: Vec<RootId> = state.multi.roots.iter().map(|r| r.id.clone()).collect();
    assert_eq!(roots.len(), 2, "two roots registered");
    let (alpha, beta) = (roots[0].clone(), roots[1].clone());
    (state, alpha, beta)
}

fn commit(root: &RootId, id: &str, time: i64) -> Commit {
    Commit {
        id: id.to_owned(),
        parents: vec![],
        author: Signature {
            name: format!("author-{id}"),
            email: "t@t".to_owned(),
            time,
        },
        committer: Signature {
            name: "t".to_owned(),
            email: "t@t".to_owned(),
            time,
        },
        message: format!("message {id}"),
        time,
        root: root.clone(),
        signature: SignatureState::Unsigned,
    }
}

fn ids(window: &turbogit_app::log_display::LogDisplay) -> Vec<CommitId> {
    window.rows().iter().map(|c| c.id.clone()).collect()
}

fn ids_at(window: &turbogit_app::log_display::LogDisplay, index: usize) -> Option<CommitId> {
    window.row(index).map(|c| c.id.clone())
}

/// With nothing changing, consecutive frames read the SAME derived value: the
/// second frame is handed the very allocation the first was, which is only
/// possible if it was not rebuilt. No counter, no instrumentation — the handle
/// is the evidence.
#[test]
fn an_idle_frame_reads_the_same_window_the_frame_before_it_built() {
    let (mut state, alpha, _) = state_over_two_roots();
    state
        .caches
        .store_log(alpha.clone(), vec![commit(&alpha, "a1", 100)]);
    let first = state.sync_log_display();
    for _ in 0..5 {
        let next = state.sync_log_display();
        assert!(
            Arc::ptr_eq(&first, &next),
            "an idle frame rebuilt the displayed list and the lane colours"
        );
    }
    // Nothing was dropped from the caches either — the same answer, twice.
    assert_eq!(first.len(), 1);
    assert_eq!(state.caches.log(&alpha).map(<[Commit]>::len), Some(1));
}

/// The teeth of the promise above: a batch appended IS a change, and the frame
/// after it must be handed a different window.
#[test]
fn a_batch_appended_rebuilds_the_displayed_list() {
    let (mut state, alpha, _) = state_over_two_roots();
    state
        .caches
        .store_log(alpha.clone(), vec![commit(&alpha, "a1", 100)]);
    let first = state.sync_log_display();
    assert_eq!(first.len(), 1);

    state
        .caches
        .append_log(alpha.clone(), vec![commit(&alpha, "a2", 50)]);
    let second = state.sync_log_display();
    assert!(
        !Arc::ptr_eq(&first, &second),
        "an appended batch must rebuild the window"
    );
    assert_eq!(ids(&second), ["a1", "a2"], "newest first, the batch added");
}

/// The roots filter narrowing and widening are two directions of one change:
/// each rebuilds, and the union follows the filter.
#[test]
fn the_roots_filter_narrowing_and_widening_rebuilds() {
    let (mut state, alpha, beta) = state_over_two_roots();
    state
        .caches
        .store_log(alpha.clone(), vec![commit(&alpha, "a1", 100)]);
    state
        .caches
        .store_log(beta.clone(), vec![commit(&beta, "b1", 90)]);

    let both = state.sync_log_display();
    assert_eq!(ids(&both), ["a1", "b1"], "the union is newest first");

    state.ui.log_root_filter = Some(beta.clone());
    let narrowed = state.sync_log_display();
    assert!(!Arc::ptr_eq(&both, &narrowed));
    assert_eq!(ids(&narrowed), ["b1"], "narrowed to the one root");

    state.ui.log_root_filter = None;
    let widened = state.sync_log_display();
    assert!(!Arc::ptr_eq(&narrowed, &widened));
    assert_eq!(ids(&widened), ["a1", "b1"], "widened back to the union");
}

/// A scope change reads a different listing, so it rebuilds — and the scoped
/// listing replaces the unscoped one rather than joining it.
#[test]
fn a_scope_change_rebuilds_from_the_scoped_listing() {
    let (mut state, alpha, _) = state_over_two_roots();
    state
        .caches
        .store_log(alpha.clone(), vec![commit(&alpha, "a1", 100)]);
    let unscoped = state.sync_log_display();
    assert_eq!(ids(&unscoped), ["a1"]);

    let path = PathBuf::from("f.txt");
    state.caches.settle_scoped_log_batch(
        &alpha,
        &LogScope::Path(path.clone()),
        turbogit_app::events::LogBatchMode::Replace,
        vec![commit(&alpha, "s1", 100)],
    );
    state.ui.log_path_scope = Some(path);
    let scoped = state.sync_log_display();
    assert!(!Arc::ptr_eq(&unscoped, &scoped), "a scope must rebuild");
    assert_eq!(
        ids(&scoped),
        ["s1"],
        "the scoped listing replaces the whole"
    );

    state.ui.log_path_scope = None;
    let back = state.sync_log_display();
    assert_eq!(ids(&back), ["a1"], "leaving the scope restores the listing");
}

/// The live search term rebuilds, and the pickaxe union it reads comes out of
/// the search cache — a commit whose content changed the query's count shows
/// even when its message, hash and author do not match.
#[test]
fn a_search_term_rebuilds_and_unions_the_pickaxe_hits() {
    let (mut state, alpha, _) = state_over_two_roots();
    state
        .caches
        .store_log(alpha.clone(), vec![commit(&alpha, "a1", 100)]);
    let before = state.sync_log_display();

    state.ui.log_filter = "needle".to_owned();
    let filtered = state.sync_log_display();
    assert!(!Arc::ptr_eq(&before, &filtered), "typing must rebuild");
    assert_eq!(filtered.len(), 0, "nothing matches the term yet");

    // The pickaxe cache answers: the hit is a commit the term does not match
    // textually, which is the whole point of the union.
    state.caches.settle_scoped_log_batch(
        &alpha,
        &LogScope::Search("needle".to_owned()),
        turbogit_app::events::LogBatchMode::Replace,
        vec![commit(&alpha, "hit", 60)],
    );
    let with_hits = state.sync_log_display();
    assert_eq!(ids(&with_hits), ["hit"], "the pickaxe hit is unioned in");
    assert_eq!(
        ids_at(&with_hits, 0).as_deref(),
        Some("hit"),
        "a hit is a row the list can index"
    );
}

/// The shown count is the whole loaded window's, not a slice of it: the log
/// pane's "N shown" line reads exactly this number.
#[test]
fn the_shown_count_is_the_whole_loaded_window() {
    let (mut state, alpha, _) = state_over_two_roots();
    let held: Vec<Commit> = (0..3)
        .map(|i| commit(&alpha, &format!("a{i}"), 100 - i))
        .collect();
    state.caches.store_log(alpha.clone(), held);
    let window = state.sync_log_display();
    assert_eq!(
        window.len(),
        state.caches.log(&alpha).map(<[Commit]>::len).unwrap(),
        "the displayed list is the whole loaded window"
    );
    assert_eq!(window.len(), 3);
}

/// Selecting a row changes nothing the window holds, so it must not rebuild
/// one: the scope's repository is the only place the selection is read from.
#[test]
fn selecting_a_row_does_not_rebuild_the_window() {
    let (mut state, alpha, beta) = state_over_two_roots();
    state
        .caches
        .store_log(alpha.clone(), vec![commit(&alpha, "a1", 100)]);
    state.selected_root = Some(alpha.clone());
    let before = state.sync_log_display();

    state.ui.selected_commit = Some("a1".to_owned());
    let after = state.sync_log_display();
    assert!(
        Arc::ptr_eq(&before, &after),
        "a selection is not a change to the displayed list"
    );

    // Inside a path scope the selection IS what says whose history to read.
    let path = PathBuf::from("f.txt");
    state.caches.settle_scoped_log_batch(
        &beta,
        &LogScope::Path(path.clone()),
        turbogit_app::events::LogBatchMode::Replace,
        vec![commit(&beta, "b1", 100)],
    );
    state.ui.log_path_scope = Some(path);
    state.selected_root = Some(alpha.clone());
    state.sync_log_display();
    state.selected_root = Some(beta.clone());
    let rescoped = state.sync_log_display();
    assert_eq!(
        ids(&rescoped),
        ["b1"],
        "the scoped listing is read for the selected repository"
    );
}
