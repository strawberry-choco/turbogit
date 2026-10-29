use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use test_support::git_seed::git;
use turbogit_app::root_caches::LogScope;
use turbogit_app::state::AppState;
use turbogit_domain::model::{Commit, CommitId, RootId, Signature, SignatureState};

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
    assert_eq!(first.len(), 1);
    assert_eq!(state.caches.log(&alpha).map(<[Commit]>::len), Some(1));
}

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
