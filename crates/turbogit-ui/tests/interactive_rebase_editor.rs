//! Issue 30 — interactive rebase editor rework (screen 17).
//!
//! Drives the real `turbogit_ui::ui::render()` through `egui_kittest` against
//! temporary git repositories, asserting painted labels, public `AppState`
//! transitions, and the calls recorded at the executor boundary.
//!
//! Covered behaviors:
//! - the Plan / Preview / Log tab strip and the plan rows with action chips
//!   and move buttons
//! - the ⇧↑/⇧↓ and p/s/f/d keyboard shortcuts on the selected row
//! - the RESULT PREVIEW tab: post-plan sequence, N → M counts, fold/drop
//!   summaries
//! - the Log tab: the raw REBASE-TODO edits re-parse back into the plan
//! - the right rail: CAUTIONS, AFFECTED REPOS, RECOVERY naming the backup ref
//! - the footer: time estimate, and Start rebase writing the backup ref at
//!   the pre-rebase HEAD before the plan replays

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable as _;
use egui_kittest::{Harness, Node};
use test_support::RecordingExecutor;
use test_support::harness::{assert_not_painted, assert_painted};
use turbogit_app::state::{AppState, Dialog};
use turbogit_domain::model::{RootId, VcsSettings};
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

/// One repo on `main` (one base commit) with a local `feature` branch three
/// commits ahead, checked out — the interactive-editing subject.
fn plan_repo(parent: &Path, name: &str) -> PathBuf {
    let repo = temp_repo(parent, name);
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    commit(&repo, "a.txt", "feature-1");
    commit(&repo, "b.txt", "feature-2");
    commit(&repo, "c.txt", "feature-3");
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
    let mut h = Harness::builder().with_max_steps(1024).build_ui_state(
        |ui, state| {
            state.drain_events();
            turbogit_ui::ui::render(ui, state);
        },
        state,
    );
    // The editor window is wider than kittest's 800×600 default; the
    // right-aligned footer button needs the viewport to contain it.
    h.set_size(egui::vec2(1400.0, 900.0));
    h
}

/// Open the interactive rebase editor on the oldest commit past `main` —
/// the whole feature range becomes the plan.
fn open_editor(h: &mut Harness<'_, AppState>, root: &Path) {
    let log = git(root, &["log", "main..HEAD", "--format=%H"]);
    let oldest = log.lines().last().expect("a commit past main").trim();
    h.state_mut().selected_root = Some(RootId(root.to_path_buf().into()));
    h.state_mut().ui.selected_commit = Some(oldest.to_string());
    h.state_mut().ui.dialog = Some(Dialog::InteractiveRebase);
    h.run();
}

/// The node labeled `label` nearest to `anchor`'s rect — per-row controls
/// repeat labels across rows, so pick by proximity to the row's subject.
fn nearest<'h>(h: &'h Harness<'_, AppState>, label: &'h str, anchor: &str) -> Node<'h> {
    let anchor = h
        .query_all_by_label(anchor)
        .next()
        .expect("the anchor node")
        .rect();
    let mut best: Option<(f32, Node<'h>)> = None;
    for node in h.query_all_by_label(label) {
        let d = node.rect().center().distance(anchor.center());
        if best.as_ref().is_none_or(|(bd, _)| d < *bd) {
            best = Some((d, node));
        }
    }
    best.expect("no node with the label").1
}

/// Poll until `f` is true or the deadline elapses.
fn wait_until<F: FnMut() -> bool>(ms: u64, mut f: F) -> bool {
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

// ------------------------------------------------------------------ tests --

#[test]
fn the_editor_paints_tabs_and_plan_rows_with_chips_and_moves() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = plan_repo(tmp.path(), "repo");
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_editor(&mut h, &repo);

    // Screen 17: the tab strip, the title, every plan row (oldest first),
    // the action chips, and the move buttons.
    for tab in ["Plan", "Preview", "Log"] {
        assert_painted(&h, tab);
    }
    assert_painted(&h, "Interactive rebase");
    for subject in ["feature-1", "feature-2", "feature-3"] {
        assert_painted(&h, subject);
    }
    for chip in ["pick", "reword", "squash", "fixup", "drop"] {
        assert_painted(&h, chip);
    }
    assert_painted(&h, "↑");
    assert_painted(&h, "↓");
}

