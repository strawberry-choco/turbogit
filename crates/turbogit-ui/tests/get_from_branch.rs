use std::path::{Path, PathBuf};

use egui::{Color32, Rect, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use test_support::git_seed::{commit as git_commit, git, pin_eol_lf};
use test_support::harness::{
    PaintedGalley, filled_rects, painted_galleys, settle, settle_quiet, shell_harness_over,
};
use turbogit_app::state::{AppState, Dialog, PendingConfirm};
use turbogit_domain::model::{Branch, BranchKind, RootId, VcsSettings};
use turbogit_ui::theme::Palette;
use turbogit_ui::ui::branch_menu::{BranchMenuAction, branch_menu_items};
use turbogit_ui::ui::get_from_branch::VERB;

fn repo(parent: &Path) -> PathBuf {
    let dir = parent.join("app");
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.email", "t@t"]);
    git(&dir, &["config", "user.name", "t"]);
    pin_eol_lf(&dir);
    git_commit(
        &dir,
        "base.txt",
        "same on both sides\n",
        "the shared commit",
    );
    git_commit(&dir, "both.txt", "one\n", "both.txt exists");
    git(&dir, &["checkout", "-q", "-b", "feature"]);
    git_commit(
        &dir,
        "only-feature.txt",
        "theirs\n",
        "a file only the branch has",
    );
    git_commit(&dir, "both.txt", "two\n", "both.txt rewritten");
    git(&dir, &["checkout", "-q", "main"]);
    git_commit(&dir, "only-main.txt", "mine\n", "a file only main has");
    dir
}

fn root_id(dir: &Path) -> RootId {
    RootId(dir.to_path_buf().into())
}

fn shell(dir: &Path) -> Harness<'static, AppState> {
    let state = AppState::for_roots(dir.parent().unwrap(), &[dir.to_path_buf()])
        .with_settings(VcsSettings::default());
    let mut harness = shell_harness_over(state, Vec2::new(1280.0, 800.0));
    harness
        .state_mut()
        .open_get_from_branch(&root_id(dir), "feature");
    settle(&mut harness);
    assert_eq!(
        harness.state().ui.dialog,
        Some(Dialog::GetFromBranch),
        "the fixture's window is the surface under test"
    );
    harness
}

#[derive(Debug, PartialEq)]
struct Row {
    y: f32,
    path: String,
    word: String,
    ink: Color32,
}

fn rows_band(galleys: &[PaintedGalley]) -> Rect {
    let top = galleys
        .iter()
        .find(|g| g.text == "FILES THAT DIFFER")
        .expect("the comparison list is headed by its group title")
        .pos
        .y;
    let bottom = galleys
        .iter()
        .find(|g| g.text == "Swap Branches")
        .expect("the footer carries the swap link")
        .pos
        .y;
    Rect::from_min_max(
        egui::pos2(0.0, top),
        egui::pos2(f32::INFINITY, bottom - 1.0),
    )
}

fn rows(harness: &Harness<'_, AppState>) -> Vec<Row> {
    let galleys = painted_galleys(harness);
    let band = rows_band(&galleys);
    let inside: Vec<&PaintedGalley> = galleys.iter().filter(|g| band.contains(g.pos)).collect();
    let mut out = Vec::new();
    for row in harness.state().ui.dlg.get_from_branch_rows.clone() {
        let text = row.path.display().to_string();
        let path = inside
            .iter()
            .rev()
            .find(|g| g.text == text)
            .unwrap_or_else(|| panic!("`{text}` is a payload row the window never painted"));
        let word = inside
            .iter()
            .rev()
            .find(|g| {
                matches!(g.text.as_str(), "Deleted" | "Added" | "Modified")
                    && g.rect.max.y >= path.rect.min.y
                    && path.rect.max.y >= g.rect.min.y
                    && g.rect.min.x >= path.rect.max.x
                    && g.rect.min.x - path.rect.max.x < 24.0
            })
            .unwrap_or_else(|| panic!("`{text}` paints no rung word beside it"));
        out.push(Row {
            y: (path.rect.center().y + word.rect.center().y) / 2.0,
            path: text,
            word: word.text.clone(),
            ink: word.color,
        });
    }
    out.sort_by(|a, b| a.y.total_cmp(&b.y));
    out
}

#[track_caller]
fn fill_of(harness: &Harness<'_, AppState>, rect: Rect) -> Color32 {
    filled_rects(harness)
        .into_iter()
        .find(|(painted, _)| {
            (painted.center() - rect.center()).length() < 1.0
                && (painted.size().x - rect.size().x).abs() < 2.0
                && (painted.size().y - rect.size().y).abs() < 2.0
        })
        .unwrap_or_else(|| panic!("no filled rect painted at {rect:?}"))
        .1
}

