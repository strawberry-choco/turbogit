//! A frame's branch-row work is bounded by its viewport, not by the repository's
//! branch count, and an inline rename survives a scroll.
//!
//! These are assertions about the frame's own output: no counter is added to
//! production code and nothing reads a row model. `rows built` counts the
//! branch-row nodes in the frame's accessibility tree, which is every row egui
//! laid out whether or not the clip showed it. `rows shown` counts branch-name
//! galleys in the display list, which the clip empties. The tree has the answer
//! to "how much work did this frame do", the display list only to "how much of
//! it could be seen. That distinction is the one that made the unbounded
//! frame look healthy in a painted-output check.
//!
//! The pair `built <= shown + slack` and `shown` well above zero is what pins
//! it: the first stops the frame building the whole list, the second stops it
//! passing by building nothing.

use std::path::{Path, PathBuf};

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use tempfile::TempDir;
use test_support::git_seed::{GitEnv, git_with_env};
use test_support::harness::{
    PaintedGalley, painted_galleys, settle_quiet, settle_reads, shell_harness_over,
};
use turbogit_app::state::{AppState, Tab};

const LIST_BOX: egui::Vec2 = egui::vec2(1008.0, 768.0);
/// Rows a frame may build beyond the rows it can show, before the frame is
/// accused of laying out the whole list. One row above and one below the fold
/// is the slack a viewport-scoped build needs, and this leaves four.
const ROW_SLACK: usize = 4;

/// The leaf a painted row shows. Directory grouping strips `feature/` into its
/// own header, so the galley carries the stem while the accessibility label
/// carries the full ref name.
const PAINTED_STEM: &str = "branch-";
const LABEL_REF: &str = "feature/branch-";

// --- fixture -----------------------------------------------------------------

/// `n` local branches written straight into `.git` as loose refs.
///
/// One `git branch` per name is `n` subprocess spawns. The production branch
/// read is `git branch -a -vv` plus one `for-each-ref`, and both enumerate a
/// loose ref exactly as they enumerate a reflog-bearing branch, so the
/// `Vec<Branch>` reaching the tree is what the real read produces.
fn seed_branches(repo: &Path, n: usize) {
    let sha = git_with_env(repo, &["rev-parse", "HEAD"], &GitEnv::fixture());
    let heads = git_with_env(
        repo,
        &["rev-parse", "--git-path", "refs/heads"],
        &GitEnv::fixture(),
    );
    // An absolute `--git-path` answer replaces the join base, which is what a
    // linked worktree's `refs/heads` lives under.
    let dir = repo.join(heads.trim()).join("feature");
    std::fs::create_dir_all(&dir).unwrap();
    let sha = sha.trim();
    for i in 0..n {
        std::fs::write(dir.join(format!("branch-{i:04}")), format!("{sha}\n")).unwrap();
    }
}

/// A project dir holding one repo `alpha` on `main` with `n` local branches and
/// no remote, so Local is the only group with rows in it.
fn branches_project(n: usize) -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = project.join("alpha");
    git_with_env(
        &project,
        &["-c", "init.defaultBranch=main", "init", "alpha"],
        &GitEnv::fixture(),
    );
    std::fs::write(alpha.join("README.md"), "x\n").unwrap();
    git_with_env(&alpha, &["add", "."], &GitEnv::fixture());
    git_with_env(&alpha, &["commit", "-q", "-m", "init"], &GitEnv::fixture());
    seed_branches(&alpha, n);
    (tmp, project)
}

/// The Branches tab over a repo of `n` branches, with the temp dir it reads.
///
/// The `TempDir` is part of the return, not a local: dropping it deletes the
/// repository under a harness that is still open on it, and the tree then has
/// no roots to paint.
fn branches_tab(n: usize) -> (Harness<'static, AppState>, TempDir) {
    let (repo, project) = branches_project(n);
    let mut harness = shell_harness_over(AppState::new(project), LIST_BOX);
    harness.state_mut().ui.tab = Tab::Branches;
    settle_reads(&mut harness);
    settle_quiet(&mut harness);
    (harness, repo)
}

