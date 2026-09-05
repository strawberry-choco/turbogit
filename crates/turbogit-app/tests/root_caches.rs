//! Root caches deepening — headless suite for the [`turbogit_app::root_caches`]
//! interface and the [`AppState::refresh`] seam (plan:
//! `docs/plans/root-caches-deepening.md`).
//!
//! Everything goes through the public surface over the `AppState::for_roots`
//! headless harness (CONTEXT.md "Headless harness"): cache entries are primed
//! via event injection (`AppEvent::LogLoaded` / `AppEvent::AheadBehind`
//! through `state.tx` + `drain_events()`) or deterministic engine-backed
//! `ensure_*` calls, and invalidation is observed through the accessors.
//!
//! Covered:
//! - project switch leaves nothing stale (all five maps empty afterwards)
//! - an op scoped to `Affected::Root(a)` refreshes a and leaves root b's
//!   entries intact
//! - `refresh(All)` clears every cache, refetches only the selected log,
//!   and drops decorations / path-scoped history (manual-refresh totality)
//! - an op outside the selected root does not refetch the selected log

use std::path::{Path, PathBuf};
use tempfile::TempDir;
use turbogit_app::events::AppEvent;
use turbogit_app::root_caches::Affected;
use turbogit_app::state::AppState;
use turbogit_domain::model::{Commit, LogOpts, RootId, Signature, SignatureState, VcsSettings};
use turbogit_engine::GitExecutor;
use turbogit_engine::cli::CliExecutor;

// --- Seeded fixtures (mirrors the other redesign suites) ----------------------

