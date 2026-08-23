//! Issue #20 — Push dialog core tests.
//!
//! Drives the real `turbogit::ui::render()` through `egui_kittest` against
//! temporary multi-root git repositories, each with its own local bare
//! "remote", asserting painted labels and public `AppState` transitions only.
//!
//! Covered behaviors (ADR-0006 / ADR-0007):
//! - aggregated outgoing-commit tree (project node → per-root nodes → commits)
//! - clicking a root node filters the changed-files PREVIEW only — the Push
//!   action still batches across ALL roots
//! - edited Remote/Branch fields propagate into the narrowed execution
//! - per-root failures surface while healthy roots still succeed

#![allow(dead_code)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{assert_not_painted, assert_painted, painted_text};
use egui_kittest::kittest::Queryable;
use egui_kittest::{Harness, Node};
use turbogit::core::multi_root::build_root;
use turbogit::engine::{AppEvent, GitExecutor};
use turbogit::model::{RootId, VcsSettings};
use turbogit::state::{AppState, Dialog, Toast};

// ---------------------------------------------------------------- helpers --

/// Run `git` in `dir`, asserting success, and return stdout.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git should be on PATH");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn head_sha(repo: &Path) -> String {
    git(repo, &["rev-parse", "HEAD"]).trim().to_string()
}

/// Resolve `refname` in a bare repository; `None` when it does not exist.
fn bare_ref(bare: &Path, refname: &str) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("--git-dir")
        .arg(bare)
        .args(["rev-parse", "--verify", refname])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// A temp repo on `main` that is 2 commits ahead of its own local bare
/// remote (`<parent>/<name>-remote.git`). Commits c2/c3 each touch a
/// distinctly named file so changed-file previews can distinguish repos.
struct Repo {
    path: PathBuf,
    remote: PathBuf,
    /// `[c1 (pushed), c2, c3]` full SHAs.
    shas: [String; 3],
}

fn repo_ahead_of_origin(parent: &Path, name: &str) -> Repo {
    let path = parent.join(name);
    let remote = parent.join(format!("{name}-remote.git"));
    std::fs::create_dir_all(&path).unwrap();

    git(&path, &["init", "-q", "-b", "main"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);

    std::fs::write(path.join("base.txt"), "base\n").unwrap();
    git(&path, &["add", "."]);
    let base_msg = format!("{name} base");
    git(&path, &["commit", "-q", "-m", &base_msg]);
    let c1 = head_sha(&path);

    git(&path, &["init", "--bare", remote.to_str().unwrap()]);
    git(
        &path,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&path, &["push", "-q", "-u", "origin", "main"]);

    let c2_file = format!("{name}-c2.txt");
    std::fs::write(path.join(&c2_file), "second\n").unwrap();
    git(&path, &["add", "."]);
    let c2_msg = format!("{name} second");
    git(&path, &["commit", "-q", "-m", &c2_msg]);
    let c2 = head_sha(&path);

    let c3_file = format!("{name}-c3.txt");
    std::fs::write(path.join(&c3_file), "third\n").unwrap();
    git(&path, &["add", "."]);
    let c3_msg = format!("{name} third");
    git(&path, &["commit", "-q", "-m", &c3_msg]);
    let c3 = head_sha(&path);

    Repo {
        path,
        remote,
        shas: [c1, c2, c3],
    }
}

/// Build an `AppState` over fully-built roots (branches/remotes/tracking
/// included via `build_root`) with synchronous scans — no background threads.
fn app_state(project_dir: &Path, roots: &[PathBuf]) -> AppState {
    let (tx, rx) = crossbeam_channel::unbounded();
    let settings = VcsSettings::default();
    let executor: Arc<dyn GitExecutor> = Arc::new(turbogit::engine::cli::CliExecutor {
        settings: settings.clone(),
    });
    let mut st = AppState {
        project_dir: project_dir.to_path_buf(),
        executor,
        settings,
        multi: Default::default(),
        tx,
        rx,
        selected_root: None,
        clone_url: String::new(),
        last_error: None,
        ui: Default::default(),
        log_cache: Default::default(),
        ahead_behind: Default::default(),
        recents_config_dir: None,
        dir_picker: None,
        ref_cache: Default::default(),
        files_cache: Default::default(),
        log_path_cache: Default::default(),
    };
    for r in roots {
        let root = build_root(st.executor.as_ref(), r).expect("build root");
        st.multi.register_root(root);
    }
    st.selected_root = st.multi.roots.first().map(|r| r.id.clone());
    st
}

/// Drain worker-thread events exactly like `app.rs`, but re-status
/// synchronously after completed ops so tests stay deterministic.
fn drain_events(state: &mut AppState) {
    while let Ok(ev) = state.rx.try_recv() {
        match ev {
            AppEvent::StatusScanned {
                root,
                status: Ok(s),
            } => {
                if let Some(r) = state.multi.roots.iter_mut().find(|r| r.id == root) {
                    r.status = s;
                }
            }
            AppEvent::StatusScanned { .. } => {}
            AppEvent::OpCompleted { label, result } => {
                state.ui.busy = false;
                match result {
                    Ok(()) => {
                        state.ui.toast = Some(Toast::success(label));
                        for root in &mut state.multi.roots {
                            if let Ok(s) = state.executor.status(&root.path) {
                                root.status = s;
                            }
                        }
                    }
                    Err(e) => {
                        state.ui.toast = Some(Toast::error(format!("{label}: {e}")));
                        state.last_error = Some(e.to_string());
                    }
                }
            }
            AppEvent::DiffReady { key, result } => {
                state.ui.diff_loading = false;
                match result {
                    Ok(text) => {
                        state.ui.diff_error = None;
                        state.ui.diff_cache = Some((key, text));
                    }
                    Err(e) => {
                        state.ui.diff_error = Some(e.to_string());
                    }
                }
            }
            AppEvent::AheadBehind {
                root,
                ahead,
                behind,
            } => {
                state.ahead_behind.insert(root, (ahead, behind));
            }
            _ => {}
        }
    }
}

/// Headless harness driving the full app UI with event draining per frame.
fn harness(state: AppState) -> Harness<'static, AppState> {
    Harness::builder().with_max_steps(1024).build_ui_state(
        |ui, state| {
            drain_events(state);
            turbogit::ui::render(ui, state);
        },
        state,
    )
}