#[test]
fn chips_retarget_their_row_and_the_move_buttons_reorder() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = plan_repo(tmp.path(), "repo");
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_editor(&mut h, &repo);

    // Squash the oldest commit via its row's chip.
    nearest(&h, "squash", "feature-1").click();
    h.run();
    let plan = h.state().ui.dlg.rebase_plan.clone().expect("plan");
    assert_eq!(
        plan[0].action,
        turbogit_domain::model::RebaseAction::Squash,
        "full plan: {plan:?}"
    );

    // Move it down one slot with the row's ↓ button.
    nearest(&h, "↓", "feature-1").click();
    h.run();
    let plan = h.state().ui.dlg.rebase_plan.clone().expect("plan");
    let subjects: Vec<&str> = plan.iter().map(|e| e.subject.as_str()).collect();
    assert_eq!(subjects, ["feature-2", "feature-1", "feature-3"]);

    // And back up with ↑.
    nearest(&h, "↑", "feature-1").click();
    h.run();
    let plan = h.state().ui.dlg.rebase_plan.clone().expect("plan");
    let subjects: Vec<&str> = plan.iter().map(|e| e.subject.as_str()).collect();
    assert_eq!(subjects, ["feature-1", "feature-2", "feature-3"]);
}

#[test]
fn dragging_a_row_onto_another_reorders_the_plan() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = plan_repo(tmp.path(), "repo");
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_editor(&mut h, &repo);

    // Drag the feature-1 row down onto the feature-3 row: press, move the
    // pointer over the target (the plan reorders live), release.
    let from = nearest(&h, "feature-1", "feature-1").rect();
    let to = nearest(&h, "feature-3", "feature-3").rect();
    h.drag_at(from.center());
    h.run();
    // Pass egui's drag threshold before moving to the target row.
    h.hover_at(from.center() + egui::vec2(6.0, 0.0));
    h.run();
    h.hover_at(to.center());
    h.run();
    h.drop_at(to.center());
    h.run();

    let plan = h.state().ui.dlg.rebase_plan.clone().expect("plan");
    let subjects: Vec<&str> = plan.iter().map(|e| e.subject.as_str()).collect();
    assert_eq!(subjects, ["feature-2", "feature-3", "feature-1"]);
}

#[test]
fn keyboard_shortcuts_move_and_retarget_the_selected_row() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = plan_repo(tmp.path(), "repo");
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_editor(&mut h, &repo);

    // Click the oldest row to focus it.
    nearest(&h, "feature-1", "feature-1").click();
    h.run();

    // ⇧↓ moves the selected row down; the selection follows it.
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowDown);
    h.run();
    let plan = h.state().ui.dlg.rebase_plan.clone().expect("plan");
    let subjects: Vec<&str> = plan.iter().map(|e| e.subject.as_str()).collect();
    assert_eq!(subjects, ["feature-2", "feature-1", "feature-3"]);

    // p/s/f/d retarget the selected row's action: squash feature-1.
    h.key_press(egui::Key::S);
    h.run();
    let plan = h.state().ui.dlg.rebase_plan.clone().expect("plan");
    assert_eq!(
        plan[1].action,
        turbogit_domain::model::RebaseAction::Squash,
        "the selection followed the move, so 's' squashes feature-1"
    );

    // ⇧↑ moves it back up.
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowUp);
    h.run();
    let plan = h.state().ui.dlg.rebase_plan.clone().expect("plan");
    let subjects: Vec<&str> = plan.iter().map(|e| e.subject.as_str()).collect();
    assert_eq!(subjects, ["feature-1", "feature-2", "feature-3"]);
    assert_eq!(
        plan[0].action,
        turbogit_domain::model::RebaseAction::Squash,
        "the action stays with its commit"
    );

    // And "d" drops the selected (moved-back) row.
    h.key_press(egui::Key::D);
    h.run();
    let plan = h.state().ui.dlg.rebase_plan.clone().expect("plan");
    assert_eq!(
        plan[0].action,
        turbogit_domain::model::RebaseAction::Drop,
        "'d' drops the selected row"
    );
}

fn plan_entry(
    action: turbogit_domain::model::RebaseAction,
    commit: &str,
    subject: &str,
) -> turbogit_domain::model::RebasePlanEntry {
    turbogit_domain::model::RebasePlanEntry {
        action,
        commit: commit.to_string(),
        subject: subject.to_string(),
    }
}

