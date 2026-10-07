//! Branches — what a delete discloses, before it and after.
//!
//! TurboGit asks *before* deleting (a recorded divergence from the reference,
//! which deletes first and tells you afterwards), and its confirmation already
//! says how many commits become unreachable. What neither the pre-flight
//! question nor the post-hoc notification carried was the way to **look at
//! them**: a link naming the count, a viewer that says which branch was not
//! merged into *what*, and — for a branch that tracked a remote — a link to
//! remove that remote branch too.
//!
//! Drives the real `turbogit_ui::ui::render()` over temporary git repositories
//! and the production CLI engine, so "removed the tracking remote branch" is
//! checked against the remote and not against a flag.
//! Asserts only on public surfaces: painted text/geometry + `AppState`.

use std::path::PathBuf;
use std::time::Duration;

use egui::{Color32, Rect};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use tempfile::TempDir;
use test_support::git_seed::git;
use test_support::harness::{
    KITTEST_DEFAULT_BOX, assert_not_painted, assert_painted, filled_rects, galley_origin,
    painted_galleys, painted_text, settle_quiet, shell_harness_over,
};
use turbogit_app::root_caches::Affected;
use turbogit_app::state::{AppState, Tab};
use turbogit_domain::model::BranchKind;
use turbogit_ui::theme::Palette;

// --- git fixture -------------------------------------------------------------

/// One repo `alpha` on `main` with:
/// - `work` — three commits `main` does not have, **and tracking
///   `origin/work`**, so deleting it earns both disclosure links;
/// - `tidy` — no commits of its own and no upstream, so deleting it earns
///   neither. The negative case is what stops a link from being chrome.
fn disclosure_project() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = project.join("alpha");
    git(
        &project,
        &["-c", "init.defaultBranch=main", "init", "alpha"],
    );
    git(&alpha, &["config", "user.email", "t@t"]);
    git(&alpha, &["config", "user.name", "t"]);
    std::fs::write(alpha.join("README.md"), "x\n").unwrap();
    git(&alpha, &["add", "."]);
    git(&alpha, &["commit", "-m", "init"]);
    git(&project, &["clone", "--bare", "alpha", "origin.git"]);
    git(&alpha, &["remote", "add", "origin", "../origin.git"]);
    git(&alpha, &["push", "-q", "origin", "main"]);
    git(&alpha, &["branch", "tidy"]);

    git(&alpha, &["checkout", "-q", "-b", "work"]);
    for (file, msg) in [
        ("w1.txt", "work one: first change"),
        ("w2.txt", "work two: second change"),
        ("w3.txt", "work three: third change"),
    ] {
        std::fs::write(alpha.join(file), "x\n").unwrap();
        git(&alpha, &["add", "."]);
        git(&alpha, &["commit", "-m", msg]);
    }
    git(&alpha, &["push", "-q", "-u", "origin", "work"]);
    git(&alpha, &["checkout", "-q", "main"]);
    git(&alpha, &["fetch", "-q", "origin"]);
    (tmp, project)
}

// --- harness -----------------------------------------------------------------

fn shell_harness(project_dir: PathBuf) -> Harness<'static, AppState> {
    let state = AppState::new(project_dir);
    let mut harness = shell_harness_over(state, KITTEST_DEFAULT_BOX);
    harness.set_size(egui::vec2(1024.0, 768.0));
    harness
}

fn branch<'a>(state: &'a AppState, name: &str) -> Option<&'a turbogit_domain::model::Branch> {
    state
        .selected_root
        .as_ref()
        .and_then(|id| state.multi.by_id(id))
        .and_then(|r| {
            r.branches
                .iter()
                .find(|b| b.name == name && b.kind == BranchKind::Local)
        })
}

/// Is `name` still listed as a remote-tracking branch? A `git push --delete`
/// drops the local remote-tracking ref with the remote's, so this is the app's
/// own answer to "did the remote branch go" — what the pump below waits on
/// instead of a wall clock. The kind-scoped twin of [`branch_gone`], because the
/// engine lists `origin/work` under the short name `work`.
fn remote_listed(state: &AppState, name: &str) -> bool {
    state
        .selected_root
        .as_ref()
        .and_then(|id| state.multi.by_id(id))
        .is_some_and(|r| {
            r.branches
                .iter()
                .any(|b| b.kind == BranchKind::Remote && b.name == name)
        })
}

