#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui_kittest::kittest::Queryable as _;
use egui_kittest::{Harness, Node};
use test_support::RecordingExecutor;
use test_support::git_seed::{commit as git_seed_commit, git};
use test_support::harness::{
    KITTEST_DEFAULT_BOX, assert_not_painted, assert_painted, shell_harness_over_unstyled,
};
use test_support::wait_until;
use turbogit_app::state::{AppState, Dialog};
use turbogit_domain::model::{MergeOpts, MergeStrategy, RootId, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_services::bulk_ops::BulkOp;

fn commit(dir: &Path, name: &str, text: &str) {
    let file = dir.join(name);
    let mut body = std::fs::read_to_string(&file).unwrap_or_default();
    body.push_str(text);
    body.push('\n');
    git_seed_commit(dir, name, &body, text);
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
///
/// Deliberately unstyled: the production tokens and the embedded font would change
/// what this frame paints. `max_steps` is 1024 rather than kittest's default of 4,
/// and `Harness::run` panics once a run exceeds its budget.
fn harness(state: AppState) -> Harness<'static, AppState> {
    shell_harness_over_unstyled(state, KITTEST_DEFAULT_BOX, 1024)
}

fn open_merge(h: &mut Harness<'_, AppState>, root: &Path) {
    h.state_mut().selected_root = Some(RootId(root.to_path_buf().into()));
    h.state_mut().ui.dialog = Some(Dialog::Merge);
    h.run();
}

fn pick_source_branch(h: &mut Harness<'_, AppState>, branch: &str) {
    dialog_node(h, "Change…").click();
    h.run();
    dialog_node(h, branch).click();
    h.run();
}

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

fn dialog_merge_button<'h>(h: &'h Harness<'_, AppState>) -> Node<'h> {
    h.query_all_by_label("Merge")
        .next()
        .expect("the dialog Merge button")
}

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

    assert_eq!(h.state().ui.dlg.merge_strategy, MergeStrategy::NoCommit);
    for (label, want) in [
        ("Commit", MergeStrategy::Commit),
        ("Squash", MergeStrategy::Squash),
        ("Fast-forward", MergeStrategy::FastForward),
    ] {
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
    state.ui.dlg.merge_no_ff = true;
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
