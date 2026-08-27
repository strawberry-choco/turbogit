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
//!
//! Issue #21 — Push safety:
//! - Preview runs a REAL `git push --dry-run` through the engine seam and
//!   paints the report VERBATIM in-dialog (success and rejection render
//!   distinctly)
//! - checking the force-push acknowledgment switches the executed push to
//!   `--force-with-lease` (asserted at the executor boundary via a recording
//!   executor)
//! - force-push to a protected branch (settings.protected_branch_patterns,
//!   keyed off the exact Remote/Branch fields) is BLOCKED in-dialog: nothing
//!   reaches the engine and the block is painted instead of silently
//!   downgrading

#![allow(dead_code)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{RecordingExecutor, assert_not_painted, assert_painted, painted_text};
use egui_kittest::kittest::Queryable;
use egui_kittest::{Harness, Node};
use turbogit::engine::GitExecutor;
use turbogit::error::TgError;
use turbogit::model::{RootId, VcsSettings};
use turbogit::state::{AppState, Dialog};

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

/// `repo_ahead_of_origin` plus true divergence: a second clone fast-forwards
/// the bare remote to its own commit d1, so the original repo (c2, c3) and
/// the remote (d1) have divergent `main` tips. The original repo fetches
/// afterwards so git reports `(non-fast-forward)` rather than `(fetch first)`.
///
/// Returns the repo plus the remote-side d1 SHA.
fn repo_diverged_from_origin(parent: &Path, name: &str) -> (Repo, String) {
    let repo = repo_ahead_of_origin(parent, name);

    let other = parent.join(format!("{name}-other"));
    git(
        parent,
        &[
            "clone",
            repo.remote.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    // The bare remote's HEAD may be unborn; land on main before committing.
    git(&other, &["checkout", "main"]);
    git(&other, &["config", "user.email", "test@example.com"]);
    git(&other, &["config", "user.name", "Test"]);
    std::fs::write(other.join("divergent.txt"), "d1\n").unwrap();
    git(&other, &["add", "."]);
    let d1_msg = format!("{name} divergent");
    git(&other, &["commit", "-q", "-m", &d1_msg]);
    let d1 = head_sha(&other);
    git(&other, &["push", "-q", "origin", "main"]);

    // Teach the original repo about d1 (updates origin/main only).
    git(&repo.path, &["fetch", "origin"]);

    (repo, d1)
}

/// Build an `AppState` over fully-built roots (branches/remotes/tracking
/// included via the production registration path) with synchronous scans —
/// no background threads.
fn app_state(project_dir: &Path, roots: &[PathBuf]) -> AppState {
    AppState::for_roots(project_dir, roots)
}

/// [`app_state`] with an injected engine and settings — lets tests assert at
/// the executor boundary through [`RecordingExecutor`] and configure
/// protected-branch patterns. The executor/settings swap happens AFTER
/// synchronous registration, so setup reads never reach the recorder.
fn app_state_with(
    project_dir: &Path,
    roots: &[PathBuf],
    executor: Arc<dyn GitExecutor>,
    settings: VcsSettings,
) -> AppState {
    AppState::for_roots(project_dir, roots)
        .with_executor(executor)
        .with_settings(settings)
}

/// Headless harness driving the full app UI with event draining per frame.
fn harness(state: AppState) -> Harness<'static, AppState> {
    Harness::builder().with_max_steps(1024).build_ui_state(
        |ui, state| {
            state.drain_events();
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
/// `shell_frame.rs`).
fn open_push_dialog(h: &mut Harness<'_, AppState>) {
    h.state_mut().ui.dialog = Some(Dialog::Push);
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
        Some(RootId(alpha.path.clone().into())),
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

// ------------------------------------------------------- issue #21: safety --

/// A real CLI engine behind a recording wrapper, plus the settings clone to
/// hand to [`app_state_with`].
fn recording_engine(settings: VcsSettings) -> (Arc<RecordingExecutor>, VcsSettings) {
    let cli: Arc<dyn GitExecutor> = Arc::new(turbogit::engine::cli::CliExecutor {
        settings: settings.clone(),
    });
    let rec = Arc::new(RecordingExecutor::new(cli));
    (rec, settings)
}

#[test]
fn preview_runs_real_dry_run_and_paints_report_verbatim() {
    let parent = tempfile::tempdir().unwrap();
    let solo = repo_ahead_of_origin(parent.path(), "solo");

    let (rec, settings) = recording_engine(VcsSettings::default());
    // Expected VERBATIM report, captured straight from the engine seam before
    // the UI runs (dry-run is non-mutating, so the second invocation inside
    // the dialog must produce byte-identical output).
    let expected = rec
        .inner
        .push_dry_run(&solo.path, "origin", "main", false)
        .expect("dry-run succeeds when ahead of origin");
    assert!(expected.contains("main -> main"));

    let dyn_exec: Arc<dyn GitExecutor> = rec.clone();
    let mut h = harness(app_state_with(
        parent.path(),
        std::slice::from_ref(&solo.path),
        dyn_exec,
        settings,
    ));
    open_push_dialog(&mut h);

    h.get_by_label("Preview dry-run").click();
    h.run();

    // The ENTIRE git report is painted verbatim (single galley substring).
    let texts = painted_text(&h);
    assert!(
        texts.iter().any(|t| t.contains(expected.trim_end())),
        "verbatim dry-run report not painted;\nexpected:\n{expected}\n\npainted:\n{texts:#?}"
    );
    assert_painted(&h, "Dry-run report (verbatim):");

    // REAL dry-run: the remote ref is provably untouched.
    let c1 = solo.shas[0].clone();
    assert_eq!(
        bare_ref(&solo.remote, "refs/heads/main").as_deref(),
        Some(c1.as_str()),
        "preview must not mutate the remote"
    );

    // Flag selection asserted at the executor boundary.
    assert!(
        rec.contains_dry_run("origin", "main", false),
        "expected PushDryRun {{ origin, main, force: false }}, got: {:?}",
        rec.recorded()
    );
}

#[test]
fn preview_shows_rejection_verbatim_when_diverged() {
    let parent = tempfile::tempdir().unwrap();
    let (solo, d1) = repo_diverged_from_origin(parent.path(), "solo");

    let (rec, settings) = recording_engine(VcsSettings::default());
    let err = rec
        .inner
        .push_dry_run(&solo.path, "origin", "main", false)
        .expect_err("diverged push must be rejected");
    let expected = match &err {
        TgError::Cli { stderr, .. } => stderr.clone(),
        other => panic!("expected TgError::Cli, got: {other:?}"),
    };
    assert!(expected.contains("[rejected]"));
    assert!(expected.contains("(non-fast-forward)"));

    let dyn_exec: Arc<dyn GitExecutor> = rec.clone();
    let mut h = harness(app_state_with(
        parent.path(),
        std::slice::from_ref(&solo.path),
        dyn_exec,
        settings,
    ));
    open_push_dialog(&mut h);

    h.get_by_label("Preview dry-run").click();
    h.run();

    // Rejected output renders VERBATIM, distinctly from the success case.
    let texts = painted_text(&h);
    assert!(
        texts.iter().any(|t| t.contains(expected.trim_end())),
        "verbatim rejection output not painted;\nexpected:\n{expected}\n\npainted:\n{texts:#?}"
    );
    assert_painted(&h, "Push rejected by git:");
    assert_not_painted(&h, "Dry-run report (verbatim):");

    // Still non-mutating: the remote tip remains the divergent d1 commit.
    assert_eq!(
        bare_ref(&solo.remote, "refs/heads/main").as_deref(),
        Some(d1.as_str()),
        "rejected preview must not mutate the remote"
    );
}

#[test]
fn force_acknowledgment_switches_push_to_force_with_lease_at_boundary() {
    let parent = tempfile::tempdir().unwrap();
    // Clear the default protected patterns so 'main' may be force-pushed
    // here; the blocking behavior has its own dedicated test below.
    let mut settings = VcsSettings::default();
    settings.protected_branch_patterns.clear();

    // Phase 1: checking the acknowledgment box sends force=true downstream.
    let ack = repo_ahead_of_origin(parent.path(), "ack");
    let (rec, settings) = recording_engine(settings);
    let dyn_exec: Arc<dyn GitExecutor> = rec.clone();
    let mut h = harness(app_state_with(
        parent.path(),
        std::slice::from_ref(&ack.path),
        dyn_exec,
        settings.clone(),
    ));
    open_push_dialog(&mut h);

    h.get_by_label("Force push (--force-with-lease)").click();
    h.run();
    h.get_by_label("Push current branch only").click();
    h.run();
    dialog_push_button(&h).click();
    h.run();

    assert!(
        wait_until(15_000, || rec.contains_push("origin", "main", true)),
        "acknowledged force push must reach the boundary as force=true, got: {:?}",
        rec.recorded()
    );
    // The real --force-with-lease push landed too (lease matched: we pushed
    // the base commit ourselves during setup). Recording precedes the actual
    // subprocess, so poll the remote rather than asserting immediately.
    let ack_tip = head_sha(&ack.path);
    assert!(
        wait_until(15_000, || {
            bare_ref(&ack.remote, "refs/heads/main").as_deref() == Some(ack_tip.as_str())
        }),
        "--force-with-lease push must update the remote"
    );

    // Phase 2: without the acknowledgment the boundary sees force=false.
    let plain = repo_ahead_of_origin(parent.path(), "plain");
    let (rec2, settings2) = recording_engine(settings);
    let dyn_exec2: Arc<dyn GitExecutor> = rec2.clone();
    let mut h2 = harness(app_state_with(
        parent.path(),
        std::slice::from_ref(&plain.path),
        dyn_exec2,
        settings2,
    ));
    open_push_dialog(&mut h2);

    h2.get_by_label("Push current branch only").click();
    h2.run();
    dialog_push_button(&h2).click();
    h2.run();

    assert!(
        wait_until(15_000, || rec2.contains_push("origin", "main", false)),
        "plain push must reach the boundary as force=false, got: {:?}",
        rec2.recorded()
    );
    assert!(
        !rec2.contains_push("origin", "main", true),
        "unchecked acknowledgment must never send force=true"
    );
}

#[test]
fn protected_branch_force_push_is_blocked_in_dialog_not_downgraded() {
    let parent = tempfile::tempdir().unwrap();
    let guarded = repo_ahead_of_origin(parent.path(), "guarded");
    // Default settings protect 'main'; the Branch field prefills to 'main'.
    let (rec, settings) = recording_engine(VcsSettings::default());
    let dyn_exec: Arc<dyn GitExecutor> = rec.clone();
    let mut h = harness(app_state_with(
        parent.path(),
        std::slice::from_ref(&guarded.path),
        dyn_exec,
        settings,
    ));
    open_push_dialog(&mut h);

    // Acknowledge force against the protected branch and narrow the scope so
    // the exact Remote/Branch fields drive execution…
    h.get_by_label("Force push (--force-with-lease)").click();
    h.run();
    h.get_by_label("Push current branch only").click();
    h.run();

    // …the block surfaces IN-DIALOG…
    assert_painted(&h, "is protected");
    assert_painted(&h, "force-push blocked");

    // …and clicking Push neither dispatches anything nor closes the dialog.
    dialog_push_button(&h).click();
    h.run();
    std::thread::sleep(Duration::from_millis(400));
    h.run();
    assert!(
        rec.recorded().is_empty(),
        "blocked force-push must never reach the engine, got: {:?}",
        rec.recorded()
    );
    assert_eq!(
        h.state().ui.dialog,
        Some(Dialog::Push),
        "a blocked push keeps the dialog open"
    );
    let c3 = guarded.shas[2].clone();
    assert_ne!(
        bare_ref(&guarded.remote, "refs/heads/main").as_deref(),
        Some(c3.as_str()),
        "blocked force-push must leave the remote untouched"
    );

    // Keyed off the EXACT Branch field: retargeting an unprotected branch
    // lifts the block, and the executed push KEEPS force=true (no silent
    // downgrade to a regular push).
    h.state_mut().ui.dlg.push_branch = "feature".into();
    h.run();
    assert_not_painted(&h, "force-push blocked");

    dialog_push_button(&h).click();
    h.run();
    assert!(
        wait_until(15_000, || rec.contains_push("origin", "feature", true)),
        "unprotected target must execute with force=true at the boundary, got: {:?}",
        rec.recorded()
    );
}
