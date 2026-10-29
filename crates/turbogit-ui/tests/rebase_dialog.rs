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
use turbogit_domain::model::{RebaseMode, RootId, VcsSettings};
use turbogit_engine::cli::CliExecutor;

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

fn rebase_repo(parent: &Path, name: &str) -> PathBuf {
    let repo = temp_repo(parent, name);
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    commit(&repo, "a.txt", "feature-1");
    commit(&repo, "b.txt", "feature-2");
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

fn harness(state: AppState) -> Harness<'static, AppState> {
    shell_harness_over_unstyled(state, KITTEST_DEFAULT_BOX, 1024)
}

fn open_rebase(h: &mut Harness<'_, AppState>, root: &Path) {
    h.state_mut().selected_root = Some(RootId(root.to_path_buf().into()));
    h.state_mut().ui.dialog = Some(Dialog::Rebase);
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

#[test]
fn dialog_paints_mode_onto_and_options() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = rebase_repo(tmp.path(), "repo");
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_rebase(&mut h, &repo);

    assert_painted(&h, "Rebase feature");
    assert_painted(&h, "ONTO BRANCH");
    assert_painted(&h, "MODE");
    for segment in ["Interactive", "Standard", "Autosquash"] {
        assert_painted(&h, segment);
    }
    assert_painted(&h, "OPTIONS");
    assert_painted(&h, "Autosquash fixup commits");
    assert_painted(&h, "Update branches (--update-refs)");
    assert_painted(&h, "Keep empty commits");
    assert_eq!(h.state().ui.dlg.rebase_mode, RebaseMode::Interactive);
    assert_painted(&h, "Start interactive rebase");
}

#[test]
fn picking_an_onto_branch_lists_the_commits_to_rebase() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = rebase_repo(tmp.path(), "repo");
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_rebase(&mut h, &repo);
    assert_not_painted(&h, "COMMITS TO REBASE");

    dialog_node(&h, "Change…").click();
    h.run();
    dialog_node(&h, "main").click();
    h.run();

    assert_painted(&h, "2 COMMITS TO REBASE");
    assert_painted(&h, "feature-1");
    assert_painted(&h, "feature-2");
}

#[test]
fn mode_clicks_select_the_segment() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = rebase_repo(tmp.path(), "repo");
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_rebase(&mut h, &repo);

    for (label, want) in [
        ("Standard", RebaseMode::Standard),
        ("Autosquash", RebaseMode::Autosquash),
        ("Interactive", RebaseMode::Interactive),
    ] {
        dialog_node(&h, label).click();
        h.run();
        assert_eq!(h.state().ui.dlg.rebase_mode, want, "clicked {label}");
    }
}

fn recorded_rebases(exec: &RecordingExecutor) -> Vec<(String, turbogit_domain::model::RebaseOpts)> {
    exec.recorded()
        .iter()
        .filter_map(|c| match c {
            test_support::RecordedCall::Rebase { onto, opts, .. } => {
                Some((onto.clone(), opts.clone()))
            }
            _ => None,
        })
        .collect()
}

