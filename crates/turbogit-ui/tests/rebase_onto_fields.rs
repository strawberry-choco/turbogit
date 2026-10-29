#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{Color32, Rect};
use egui_kittest::kittest::Queryable as _;
use egui_kittest::{Harness, Node};
use test_support::RecordingExecutor;
use test_support::git_seed::{commit as git_seed_commit, git};
use test_support::harness::{
    KITTEST_DEFAULT_BOX, assert_not_painted, assert_painted, filled_rects, galley_origin,
    painted_galleys, shell_harness_over_unstyled,
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

fn rebase_repo(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q", "-b", "main"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    commit(&path, "base.txt", "base");
    git(&path, &["checkout", "-q", "-b", "feature"]);
    commit(&path, "a.txt", "feature-1");
    commit(&path, "b.txt", "feature-2");
    path
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

fn open_onto(h: &mut Harness<'_, AppState>, root: &Path) {
    h.state_mut().selected_root = Some(RootId(root.to_path_buf().into()));
    h.state_mut().ui.dlg.rebase_mode = RebaseMode::Standard;
    h.state_mut().ui.dlg.rebase_onto_enabled = true;
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

fn field_band(h: &Harness<'_, AppState>, title: &str, next: Option<&str>) -> Rect {
    let galleys = painted_galleys(h);
    let top = galleys
        .iter()
        .find(|g| g.text == title)
        .unwrap_or_else(|| panic!("no {title:?} title painted"))
        .pos
        .y;
    let bottom = next
        .and_then(|next| {
            galleys
                .iter()
                .find(|g| g.text == next)
                .map(|g| g.pos.y - 0.5)
        })
        .unwrap_or(f32::INFINITY);
    Rect::from_min_max(egui::pos2(0.0, top), egui::pos2(f32::INFINITY, bottom))
}

fn painted_in(h: &Harness<'_, AppState>, text: &str, band: Rect) -> bool {
    painted_galleys(h)
        .iter()
        .any(|g| g.text.contains(text) && band.contains(g.rect.center()))
}

fn confirm_fill(h: &Harness<'_, AppState>, label: &str) -> Option<Color32> {
    let rect = dialog_node(h, label).rect();
    filled_rect_at(h, rect)
}

fn filled_rect_at(h: &Harness<'_, AppState>, rect: Rect) -> Option<Color32> {
    filled_rects(h)
        .into_iter()
        .find(|(r, _)| *r == rect)
        .map(|(_, fill)| fill)
}

fn armed(fill: Option<Color32>) -> bool {
    fill.is_some_and(|fill| fill.a() == 255)
}

fn recorded_rebases(exec: &RecordingExecutor) -> Vec<(String, Option<String>)> {
    exec.recorded()
        .iter()
        .filter_map(|c| match c {
            test_support::RecordedCall::Rebase { onto, opts, .. } => {
                Some((onto.clone(), opts.onto.clone()))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn the_two_bases_hide_until_onto_is_enabled() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = rebase_repo(tmp.path(), "repo");
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    h.state_mut().selected_root = Some(RootId(repo.to_path_buf().into()));
    h.state_mut().ui.dialog = Some(Dialog::Rebase);
    h.run();

    assert_not_painted(&h, "NEW BASE");
    assert_not_painted(&h, "OLD BASE");
    assert_painted(&h, "Custom base (--onto)");

    dialog_node(&h, "Custom base (--onto)").click();
    h.run();

    assert_painted(&h, "NEW BASE");
    assert_painted(&h, "OLD BASE");
    assert_painted(&h, "superseded by the two base fields");
}

#[test]
fn the_new_base_paints_strictly_above_the_old_base() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = rebase_repo(tmp.path(), "repo");
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_onto(&mut h, &repo);

    let new = galley_origin(&h, "NEW BASE").expect("NEW BASE painted");
    let old = galley_origin(&h, "OLD BASE").expect("OLD BASE painted");
    assert!(
        new.y < old.y,
        "the new base must paint above the old one: {new:?} vs {old:?}"
    );
}

#[test]
fn an_empty_new_base_errors_in_its_own_field_and_nowhere_else() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = rebase_repo(tmp.path(), "repo");
    let (mut state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    state.ui.dlg.rebase_old_base = "base.txt".to_string();
    let mut h = harness(state);
    open_onto(&mut h, &repo);
    h.state_mut().ui.dlg.rebase_old_base = "main".to_string();
    h.run();

    let new_band = field_band(&h, "NEW BASE", Some("OLD BASE"));
    assert!(
        painted_in(&h, "Select a new base", new_band),
        "the new base's own error must paint inside its own field"
    );
    assert_not_painted(&h, "Select an old base");
}

#[test]
fn an_empty_old_base_errors_in_its_own_field_and_nowhere_else() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = rebase_repo(tmp.path(), "repo");
    let (mut state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    state.ui.dlg.rebase_new_base = "main".to_string();
    let mut h = harness(state);
    open_onto(&mut h, &repo);

    let old_band = field_band(&h, "OLD BASE", None);
    assert!(
        painted_in(&h, "Select an old base", old_band),
        "the old base's own error must paint inside its own field"
    );
    assert_not_painted(&h, "Select a new base");
}

#[test]
fn a_base_that_names_nothing_errors_with_the_missing_ref_message() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = rebase_repo(tmp.path(), "repo");
    let (mut state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    state.ui.dlg.rebase_new_base = "no-such-branch-or-hash".to_string();
    state.ui.dlg.rebase_old_base = "main".to_string();
    let mut h = harness(state);
    open_onto(&mut h, &repo);

    let new_band = field_band(&h, "NEW BASE", Some("OLD BASE"));
    assert!(
        painted_in(&h, "There is no such branch or hash", new_band),
        "the unresolvable field states its own reason"
    );
    assert_not_painted(&h, "Select an old base");
}

#[test]
fn the_confirm_is_withheld_until_both_bases_carry_a_usable_value() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = rebase_repo(tmp.path(), "repo");
    let (mut state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    state.ui.dlg.rebase_new_base = "main".to_string();
    let mut h = harness(state);
    open_onto(&mut h, &repo);

    let withheld = confirm_fill(&h, "Start rebase");
    assert!(
        !armed(withheld),
        "an empty old base must withhold the confirm, and the withheld fill was {withheld:?}"
    );

    h.state_mut().ui.dlg.rebase_old_base = "main".to_string();
    h.state_mut().ui.dlg.rebase_new_base = "no-such-branch-or-hash".to_string();
    h.run();
    let withheld = confirm_fill(&h, "Start rebase");
    assert!(
        !armed(withheld),
        "an unresolvable base must withhold the confirm, and the withheld fill was {withheld:?}"
    );

    h.state_mut().ui.dlg.rebase_new_base = "main".to_string();
    h.run();
    let armed_fill = confirm_fill(&h, "Start rebase");
    assert!(
        armed(armed_fill),
        "two usable bases must arm the confirm; fill was {armed_fill:?}"
    );
}

/// Both bases reach git as `--onto <new base> <old base>`: the positional
/// upstream is the OLD base and the flag's value is the NEW base. Swapping them
/// rebases the wrong commits, so the pair is asserted as an ordered tuple.
#[test]
fn the_dispatch_passes_both_bases_in_git_own_order() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = rebase_repo(tmp.path(), "repo");
    let (mut state, exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    state.ui.dlg.rebase_new_base = "main".to_string();
    state.ui.dlg.rebase_old_base = "main".to_string();
    let mut h = harness(state);
    open_onto(&mut h, &repo);
    // The old base is the cutoff, not the destination: `HEAD~1` names the commit
    // `feature-2` was built on, so only that one commit is expected to move.
    h.state_mut().ui.dlg.rebase_old_base = "HEAD~1".to_string();
    h.run();

    dialog_node(&h, "Start rebase").click();
    h.run();

    let ok = wait_until(5_000, || !recorded_rebases(&exec).is_empty());
    assert!(ok, "no rebase was dispatched at the executor boundary");
    let rebases = recorded_rebases(&exec);
    assert_eq!(rebases.len(), 1, "one rebase dispatched");
    assert_eq!(
        rebases[0],
        ("HEAD~1".to_string(), Some("main".to_string())),
        "git rebase --onto <new base> <old base>"
    );
}

#[test]
fn onto_moves_only_the_commits_after_the_old_base() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = rebase_repo(tmp.path(), "repo");
    git(&repo, &["checkout", "-q", "main"]);
    git(&repo, &["checkout", "-q", "-b", "side"]);
    commit(&repo, "s.txt", "side-1");
    git(&repo, &["checkout", "-q", "feature"]);
    let cutoff = git(&repo, &["rev-parse", "HEAD~1"]).trim().to_string();
    assert_eq!(
        git(&repo, &["log", "-1", "--format=%s", &cutoff]).trim(),
        "feature-1",
        "the cutoff commit is the one that must NOT move"
    );

    let (mut state, exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    state.ui.dlg.rebase_new_base = "side".to_string();
    state.ui.dlg.rebase_old_base = cutoff.clone();
    let mut h = harness(state);
    open_onto(&mut h, &repo);
    h.state_mut().ui.dlg.rebase_new_base = "side".to_string();
    h.state_mut().ui.dlg.rebase_old_base = cutoff.clone();
    h.run();

    dialog_node(&h, "Start rebase").click();
    h.run();

    let ok = wait_until(10_000, || {
        git(&repo, &["log", "--format=%s", "feature"])
            .lines()
            .count()
            == 3
    });
    assert!(
        ok,
        "the --onto rebase never landed: {:?}",
        recorded_rebases(&exec)
    );
    let log = git(&repo, &["log", "--format=%s", "feature"]);
    let subjects: Vec<&str> = log.lines().collect();
    assert_eq!(
        subjects,
        ["feature-2", "side-1", "base"],
        "--onto replays only the sequence above the cutoff, onto the new base"
    );
}
