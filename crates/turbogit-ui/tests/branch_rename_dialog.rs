//! Branches — the rename dialog's upstream decision.
//!
//! The reference's Rename dialog is two fields, not one: the new name and an
//! `Unset upstream branch` option, and the thing it has to do at the point of
//! decision is make a **non-local consequence legible** — renaming the local
//! branch leaves the remote branch behind, and the next push creates a new one.
//! A checkbox that says nothing about that is the failure mode these tests
//! exist to prevent, so the consequence line is asserted as a deliverable.
//!
//! Drives the real `turbogit_ui::ui::render()` over temporary git repositories
//! and the production CLI engine, so the upstream that the dialog promises to
//! clear is cleared by git and read back by the app's own branch refresh.
//! Asserts only on public surfaces: painted text/geometry + `AppState`.

use std::path::PathBuf;
use std::time::Duration;

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use tempfile::TempDir;
use test_support::git_seed::git;
use test_support::harness::{
    KITTEST_DEFAULT_BOX, PaintedGalley, assert_not_painted, assert_painted, filled_rects,
    galley_origin, painted_galleys, painted_text, settle_quiet, shell_harness_over,
};
use turbogit_app::state::{AppState, Tab};
use turbogit_domain::model::{BranchKind, Upstream};
use turbogit_ui::theme::Palette;

// --- git fixture -------------------------------------------------------------

/// One repo `alpha` on `main`, with `feature-a` a local branch **really
/// tracking `origin/feature-a`** — the checkbox's precondition. The tracking
/// config has to exist in git rather than be patched into the snapshot, because
/// the assertion that matters is that the rename clears it.
fn tracked_branch_project() -> (TempDir, PathBuf) {
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
    git(&alpha, &["branch", "feature-a"]);
    git(&alpha, &["push", "-q", "-u", "origin", "feature-a"]);
    // A plain local branch with no upstream at all — the dialog's other case.
    git(&alpha, &["branch", "zebra"]);
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

/// The prefix every test here needs: a loaded repository in which `feature-a`
/// answers with its upstream, so opening the dialog has something to say.
fn tracked_and_loaded() -> (TempDir, PathBuf, Harness<'static, AppState>) {
    let (project, dir) = tracked_branch_project();
    let mut harness = shell_harness(dir.clone());
    harness.state_mut().ui.tab = Tab::Branches;
    settle_quiet(&mut harness);
    pump_until(&mut harness, "feature-a reports its upstream", |s| {
        branch(s, "feature-a")
            .and_then(|b| b.tracking.as_ref())
            .map(Upstream::git_ref)
            .as_deref()
            == Some("origin/feature-a")
    });
    (project, dir, harness)
}

/// Open the rename dialog for `feature-a` through the seam the popup's Rename
/// button calls.
fn open_rename(harness: &mut Harness<'_, AppState>) {
    let root = harness.state().selected_root.clone().unwrap();
    harness.state_mut().open_rename_branch(&root, "feature-a");
    settle_quiet(harness);
}

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

/// Type `text` into the dialog's name field — the one text input that carries
/// no accessible label of its own.
fn type_name(h: &mut Harness<'_, AppState>, text: &str) {
    let field = h
        .get_all_by_role(egui::accesskit::Role::TextInput)
        .find(|n| n.accesskit_node().label().is_none())
        .expect("rename dialog name input");
    field.focus();
    field.type_text(text);
    settle_quiet(h);
}

fn toggle_unset(h: &mut Harness<'_, AppState>) {
    h.get_by_label("Unset upstream branch").click();
    settle_quiet(h);
}

/// The ACCENT-filled rects sitting on the Rename button's own slot. Scoped to
/// that slot because the dialog is not the only thing in the frame that wears
/// the accent — an unscoped count passes for reasons unrelated to this button.
fn accent_on_rename(h: &Harness<'_, AppState>) -> bool {
    let slot = h.get_by_label("Rename").rect();
    filled_rects(h)
        .iter()
        .any(|(r, c)| *c == Palette::ACCENT && r.contains(slot.center()))
}

/// Galleys painted inside `rect` — how a row's own tracking marker is read
/// while the dialog may still be mounted over the list.
fn galleys_in(h: &Harness<'_, AppState>, rect: egui::Rect) -> Vec<PaintedGalley> {
    painted_galleys(h)
        .into_iter()
        .filter(|g| rect.intersects(g.rect))
        .collect()
}

fn row_rect(h: &Harness<'_, AppState>, name: &str) -> egui::Rect {
    h.get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| n.accesskit_node().label() == Some(name.to_string()))
        .unwrap_or_else(|| panic!("row button for {name}"))
        .rect()
}

// --- verification 1: the dialog carries the decision and its consequence ----

/// The two-field dialog: the name, the `Unset upstream branch` option, and the
/// consequence line that names **both** outcomes the user cannot see from the
/// dialog itself — a new remote branch created on the next push, and the old
/// one left behind.
#[test]
fn rename_dialog_offers_the_upstream_option_and_states_what_the_rename_leaves_behind() {
    let (_project, _dir, mut harness) = tracked_and_loaded();
    open_rename(&mut harness);

    // Field one, the name — the dialog's own copy, scoped to this dialog by the
    // branch it was opened for.
    assert_painted(&harness, "Rename 'feature-a' to:");

    // Field two.
    assert_painted(&harness, "Unset upstream branch");

    // The consequence line: local-only + both outcomes, in one sentence.
    assert_painted(&harness, "Only the local branch is renamed");
    assert_painted(&harness, "creates and tracks a new remote branch");
    assert_painted(&harness, "origin/feature-a");
    assert_painted(&harness, "left behind");
}

