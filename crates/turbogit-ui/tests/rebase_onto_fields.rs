//! `--onto` in the Rebase dialog as **two ordered fields**.
//!
//! The reference's rebase dialog bundles name four fields
//! (`rebase.dialog.target`, `rebase.dialog.new.base`, `rebase.dialog.old.base`,
//! `rebase.dialog.branch.field`), and `--onto` is the pair of them that no single
//! "base" field can express: new base first, old base second, each validated in
//! its own right. This suite drives the real `turbogit_ui::ui::render()` through
//! `egui_kittest` over temporary git repositories and asserts three structural
//! facts:
//!
//! 1. **order** — the `NEW BASE` label's galley origin is strictly above the
//!    `OLD BASE` label's, which presence alone cannot show;
//! 2. **per-field validation** — an error paints inside the field it belongs to
//!    and no other field's error paints anywhere, so the two fields cannot share
//!    one error region;
//! 3. **the confirm control's own fill** — measured at the confirm node's rect,
//!    because the two base fields are filled controls too and an unscoped rect
//!    count cannot tell them apart.
//!
//! The dispatch side is asserted at the executor boundary: both bases reach
//! `git rebase --onto <new base> <old base>`.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Color32, Rect};
use egui_kittest::kittest::Queryable as _;
use egui_kittest::{Harness, Node};
use test_support::RecordingExecutor;
use test_support::git_seed::{commit as git_seed_commit, git};
use test_support::harness::{
    KITTEST_DEFAULT_BOX, assert_not_painted, assert_painted, filled_rects, galley_origin,
    painted_galleys, shell_harness_over_unstyled,
};
use turbogit_app::state::{AppState, Dialog};
use turbogit_domain::model::{RebaseMode, RootId, VcsSettings};
use turbogit_engine::cli::CliExecutor;

// ---------------------------------------------------------------- helpers --

/// Append `text` to `<dir>/<name>`, stage, commit.
fn commit(dir: &Path, name: &str, text: &str) {
    let file = dir.join(name);
    let mut body = std::fs::read_to_string(&file).unwrap_or_default();
    body.push_str(text);
    body.push('\n');
    git_seed_commit(dir, name, &body, text);
}

/// One repo on `main` (one base commit) with a local `feature` branch two
/// commits ahead, checked out — the rebase subject.
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

fn harness(state: AppState) -> Harness<'static, AppState> {
    shell_harness_over_unstyled(state, KITTEST_DEFAULT_BOX, 1024)
}

/// Open the Rebase dialog in Standard mode with `--onto` showing its two fields.
fn open_onto(h: &mut Harness<'_, AppState>, root: &Path) {
    h.state_mut().selected_root = Some(RootId(root.to_path_buf().into()));
    h.state_mut().ui.dlg.rebase_mode = RebaseMode::Standard;
    h.state_mut().ui.dlg.rebase_onto_enabled = true;
    h.state_mut().ui.dialog = Some(Dialog::Rebase);
    h.run();
}

/// The node labeled `label` nearest the dialog's "Change…" anchor: the shell
/// paints other nodes with the same labels, so the proximity scopes the query.
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

/// The painted band a field owns: from its own group title down to the next
/// field's title (or the bottom of the frame for the last field). Every
/// per-field assertion is scoped by it, because both error sentences are short
/// and an unscoped `assert_painted` would pass on either field's text.
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

/// Does the frame paint `text` inside `band`?
fn painted_in(h: &Harness<'_, AppState>, text: &str, band: Rect) -> bool {
    painted_galleys(h)
        .iter()
        .any(|g| g.text.contains(text) && band.contains(g.rect.center()))
}

/// The fill painted at the confirm control's own rect.
///
/// `None` means the frame painted no fill there at all. Reading the fill AT that
/// rect — rather than counting fills in the frame — is what separates the confirm
/// from the two base fields, which are filled controls as well.
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

/// A control the user can act on wears an opaque fill; a withheld one does not.
fn armed(fill: Option<Color32>) -> bool {
    fill.is_some_and(|fill| fill.a() == 255)
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

/// The rebase calls recorded at the executor boundary as
/// `(positional upstream, --onto value)`.
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

// ------------------------------------------------------------------ tests --

/// The two fields are hidden until the option that reveals them is on — the
/// reference's `Modify options → --onto` reading — and both appear together.
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
    // The plain target is superseded while the two bases are in charge, and the
    // dialog says so rather than letting a filled target read as a live field.
    assert_painted(&h, "superseded by the two base fields");
}

/// Order is the requirement, not presence: `new base` before `old base`, proven
/// by the first label's galley origin being strictly above the second's.
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

/// With the new base empty and the old base filled, only the new base's own
/// error paints, and it paints inside the new base's band — the two fields must
/// not share one error region.
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

/// The mirror: an empty old base errors in the old base's band only.
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

/// A base that names nothing in this root is refused by name, per field — the
/// reference's `There is no such branch or tag` reading, which is a second
/// message rather than a re-statement of the empty one.
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

/// The confirm control is withheld while either base is empty and armed once
/// both carry a usable value — measured at the confirm's own rect, never as a
/// frame-wide fill count.
#[test]
fn the_confirm_is_withheld_until_both_bases_carry_a_usable_value() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = rebase_repo(tmp.path(), "repo");
    let (mut state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    state.ui.dlg.rebase_new_base = "main".to_string();
    let mut h = harness(state);
    open_onto(&mut h, &repo);

    // New base filled, old base empty: withheld.
    let withheld = confirm_fill(&h, "Start rebase");
    assert!(
        !armed(withheld),
        "an empty old base must withhold the confirm, and the withheld fill was {withheld:?}"
    );

    // An unresolvable new base is no better than an empty one.
    h.state_mut().ui.dlg.rebase_old_base = "main".to_string();
    h.state_mut().ui.dlg.rebase_new_base = "no-such-branch-or-hash".to_string();
    h.run();
    let withheld = confirm_fill(&h, "Start rebase");
    assert!(
        !armed(withheld),
        "an unresolvable base must withhold the confirm, and the withheld fill was {withheld:?}"
    );

    // Both bases usable: armed.
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

/// `--onto` moves only the sequence after the cutoff: the commits at or below
/// the old base stay where they are and the ones above it land on the new base.
/// Proven against real git through the dialog's own dispatch path.
#[test]
fn onto_moves_only_the_commits_after_the_old_base() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = rebase_repo(tmp.path(), "repo");
    // `side` branches off the base commit and carries its own commit, so it is a
    // destination that is neither `main` nor `feature`.
    git(&repo, &["checkout", "-q", "main"]);
    git(&repo, &["checkout", "-q", "-b", "side"]);
    commit(&repo, "s.txt", "side-1");
    git(&repo, &["checkout", "-q", "feature"]);
    // The cutoff is `feature-1` itself: everything above it moves, it does not.
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
    // [`open_onto`] writes the dialog state too, so the bases are set last and
    // one more frame validates them.
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