fn run_git(dir: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git should be on PATH");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// Seed a minimal repo at `dir`: one branch, one commit touching file.txt.
fn seed_repo(dir: &Path, name: &str) {
    std::fs::create_dir_all(dir).expect("repo dir");
    run_git(dir, &["init", "-q", "-b", "main"]);
    run_git(dir, &["config", "user.email", "test@example.com"]);
    run_git(dir, &["config", "user.name", "Test"]);
    std::fs::write(dir.join("file.txt"), format!("{name}: v1\n")).expect("work file");
    run_git(dir, &["add", "."]);
    run_git(dir, &["commit", "-q", "-m", &format!("{name}: initial")]);
}

fn head_commit(dir: &Path) -> String {
    run_git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

/// A recognizable fake entry so untouched caches can be told apart from
/// freshly computed ones.
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

/// Prime log + ahead/behind entries for every given root through the
/// production event path (decision 9): inject events via `state.tx`, then
/// `drain_events()` — no seed methods on the interface.
fn prime_fake_entries(state: &mut AppState, roots: &[RootId]) {
    for root in roots {
        state
            .tx
            .send(AppEvent::LogLoaded {
                root: root.clone(),
                commits: Ok(vec![fake_commit(root, "fake: untouched")]),
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

/// Prime the remaining three caches (decorations, changed files, path-scoped
/// history) through deterministic engine-backed `ensure_*` calls.
fn prime_engine_backed_entries(state: &mut AppState, root_dir: &Path) {
    let exec = CliExecutor {
        settings: VcsSettings::default(),
    };
    let root = RootId(root_dir.to_path_buf().into());
    state.caches.ensure_refs(&exec, &root);
    state
        .caches
        .ensure_files(&exec, &root, &head_commit(root_dir));
    state
        .caches
        .ensure_path_log(&exec, &root, Path::new("file.txt"));
}

fn engine_log(dir: &Path) -> Vec<Commit> {
    CliExecutor {
        settings: VcsSettings::default(),
    }
    .log(dir, &LogOpts::default())
    .expect("engine log")
}

/// First cached commit message for `root`, if any.
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

/// A project with two seeded roots.
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

// --- Project switch ------------------------------------------------------------

#[test]
fn project_switch_leaves_no_cache_entries_stale() {
    let first = two_root_project();
    let second = two_root_project();
    // open_project records a recent; inject a throwaway config dir so the
    // real user recents file is never touched (ADR-0005 test seam).
    let cfg = tempfile::tempdir().expect("config tempdir");

    let mut state = AppState::for_roots(&first.dir, std::slice::from_ref(&first.alpha));
    state.recents_config_dir = Some(cfg.path().to_path_buf());
    prime_fake_entries(&mut state, &[RootId(first.alpha.clone().into())]);
    prime_engine_backed_entries(&mut state, &first.alpha);
    assert!(
        !state.caches.is_empty(),
        "priming must populate all five maps"
    );

    // Switching projects must drop EVERY cache entry — including ref
    // decorations, changed files and path-scoped logs that the old partial
    // clears leaked across projects.
    state.open_project(&second.dir);

    assert!(
        state.caches.is_empty(),
        "project switch must leave nothing stale"
    );
}

// --- Scoped op completion --------------------------------------------------------

#[test]
fn scoped_op_completion_keeps_unaffected_roots_cached() {
    let p = two_root_project();
    let alpha_id = RootId(p.alpha.clone().into());
    let beta_id = RootId(p.beta.clone().into());

    let mut state = AppState::for_roots(&p.dir, &[p.alpha.clone(), p.beta.clone()]);
    prime_fake_entries(&mut state, &[alpha_id.clone(), beta_id.clone()]);
    // for_roots selects the first registered root (alpha).

    state
        .tx
        .send(AppEvent::OpCompleted {
            label: "op".to_string(),
            affected: Affected::Root(alpha_id.clone()),
            result: Ok(()),
            retry: None,
        })
        .expect("send OpCompleted");
    state.drain_events();

    // alpha was invalidated AND refetched (selected ∈ affected): its fake
    // entry is replaced by the real git log…
    let expected = engine_log(&p.alpha);
    assert!(!expected.is_empty(), "seeded repo must have commits");
    assert_eq!(
        state.caches.log(&alpha_id),
        Some(expected.as_slice()),
        "affected selected root must be refetched from git"
    );
    // …and its ahead/behind recomputed synchronously (no upstream → (0, 0)).
    assert_eq!(state.caches.ahead_behind(&alpha_id), Some((0, 0)));

    // beta (unaffected) keeps its primed entries verbatim.
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

#[test]
fn op_outside_selected_root_does_not_refetch_selected_log() {
    let p = two_root_project();
    let alpha_id = RootId(p.alpha.clone().into());
    let beta_id = RootId(p.beta.clone().into());

    let mut state = AppState::for_roots(&p.dir, &[p.alpha.clone(), p.beta.clone()]);
    prime_fake_entries(&mut state, &[alpha_id.clone(), beta_id.clone()]);
    // Select beta so it is OUTSIDE the op's scope.
    state.selected_root = Some(beta_id.clone());

    state
        .tx
        .send(AppEvent::OpCompleted {
            label: "op".to_string(),
            affected: Affected::Root(alpha_id.clone()),
            result: Ok(()),
            retry: None,
        })
        .expect("send OpCompleted");
    state.drain_events();

    // alpha invalidated; NOT refetched because the selection is out of scope.
    assert!(
        state.caches.log(&alpha_id).is_none(),
        "out-of-scope invalidation must not trigger a refetch"
    );
    // beta untouched on every axis.
    assert_eq!(
        first_message(&state, &beta_id),
        Some("fake: untouched".to_string())
    );
    assert_eq!(state.caches.ahead_behind(&beta_id), Some((9, 9)));
}

// --- Manual refresh totality -------------------------------------------------------

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

    // What Ctrl+T / palette Refresh dispatch (decision 8).
    state.refresh(Affected::All);

    // Decorations and path-scoped history are dropped and nothing recomputes
    // them outside the Git Log window — today's manual refresh leaked them.
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

    // The selected root's log comes back fresh from git…
    let expected = engine_log(&p.alpha);
    assert_eq!(state.caches.log(&alpha_id), Some(expected.as_slice()));
    // …and ahead/behind is recomputed synchronously.
    assert_eq!(state.caches.ahead_behind(&alpha_id), Some((0, 0)));
}

// --- Issue 20: per-root hunk-span statistics ---------------------------------

/// One unstaged edit (file.txt) plus one staged edit (other.txt), so all
/// three working-tree diffs carry distinct content.
fn seed_staged_and_unstaged_edits(dir: &Path) {
    std::fs::write(dir.join("file.txt"), "file: v1\nfile: v2\n").expect("unstaged edit");
    std::fs::write(dir.join("other.txt"), "other: v1\nother: v2\n").expect("staged edit");
    run_git(dir, &["add", "other.txt"]);
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

    let exec = CliExecutor {
        settings: VcsSettings::default(),
    };
    state.caches.ensure_hunk_stats(&exec, &root);
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

    // The staged edit lives only in the staged (HEAD↔index) view, the
    // unstaged edit only in the local (index↔worktree) view; the HEAD↔
    // worktree view shows both.
    assert_eq!(staged_of("other.txt"), Some(1));
    assert_eq!(local_of("other.txt"), None);
    assert_eq!(local_of("file.txt"), Some(1));
    assert_eq!(staged_of("file.txt"), None);
    assert_eq!(repo_of("file.txt"), Some(1));
    assert_eq!(repo_of("other.txt"), Some(1));

    // A completed op refreshes the affected root and drops the stats with
    // the rest of the caches (one invalidation unit).
    state.refresh(Affected::Root(root.clone()));
    assert!(
        state.caches.hunk_stats(&root).is_none(),
        "refresh must drop the hunk stats with the other caches"
    );
    // And the next ensure recomputes them.
    state.caches.ensure_hunk_stats(&exec, &root);
    assert!(state.caches.hunk_stats(&root).is_some());
}