/// A branch with no upstream has nothing to unset and no remote to leave
/// behind, so the dialog neither offers the option nor makes a promise about an
/// upstream it does not have.
#[test]
fn an_untracked_branch_gets_the_name_field_and_no_upstream_promise() {
    let (project, _dir, mut harness) = tracked_and_loaded();
    // `zebra` is a plain local branch in the same fixture.
    assert!(
        branch(harness.state(), "zebra").is_some(),
        "the fixture carries an untracked branch"
    );
    assert!(
        branch(harness.state(), "zebra")
            .and_then(|b| b.tracking.as_ref())
            .is_none(),
        "zebra tracks nothing — that is what this test is about"
    );
    let root = harness.state().selected_root.clone().unwrap();
    harness.state_mut().open_rename_branch(&root, "zebra");
    settle_quiet(&mut harness);

    assert_painted(&harness, "Rename 'zebra' to:");
    assert_not_painted(&harness, "Unset upstream branch");
    assert_not_painted(&harness, "left behind");
    drop(project);
}

// --- verification 2: the confirm is inert while the name is blank -----------

/// The confirm control's fill is **withheld** while the name field is blank —
/// no `Palette::ACCENT` rect on its slot, not a tinted one — and the accent
/// paints once a name is written. Asserted through the fill because a
/// `kit_button` label carries WHITE in its galley job, so its text colour says
/// nothing about its state.
#[test]
fn rename_confirm_wears_no_accent_fill_until_a_name_is_written() {
    let (_project, _dir, mut harness) = tracked_and_loaded();
    open_rename(&mut harness);

    assert!(
        !accent_on_rename(&harness),
        "the Rename control must paint no accent fill while the name is blank"
    );

    type_name(&mut harness, "feature-b");
    assert!(
        accent_on_rename(&harness),
        "the Rename control must wear the accent fill once a name is written"
    );
}

// --- verification 3: the option decides what the painted consequence says ---

/// The upstream-specific consequence is the **checked** state's, so it is the
/// only state that paints it.
///
/// The needle is a substring of the consequence sentence and not of the
/// checkbox's own label: `Unset upstream branch` is painted in both states, so
/// a bare `upstream` needle would match the control and report the copy's
/// presence whether or not the copy is there.
#[test]
fn the_upstream_consequence_is_painted_only_while_the_option_is_checked() {
    let (_project, _dir, mut harness) = tracked_and_loaded();
    open_rename(&mut harness);

    assert_not_painted(&harness, "The upstream is cleared");
    toggle_unset(&mut harness);
    assert_painted(&harness, "The upstream is cleared");
}

/// The decision reaches git: leaving the option alone keeps the renamed branch
/// tracking its old upstream, and checking it untracks the renamed branch —
/// read back through the app's own branch refresh, not through the dialog.
///
/// The marker is asserted **inside the renamed row's own rect**, because the
/// dialog is a floating surface and the frame still holds the rest of the list.
#[test]
fn checking_the_option_unsets_the_upstream_the_rename_would_otherwise_keep() {
    for (unset, what) in [
        (false, "the option left alone keeps the tracking"),
        (true, "the option checked clears the tracking"),
    ] {
        let (_project, _dir, mut harness) = tracked_and_loaded();
        open_rename(&mut harness);
        if unset {
            toggle_unset(&mut harness);
        }
        type_name(&mut harness, "renamed-a");
        harness.get_by_label("Rename").click();

        pump_until(&mut harness, what, |s| {
            branch(s, "renamed-a").is_some()
                && branch(s, "feature-a").is_none()
                && s.ui.dialog.is_none()
        });

        let tracking = branch(harness.state(), "renamed-a")
            .and_then(|b| b.tracking.clone())
            .map(|u| u.git_ref());
        if unset {
            assert_eq!(tracking, None, "the renamed branch tracks nothing — {what}");
        } else {
            assert_eq!(
                tracking.as_deref(),
                Some("origin/feature-a"),
                "the renamed branch still tracks its old upstream — {what}"
            );
        }

        // The row's own tracking marker, scoped to the row.
        let row = row_rect(&harness, "renamed-a");
        let marks: Vec<String> = galleys_in(&harness, row)
            .iter()
            .map(|g| g.text.clone())
            .filter(|t| t.starts_with("origin/"))
            .collect();
        if unset {
            assert!(
                marks.is_empty(),
                "the renamed row paints no upstream marker, got {marks:?} in {row:?}"
            );
        } else {
            assert!(
                marks.iter().any(|t| t == "origin/feature-a"),
                "the renamed row keeps its upstream marker, got {marks:?} in {row:?}"
            );
        }
    }
}

/// The consequence line is not decoration the dialog forgets to honour: the
/// branch it names is the one the rename leaves behind, and the dialog says so
/// before anything runs.
#[test]
fn the_rename_without_the_option_leaves_the_old_remote_branch_in_place() {
    let (_project, _dir, mut harness) = tracked_and_loaded();
    let repo = _dir.join("alpha");
    open_rename(&mut harness);
    type_name(&mut harness, "renamed-a");
    harness.get_by_label("Rename").click();
    pump_until(&mut harness, "rename lands", |s| {
        branch(s, "renamed-a").is_some()
    });

    // What the consequence line promised: the remote branch it named is still
    // there under the old name, because only the local branch moved.
    let remote_heads = git(&repo, &["ls-remote", "--heads", "origin"]);
    assert!(
        remote_heads.contains("refs/heads/feature-a"),
        "the old remote branch is left behind, as the consequence line states; \
         origin carries {remote_heads}"
    );
    assert!(!remote_heads.contains("refs/heads/renamed-a"));
    assert!(
        galley_origin(&harness, "renamed-a").is_some(),
        "the renamed branch is the one the list now shows"
    );
}
