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
//! - the body-to-footer rule: the shared `RULE_FOOTER` tone plus the 1px
//!   full-width geometry, which no acceptance capture covers

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::accesskit::Role;
use egui::{Color32, Pos2, Rect};
use egui_kittest::kittest::Queryable as _;
use egui_kittest::{Harness, Node};
use test_support::RecordingExecutor;
use test_support::git_seed::{commit as git_seed_commit, git};
use test_support::harness::{
    assert_not_painted, assert_painted, filled_rects, galley_origin, painted_galleys,
    shell_harness_over_unstyled,
};
use test_support::wait_until;
use turbogit_app::state::{AppState, Dialog};
use turbogit_domain::model::{RootId, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_ui::theme::Palette;

// ---------------------------------------------------------------- helpers --

/// Append `text` to `<dir>/<name>`, stage, commit.
fn commit(dir: &Path, name: &str, text: &str) {
    let file = dir.join(name);
    let mut body = std::fs::read_to_string(&file).unwrap_or_default();
    body.push_str(text);
    body.push('\n');
    git_seed_commit(dir, name, &body, text);
}

/// Kept local rather than `git_seed::repo_with_one_commit`: the seeded commit is
/// `base.txt`, and `plan_repo` names those commits when it opens the editor.
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
///
/// Unstyled, and all three differences are load-bearing: `max_steps` is 1024
/// because `open_editor` drives the suite through `Harness::run()`, which PANICS
/// past the budget kittest defaults to 4; no `configure_style`, because `rule_tone`
/// documents the egui default `noninteractive.bg_stroke` this frame shows instead
/// of the production token set; and no `install_fonts`, because the embedded font
/// changes the glyph metrics the geometry assertions measure.
fn harness(state: AppState) -> Harness<'static, AppState> {
    // The editor window is wider than kittest's 800×600 default; the
    // right-aligned footer button needs the viewport to contain it.
    shell_harness_over_unstyled(state, egui::vec2(1400.0, 900.0), 1024)
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
    for chip in [
        "pick",
        "edit",
        "reword",
        "squash",
        "fixup",
        "drop",
        "update-ref",
    ] {
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

/// The two most common edits finally have keys: `r` rewords and `e` edits the
/// selected row; `u` marks it Update Ref. Each retargets exactly the selected
/// row — the neighbours' actions are untouched.
#[test]
fn reword_edit_and_update_ref_shortcuts_retarget_the_selected_row() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = plan_repo(tmp.path(), "repo");
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_editor(&mut h, &repo);

    nearest(&h, "feature-2", "feature-2").click();
    h.run();

    use turbogit_domain::model::RebaseAction;
    for (key, want, what) in [
        (
            egui::Key::R,
            RebaseAction::Reword,
            "'r' rewords the selected row",
        ),
        (
            egui::Key::E,
            RebaseAction::Edit,
            "'e' edits the selected row",
        ),
        (
            egui::Key::U,
            RebaseAction::UpdateRef,
            "'u' marks the selected row Update Ref",
        ),
    ] {
        h.key_press(key);
        h.run();
        let plan = h.state().ui.dlg.rebase_plan.clone().expect("plan");
        assert_eq!(plan[1].action, want, "{what}");
        assert_eq!(
            plan[0].action,
            RebaseAction::Pick,
            "{what}: the row above is untouched"
        );
        assert_eq!(
            plan[2].action,
            RebaseAction::Pick,
            "{what}: the row below is untouched"
        );
    }
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
        message: None,
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
fn plan_rows_render_unicode_commit_references_without_splitting_them() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = plan_repo(tmp.path(), "repo");
    let (mut state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    state.ui.dlg.rebase_plan = Some(vec![plan_entry(
        turbogit_domain::model::RebaseAction::Pick,
        "界界界界界界界界",
        "unicode reference",
    )]);
    let mut h = harness(state);
    h.state_mut().selected_root = Some(RootId(repo.to_path_buf().into()));
    h.state_mut().ui.dialog = Some(Dialog::InteractiveRebase);

    h.run();

    assert_painted(&h, "界界界界界界界");
    assert_not_painted(&h, "界界界界界界界界");
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

// --- the seven plan verbs -----------------------------------------------------
//
// The three verification assertions. All row claims are scoped by
// the row's own `Node::rect()` — every plan row is a labeled node ("plan row
// <subject>") — because verb words repeat across rows AND in the SHORTCUTS
// legend, so unscoped painted-text claims would collide with both.

/// The verb chips a row offers, painted once per row.
const CHIP_LABELS: [&str; 7] = [
    "pick",
    "edit",
    "reword",
    "squash",
    "fixup",
    "drop",
    "update-ref",
];

/// The plan row carrying `subject`, as a labeled node.
fn row_rect(h: &Harness<'_, AppState>, subject: &str) -> Rect {
    let label = format!("plan row {subject}");
    h.query_by_label(&label)
        .expect("the plan row is a labeled node")
        .rect()
}

/// The chip labeled `label` painted inside `row`'s rect.
fn chip_in_row<'h>(h: &'h Harness<'_, AppState>, label: &'h str, row: Rect) -> Node<'h> {
    h.query_all_by_label(label)
        .find(|n| row.contains(n.rect().center()))
        .unwrap_or_else(|| panic!("no {label} chip inside the row at {row:?}"))
}

/// Whether `chip`'s row currently marks that chip as the row's action: the
/// selected chip is the one egui paints a fill behind, roughly the chip's own
/// size (the row's background fill spans the whole row and is excluded by the
/// width bound).
fn chip_is_selected(h: &Harness<'_, AppState>, chip: &Node<'_>, row: Rect) -> bool {
    let chip_rect = chip.rect();
    filled_rects(h).iter().any(|(r, _)| {
        r.contains(chip_rect.center())
            && r.width() < chip_rect.width() + 12.0
            && r.height() < row.height()
    })
}

/// How many chip-label galleys paint inside each row — the "chip count per
/// row" the no-reflow assertion is made of.
fn chip_galley_count(h: &Harness<'_, AppState>, row: Rect) -> usize {
    painted_galleys(h)
        .iter()
        .filter(|g| CHIP_LABELS.contains(&g.text.as_str()) && row.contains(g.rect.center()))
        .count()
}

/// The row's marked action, as (chip label, galley origin): the chip whose
/// painted galley sits behind the selected chip's fill.
fn selected_chip_galley(h: &Harness<'_, AppState>, row: Rect) -> (String, Pos2) {
    let fills = filled_rects(h);
    let galleys = painted_galleys(h);
    let marked = galleys
        .iter()
        .filter(|g| CHIP_LABELS.contains(&g.text.as_str()) && row.contains(g.rect.center()))
        .find(|g| {
            fills
                .iter()
                .any(|(r, _)| r.contains(g.rect.center()) && r.width() < g.rect.width() + 16.0)
        });
    match marked {
        Some(g) => (g.text.clone(), g.pos),
        None => panic!("no selected chip galley in the row at {row:?}"),
    }
}

/// Verification 1 — a parsed REBASE-TODO containing `edit` and `pick` paints
/// an `Edit` chip on the first row and a `Pick` chip on the second, with no
/// shortcut-hint collisions: the SHORTCUTS legend names all three new keys,
/// and the row-scoped chip claims still hold beside it.
#[test]
fn a_parsed_todo_paints_the_edit_chip_on_its_row_and_pick_on_the_next() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = plan_repo(tmp.path(), "repo");
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_editor(&mut h, &repo);

    nearest(&h, "Log", "Plan").click();
    h.run();
    h.get_by_role_and_label(Role::TextInput, "REBASE-TODO")
        .focus();
    h.run();
    // The parsed lines join the generated todo (the suite's established
    // Log-tab flow): one edit row, one pick row, with subjects distinct from
    // the generated ones so the rows stay uniquely addressable.
    h.get_by_role_and_label(Role::TextInput, "REBASE-TODO")
        .type_text("edit 9b2e440a stop-to-edit\npick a91c3f7c keep-as-is");
    h.run();

    nearest(&h, "Plan", "Log").click();
    h.run();

    let edited = row_rect(&h, "stop-to-edit");
    let picked = row_rect(&h, "keep-as-is");
    assert!(
        chip_is_selected(&h, &chip_in_row(&h, "edit", edited), edited),
        "the parsed edit row paints its Edit chip"
    );
    assert!(
        !chip_is_selected(&h, &chip_in_row(&h, "pick", edited), edited),
        "the parsed edit row must not read as a Pick"
    );
    assert!(
        chip_is_selected(&h, &chip_in_row(&h, "pick", picked), picked),
        "the parsed pick row paints its Pick chip"
    );
    assert!(
        !chip_is_selected(&h, &chip_in_row(&h, "edit", picked), picked),
        "the parsed pick row must not read as an Edit"
    );

    // The shortcut hints for the new keys paint in the rail — and none of
    // them collides with the row-scoped claims above, which are scoped by the
    // rows' own rects precisely because "Edit" is a word the legend also has.
    assert_painted(&h, "R     Reword · change the message");
    assert_painted(&h, "E     Edit · stop to amend the commit");
    assert_painted(&h, "U     Update Ref · follow the rewrite");
    assert!(
        chip_is_selected(&h, &chip_in_row(&h, "edit", edited), edited),
        "the legend's Edit hint must not be mistaken for the row's chip"
    );
}

