use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;
use test_support::RecordingExecutor;
use test_support::git_seed::git;
use turbogit_app::events::{AppEvent, LogBatchMode};
use turbogit_app::operation::OpKind;
use turbogit_app::root_caches::{Affected, LogScope, RootCaches};
use turbogit_app::shell_writes::tag as tag_writes;
use turbogit_app::state::AppState;
use turbogit_domain::model::{
    Commit, LogOpts, RecentCheckout, RefState, RootId, Signature, SignatureState, Stash, TagSpec,
    VcsSettings,
};
use turbogit_engine::GitExecutor;
use turbogit_engine::cli::CliExecutor;

fn seed_repo(dir: &Path, name: &str) {
    std::fs::create_dir_all(dir).expect("repo dir");
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "user.name", "Test"]);
    test_support::git_seed::commit(
        dir,
        "file.txt",
        &format!("{name}: v1\n"),
        &format!("{name}: initial"),
    );
}

fn head_commit(dir: &Path) -> String {
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

fn fake_commit(root: &RootId, message: &str) -> Commit {
    let sig = Signature {
        name: "Fake".to_string(),
        email: "fake@example.com".to_string(),
        time: 0,
    };
    Commit {
        id: format!("{}-fake", message.replace(' ', "-")),
        parents: Vec::new(),
        author: sig.clone(),
        committer: sig,
        message: message.to_string(),
        time: 0,
        root: root.clone(),
        signature: SignatureState::Unsigned,
    }
}

fn prime_fake_entries(state: &mut AppState, roots: &[RootId]) {
    for root in roots {
        state
            .tx
            .send(AppEvent::LogLoaded {
                root: root.clone(),
                commits: Ok(vec![fake_commit(root, "fake: untouched")]),
                mode: LogBatchMode::Replace,
            })
            .expect("send LogLoaded");
        state
            .tx
            .send(AppEvent::AheadBehind {
                root: root.clone(),
                ahead: 9,
                behind: 9,
            })
            .expect("send AheadBehind");
    }
    state.drain_events();
}

fn prime_engine_backed_entries(state: &mut AppState, root_dir: &Path) {
    let exec = CliExecutor {
        settings: VcsSettings::default(),
    };
    let root = RootId(root_dir.to_path_buf().into());
    let deco = exec.ref_decorations(&root.0).expect("decorations");
    state
        .tx
        .send(AppEvent::RefsLoaded {
            root: root.clone(),
            deco: Ok(deco),
        })
        .expect("send RefsLoaded");
    state.ensure_files(&root, &head_commit(root_dir));
    state.ensure_log(&root, LogScope::Path(Path::new("file.txt").to_path_buf()));
    state.drain_events();
}

fn engine_log(dir: &Path) -> Vec<Commit> {
    CliExecutor {
        settings: VcsSettings::default(),
    }
    .log(dir, &LogOpts::default())
    .expect("engine log")
}

fn first_message(state: &AppState, root: &RootId) -> Option<String> {
    state
        .caches
        .log(root)
        .and_then(|c| c.first().map(|c| c.message.clone()))
}

struct Project {
    _tmp: TempDir,
    dir: PathBuf,
    alpha: PathBuf,
    beta: PathBuf,
}

fn two_root_project() -> Project {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("project");
    let alpha = dir.join("alpha");
    let beta = dir.join("beta");
    seed_repo(&alpha, "alpha");
    seed_repo(&beta, "beta");
    Project {
        _tmp: tmp,
        dir,
        alpha,
        beta,
    }
}

#[test]
fn project_switch_leaves_no_cache_entries_stale() {
    let first = two_root_project();
    let second = two_root_project();
    let cfg = tempfile::tempdir().expect("config tempdir");

    let mut state = AppState::for_roots(&first.dir, std::slice::from_ref(&first.alpha));
    state.recents_config_dir = Some(cfg.path().to_path_buf());
    prime_fake_entries(&mut state, &[RootId(first.alpha.clone().into())]);
    prime_engine_backed_entries(&mut state, &first.alpha);
    assert!(
        !state.caches.is_empty(),
        "priming must populate all five maps"
    );

    state.open_project(&second.dir);

    assert!(
        state.caches.is_empty(),
        "project switch must leave nothing stale"
    );
}

#[test]
fn scoped_op_completion_keeps_unaffected_roots_cached() {
    let p = two_root_project();
    let alpha_id = RootId(p.alpha.clone().into());
    let beta_id = RootId(p.beta.clone().into());

    let mut state = AppState::for_roots(&p.dir, &[p.alpha.clone(), p.beta.clone()]);
    prime_fake_entries(&mut state, &[alpha_id.clone(), beta_id.clone()]);

    state
        .tx
        .send(AppEvent::OpCompleted {
            kind: OpKind::Other,
            label: "op".to_string(),
            affected: Affected::Root(alpha_id.clone()),
            result: Ok(()),
        })
        .expect("send OpCompleted");
    state.drain_events();

    let expected = engine_log(&p.alpha);
    assert!(!expected.is_empty(), "seeded repo must have commits");
    assert_eq!(
        state.caches.log(&alpha_id),
        Some(expected.as_slice()),
        "affected selected root must be refetched from git"
    );
    assert_eq!(state.caches.ahead_behind(&alpha_id), Some((0, 0)));

    assert_eq!(
        first_message(&state, &beta_id),
        Some("fake: untouched".to_string()),
        "unaffected root's log must survive a scoped op"
    );
    assert_eq!(
        state.caches.ahead_behind(&beta_id),
        Some((9, 9)),
        "unaffected root's ahead/behind must survive a scoped op"
    );
}

fn stash_entry(root: &RootId, message: &str) -> Stash {
    Stash {
        message: message.to_string(),
        root: root.clone(),
        index: 0,
    }
}

#[test]
fn a_stash_listing_is_invalidated_with_its_own_root_and_no_further() {
    let p = two_root_project();
    let alpha_id = RootId(p.alpha.clone().into());
    let beta_id = RootId(p.beta.clone().into());

    let mut state = AppState::for_roots(&p.dir, &[p.alpha.clone(), p.beta.clone()]);
    state
        .caches
        .store_stashes(alpha_id.clone(), vec![stash_entry(&alpha_id, "alpha work")]);
    state
        .caches
        .store_stashes(beta_id.clone(), vec![stash_entry(&beta_id, "beta work")]);

    state
        .tx
        .send(AppEvent::OpCompleted {
            kind: OpKind::Other,
            label: "op".to_string(),
            affected: Affected::Root(alpha_id.clone()),
            result: Ok(()),
        })
        .expect("send OpCompleted");
    state.drain_events();

    assert!(
        state.caches.stashes(&alpha_id).is_none(),
        "the root the op named loses its listing"
    );
    assert_eq!(
        state.caches.stashes(&beta_id).map(|rows| rows.len()),
        Some(1),
        "and the root the op never named keeps its own"
    );

    state
        .caches
        .store_stashes(alpha_id.clone(), vec![stash_entry(&alpha_id, "alpha work")]);
    state
        .caches
        .store_stashes(beta_id.clone(), vec![stash_entry(&beta_id, "beta work")]);
    state
        .tx
        .send(AppEvent::OpCompleted {
            kind: OpKind::Other,
            label: "switch".to_string(),
            affected: Affected::All,
            result: Ok(()),
        })
        .expect("send OpCompleted");
    state.drain_events();
    assert!(
        state.caches.stashes(&alpha_id).is_none() && state.caches.stashes(&beta_id).is_none(),
        "every root's listing goes with a project-wide invalidation"
    );
}

fn recent(name: &str) -> RecentCheckout {
    RecentCheckout {
        name: name.to_string(),
        at: 0,
    }
}

#[test]
fn a_tag_list_and_checkout_history_are_invalidated_with_their_own_root_and_no_further() {
    let p = two_root_project();
    let alpha_id = RootId(p.alpha.clone().into());
    let beta_id = RootId(p.beta.clone().into());

    let mut state = AppState::for_roots(&p.dir, &[p.alpha.clone(), p.beta.clone()]);
    state.caches.store_tags(
        alpha_id.clone(),
        vec![("alpha-v1".to_string(), RefState::Default)],
    );
    state
        .caches
        .store_recents(alpha_id.clone(), vec![recent("alpha-move")]);
    state.caches.store_tags(
        beta_id.clone(),
        vec![("beta-v1".to_string(), RefState::Default)],
    );
    state
        .caches
        .store_recents(beta_id.clone(), vec![recent("beta-move")]);

    state
        .tx
        .send(AppEvent::OpCompleted {
            kind: OpKind::Other,
            label: "op".to_string(),
            affected: Affected::Root(alpha_id.clone()),
            result: Ok(()),
        })
        .expect("send OpCompleted");
    state.drain_events();

    assert!(
        state.caches.tags(&alpha_id).is_none() && state.caches.recents(&alpha_id).is_none(),
        "the root the op named loses its tag list and its checkout history"
    );
    assert_eq!(
        state.caches.tags(&beta_id).map(|t| t.len()),
        Some(1),
        "and the root the op never named keeps its own tag list"
    );
    assert_eq!(
        state.caches.recents(&beta_id).map(|r| r.len()),
        Some(1),
        "…and its own checkout history"
    );

    state.caches.store_tags(
        alpha_id.clone(),
        vec![("alpha-v1".to_string(), RefState::Default)],
    );
    state
        .caches
        .store_recents(alpha_id.clone(), vec![recent("alpha-move")]);
    state.caches.store_tags(
        beta_id.clone(),
        vec![("beta-v1".to_string(), RefState::Default)],
    );
    state
        .caches
        .store_recents(beta_id.clone(), vec![recent("beta-move")]);
    state
        .tx
        .send(AppEvent::OpCompleted {
            kind: OpKind::Other,
            label: "switch".to_string(),
            affected: Affected::All,
            result: Ok(()),
        })
        .expect("send OpCompleted");
    state.drain_events();
    assert!(
        state.caches.tags(&alpha_id).is_none()
            && state.caches.tags(&beta_id).is_none()
            && state.caches.recents(&alpha_id).is_none()
            && state.caches.recents(&beta_id).is_none(),
        "every root's tag list and checkout history go with a project-wide invalidation"
    );
}

#[test]
fn a_tag_created_in_session_is_in_the_cached_tag_list_once_the_op_settles() {
    let p = two_root_project();
    let alpha_id = RootId(p.alpha.clone().into());

    let mut state = AppState::for_roots(&p.dir, std::slice::from_ref(&p.alpha));
    state.warm_branch_tags();
    assert!(
        state
            .caches
            .tags(&alpha_id)
            .is_some_and(|tags| tags.iter().all(|(name, _)| name != "v1")),
        "the cold tag list must not already hold the tag"
    );

    let spec = TagSpec {
        name: "v1".to_string(),
        target: None,
        message: None,
        tagger: None,
        sign: false,
    };
    state.dispatch(tag_writes::create(&alpha_id, &spec, false));
    state.drain_events();

    state.warm_branch_tags();
    assert!(
        state
            .caches
            .tags(&alpha_id)
            .is_some_and(|tags| tags.iter().any(|(name, _)| name == "v1")),
        "a tag created in-session must be in the root caches' tag list after the op settles"
    );
}

#[test]
fn op_outside_selected_root_does_not_refetch_selected_log() {
    let p = two_root_project();
    let alpha_id = RootId(p.alpha.clone().into());
    let beta_id = RootId(p.beta.clone().into());

    let mut state = AppState::for_roots(&p.dir, &[p.alpha.clone(), p.beta.clone()]);
    prime_fake_entries(&mut state, &[alpha_id.clone(), beta_id.clone()]);
    state.selected_root = Some(beta_id.clone());

    state
        .tx
        .send(AppEvent::OpCompleted {
            kind: OpKind::Other,
            label: "op".to_string(),
            affected: Affected::Root(alpha_id.clone()),
            result: Ok(()),
        })
        .expect("send OpCompleted");
    state.drain_events();

    assert!(
        state.caches.log(&alpha_id).is_none(),
        "out-of-scope invalidation must not trigger a refetch"
    );
    assert_eq!(
        first_message(&state, &beta_id),
        Some("fake: untouched".to_string())
    );
    assert_eq!(state.caches.ahead_behind(&beta_id), Some((9, 9)));
}

#[test]
fn refresh_all_clears_every_cache_and_refetches_selected_log() {
    let p = two_root_project();
    let alpha_id = RootId(p.alpha.clone().into());

    let mut state = AppState::for_roots(&p.dir, std::slice::from_ref(&p.alpha));
    prime_fake_entries(&mut state, std::slice::from_ref(&alpha_id));
    prime_engine_backed_entries(&mut state, &p.alpha);
    assert!(
        state.caches.ref_groups(&alpha_id).next().is_some(),
        "seeded repo must have decorations to drop"
    );
    assert!(
        state
            .caches
            .path_log(&alpha_id, Path::new("file.txt"))
            .is_some(),
        "path-scoped history must be primed before the refresh"
    );

    state.refresh(Affected::All);
    state.drain_events();

    assert!(
        state.caches.ref_groups(&alpha_id).next().is_none(),
        "refresh(All) must drop ref decorations"
    );
    assert!(
        state
            .caches
            .path_log(&alpha_id, Path::new("file.txt"))
            .is_none(),
        "refresh(All) must drop path-scoped history"
    );

    let expected = engine_log(&p.alpha);
    assert_eq!(state.caches.log(&alpha_id), Some(expected.as_slice()));
    assert_eq!(state.caches.ahead_behind(&alpha_id), Some((0, 0)));
}

#[test]
fn refs_loaded_event_populates_the_ref_readers_for_a_root() {
    let p = two_root_project();
    let alpha_id = RootId(p.alpha.clone().into());

    let mut state = AppState::for_roots(&p.dir, std::slice::from_ref(&p.alpha));
    let exec = CliExecutor {
        settings: VcsSettings::default(),
    };
    let deco = exec.ref_decorations(&p.alpha).expect("decorations");
    assert!(!deco.is_empty(), "seeded repo must have decorations");

    assert!(
        !state.caches.refs_loaded(&alpha_id),
        "a cold ref cache reports nothing loaded"
    );
    state
        .tx
        .send(AppEvent::RefsLoaded {
            root: alpha_id.clone(),
            deco: Ok(deco.clone()),
        })
        .expect("send RefsLoaded");
    state.drain_events();

    assert!(
        state.caches.refs_loaded(&alpha_id),
        "refs_loaded() must reflect the injected decorations"
    );
    assert!(
        state.caches.ref_groups(&alpha_id).next().is_some(),
        "ref readers must return the injected decorations after RefsLoaded"
    );
    for (cid, refs) in &deco {
        assert_eq!(
            state.caches.refs_for(&alpha_id, cid),
            refs.as_slice(),
            "every injected commit's decorations must be readable"
        );
    }
}

#[test]
fn refs_loaded_error_leaves_the_cache_empty_and_surfaces_the_error() {
    let p = two_root_project();
    let alpha_id = RootId(p.alpha.clone().into());

    let mut state = AppState::for_roots(&p.dir, std::slice::from_ref(&p.alpha));
    state
        .tx
        .send(AppEvent::RefsLoaded {
            root: alpha_id.clone(),
            deco: Err(turbogit_domain::error::TgError::Other(
                "offline".to_string(),
            )),
        })
        .expect("send RefsLoaded");
    state.drain_events();

    assert!(
        !state.caches.refs_loaded(&alpha_id),
        "a failing refs load must leave the cache empty"
    );
    assert_eq!(
        state.caches.ref_groups(&alpha_id).count(),
        0,
        "no decorations may be readable after a failing load"
    );
    assert!(
        state.last_error.is_some(),
        "the failing refs load must surface through the last-error path"
    );
}

#[test]
fn refs_fetch_guard_dedupes_and_releases_on_refs_loaded_ok_and_err() {
    let p = two_root_project();
    let alpha_id = RootId(p.alpha.clone().into());

    let recorder: Arc<RecordingExecutor> =
        Arc::new(RecordingExecutor::new(Arc::new(CliExecutor {
            settings: VcsSettings::default(),
        })));
    let mut state =
        AppState::for_roots(&p.dir, std::slice::from_ref(&p.alpha)).with_executor(recorder.clone());

    state.fetch_refs(alpha_id.clone());
    state.fetch_refs(alpha_id.clone());
    state.drain_events();
    assert!(state.caches.refs_loaded(&alpha_id), "decorations settled");
    assert_eq!(
        recorder.ref_call_count(),
        1,
        "two same-frame fetches must produce exactly one ref_decorations call"
    );

    state.fetch_refs(alpha_id.clone());
    state.drain_events();
    assert_eq!(
        recorder.ref_call_count(),
        2,
        "the guard freed after the drain"
    );

    state
        .tx
        .send(AppEvent::RefsLoaded {
            root: alpha_id.clone(),
            deco: Err(turbogit_domain::error::TgError::Other(
                "offline".to_string(),
            )),
        })
        .expect("send failing RefsLoaded");
    state.drain_events();
    assert!(
        state.last_error.is_some(),
        "a failing refs load must surface through last_error"
    );
    state.fetch_refs(alpha_id.clone());
    state.drain_events();
    assert!(
        recorder.ref_call_count() >= 3,
        "a later fetch must run again after the Err drain"
    );
}

fn commit_as(root: &RootId, id: &str) -> Commit {
    Commit {
        id: id.to_string(),
        ..fake_commit(root, &format!("msg {id}"))
    }
}

fn cached_ids(caches: &RootCaches, root: &RootId) -> Vec<String> {
    caches
        .log(root)
        .unwrap_or_default()
        .iter()
        .map(|c| c.id.clone())
        .collect()
}

#[test]
fn appending_to_a_cold_root_stores_the_batch() {
    let mut caches = RootCaches::default();
    let root = RootId(PathBuf::from("alpha").into());
    assert!(caches.log(&root).is_none(), "a cold root has nothing");

    caches.append_log(
        root.clone(),
        vec![commit_as(&root, "a3"), commit_as(&root, "a2")],
    );
    assert_eq!(cached_ids(&caches, &root), ["a3", "a2"]);
}

#[test]
fn an_overlapping_batch_appends_once_and_keeps_newest_first() {
    let mut caches = RootCaches::default();
    let root = RootId(PathBuf::from("alpha").into());
    caches.store_log(
        root.clone(),
        vec![
            commit_as(&root, "a5"),
            commit_as(&root, "a4"),
            commit_as(&root, "a3"),
        ],
    );

    caches.append_log(
        root.clone(),
        vec![
            commit_as(&root, "a3"),
            commit_as(&root, "a2"),
            commit_as(&root, "a1"),
        ],
    );
    assert_eq!(
        cached_ids(&caches, &root),
        ["a5", "a4", "a3", "a2", "a1"],
        "the anchor is already held, so it must not appear twice"
    );

    caches.append_log(
        root.clone(),
        vec![
            commit_as(&root, "a3"),
            commit_as(&root, "a2"),
            commit_as(&root, "a1"),
        ],
    );
    assert_eq!(cached_ids(&caches, &root), ["a5", "a4", "a3", "a2", "a1"]);
}

#[test]
fn an_empty_append_leaves_the_window_alone() {
    let mut caches = RootCaches::default();
    let root = RootId(PathBuf::from("alpha").into());
    caches.store_log(
        root.clone(),
        vec![commit_as(&root, "a2"), commit_as(&root, "a1")],
    );
    caches.set_log_has_more(&root, true);
    assert!(caches.log_has_more(&root));

    caches.append_log(root.clone(), Vec::new());
    assert_eq!(cached_ids(&caches, &root), ["a2", "a1"]);
    assert!(
        caches.log_has_more(&root),
        "an empty batch on its own proves nothing about history"
    );

    caches.set_log_has_more(&root, false);
    assert!(!caches.log_has_more(&root));
    assert_eq!(cached_ids(&caches, &root), ["a2", "a1"]);
}

#[test]
fn dropping_a_root_log_drops_its_has_more_flag_too() {
    let mut caches = RootCaches::default();
    let root = RootId(PathBuf::from("alpha").into());
    caches.store_log(root.clone(), vec![commit_as(&root, "a1")]);
    caches.set_log_has_more(&root, true);
    assert!(!caches.is_empty(), "priming must populate the log window");

    caches.invalidate(&Affected::Root(root.clone()));
    assert!(
        caches.log(&root).is_none(),
        "the scoped invalidation must drop the root's log"
    );
    assert!(
        !caches.log_has_more(&root),
        "the dropped log's has-more flag must go with it"
    );
    assert!(caches.is_empty(), "one invalidation unit leaves nothing");

    caches.set_log_has_more(&root, true);
    caches.invalidate_all();
    assert!(!caches.log_has_more(&root));
    assert!(caches.is_empty());
}

fn seed_staged_and_unstaged_edits(dir: &Path) {
    std::fs::write(dir.join("file.txt"), "file: v1\nfile: v2\n").expect("unstaged edit");
    std::fs::write(dir.join("other.txt"), "other: v1\nother: v2\n").expect("staged edit");
    git(dir, &["add", "other.txt"]);
}

#[test]
fn hunk_stats_ensure_fills_per_root_and_refresh_invalidates() {
    let parent = tempfile::tempdir().unwrap();
    let dir = parent.path().join("stats");
    seed_repo(&dir, "stats");
    seed_staged_and_unstaged_edits(&dir);

    let mut state = AppState::for_roots(parent.path(), std::slice::from_ref(&dir));
    let root = RootId(dir.clone().into());
    assert!(state.caches.hunk_stats(&root).is_none());

    state.ensure_hunk_stats(&root);
    let stats = state
        .caches
        .hunk_stats(&root)
        .expect("stats filled on miss");

    let local_of = |name: &str| {
        stats
            .local
            .iter()
            .find(|f| f.path == name)
            .map(|f| f.hunks.len())
    };
    let staged_of = |name: &str| {
        stats
            .staged
            .iter()
            .find(|f| f.path == name)
            .map(|f| f.hunks.len())
    };
    let repo_of = |name: &str| {
        stats
            .repo
            .iter()
            .find(|f| f.path == name)
            .map(|f| f.hunks.len())
    };

    assert_eq!(staged_of("other.txt"), Some(1));
    assert_eq!(local_of("other.txt"), None);
    assert_eq!(local_of("file.txt"), Some(1));
    assert_eq!(staged_of("file.txt"), None);
    assert_eq!(repo_of("file.txt"), Some(1));
    assert_eq!(repo_of("other.txt"), Some(1));

    state.refresh(Affected::Root(root.clone()));
    assert!(
        state.caches.hunk_stats(&root).is_none(),
        "refresh must drop the hunk stats with the other caches"
    );
    state.ensure_hunk_stats(&root);
    assert!(state.caches.hunk_stats(&root).is_some());
}
