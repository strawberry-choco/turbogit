//! Issue 28 — merge dialog upgrade (screen 14).
//!
//! Drives the real `turbogit_ui::ui::render()` through `egui_kittest` against
//! temporary git repositories, asserting painted labels, public `AppState`
//! transitions, and the exact `MergeOpts` handed to the executor boundary
//! (via [`RecordingExecutor`]).
//!
//! Covered behaviors:
//! - STRATEGY segmented control (No-commit / Commit / Squash / Fast-forward);
//!   clicking a segment selects it
//! - the strategy + option rows map onto the dispatched `MergeOpts`
//! - the preview box shows the merge-commit count and file/insertion/deletion
//!   totals computed from the real repository before merging
//! - the cascade banner appears only when sibling repos share the branch
//!   being merged into; "View plan →" opens the cascade preflight for them

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui_kittest::kittest::Queryable as _;
use egui_kittest::{Harness, Node};
use test_support::RecordingExecutor;
use test_support::harness::{assert_not_painted, assert_painted};
use turbogit_app::state::{AppState, Dialog};
use turbogit_domain::model::{MergeOpts, MergeStrategy, RootId, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_services::bulk_ops::BulkOp;

// ---------------------------------------------------------------- helpers --

/// Run `git <args>` in `dir`, asserting success; returns stdout.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git should be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Append `text` to `<dir>/<name>`, stage, commit.
fn commit(dir: &Path, name: &str, text: &str) {
    let file = dir.join(name);
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
        .expect("opening work file");
    use std::io::Write;
    writeln!(f, "{text}").expect("appending work file");
    drop(f);
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", text]);
}

fn temp_repo(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q", "-b", "main"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    commit(&path, "base.txt", "base");
    path
}

/// One repo on `main` that diverged from a local `feature` branch: feature
/// carries two commits (a.txt +1 line; b.txt rewritten +1/−1) and main moved
/// on with its own commit (c.txt), so the merge is a true merge.
fn merge_repo(parent: &Path, name: &str) -> PathBuf {
    let repo = temp_repo(parent, name);
    commit(&repo, "a.txt", "a1");
    commit(&repo, "b.txt", "b1");
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    commit(&repo, "a.txt", "a2");
    std::fs::write(repo.join("b.txt"), "b2\nb3\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "rewrite b"]);
    git(&repo, &["checkout", "-q", "main"]);
    commit(&repo, "c.txt", "c1");
    repo
}

/// AppState with a recording executor wrapped around the real CLI engine.
fn app_state_recording(project: &Path, roots: &[PathBuf]) -> (AppState, Arc<RecordingExecutor>) {
    let exec: Arc<RecordingExecutor> = Arc::new(RecordingExecutor::new(Arc::new(CliExecutor {
        settings: VcsSettings::default(),
    })));
    let state = AppState::for_roots(project, roots)
        .with_executor(exec.clone())
        .with_settings(VcsSettings::default());
    (state, exec)
}

/// Headless harness driving the full app UI with event draining per frame.
fn harness(state: AppState) -> Harness<'static, AppState> {
    Harness::builder().with_max_steps(1024).build_ui_state(
        |ui, state| {
            state.drain_events();
            turbogit_ui::ui::render(ui, state);
        },
        state,
    )
}

/// Open the Merge dialog on the given root.
fn open_merge(h: &mut Harness<'_, AppState>, root: &Path) {
    h.state_mut().selected_root = Some(RootId(root.to_path_buf().into()));
    h.state_mut().ui.dialog = Some(Dialog::Merge);
    h.run();
}

/// Pick `branch` as the source branch through the picker (Change… → click).
fn pick_source_branch(h: &mut Harness<'_, AppState>, branch: &str) {
    dialog_node(h, "Change…").click();
    h.run();
    dialog_node(h, branch).click();
    h.run();
}

/// The node labeled `label` inside the merge dialog. The shell paints other
/// nodes with the same labels (the Commit tab, sidebar branch names, menu
/// items), so disambiguate by proximity to the in-dialog "Change…" anchor.
fn dialog_node<'h>(h: &'h Harness<'_, AppState>, label: &'h str) -> Node<'h> {
    let anchor = h
        .query_all_by_label("Change…")
        .next()
        .expect("the dialog's Change… anchor")
        .rect();
    let mut best: Option<(f32, Node<'h>)> = None;
    for node in h.query_all_by_label(label) {
        let d = node.rect().center().distance(anchor.center());
        if best.as_ref().is_none_or(|(bd, _)| d < *bd) {
            best = Some((d, node));
        }
    }
    best.expect("no in-dialog node with the label").1
}

/// The dialog's footer Merge button.
fn dialog_merge_button<'h>(h: &'h Harness<'_, AppState>) -> Node<'h> {
    h.query_all_by_label("Merge")
        .next()
        .expect("the dialog Merge button")
}

/// Poll until `f` is true or the deadline elapses.
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

// ------------------------------------------------------------------ tests --

#[test]
fn dialog_paints_picker_strategy_and_options() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = merge_repo(tmp.path(), "repo");
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_merge(&mut h, &repo);

    assert_painted(&h, "Merge into main");
    assert_painted(&h, "SOURCE BRANCH");
    assert_painted(&h, "STRATEGY");
    for segment in ["No-commit", "Commit", "Squash", "Fast-forward"] {
        assert_painted(&h, segment);
    }
    assert_painted(&h, "OPTIONS");
    assert_painted(&h, "No fast-forward (--no-ff)");
    assert_painted(&h, "Verify signatures on incoming");
    assert_painted(
        &h,
        "Allow unrelated histories (--allow-unrelated-histories)",
    );
}