/// Verification 2 — `Pick` resets a modified row (the chip claim moves back to
/// pick and squash stops being painted as the row's action), and a REBASE-TODO
/// line with an unrecognised verb paints `Unknown`, **not** `Pick`: coercing
/// it to pick would produce a valid-looking wrong plan.
#[test]
fn pick_resets_a_modified_row_and_an_unrecognised_verb_paints_unknown() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = plan_repo(tmp.path(), "repo");
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_editor(&mut h, &repo);

    // Squash the first row, then take it back with Pick.
    nearest(&h, "squash", "feature-1").click();
    h.run();
    let first = row_rect(&h, "feature-1");
    assert!(chip_is_selected(
        &h,
        &chip_in_row(&h, "squash", first),
        first
    ));
    nearest(&h, "pick", "feature-1").click();
    h.run();
    let first = row_rect(&h, "feature-1");
    assert!(
        chip_is_selected(&h, &chip_in_row(&h, "pick", first), first),
        "Pick resets the modified row"
    );
    assert!(
        !chip_is_selected(&h, &chip_in_row(&h, "squash", first), first),
        "the row no longer reads as a Squash"
    );

    // An unrecognised todo verb survives as its own row — rendered Unknown.
    nearest(&h, "Log", "Plan").click();
    h.run();
    h.get_by_role_and_label(Role::TextInput, "REBASE-TODO")
        .focus();
    h.run();
    h.get_by_role_and_label(Role::TextInput, "REBASE-TODO")
        .type_text("\nrebase 0badc0de docs: runbook");
    h.run();
    nearest(&h, "Plan", "Log").click();
    h.run();

    let unknown = row_rect(&h, "docs: runbook");
    let galleys = painted_galleys(&h);
    let unknown_painted = galleys
        .iter()
        .find(|g| g.text == "unknown" && unknown.contains(g.rect.center()))
        .expect("the unrecognised row paints `unknown` inside its own row");
    assert_eq!(
        unknown_painted.color,
        Palette::STATE_WARNING,
        "the fallback verb is painted as the warning it is"
    );
    assert!(
        !chip_is_selected(&h, &chip_in_row(&h, "pick", unknown), unknown),
        "the unknown row must NOT read as a Pick"
    );
    assert_eq!(
        h.state().ui.dlg.rebase_plan.clone().expect("plan")[3].action,
        turbogit_domain::model::RebaseAction::Unknown,
    );
}