fn recorded_plans(exec: &RecordingExecutor) -> Vec<Vec<turbogit_domain::model::RebasePlanEntry>> {
    exec.recorded()
        .iter()
        .filter_map(|c| match c {
            test_support::RecordedCall::RebaseInteractive { plan, .. } => Some(plan.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn interactive_mode_dispatches_the_plan_replay() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = rebase_repo(tmp.path(), "repo");
    let (state, exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_rebase(&mut h, &repo);

    dialog_node(&h, "Change…").click();
    h.run();
    dialog_node(&h, "main").click();
    h.run();
    dialog_node(&h, "Start interactive rebase").click();
    h.run();

    let ok = wait_until(5_000, || !recorded_plans(&exec).is_empty());
    assert!(ok, "no interactive plan was dispatched");
    let plans = recorded_plans(&exec);
    assert_eq!(plans.len(), 1);
    let subjects: Vec<&str> = plans[0].iter().map(|e| e.subject.as_str()).collect();
    assert_eq!(
        subjects,
        ["feature-1", "feature-2"],
        "oldest-first all-pick plan"
    );
    assert!(
        recorded_rebases(&exec).is_empty(),
        "no plain rebase dispatched"
    );
}

#[test]
fn standard_mode_dispatches_the_mode_mapped_rebase() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = rebase_repo(tmp.path(), "repo");
    let (mut state, exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    state.ui.dlg.rebase_mode = RebaseMode::Standard;
    let mut h = harness(state);
    open_rebase(&mut h, &repo);

    dialog_node(&h, "Update branches (--update-refs)").click();
    dialog_node(&h, "Change…").click();
    h.run();
    dialog_node(&h, "main").click();
    h.run();
    dialog_node(&h, "Start rebase").click();
    h.run();

    let ok = wait_until(5_000, || !recorded_rebases(&exec).is_empty());
    assert!(ok, "no rebase was dispatched");
    let rebases = recorded_rebases(&exec);
    assert_eq!(rebases.len(), 1);
    assert_eq!(rebases[0].0, "main");
    assert_eq!(
        rebases[0].1,
        turbogit_domain::model::RebaseOpts {
            update_refs: true,
            ..Default::default()
        }
    );
    assert!(recorded_plans(&exec).is_empty(), "no plan dispatched");
}

#[test]
fn autosquash_mode_forces_the_flag_in_the_dispatch() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = rebase_repo(tmp.path(), "repo");
    let (mut state, exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    state.ui.dlg.rebase_mode = RebaseMode::Autosquash;
    let mut h = harness(state);
    open_rebase(&mut h, &repo);

    dialog_node(&h, "Change…").click();
    h.run();
    dialog_node(&h, "main").click();
    h.run();
    dialog_node(&h, "Start autosquash rebase").click();
    h.run();

    let ok = wait_until(5_000, || !recorded_rebases(&exec).is_empty());
    assert!(ok, "no rebase was dispatched");
    let rebases = recorded_rebases(&exec);
    assert_eq!(rebases.len(), 1);
    assert!(rebases[0].1.autosquash, "autosquash mode must set the flag");
}

#[test]
fn cross_repo_banner_names_the_rewrite_and_view_affect_scopes_the_selection() {
    let tmp = tempfile::tempdir().unwrap();
    let focused = rebase_repo(tmp.path(), "focused");
    let sibling = rebase_repo(tmp.path(), "sibling");
    let (state, _exec) = app_state_recording(tmp.path(), &[focused.clone(), sibling.clone()]);
    let mut h = harness(state);
    open_rebase(&mut h, &focused);
    assert_not_painted(&h, "Rewrites");

    dialog_node(&h, "Change…").click();
    h.run();
    dialog_node(&h, "main").click();
    h.run();

    assert_painted(&h, "Rewrites 2 commits across 2 repos");
    dialog_node(&h, "View affected →").click();
    h.run();

    let selection = h.state().ui.repo_selection.clone();
    assert_eq!(selection.len(), 1);
    assert!(selection.contains(&RootId(sibling.to_path_buf().into())));
    assert_eq!(h.state().ui.dialog, None);
}

#[test]
fn no_banner_without_affected_siblings() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = rebase_repo(tmp.path(), "repo");
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_rebase(&mut h, &repo);

    dialog_node(&h, "Change…").click();
    h.run();
    dialog_node(&h, "main").click();
    h.run();

    assert_not_painted(&h, "Rewrites");
    assert_not_painted(&h, "View affected →");
}

#[test]
fn protected_branch_blocks_the_start_button() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = rebase_repo(tmp.path(), "repo");
    let (mut state, exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    state.settings.protected_branch_patterns = vec!["feature".to_string()];
    let mut h = harness(state);
    open_rebase(&mut h, &repo);

    dialog_node(&h, "Change…").click();
    h.run();
    dialog_node(&h, "main").click();
    h.run();
    dialog_node(&h, "Start interactive rebase").click();
    h.run();

    assert!(recorded_rebases(&exec).is_empty(), "a plain rebase escaped");
    assert!(recorded_plans(&exec).is_empty(), "a plan replay escaped");
}