// --- counting one frame's work ------------------------------------------------

/// Branch rows this frame laid out, read off its accessibility tree.
fn rows_built(harness: &Harness<'_, AppState>) -> usize {
    harness
        .query_all_by_role(Role::Button)
        .filter(|node| {
            node.accesskit_node()
                .label()
                .is_some_and(|label| label.starts_with(LABEL_REF))
        })
        .count()
}

/// Branch names this frame could be seen showing.
fn rows_shown(harness: &Harness<'_, AppState>) -> usize {
    painted_galleys(harness)
        .iter()
        .filter(|g| g.text.starts_with(PAINTED_STEM))
        .count()
}

/// One wheel tick over a row that is on screen, then one frame.
fn wheel_frame(harness: &mut Harness<'static, AppState>) {
    let over = first_row(harness).pos;
    harness.hover_at(over);
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -60.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::default(),
    });
    harness.step();
}

/// The topmost branch row the frame can be seen showing, galley and all.
fn first_row(harness: &Harness<'_, AppState>) -> PaintedGalley {
    painted_galleys(harness)
        .into_iter()
        .find(|g| g.text.starts_with(PAINTED_STEM))
        .expect("the branches tab paints a branch row")
}

// --- the bound -----------------------------------------------------------------

#[test]
fn a_frame_builds_only_the_rows_its_viewport_can_show() {
    let (mut harness, _repo) = branches_tab(400);
    // Deep into the list, where an unbounded frame builds 400 rows to show ~17.
    for _ in 0..12 {
        wheel_frame(&mut harness);
    }
    let (built, shown) = (rows_built(&harness), rows_shown(&harness));
    assert!(
        shown > 8,
        "the viewport should be full of rows, only {shown} were shown"
    );
    assert!(
        built <= shown + ROW_SLACK,
        "{built} rows built to show {shown}. A frame's row work must be bounded \
         by the viewport, not by the branch count"
    );
}

#[test]
fn built_rows_do_not_grow_with_the_branch_count() {
    let (mut few, _few_repo) = branches_tab(40);
    let (mut many, _many_repo) = branches_tab(400);
    for _ in 0..6 {
        wheel_frame(&mut few);
        wheel_frame(&mut many);
    }
    let (few_built, many_built) = (rows_built(&few), rows_built(&many));
    assert_eq!(
        few_built, many_built,
        "ten times the branches built {many_built} rows against {few_built}; \
         the frame must not scale with the listing"
    );
}

#[test]
fn a_deep_row_steps_the_same_pitch_as_a_row_at_the_top() {
    // What a skipped row owes the list is its height plus the flow's spacing, and
    // the arithmetic shows up as the gap between two rows the frame did build.
    // Measured against the same list's own pitch at the top, where nothing is
    // skipped, so the assertion carries no borrowed constant.
    let (mut harness, _repo) = branches_tab(400);
    let pitch = {
        let ys = painted_row_ys(&harness);
        assert!(ys.len() > 2, "the top of the list should show several rows");
        ys[1] - ys[0]
    };

    for _ in 0..12 {
        wheel_frame(&mut harness);
    }
    let ys = painted_row_ys(&harness);
    assert!(
        ys.len() > 2,
        "deep in the list the viewport should still be full of rows"
    );
    for pair in ys.windows(2) {
        assert!(
            (pair[1] - pair[0] - pitch).abs() < 0.01,
            "rows deep in the list step {} while the unskipped top steps {pitch}. \
             A skipped row is reserving the wrong height",
            pair[1] - pair[0],
        );
    }
}