/// Has the **local** branch `name` left the listing? Scoped by kind because the
/// engine lists a remote-tracking branch under its short name too: `origin/work`
/// is `Branch { name: "work", kind: Remote }`, so an unscoped "is it gone"
/// question could never be answered yes while the remote twin is still there.
fn branch_gone(state: &AppState, name: &str) -> bool {
    state
        .selected_root
        .as_ref()
        .and_then(|id| state.multi.by_id(id))
        .is_some_and(|r| {
            !r.branches
                .iter()
                .any(|b| b.name == name && b.kind == BranchKind::Local)
        })
}

/// Step frames until `pred` holds on public state (async op completion).
fn pump_until(h: &mut Harness<'_, AppState>, what: &str, pred: impl Fn(&AppState) -> bool) {
    for _ in 0..600 {
        if pred(h.state()) {
            return;
        }
        h.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "timed out waiting for: {what}\n  painted={:?}",
        painted_text(h)
    );
}

fn loaded() -> (TempDir, PathBuf, Harness<'static, AppState>) {
    let (project, dir) = disclosure_project();
    let mut harness = shell_harness(dir.clone());
    harness.state_mut().ui.tab = Tab::Branches;
    pump_until(&mut harness, "both branches listed", |s| {
        branch(s, "work").is_some() && branch(s, "tidy").is_some()
    });
    // The state carries the branches a frame or two before the tree paints
    // them, and every interaction below starts from a row.
    settle_quiet(&mut harness);
    (project, dir, harness)
}

fn row_node<'t>(harness: &'t Harness<'_, AppState>, name: &str) -> egui_kittest::Node<'t> {
    harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| n.accesskit_node().label() == Some(name.to_string()))
        .unwrap_or_else(|| {
            panic!(
                "row button for {name}; painted={:?}, labels={:?}",
                painted_text(harness),
                harness
                    .get_all_by_role(egui::accesskit::Role::Button)
                    .filter_map(|n| n.accesskit_node().label())
                    .collect::<Vec<_>>()
            )
        })
}

/// Open `branch`'s row menu and run one item on it — the production path to
/// every branch action, and the one that reaches the pre-flight confirm.
fn menu_action(h: &mut Harness<'_, AppState>, branch: &str, label: &str) {
    row_node(h, branch).click_secondary();
    h.step();
    h.step();
    h.remove_cursor();
    h.step();
    h.get_all_by_label(label)
        .next()
        .unwrap_or_else(|| panic!("menu item {label}"))
        .click();
    settle_quiet(h);
}

/// Ask for `branch`'s deletion, confirm it, and wait for the notification.
fn delete(h: &mut Harness<'static, AppState>, branch: &str) {
    menu_action(h, branch, "Delete branch");
    h.get_by_label("OK").click();
    pump_until(h, "the delete notification is up", |s| {
        branch_gone(s, branch) && s.ui.branches_undo.is_some()
    });
}

/// The painted extent of a string, for geometry that has to say where the text
/// **ends** and not only where it starts.
fn galley_rect(h: &Harness<'_, AppState>, text: &str) -> Rect {
    painted_galleys(h)
        .into_iter()
        .find(|g| g.text == text)
        .unwrap_or_else(|| panic!("`{text}` was not painted"))
        .rect
}

/// A fill compared by name so a failure says which token moved rather than
/// printing four hex digits.
#[derive(Clone, Copy, PartialEq, Debug)]
struct NamedFill(Color32);

/// The fill a point sits on, taken as the **smallest** painted rect covering it.
/// Without that narrowing this answers with the shell's own panel: `window_fill`
/// and [`Palette::RAISED`] are the same colour, so a "what is behind this
/// control" question answered by the largest containing rect reports a fill the
/// control never painted.
fn fill_behind(h: &Harness<'_, AppState>, point: egui::Pos2) -> Option<NamedFill> {
    filled_rects(h)
        .iter()
        .filter(|(r, _)| r.contains(point))
        .min_by(|(a, _), (b, _)| a.area().total_cmp(&b.area()))
        .map(|(_, fill)| NamedFill(*fill))
}

// --- verification 1: the count is a link, before the delete and after -------

/// Deleting a branch with unmerged commits offers the count **as a link**, and
/// offers it at the moment of decision — not only in the notification after the
/// ref is gone.
#[test]
fn an_unmerged_delete_offers_its_commit_count_as_a_link_before_and_after() {
    let (_project, _dir, mut harness) = loaded();

    menu_action(&mut harness, "work", "Delete branch");
    assert_painted(&harness, "Delete local branch 'work'?");
    assert_painted(&harness, "View 3 unmerged commits");

    delete(&mut harness, "work");

    // The same disclosure survives into the notification, beside the restore.
    assert_painted(&harness, "Deleted 'work'");
    assert_painted(&harness, "View 3 unmerged commits");
    assert_painted(&harness, "Undo delete");
}