#[test]
fn the_preview_tab_shows_the_result_sequence_counts_and_folds() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = plan_repo(tmp.path(), "repo");
    let (mut state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    use turbogit_domain::model::RebaseAction::{Drop, Fixup, Pick, Squash};
    // Preset the plan so no repository rebuild overwrites the fold shape:
    // pick A, fixup B, squash C, drop D, pick E → 5 → 2 commits.
    state.ui.dlg.rebase_plan = Some(vec![
        plan_entry(Pick, "a91c3f7aaa", "feat: dry-run mode"),
        plan_entry(Fixup, "f7c1d08bbb", "wip: trim checks"),
        plan_entry(Squash, "3a90be2ccc", "wip: rename hooks"),
        plan_entry(Drop, "7d10b4eddd", "test: redundant case"),
        plan_entry(Pick, "f4e2a91eee", "refactor: split"),
    ]);
    let mut h = harness(state);
    h.state_mut().selected_root = Some(RootId(repo.to_path_buf().into()));
    h.state_mut().ui.dialog = Some(Dialog::InteractiveRebase);
    h.run();

    nearest(&h, "Preview", "Plan").click();
    h.run();

    assert_painted(&h, "5 → 2 COMMITS");
    assert_painted(&h, "f7c1d08 + 3a90be2 folded into a91c3f7");
    assert_painted(&h, "1 commit dropped");
    // The post-plan sequence names the survivors in replay order.
    assert_painted(&h, "feat: dry-run mode");
    assert_painted(&h, "refactor: split");
    assert_not_painted(&h, "test: redundant case");
}

#[test]
fn editing_the_raw_todo_reparses_into_the_plan() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = plan_repo(tmp.path(), "repo");
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_editor(&mut h, &repo);

    nearest(&h, "Log", "Plan").click();
    h.run();

    // The generated REBASE-TODO is in the buffer: one line per plan entry.
    let todo = h.state().ui.dlg.rebase_todo.clone();
    assert_eq!(
        todo.lines().count(),
        3,
        "one line per planned commit: {todo}"
    );

    // Append a line as if hand-editing; it must parse back into the plan.
    h.get_by_role_and_label(Role::TextInput, "REBASE-TODO")
        .focus();
    h.run();
    h.get_by_role_and_label(Role::TextInput, "REBASE-TODO")
        .type_text("drop abc1234 extra commit");
    h.run();

    let plan = h.state().ui.dlg.rebase_plan.clone().expect("plan");
    assert_eq!(plan.len(), 4, "the appended line joins the plan");
    assert_eq!(plan[3].action, turbogit_domain::model::RebaseAction::Drop);
    assert_eq!(plan[3].commit, "abc1234");
    assert_eq!(plan[3].subject, "extra commit");
}

#[test]
fn the_rail_names_computed_cautions_and_the_recovery_ref() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = plan_repo(tmp.path(), "repo");
    // A fourth commit re-edits a.txt (conflict risk) authored by a second
    // identity (mixed committers).
    git(&repo, &["config", "user.name", "Other"]);
    commit(&repo, "a.txt", "feature-4");

    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_editor(&mut h, &repo);

    assert_painted(&h, "CAUTIONS");
    assert_painted(&h, "Conflicts likely on 1 file");
    assert_painted(&h, "Mixed committer identities (2 authors)");
    // RECOVERY names the backup ref that restores the pre-rebase state.
    assert_painted(&h, "RECOVERY");
    assert_painted(&h, "refs/turbogit/preflight-backup");
}

#[test]
fn the_rail_lists_the_affected_repos() {
    let tmp = tempfile::tempdir().unwrap();
    // Two repos of one project with feature checked out: the rewrite
    // affects both.
    let focused = plan_repo(tmp.path(), "focused");
    let sibling = plan_repo(tmp.path(), "sibling");
    let (state, _exec) = app_state_recording(tmp.path(), &[focused.clone(), sibling.clone()]);
    let mut h = harness(state);
    open_editor(&mut h, &focused);

    assert_painted(&h, "AFFECTED REPOS");
    assert_painted(&h, "sibling");
    assert_painted(&h, "SHORTCUTS");
}

#[test]
fn the_footer_estimates_and_start_dispatches_the_backup_backed_plan() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = plan_repo(tmp.path(), "repo");
    let (state, exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_editor(&mut h, &repo);

    // Three replayed commits at ~3s each.
    assert_painted(&h, "Ready to rebase 3 commits");
    assert_painted(&h, "~9s estimated");

    let pre_head = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    h.get_by_label("Start rebase").click();
    // Advance frames while polling: the dispatch runs on a worker thread
    // and the queued click events may need more than one run loop.
    let ok = wait_until(5_000, || {
        h.run();
        !recorded_plans(&exec).is_empty()
    });
    assert!(ok, "no plan was dispatched");
    h.run();

    // Exactly the editor's plan reached the boundary, and the backup ref
    // was written at the pre-rebase HEAD before the first replay.
    let plans = recorded_plans(&exec);
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].len(), 3);
    assert_eq!(
        git(&repo, &["rev-parse", "refs/turbogit/preflight-backup"]).trim(),
        pre_head,
        "the backup ref names the pre-rebase state"
    );
    // The editor closed itself after dispatching.
    assert_eq!(h.state().ui.dialog, None);
    assert_eq!(h.state().ui.dlg.rebase_plan, None);
}