/// The painted branch rows' `y`, in list order.
fn painted_row_ys(harness: &Harness<'_, AppState>) -> Vec<f32> {
    let mut ys: Vec<f32> = painted_galleys(harness)
        .into_iter()
        .filter(|g| g.text.starts_with(PAINTED_STEM))
        .map(|g| g.pos.y)
        .collect();
    ys.sort_by(|a, b| a.total_cmp(b));
    ys
}

// --- identity ------------------------------------------------------------------

/// A row reached by scrolling still takes a click.
///
/// Skipping rows leaves every widget inside the built rows on the parent Ui's
/// running auto-id counter, so those ids move as the built set moves. That is
/// sound for these rows and unsound for the rename editor below, and the
/// difference is worth writing down: a click needs the pointer to stay over one
/// row, which needs the scroll offset to hold between press and release, and the
/// built set is a pure function of that offset. So no click can straddle a
/// change in it. A caret lives across many frames at many offsets, which is why
/// only it needs an id that does not read the counter.
#[test]
fn a_row_reached_by_scrolling_still_takes_its_click() {
    let (mut harness, _repo) = branches_tab(400);
    for _ in 0..12 {
        wheel_frame(&mut harness);
    }
    // egui keeps decaying the flick's momentum in every frame after it, so a
    // click issued straight away lands on whatever row has since scrolled under
    // the pointer. Quiet first, then the pointer and the row agree.
    settle_quiet(&mut harness);
    // Read the name off the node being clicked. Taking it from the display list
    // and the click from the accessibility tree would pair two orderings that
    // need not agree, and the test would then be about nothing.
    // A row well inside the viewport, taken off what was painted rather than off
    // tree order. The topmost built row only partly clears the clip, and its
    // centre is where a click is least reliable.
    let mut shown = painted_galleys(&harness)
        .into_iter()
        .filter(|g| g.text.starts_with(PAINTED_STEM))
        .collect::<Vec<_>>();
    shown.sort_by(|a, b| a.pos.y.total_cmp(&b.pos.y));
    let shown = shown[shown.len() / 2].clone();
    let target = format!("feature/{}", shown.text);

    let node = harness
        .get_all_by_role(Role::Button)
        .find(|node| {
            node.accesskit_node()
                .label()
                .is_some_and(|label| label == target.as_str())
        })
        .unwrap_or_else(|| panic!("{target} was painted, so it should be a built row"));
    node.click();
    harness.step();
    harness.step();

    let selected = harness.state().ui.branches_tree.selected.clone();
    assert_eq!(
        selected.as_deref(),
        Some(target.as_str()),
        "the row under the pointer did not take the click"
    );
}

#[test]
fn an_inline_rename_keeps_its_focus_through_a_scroll() {
    let (mut harness, _repo) = branches_tab(400);
    let root = harness.state().multi.roots[0].id.clone();
    let name = harness.state().multi.roots[0].branches[1].name.clone();
    {
        let tree = &mut harness.state_mut().ui.branches_tree;
        tree.selected_root = Some(root.clone());
        tree.selected = Some(name.clone());
        tree.renaming = Some(name.clone());
        tree.rename_draft = "mid-edit".to_string();
    }
    harness.step();
    let before = harness.ctx.memory(|m| m.focused());
    assert!(
        before.is_some(),
        "the inline rename editor takes focus the frame it opens"
    );

    // The rows above the editor are now skipped, which is what would renumber
    // a counter-derived id and reset the caret with it.
    for _ in 0..6 {
        wheel_frame(&mut harness);
    }

    let after = harness.ctx.memory(|m| m.focused());
    assert_eq!(
        before, after,
        "scrolling out of an inline rename moved its focus, so the editor's id \
         is still derived from the parent Ui's running counter"
    );
    assert_eq!(
        harness.state().ui.branches_tree.renaming.as_deref(),
        Some(name.as_str()),
        "the edit is still an edit"
    );
    assert_eq!(
        harness.state().ui.branches_tree.rename_draft,
        "mid-edit",
        "the draft survives the scroll"
    );
}