/// A branch with nothing unmerged has no count to link over, before or after.
#[test]
fn a_fully_merged_delete_offers_no_unmerged_link() {
    let (_project, _dir, mut harness) = loaded();

    menu_action(&mut harness, "tidy", "Delete branch");
    assert_painted(&harness, "Delete local branch 'tidy'?");
    assert_not_painted(&harness, "unmerged commit");

    // Dismiss the confirmation before asking again. `delete` drives the row's
    // own menu, and while the modal is up `tidy`'s row is under it: of the three
    // local branches it is the only one with no row in the Recent group above the
    // dialog, so the second right-click would land on the dialog and open no menu
    // at all. The ask-and-confirm path below is then the same one every other test
    // in this file takes.
    harness.get_by_label("Cancel").click();
    settle_quiet(&mut harness);

    delete(&mut harness, "tidy");
    assert_painted(&harness, "Deleted 'tidy'");
    assert_not_painted(&harness, "unmerged commit");
}

/// The count is the substance, and one commit short of merged reads in the
/// singular — a link that said "3" for three separate branches would be a
/// label, not a measurement.
#[test]
fn the_link_counts_one_unmerged_commit_in_the_singular() {
    let (_project, dir, mut harness) = loaded();
    let repo = dir.join("alpha");
    git(&repo, &["checkout", "-q", "-b", "edge"]);
    std::fs::write(repo.join("e.txt"), "x\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "edge one"]);
    git(&repo, &["checkout", "-q", "main"]);
    let root = harness.state().selected_root.clone().unwrap();
    harness.state_mut().refresh(Affected::Root(root));
    pump_until(&mut harness, "edge listed", |s| branch(s, "edge").is_some());
    // `pump_until` returns on the frame whose *state* first holds the branch, which
    // can be a frame or two before the tree paints its row; every interaction below
    // addresses that row, so settle the paint as `loaded()` does.
    settle_quiet(&mut harness);

    menu_action(&mut harness, "edge", "Delete branch");
    assert_painted(&harness, "View 1 unmerged commit");
    assert_not_painted(&harness, "View 1 unmerged commits");
}

// --- verification 2: the viewer says what it was not merged INTO ------------

/// Activating the link opens a list whose header carries **both** parameters:
/// the branch and the reference it was not merged into. A header naming only
/// the branch loses the fact the disclosure exists to deliver.
#[test]
fn the_unmerged_viewer_names_the_branch_and_the_reference_it_was_not_merged_into() {
    let (_project, _dir, mut harness) = loaded();
    menu_action(&mut harness, "work", "Delete branch");
    harness.get_by_label("View 3 unmerged commits").click();
    settle_quiet(&mut harness);

    assert_painted(&harness, "Branch Was Not Fully Merged");
    assert_painted(
        &harness,
        "The branch 'work' was not fully merged to main. Below is the list of unmerged commits.",
    );
    // The list is the payload, and its subjects are what make it recognisable
    // as the work about to be lost.
    assert_painted(&harness, "work one: first change");
    assert_painted(&harness, "work two: second change");
    assert_painted(&harness, "work three: third change");
}

/// The viewer reached from a notification after the branch is gone can restore
/// it; reached from the pre-flight confirm the branch still exists, and a
/// "Restore" there would be a promise about nothing.
#[test]
fn the_viewer_offers_restore_only_where_a_branch_is_actually_gone() {
    let (_project, _dir, mut harness) = loaded();
    menu_action(&mut harness, "work", "Delete branch");
    harness.get_by_label("View 3 unmerged commits").click();
    settle_quiet(&mut harness);

    assert_painted(&harness, "Branch Was Not Fully Merged");
    assert_not_painted(&harness, "Restore");
    harness.get_by_label("Close").click();
    settle_quiet(&mut harness);
    harness.get_by_label("Cancel").click();
    settle_quiet(&mut harness);

    delete(&mut harness, "work");
    harness.get_by_label("View 3 unmerged commits").click();
    settle_quiet(&mut harness);

    assert_painted(&harness, "Branch Was Not Fully Merged");
    assert_painted(&harness, "Restore");
    // The dialog's restore is the same recovery the notification offers, and it
    // lands: the branch returns and the offer is spent.
    harness.get_by_label("Restore").click();
    pump_until(&mut harness, "work restored", |s| {
        branch(s, "work").is_some() && s.ui.branches_undo.is_none()
    });
}

// --- verification 3: a link is not a second primary control -----------------

/// The unmerged-commits link and the restore control are different kinds of
/// thing on screen. The restore is the notification's one filled control and
/// wears the raised surface tone; the link wears no fill of its own, so no
/// second accent rect competes with the recovery beside it.
#[test]
fn the_unmerged_link_and_the_restore_control_are_not_the_same_kind_of_control() {
    let (_project, _dir, mut harness) = loaded();
    delete(&mut harness, "work");

    let undo = harness.get_by_label("Undo delete").rect();
    let said = galley_rect(&harness, "Deleted 'work'");
    let link = galley_rect(&harness, "View 3 unmerged commits");

    // Distinct rects: the link is not part of the restore control's slot.
    assert!(
        !undo.expand(1.0).contains(link.center()),
        "the link is not inside the restore control's slot: link {link:?}, undo {undo:?}"
    );

    // The restore control owns a fill, and it is the non-accent one.
    assert_eq!(
        fill_behind(&harness, undo.center()),
        Some(NamedFill(Palette::RAISED)),
        "the restore control keeps the notification's raised fill"
    );

    // The link owns none: whatever sits under it is the strip's own surface,
    // never an accent fill.
    assert_ne!(
        fill_behind(&harness, link.center()),
        Some(NamedFill(Palette::ACCENT)),
        "the unmerged-commits link must not paint as a primary button"
    );

    // Across the whole notification band — the extent its own three parts
    // occupy, and no further — there is no accent rect at all. Scoped to the
    // band because the shell wears the accent elsewhere in the frame, and an
    // unscoped count would be measuring the toolbar.
    let band = said.union(link).union(undo).expand(6.0);
    let accents: Vec<_> = filled_rects(&harness)
        .iter()
        .filter(|(r, c)| *c == Palette::ACCENT && band.intersects(*r))
        .cloned()
        .collect();
    assert!(
        accents.is_empty(),
        "the notification carries no accent-filled control, got {accents:?} in {band:?}"
    );
}

// --- link 2: remove the tracking remote branch ------------------------------

/// A deleted branch that tracked a remote branch leaves that remote branch
/// behind, and the notification carries the link to remove it.
#[test]
fn a_deleted_tracking_branch_offers_to_remove_the_remote_it_left_behind() {
    let (_project, dir, mut harness) = loaded();
    let repo = dir.join("alpha");
    assert!(remote_listed(harness.state(), "work"));

    delete(&mut harness, "work");
    assert_painted(&harness, "Remove the tracking remote branch origin/work");

    // The offer routes through the remote-delete confirmation, not a silent
    // push — the same policy a remote row's own Delete already uses.
    harness
        .get_by_label("Remove the tracking remote branch origin/work")
        .click();
    settle_quiet(&mut harness);
    assert_painted(&harness, "Delete remote branch 'origin/work'?");
    harness.get_by_label("OK").click();
    pump_until(&mut harness, "the remote delete settles", |s| {
        s.ui.confirm.is_none() && !remote_listed(s, "work")
    });

    let heads = git(&repo, &["ls-remote", "--heads", "origin"]);
    assert!(
        !heads.contains("refs/heads/work"),
        "the remote branch is gone; origin carries {heads}"
    );
}

/// A branch that tracked nothing has no remote to clean up, so the second link
/// is not painted as chrome on every delete.
#[test]
fn an_untracked_delete_offers_no_remote_removal() {
    let (_project, _dir, mut harness) = loaded();
    delete(&mut harness, "tidy");

    assert_painted(&harness, "Deleted 'tidy'");
    assert_not_painted(&harness, "Remove the tracking remote branch");
}

/// The link's own position is a fact about the notification: it sits beside the
/// restore control, in the strip that names the deleted branch, and not in the
/// branch list below it — which is where a stale notification would otherwise
/// leave the reader looking.
#[test]
fn the_unmerged_link_is_painted_inside_the_notification_strip() {
    let (_project, _dir, mut harness) = loaded();
    delete(&mut harness, "work");

    let said = galley_origin(&harness, "Deleted 'work'")
        .expect("the notification names the branch it deleted");
    let link = galley_origin(&harness, "View 3 unmerged commits").expect("the link is painted");
    let undo = harness.get_by_label("Undo delete").rect();

    // Same strip: within one row's height of the message that owns it.
    assert!(
        (link.y - said.y).abs() < 24.0,
        "the link rides the notification's own row: message at {said:?}, link at {link:?}"
    );
    // And on the same side of it as the restore control it sits beside.
    assert!(
        (link.y - undo.center().y).abs() < 24.0,
        "the link and the restore control share the row: undo {undo:?}, link at {link:?}"
    );
}