/// Poll `f` until it returns true or the deadline elapses (worker threads run
/// asynchronously, so completion is observed by polling).
fn wait_until<F: Fn() -> bool>(ms: u64, f: F) -> bool {
    let start = Instant::now();
    loop {
        if f() {
            return true;
        }
        if start.elapsed() >= Duration::from_millis(ms) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Open the Push dialog directly (the Ctrl+Shift+K shortcut is covered by
/// tests/redesign_harness.rs).
fn open_push_dialog(h: &mut Harness<'_, AppState>) {
    h.state_mut().ui.dialog = Some(Dialog::Push);
    h.run();
    h.run();
}

/// The dialog's primary Push button. The shell toolbar also paints a "Push"
/// item, so disambiguate geometrically: the dialog footer sits below it.
fn dialog_push_button<'h>(h: &'h Harness<'_, AppState>) -> Node<'h> {
    let mut nodes: Vec<Node<'h>> = h.get_all_by_label("Push").collect();
    assert!(
        nodes.len() >= 2,
        "expected the toolbar Push plus the dialog Push button"
    );
    nodes.sort_by(|a, b| a.rect().center().y.total_cmp(&b.rect().center().y));
    nodes.pop().expect("at least one Push button")
}

/// The painted toast galley carrying the last op result, if visible.
/// Op toasts render as `Push` (success) or `Push: <error>` (issue #22 typed
/// toasts replaced the ✓/✗ glyph prefixes); the colon disambiguates the
/// failure toast from the bare toolbar Push button.
fn toast_galley(h: &Harness<'_, AppState>) -> Option<String> {
    painted_text(h).into_iter().find(|t| t.starts_with("Push:"))
}

// ------------------------------------------------------------------ tests --

#[test]
fn outgoing_tree_aggregates_all_roots_under_project_node() {
    let parent = tempfile::tempdir().unwrap();
    let alpha = repo_ahead_of_origin(parent.path(), "alpha");
    let beta = repo_ahead_of_origin(parent.path(), "beta");

    let mut h = harness(app_state(
        parent.path(),
        &[alpha.path.clone(), beta.path.clone()],
    ));
    open_push_dialog(&mut h);

    // Project node aggregates; per-root nodes carry per-root counts.
    assert_painted(&h, "(all roots)");
    assert_painted(&h, "alpha — 2 commits ahead");
    assert_painted(&h, "beta — 2 commits ahead");

    // Every outgoing commit is listed under its root (short SHA + subject).
    for sha in [&alpha.shas[1], &alpha.shas[2], &beta.shas[1], &beta.shas[2]] {
        assert_painted(&h, &sha[..7]);
    }
    assert_painted(&h, "alpha second");
    assert_painted(&h, "alpha third");
    assert_painted(&h, "beta second");
    assert_painted(&h, "beta third");

    // Unfiltered changed-files preview spans every root's outgoing commits.
    assert_painted(&h, "alpha-c2.txt");
    assert_painted(&h, "alpha-c3.txt");
    assert_painted(&h, "beta-c2.txt");
    assert_painted(&h, "beta-c3.txt");
}

#[test]
fn clicking_root_node_filters_preview_but_push_still_batches_all_roots() {
    let parent = tempfile::tempdir().unwrap();
    let alpha = repo_ahead_of_origin(parent.path(), "alpha");
    let beta = repo_ahead_of_origin(parent.path(), "beta");

    let mut h = harness(app_state(
        parent.path(),
        &[alpha.path.clone(), beta.path.clone()],
    ));
    open_push_dialog(&mut h);

    // Clicking a root node narrows the PREVIEW only…
    h.get_by_label("alpha — 2 commits ahead").click();
    h.run();

    assert_painted(&h, "alpha-c2.txt");
    assert_not_painted(&h, "beta-c3.txt");
    assert_eq!(
        h.state().ui.dlg.push_preview_root,
        Some(RootId(alpha.path.clone())),
        "root click must set the preview filter"
    );

    // …while Push still executes the batch across ALL roots (ADR-0006).
    dialog_push_button(&h).click();
    h.run();

    let alpha_tip = head_sha(&alpha.path);
    let beta_tip = head_sha(&beta.path);
    assert!(
        wait_until(15_000, || {
            bare_ref(&alpha.remote, "refs/heads/main").as_deref() == Some(alpha_tip.as_str())
                && bare_ref(&beta.remote, "refs/heads/main").as_deref() == Some(beta_tip.as_str())
        }),
        "batch push must reach BOTH roots' remotes regardless of the preview selection"
    );
}

#[test]
fn remote_and_branch_edits_propagate_to_execution() {
    let parent = tempfile::tempdir().unwrap();
    let repo_path = parent.path().join("solo");
    std::fs::create_dir_all(&repo_path).unwrap();
    git(&repo_path, &["init", "-q", "-b", "main"]);
    git(&repo_path, &["config", "user.email", "test@example.com"]);
    git(&repo_path, &["config", "user.name", "Test"]);

    std::fs::write(repo_path.join("base.txt"), "base\n").unwrap();
    git(&repo_path, &["add", "."]);
    git(&repo_path, &["commit", "-q", "-m", "base"]);

    // Two bare remotes; origin carries main, upstream is registered but empty.
    let origin = parent.path().join("solo-origin.git");
    let upstream = parent.path().join("solo-upstream.git");
    git(&repo_path, &["init", "--bare", origin.to_str().unwrap()]);
    git(
        &repo_path,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(&repo_path, &["push", "-q", "-u", "origin", "main"]);
    git(&repo_path, &["init", "--bare", upstream.to_str().unwrap()]);
    git(
        &repo_path,
        &["remote", "add", "upstream", upstream.to_str().unwrap()],
    );

    // Local branch with no tracking: prefill falls back to first remote.
    git(&repo_path, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(repo_path.join("feature-file.txt"), "work\n").unwrap();
    git(&repo_path, &["add", "."]);
    git(&repo_path, &["commit", "-q", "-m", "feature work"]);
    let feature_sha = head_sha(&repo_path);

    let mut h = harness(app_state(parent.path(), std::slice::from_ref(&repo_path)));
    open_push_dialog(&mut h);
    assert_eq!(
        h.state().ui.dlg.push_branch,
        "feature",
        "dialog must prefill the current branch"
    );

    // Edit the Remote field, narrow the scope, push.
    h.state_mut().ui.dlg.push_remote = "upstream".into();
    h.run();
    h.get_by_label("Push current branch only").click();
    h.run();
    dialog_push_button(&h).click();
    h.run();

    assert!(
        wait_until(15_000, || {
            bare_ref(&upstream, "refs/heads/feature").as_deref() == Some(feature_sha.as_str())
        }),
        "the edited Remote field must drive the narrowed push target"
    );
    assert!(
        bare_ref(&origin, "refs/heads/feature").is_none(),
        "origin must not receive a push aimed at the edited remote"
    );
}

#[test]
fn per_root_failure_is_surfaced_while_other_roots_succeed() {
    let parent = tempfile::tempdir().unwrap();
    let alpha = repo_ahead_of_origin(parent.path(), "alpha");
    let beta = repo_ahead_of_origin(parent.path(), "beta");
    // Break beta's remote after setup so its config still names it but the
    // target directory is gone — its push must fail while alpha's succeeds.
    std::fs::remove_dir_all(&beta.remote).unwrap();

    let mut h = harness(app_state(
        parent.path(),
        &[alpha.path.clone(), beta.path.clone()],
    ));
    open_push_dialog(&mut h);

    dialog_push_button(&h).click();
    h.run();

    let alpha_tip = head_sha(&alpha.path);
    assert!(
        wait_until(15_000, || {
            bare_ref(&alpha.remote, "refs/heads/main").as_deref() == Some(alpha_tip.as_str())
        }),
        "the healthy root must still be pushed by the batch"
    );

    let mut surfaced = false;
    for _ in 0..40 {
        if let Some(toast) = toast_galley(&h) {
            assert!(
                toast.contains("Push"),
                "expected a failure toast, got: {toast}"
            );
            assert!(
                toast.contains("beta"),
                "failure toast must name the failing root, got: {toast}"
            );
            surfaced = true;
            break;
        }
        h.run();
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(surfaced, "per-root failure must surface in the op toast");
}
