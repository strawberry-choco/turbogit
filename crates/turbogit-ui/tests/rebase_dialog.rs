//! Issue 29 — rebase dialog upgrade (screen 15).
//!
//! Drives the real `turbogit_ui::ui::render()` through `egui_kittest` against
//! temporary git repositories, asserting painted labels, public `AppState`
//! transitions, and the exact rebase invocation handed to the executor
//! boundary (via [`RecordingExecutor`]).
//!
//! Covered behaviors:
//! - the MODE segmented control (Interactive / Standard / Autosquash);
//!   clicking a segment selects it
//! - the ONTO BRANCH picker and the commits-to-rebase list computed from the
//!   real repository before starting
//! - the mode + option rows map onto the dispatched invocation (plan replay
//!   for Interactive, `RebaseOpts` for Standard/Autosquash)
//! - the cross-repo warning banner names the affected repos and
//!   "View affected →" hands off to the affected-repo list
//! - a protected current branch blocks the start button

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui_kittest::kittest::Queryable as _;
use egui_kittest::{Harness, Node};
use test_support::RecordingExecutor;
use test_support::harness::{assert_not_painted, assert_painted};
use turbogit_app::state::{AppState, Dialog};
use turbogit_domain::model::{RebaseMode, RootId, VcsSettings};
use turbogit_engine::cli::CliExecutor;

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

/// One repo on `main` (one base commit) with a local `feature` branch two
/// commits ahead, checked out — the rebase subject.
fn rebase_repo(parent: &Path, name: &str) -> PathBuf {
    let repo = temp_repo(parent, name);
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    commit(&repo, "a.txt", "feature-1");
    commit(&repo, "b.txt", "feature-2");
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

/// Open the Rebase dialog on the given root.
fn open_rebase(h: &mut Harness<'_, AppState>, root: &Path) {
    h.state_mut().selected_root = Some(RootId(root.to_path_buf().into()));
    h.state_mut().ui.dialog = Some(Dialog::Rebase);
    h.run();
}

/// The node labeled `label` inside the rebase dialog. The shell paints other
/// nodes with the same labels, so disambiguate by proximity to the in-dialog
/// "Change…" anchor.
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
fn dialog_paints_mode_onto_and_options() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = rebase_repo(tmp.path(), "repo");
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_rebase(&mut h, &repo);

    // The title names the branch being rewritten (screen 15).
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
    // Interactive is the default mode; the start button names it.
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

    // feature carries two commits not on main; the list names them before
    // anything runs.
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

/// The rebase calls recorded at the executor boundary.
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

/// The interactive plans recorded at the executor boundary.
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
    // The "Autosquash fixup commits" row is left unticked — the mode itself
    // must turn the flag on.
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
    // Two repos of one project, both with feature checked out: rewriting
    // focused's feature diverges sibling's feature too.
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

    // The hand-off scopes the fleet selection to the affected repos — the
    // affected-repo list — and closes the dialog.
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

    // Nothing may reach the executor boundary: the service guard refuses a
    // protected rewrite, and the disabled start button never dispatches.
    assert!(recorded_rebases(&exec).is_empty(), "a plain rebase escaped");
    assert!(recorded_plans(&exec).is_empty(), "a plan replay escaped");
}