fn verb_controls(harness: &Harness<'_, AppState>) -> Vec<Rect> {
    harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .filter(|n| n.accesskit_node().label().as_deref() == Some(VERB))
        .map(|n| n.rect())
        .collect()
}

#[track_caller]
fn click_by_label(harness: &mut Harness<'_, AppState>, label: &str) {
    let node = harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| n.accesskit_node().label().as_deref() == Some(label))
        .unwrap_or_else(|| panic!("no button labelled `{label}` is painted"));
    node.click();
    // A synthetic press+release is delivered across frames, not within one: the
    // shell's own confirm suites step until the state moved
    // (`feedback_confirm_discard.rs::wait_confirm_cleared`), because asserting on the
    // frame straight after the click measures a widget that has not seen it yet.
    harness.step();
}

#[track_caller]
fn wait_until(harness: &mut Harness<'_, AppState>, what: &str, done: impl Fn(&AppState) -> bool) {
    for _ in 0..200 {
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(10));
        if done(harness.state()) {
            // The state moves a frame before the tree does: a floating window's
            // first painted frame has no known size, so its anchored rect, and the
            // node rects the next click is aimed at, only land on the frame after.
            // Stepping until the paint repeats is what makes a click following this
            // wait aim at the rect the window actually occupies.
            settle(harness);
            return;
        }
    }
    panic!(
        "timed out waiting for {what}; last_error={:?}",
        harness.state().last_error
    );
}

#[test]
fn all_three_rungs_paint_each_on_its_own_row() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    let harness = shell(&dir);

    let painted = rows(&harness);
    assert_eq!(
        painted
            .iter()
            .map(|r| (r.path.as_str(), r.word.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("only-feature.txt", "Deleted"),
            ("only-main.txt", "Added"),
            ("both.txt", "Modified"),
        ],
        "grey leads, green next, blue last — and the identical `base.txt` is not a row"
    );
    assert_eq!(
        painted.iter().map(|r| r.ink).collect::<Vec<_>>(),
        vec![
            Palette::STATUS_DELETED,
            Palette::STATUS_ADDED,
            Palette::STATUS_MODIFIED,
        ],
        "each rung's word is painted with the token that owns the rung, so the word \
         and the colour cannot disagree"
    );
    let ys: Vec<f32> = painted.iter().map(|r| r.y).collect();
    assert!(ys[0] < ys[1] && ys[1] < ys[2], "one rung per row: {ys:?}");
}

#[test]
fn swapping_the_branches_exchanges_the_two_one_sided_rows_and_holds_the_blue_one() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    let mut harness = shell(&dir);

    let before = rows(&harness);
    click_by_label(&mut harness, "Swap Branches");
    wait_until(&mut harness, "the swap to flip the window's posture", |s| {
        s.ui.dlg.get_from_branch_swapped
    });
    settle(&mut harness);
    let after = rows(&harness);

    assert_eq!(
        after
            .iter()
            .map(|r| (r.path.as_str(), r.word.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("only-main.txt", "Deleted"),
            ("only-feature.txt", "Added"),
            ("both.txt", "Modified"),
        ],
        "the two one-sided paths traded rows"
    );
    assert_eq!(
        before[2].y, after[2].y,
        "the differing path is on both sides in either posture, so it cannot move"
    );
    assert!(
        (before[0].y - after[0].y).abs() < 1.0 && (before[1].y - after[1].y).abs() < 1.0,
        "the exchange is positional: the two one-sided slots keep their painted \
         heights while the paths in them trade places, so the grey slot now holds \
         the path that was green, and the reverse"
    );
    assert!(
        harness.state().ui.dlg.get_from_branch_swapped,
        "the posture is app state, not a paint-time coincidence"
    );
    let header = painted_galleys(&harness)
        .into_iter()
        .find(|g| g.text.starts_with("Comparing "))
        .expect("the window states which two sides it is comparing")
        .text;
    assert_eq!(header, "Comparing main with feature (working tree)");
}

#[test]
fn the_verb_is_filled_on_the_blue_rung_and_withheld_on_the_green_one() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    let harness = shell(&dir);

    let painted = rows(&harness);
    let controls = verb_controls(&harness);
    assert_eq!(
        controls.len(),
        1,
        "one differing file, so exactly one row may carry the verb"
    );
    let control = controls[0];
    let blue = painted
        .iter()
        .find(|r| r.word == "Modified")
        .expect("the fixture has a differing row");
    assert!(
        control.center().y > blue.y - 8.0 && control.center().y < blue.y + 8.0,
        "the verb belongs to the blue row's own rect"
    );
    assert_eq!(
        fill_of(&harness, control),
        Palette::ACCENT,
        "the affirmative verb on the rung it can act on"
    );

    for row in painted.iter().filter(|r| r.word != "Modified") {
        assert!(
            controls.iter().all(|c| (c.center().y - row.y).abs() >= 8.0),
            "`{}` is a green or grey rung and must offer no take",
            row.path
        );
    }
}