/// Verification 3 — `galley_origin`: selecting a row and pressing `e` does not
/// move any other row's origin and the chip count per row is unchanged, while
/// setting `Edit` does change the affected row's own selected-chip galley
/// origin (from the pick chip's to the edit chip's). This pins "the shortcut
/// retargets one row" rather than "the shortcut re-flows the list".
#[test]
fn the_edit_shortcut_retargets_one_row_without_reflowing_the_plan() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = plan_repo(tmp.path(), "repo");
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_editor(&mut h, &repo);

    nearest(&h, "feature-1", "feature-1").click();
    h.run();

    let rows = ["feature-1", "feature-2", "feature-3"];
    let subject_before: Vec<_> = rows.iter().map(|r| galley_origin(&h, r)).collect();
    let rects_before: Vec<_> = rows.iter().map(|r| row_rect(&h, r)).collect();
    let chip_counts_before: Vec<_> = rects_before
        .iter()
        .map(|r| chip_galley_count(&h, *r))
        .collect();
    let (picked_label, picked_origin) = selected_chip_galley(&h, rects_before[0]);
    assert_eq!(picked_label, "pick");

    h.key_press(egui::Key::E);
    h.run();

    // No other row moved: every subject galley is exactly where it was.
    for (subject, before) in rows.iter().zip(&subject_before) {
        let origin =
            galley_origin(&h, subject).unwrap_or_else(|| panic!("{subject} is still painted"));
        assert_eq!(
            Some(origin),
            *before,
            "pressing `e` must not move {subject}'s galley"
        );
    }
    // No row re-rendered its chips: same chip count per row.
    let rects_after: Vec<_> = rows.iter().map(|r| row_rect(&h, r)).collect();
    let chip_counts_after: Vec<_> = rects_after
        .iter()
        .map(|r| chip_galley_count(&h, *r))
        .collect();
    assert_eq!(chip_counts_after, chip_counts_before, "chip count per row");
    // The affected row's own chip galley origin DID move: its marked action
    // switched from the pick chip to the edit chip.
    let (edit_label, edit_origin) = selected_chip_galley(&h, rects_after[0]);
    assert_eq!(edit_label, "edit", "`e` sets Edit on the selected row");
    assert_ne!(
        edit_origin, picked_origin,
        "the selected chip's galley origin moved from pick to edit"
    );
    // And the untouched rows still mark pick, at the same galley origins.
    for i in 1..rows.len() {
        let (label, origin) = selected_chip_galley(&h, rects_after[i]);
        assert_eq!(label, "pick", "row {} is untouched", i + 1);
        let (_, before) = selected_chip_galley(&h, rects_before[i]);
        assert_eq!(origin, before);
    }
}

