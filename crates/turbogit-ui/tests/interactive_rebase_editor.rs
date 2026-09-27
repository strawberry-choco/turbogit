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
use std::time::{Duration, Instant};

use egui::accesskit::Role;
use egui::{Color32, Rect};
use egui_kittest::kittest::Queryable as _;
use egui_kittest::{Harness, Node};
use test_support::RecordingExecutor;
use test_support::harness::{assert_not_painted, assert_painted, galley_origin};
use turbogit_app::state::{AppState, Dialog};
use turbogit_domain::model::{RootId, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_ui::theme::Palette;

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
/// the shell's topbar/status-bar edge lines (wider still) and from the two
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