#[test]
fn a_take_arms_the_confirmation_writes_the_file_and_is_recorded_durably() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    let mut harness = shell(&dir);

    let control = verb_controls(&harness)[0];
    harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| n.rect() == control && n.accesskit_node().label().as_deref() == Some(VERB))
        .expect("the blue row's control")
        .click();
    wait_until(
        &mut harness,
        "the row's control to arm the confirmation",
        |s| s.ui.confirm.is_some(),
    );

    let armed = harness.state().ui.confirm.clone();
    assert!(
        matches!(
            armed,
            Some(PendingConfirm::GetFileFromBranch {
                ref root,
                ref source,
                ref path,
            }) if *root == root_id(&dir) && source == "feature"
                && path.as_path() == Path::new("both.txt")
        ),
        "the confirmation carries the root, the branch and the path the row showed — \
         not whichever root is focused when OK is clicked"
    );

    click_by_label(&mut harness, "Replace file");
    wait_until(
        &mut harness,
        "the take to settle into the activity feed",
        |s| {
            s.ui.activity
                .entries
                .iter()
                .any(|e| e.message.contains("Get both.txt from feature"))
        },
    );
    settle(&mut harness);

    assert_eq!(
        std::fs::read_to_string(dir.join("both.txt")).unwrap(),
        "two\n",
        "the branch's entire contents, in the root the window named"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("only-main.txt")).unwrap(),
        "mine\n",
        "one row's verb touches one path"
    );

    let entries = &harness.state().ui.activity.entries;
    let recorded = entries
        .iter()
        .rev()
        .find(|e| e.message.contains("Get both.txt from feature"))
        .expect("the take settles as a durable activity record, not a transient toast");
    assert_eq!(
        recorded.repo,
        Some("app".to_string()),
        "the record names the root it acted on — the per-root fact a multi-root \
         client has to keep auditable, and never an `All`-scope collapse"
    );

    let painted = rows(&harness);
    assert_eq!(
        painted.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
        vec!["only-feature.txt", "only-main.txt"],
        "the taken path leaves the list — it no longer differs from that side"
    );
    assert_eq!(
        verb_controls(&harness).len(),
        0,
        "and with it goes the offer, so the same overwrite cannot be clicked twice"
    );
}

#[test]
fn cancelling_the_confirmation_writes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    let mut harness = shell(&dir);

    harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| n.accesskit_node().label().as_deref() == Some(VERB))
        .expect("the blue row's control")
        .click();
    wait_until(&mut harness, "the confirmation to arm", |s| {
        s.ui.confirm.is_some()
    });
    click_by_label(&mut harness, "Cancel");
    wait_until(&mut harness, "the confirmation to close", |s| {
        s.ui.confirm.is_none()
    });
    settle_quiet(&mut harness);

    assert_eq!(
        std::fs::read_to_string(dir.join("both.txt")).unwrap(),
        "one\n",
        "the local copy stands"
    );
    assert!(harness.state().ui.confirm.is_none());
    assert!(
        harness
            .state()
            .ui
            .activity
            .entries
            .iter()
            .all(|e| !e.message.contains("Get both.txt")),
        "a cancelled verb records nothing it did not do"
    );
}

#[test]
fn the_branch_row_offers_the_comparison_except_on_itself() {
    let side = Branch {
        name: "feature".to_string(),
        kind: BranchKind::Local,
        ..branch_defaults()
    };
    let items = branch_menu_items(&side, Some("main"));
    assert!(items[BranchMenuAction::DiffWithWorkingTree.index()].enabled);

    let here = Branch {
        name: "main".to_string(),
        ..branch_defaults()
    };
    let items = branch_menu_items(&here, Some("main"));
    assert!(
        !items[BranchMenuAction::DiffWithWorkingTree.index()].enabled,
        "the current branch is the working tree it would be compared with"
    );

    let up = Branch {
        name: "origin/up".to_string(),
        kind: BranchKind::Remote,
        ..branch_defaults()
    };
    let items = branch_menu_items(&up, Some("main"));
    assert!(items[BranchMenuAction::DiffWithWorkingTree.index()].enabled);
}

#[test]
fn a_root_with_no_current_branch_opens_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    git(&dir, &["checkout", "-q", "--detach"]);
    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&dir))
        .with_settings(VcsSettings::default());
    state.open_get_from_branch(&root_id(&dir), "feature");
    assert_eq!(state.ui.dialog, None, "nothing to compare against");
}

fn branch_defaults() -> Branch {
    Branch {
        name: String::new(),
        kind: BranchKind::Local,
        tracking: None,
        favorite: false,
        protected: false,
        exists: true,
        ahead: 0,
        behind: 0,
        gone: false,
        last_touched: None,
        tip: None,
        remote: None,
    }
}