// --- The body-to-footer rule wears the shared footer-rule tone (ticket 09) --
//
// The interactive rebase editor is NOT one of the acceptance-capture pages, so
// nothing downstream would notice a spacing regression on its body-to-footer
// rule. These two tests are the substitute evidence: one pins the tone, one pins
// the geometry. Between them they say what `ui.separator()` used to produce and
// what the shared footer rule produces, so the change can be reviewed as
// tone-only rather than taken on trust.

/// One full-width 1px horizontal band the editor painted, with the solid colour
/// it carries — the fill of a filled band, or the stroke colour of a stroked
/// one.
///
/// Deliberately primitive-agnostic. `ui.separator()` stroked a 1px line through
/// the middle of a 6px band; the shared footer rule fills a 1px band. Reading
/// both as "a 1px full-width band and its colour" is what lets the same helper
/// measure the rule before and after the change, so the geometry assertion below
/// compares like with like.
fn full_width_hairlines(h: &Harness<'_, AppState>) -> Vec<(Rect, Color32)> {
    fn collect(shape: &egui::Shape, out: &mut Vec<(Rect, Color32)>) {
        match shape {
            egui::Shape::Rect(r)
                if r.fill != Color32::TRANSPARENT
                    && (r.rect.height() - 1.0).abs() < f32::EPSILON
                    && r.rect.width() > 500.0 =>
            {
                out.push((r.rect, r.fill));
            }
            egui::Shape::LineSegment { points, stroke }
                if (points[1].y - points[0].y).abs() < f32::EPSILON
                    && (stroke.width - 1.0).abs() < f32::EPSILON
                    && (points[1].x - points[0].x).abs() > 500.0 =>
            {
                // The painted extent of a 1px stroke: one pixel tall, centred on
                // the segment — the same box a 1px fill covers.
                let mid = egui::pos2(
                    (points[0].x + points[1].x) / 2.0,
                    (points[0].y + points[1].y) / 2.0,
                );
                out.push((
                    Rect::from_center_size(mid, egui::vec2(points[1].x - points[0].x, 1.0)),
                    stroke.color,
                ));
            }
            egui::Shape::Vec(v) => v.iter().for_each(|s| collect(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in h.output().shapes.iter() {
        collect(&clipped.shape, &mut out);
    }
    out
}

/// The editor's body-to-footer rule: the lowest full-width hairline that still
/// sits above the footer row's own text. The tab-strip rule (the other
/// `ui.separator()` in this editor, deliberately out of scope) is higher up, so
/// "lowest above the footer text" picks the body-to-footer rule and not it.
fn body_footer_rule(h: &Harness<'_, AppState>) -> (Rect, Color32) {
    let footer_top = galley_origin(h, "Ready to rebase 3 commits")
        .expect("the footer row paints its estimate")
        .y;
    *full_width_hairlines(h)
        .iter()
        .filter(|(rect, _)| rect.center().y < footer_top)
        .max_by(|a, b| a.0.center().y.total_cmp(&b.0.center().y))
        .expect("the editor must rule its body off from its footer")
}

/// The editor's other full-content-width hairline: the tab-strip rule, which
/// `ui.separator()` lays out in the same content `Ui` as the footer rule and is
/// therefore the editor's own answer to "how wide is the available width here".
///
/// Matched on x-range, which is what distinguishes the editor's own rules from
/// the shell's status-bar edge line (wider still) and from the two
/// columns' internal rules (narrower) — both of which are painted in the same
/// frame, overlapping the window.
fn tab_strip_rule(h: &Harness<'_, AppState>, rule: Rect) -> Rect {
    full_width_hairlines(h)
        .into_iter()
        .filter(|(rect, _)| {
            (rect.left() - rule.left()).abs() < f32::EPSILON
                && (rect.right() - rule.right()).abs() < f32::EPSILON
                && rect.center().y < rule.center().y
        })
        .map(|(rect, _)| rect)
        .min_by(|a, b| a.center().y.total_cmp(&b.center().y))
        .expect("the editor also rules its tab strip off")
}

#[test]
fn the_body_to_footer_rule_paints_the_shared_footer_rule_tone() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = plan_repo(tmp.path(), "repo");
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_editor(&mut h, &repo);

    let (_rect, color) = body_footer_rule(&h);
    // Before the change this band was egui's default `ui.separator()` line: a
    // 1px stroke in the *unowned* default-separator tone, which production
    // re-points at the `INK_2` text token. (This harness drives `ui::render`
    // without the production `configure_style`, so it shows egui's own raw
    // default `noninteractive.bg_stroke` of `Color32::from_gray(60)`.) Either
    // way it is a text token wearing a hairline's job, not the footer rule.
    assert_eq!(
        color,
        Palette::RULE_FOOTER,
        "the body-to-footer rule must wear the shared modal footer rule, \
         which is what every other modal footer in the app wears"
    );
}

#[test]
fn the_body_to_footer_rule_keeps_the_separator_s_geometry() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = plan_repo(tmp.path(), "repo");
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_editor(&mut h, &repo);

    let (rule, _) = body_footer_rule(&h);
    let footer_top = galley_origin(&h, "Ready to rebase 3 commits")
        .expect("the footer row paints its estimate")
        .y;

    // A 1px rule — not a 2px band, not a rounded card edge.
    assert_eq!(rule.height(), 1.0, "the footer rule is a 1px hairline");
    // Spans the editor's content width. The tab-strip rule above it is laid out
    // by the same `Ui` at the same available width, so equalling it is a
    // self-calibrating statement of "the full available width" that never
    // hard-codes the window's size.
    let tab_strip = tab_strip_rule(&h, rule);
    assert_eq!(
        rule.width(),
        tab_strip.width(),
        "the footer rule must span the editor's full available width"
    );
    assert!(
        rule.left() >= tab_strip.left() && rule.right() <= tab_strip.right(),
        "the footer rule must not overhang the editor's content box"
    );
    // Still a body-to-footer rule: below the tab strip, above the footer row.
    assert!(
        rule.center().y > tab_strip.center().y && rule.bottom() <= footer_top,
        "the rule must separate the body from the footer row, not sit inside one"
    );
}