#[test]
fn picking_a_source_branch_shows_the_preview_totals() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = merge_repo(tmp.path(), "repo");
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_merge(&mut h, &repo);
    assert_not_painted(&h, "Will create");

    pick_source_branch(&mut h, "feature");

    // feature carries +1 (a.txt) and +2/−1 (b.txt): a true merge creates
    // 1 merge commit bringing in 2 files, 3 insertions, 1 deletion.
    assert_painted(&h, "Will create 1 merge commit");
    assert_painted(&h, "2 files changed · 3 insertions · 1 deletion");
}

#[test]
fn strategy_clicks_select_the_segment() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = merge_repo(tmp.path(), "repo");
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_merge(&mut h, &repo);

    // No-commit is the default selection.
    assert_eq!(h.state().ui.dlg.merge_strategy, MergeStrategy::NoCommit);
    for (label, want) in [
        ("Commit", MergeStrategy::Commit),
        ("Squash", MergeStrategy::Squash),
        ("Fast-forward", MergeStrategy::FastForward),
    ] {
        // Click inside the dialog only: pick the matching segment whose
        // rect is in the modal (the centered dialog, below the tab strip).
        dialog_node(&h, label).click();
        h.run();
        assert_eq!(h.state().ui.dlg.merge_strategy, want, "clicked {label}");
    }
}

#[test]
fn strategy_and_options_dispatch_the_exact_merge_opts() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = merge_repo(tmp.path(), "repo");
    let (mut state, exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    state.ui.dlg.merge_strategy = MergeStrategy::Squash;
    state.ui.dlg.merge_allow_unrelated = true;
    let mut h = harness(state);
    open_merge(&mut h, &repo);

    dialog_node(&h, "Verify signatures on incoming").click();
    pick_source_branch(&mut h, "feature");
    dialog_node(&h, "Merge").click();
    h.run();

    let ok = wait_until(5_000, || {
        exec.recorded()
            .iter()
            .any(|c| matches!(c, test_support::RecordedCall::Merge { .. }))
    });
    assert!(ok, "no merge was dispatched");
    let merges: Vec<(String, MergeOpts)> = exec
        .recorded()
        .iter()
        .filter_map(|c| match c {
            test_support::RecordedCall::Merge { target, opts, .. } => {
                Some((target.clone(), opts.clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(merges.len(), 1, "exactly one merge dispatched");
    assert_eq!(merges[0].0, "feature");
    assert_eq!(
        merges[0].1,
        MergeOpts {
            squash: true,
            verify_signatures: true,
            allow_unrelated: true,
            ..Default::default()
        }
    );
}

#[test]
fn fast_forward_strategy_drops_a_leftover_no_ff_option() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = merge_repo(tmp.path(), "repo");
    let (mut state, exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    state.ui.dlg.merge_strategy = MergeStrategy::FastForward;
    state.ui.dlg.merge_no_ff = true; // leftover toggle from an earlier pick
    let mut h = harness(state);
    open_merge(&mut h, &repo);

    pick_source_branch(&mut h, "feature");
    dialog_node(&h, "Merge").click();
    h.run();

    let ok = wait_until(5_000, || {
        exec.recorded()
            .iter()
            .any(|c| matches!(c, test_support::RecordedCall::Merge { .. }))
    });
    assert!(ok, "no merge was dispatched");
    let opts = exec
        .recorded()
        .iter()
        .find_map(|c| match c {
            test_support::RecordedCall::Merge { opts, .. } => Some(opts.clone()),
            _ => None,
        })
        .expect("a merge was recorded");
    assert!(opts.ff_only);
    assert!(!opts.no_ff);
}

#[test]
fn cascade_banner_names_the_sibling_repos_and_view_plan_opens_the_preflight() {
    let tmp = tempfile::tempdir().unwrap();
    let focused = merge_repo(tmp.path(), "focused");
    let sibling = merge_repo(tmp.path(), "sibling");
    let (state, _exec) = app_state_recording(tmp.path(), &[focused.clone(), sibling.clone()]);
    let mut h = harness(state);
    open_merge(&mut h, &focused);
    pick_source_branch(&mut h, "feature");

    assert_painted(&h, "Cascade with 1 other repo after this merge");
    dialog_node(&h, "View plan →").click();
    h.run();

    // The hand-off opens the cascade preflight for the sibling repos.
    assert_eq!(h.state().ui.bulk_op, Some(BulkOp::Merge));
    assert_eq!(
        h.state()
            .ui
            .repo_selection
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![RootId(sibling.clone().into())],
    );
    // The preflight matrix shows the sibling and what it will do.
    assert_painted(&h, "sibling");
    assert_painted(&h, "Merge feature");
}

#[test]
fn no_cascade_banner_without_sibling_repos() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = merge_repo(tmp.path(), "solo");
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_merge(&mut h, &repo);
    pick_source_branch(&mut h, "feature");

    assert_not_painted(&h, "Cascade with");
    assert_not_painted(&h, "View plan →");
}
