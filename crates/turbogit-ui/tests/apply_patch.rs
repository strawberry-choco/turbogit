//! The `Apply patch` window: the two routes, the preflight
//! that mutates nothing, the two titled outcomes, and the recovery the second one
//! carries.
//!
//! The ticket states the rule this suite is built around: **the content check is
//! the real assertion — the string alone is a claim the UI makes about itself.**
//! So the aborted state is reached end to end, over a real repository through the
//! production dispatch, and the test then reads the file off disk. The painted
//! strings are asserted alongside it, because a guarantee the developer cannot read
//! is not yet a guarantee.
//!
//! The two titled states are asserted **against each other**: a rollback that could
//! not finish paints `Rollback failed` and has to be unable to paint `Apply patch
//! aborted` in the same frame. Both painted is the failure this surface exists to
//! avoid, and an assert that only checked the first string would miss it.
//!
//! No golden image: `acceptance_matrix.rs` owns the per-page PNGs and this window is
//! not one of those pages, so a screenshot here would be an invented baseline.

use std::path::{Path, PathBuf};

use egui::Vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::NodeT as _;
use egui_kittest::kittest::Queryable;
use test_support::git_seed::{commit as git_commit, git, pin_eol_lf};
use test_support::harness::{
    assert_not_painted, assert_painted, painted_text, settle, shell_harness_over,
};
use turbogit_app::state::{AppState, Dialog, PatchSource};
use turbogit_domain::model::{RootId, VcsSettings};
use turbogit_services::patch_apply::{ApplyOutcome, Preflight, Unrestored};
use turbogit_ui::ui::popups::Action;

// ------------------------------------------------------------------ fixtures --

fn root_id(dir: &Path) -> RootId {
    RootId(dir.to_path_buf().into())
}

/// One repository with two committed files, so a two-section patch can be written
/// where the first section applies and the second cannot.
fn repo(parent: &Path) -> PathBuf {
    let dir = parent.join("app");
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.email", "ada@example.com"]);
    git(&dir, &["config", "user.name", "Ada Lovelace"]);
    pin_eol_lf(&dir);
    git_commit(&dir, "first.txt", "one\ntwo\nthree\n", "first.txt");
    git_commit(&dir, "second.txt", "alpha\nbeta\n", "second.txt");
    dir
}

/// The failing patch, written next to the repository rather than inside it so
/// `git status` is never describing the fixture.
fn conflicting_patch(parent: &Path) -> PathBuf {
    let path = parent.join("change.patch");
    std::fs::write(
        &path,
        r#"diff --git a/first.txt b/first.txt
--- a/first.txt
+++ b/first.txt
@@ -1,3 +1,3 @@
 one
-two
+TWO
 three
diff --git a/second.txt b/second.txt
--- a/second.txt
+++ b/second.txt
@@ -1,4 +1,4 @@
 alpha
-beta
+BETA
 gamma
 delta
"#,
    )
    .unwrap();
    path
}

/// A real `git format-patch` file, so the provenance assertions are about git's
/// output and not about text this file invented.
fn format_patch(parent: &Path, dir: &Path) -> PathBuf {
    git(dir, &["format-patch", "-1", "-o", "out"]);
    let written = std::fs::read_dir(dir.join("out"))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "patch"))
        .expect("format-patch wrote a patch file");
    let dest = parent.join("provenance.patch");
    std::fs::copy(&written, &dest).unwrap();
    dest
}

/// The shell with the window open on `dir`, by the route the palette item runs.
fn window(dir: &Path, parent: &Path, source: PatchSource) -> Harness<'static, AppState> {
    let mut state =
        AppState::for_roots(parent, &[dir.to_path_buf()]).with_settings(VcsSettings::default());
    turbogit_app::apply_patch::open(&mut state, source);
    let mut harness = shell_harness_over(state, Vec2::new(1280.0, 800.0));
    settle(&mut harness);
    assert_eq!(harness.state().ui.dialog, Some(Dialog::ApplyPatch));
    assert_eq!(
        harness.state().ui.dlg.apply_patch_root,
        Some(root_id(dir)),
        "the window opens on the workspace's root and says which one it is"
    );
    harness
}

#[track_caller]
fn click(harness: &mut Harness<'_, AppState>, label: &str) {
    let node = harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| n.accesskit_node().label().as_deref() == Some(label))
        .unwrap_or_else(|| panic!("no control labelled `{label}` is painted"));
    node.click();
    harness.step();
}

/// The painted form of a shipped title. `group_title` runs the design system's
/// mandatory uppercase transform (`widgets::group_title`), so the window
/// paints the title's uppercase — an assertion against the mixed-case constant
/// would pin the transform's spelling rather than the state. The mixed-case
/// constants themselves are pinned by the services suite and by the feed
/// assertions here, which read state rather than pixels.
fn painted_title(title: &str) -> String {
    title.to_uppercase()
}

// ------------------------------------------------------- the two apply routes --

/// The reference's two verbs are two routes, and the second one is defined by the
/// question it does not ask. An assert on what the clipboard route *omits* is the
/// only one that tells the two apart.
#[test]
fn the_clipboard_route_skips_the_file_path_the_file_route_asks_for() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());

    let file_route = window(&dir, tmp.path(), PatchSource::File);
    assert_painted(&file_route, "Choose file…");
    assert_not_painted(&file_route, "Paste the patch below — no file is read.");

    let clipboard = window(&dir, tmp.path(), PatchSource::Clipboard);
    assert_not_painted(&clipboard, "Choose file…");
    assert_painted(&clipboard, "Paste the patch below — no file is read.");
}

/// Both routes reach the window from the palette, because TurboGit has no main menu
/// bar — and neither joins the VCS popup, whose action set is a frozen pin.
#[test]
fn both_routes_are_palette_verbs_and_neither_touches_the_frozen_popup() {
    let palette: Vec<&str> = Action::palette_actions()
        .iter()
        .map(|action| action.label())
        .collect();
    for route in ["Apply patch\u{2026}", "Apply patch from Clipboard"] {
        assert!(
            palette.contains(&route),
            "`{route}` is unreachable: {palette:?}"
        );
    }
    let popup: Vec<&str> = Action::all().iter().map(|action| action.label()).collect();
    for route in ["Apply patch\u{2026}", "Apply patch from Clipboard"] {
        assert!(
            !popup.contains(&route),
            "the VCS popup's documented action set stays frozen"
        );
    }
}

// ------------------------------------------------------ preflight, not confirm --

/// `Show Diff` answers the question and changes nothing: no dispatch, no busy flag,
/// and a working tree whose bytes are still what the commit left. This is the verb a
/// developer is meant to press repeatedly and then walk away from.
#[test]
fn show_diff_returns_git_verdict_without_writing_and_without_dispatching() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    let patch = conflicting_patch(tmp.path());
    let before = std::fs::read(dir.join("first.txt")).unwrap();

    let mut harness = window(&dir, tmp.path(), PatchSource::File);
    harness.state_mut().ui.dlg.apply_patch_path = patch.to_string_lossy().into_owned();
    settle(&mut harness);
    // The patch's blast radius is stated before anything runs.
    assert_painted(&harness, "FILES THE PATCH NAMES · 2");
    assert_painted(&harness, "first.txt");

    click(&mut harness, "Show Diff");
    settle(&mut harness);

    assert_painted(
        &harness,
        "The patch does not apply cleanly. No file has been changed.",
    );
    // git's own line, named by the file it conflicts in — the lines the reference
    // highlights, stated here as the text that carries them.
    let painted = painted_text(&harness);
    assert!(
        painted.iter().any(|t| t.contains("second.txt")),
        "the conflicting file's line is painted among the report: {painted:?}"
    );
    assert!(
        !harness.state().ui.busy,
        "a preflight is a read, never a dispatched operation"
    );
    assert_eq!(
        std::fs::read(dir.join("first.txt")).unwrap(),
        before,
        "the tree is byte-for-byte what the commit left"
    );
    let staged = git(&dir, &["status", "--porcelain"]);
    assert!(
        staged.trim().is_empty(),
        "the preflight dirtied the tree: {staged}"
    );
}

/// A preflight verdict is an answer about one tree. Pointing the window at a second
/// root has to drop it, or the window keeps certifying a repository it is no longer
/// looking at.
#[test]
fn switching_the_target_root_drops_a_verdict_measured_against_another_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    let other = tmp.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    git(&other, &["init", "-q", "-b", "main"]);
    git(&other, &["config", "user.email", "t@t"]);
    git(&other, &["config", "user.name", "t"]);
    git_commit(&other, "first.txt", "one\ntwo\nthree\n", "first.txt");

    let mut harness = window(&dir, tmp.path(), PatchSource::File);
    harness.state_mut().ui.dlg.apply_patch_preflight = Some(Preflight::Clean { files: 2 });
    settle(&mut harness);
    assert_painted(&harness, "Applies cleanly");

    turbogit_app::apply_patch::set_root(harness.state_mut(), &root_id(&other));
    settle(&mut harness);
    assert!(
        harness.state().ui.dlg.apply_patch_preflight.is_none(),
        "the stale verdict left with the root it described"
    );
    assert_not_painted(&harness, "Applies cleanly");
}

// ------------------------------------------------------------- provenance ----

/// A git-format patch carries its commit's author and subject, and the window fills
/// those fields from the patch rather than making the developer retype what the file
/// already says. Produced by real `git format-patch`, read through the same entry
/// point the file route uses.
#[test]
fn a_git_format_patch_fills_the_authors_and_message_it_already_states() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    git_commit(&dir, "note.txt", "raised\n", "Raise the note");
    let patch = format_patch(tmp.path(), &dir);

    let mut harness = window(&dir, tmp.path(), PatchSource::File);
    harness.state_mut().ui.dlg.apply_patch_path = patch.to_string_lossy().into_owned();
    settle(&mut harness);

    assert_painted(&harness, "PATCH PROVENANCE");
    let state = harness.state();
    assert_eq!(
        state.ui.dlg.apply_patch_author, "Ada Lovelace <ada@example.com>",
        "the author the patch names, in the field the developer can still edit"
    );
    assert_eq!(state.ui.dlg.apply_patch_message, "Raise the note");
}

/// A plain `git diff` file has no header, and the window says so instead of painting
/// two fields that look like they failed to fill.
#[test]
fn a_patch_with_no_header_says_it_has_none() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    let patch = conflicting_patch(tmp.path());

    let mut harness = window(&dir, tmp.path(), PatchSource::File);
    harness.state_mut().ui.dlg.apply_patch_path = patch.to_string_lossy().into_owned();
    settle(&mut harness);

    assert_painted(
        &harness,
        "This patch carries no commit header, so there is nothing to fill.",
    );
}

// --------------------------------------------------- the two titled states ----

/// THE ticket assertion, run as far as it can be run: a real repository, a real
/// patch that fails on its second file, the production dispatch, and the tree read
/// back off disk afterwards. The painted strings ride along, but what makes the test
/// worth writing is the byte comparison beside them.
#[test]
fn a_failed_apply_paints_the_aborted_state_over_a_tree_that_is_back_to_what_it_held() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    let patch = conflicting_patch(tmp.path());
    let before_first = std::fs::read(dir.join("first.txt")).unwrap();
    let before_second = std::fs::read(dir.join("second.txt")).unwrap();
    assert_eq!(before_first, b"one\ntwo\nthree\n");

    let mut harness = window(&dir, tmp.path(), PatchSource::File);
    harness.state_mut().ui.dlg.apply_patch_path = patch.to_string_lossy().into_owned();
    settle(&mut harness);

    turbogit_app::apply_patch::dispatch(harness.state_mut());
    settle(&mut harness);

    // The content check: the first file WAS written by the apply, and the tree has
    // to be holding its original bytes now.
    assert_eq!(
        std::fs::read(dir.join("first.txt")).unwrap(),
        before_first,
        "the aborted state's message is a claim about these bytes"
    );
    assert_eq!(
        std::fs::read(dir.join("second.txt")).unwrap(),
        before_second
    );

    // The two strings the ticket ships, painted by the state that earned them.
    assert_painted(&harness, &painted_title(ApplyOutcome::ABORTED_TITLE));
    assert_painted(&harness, ApplyOutcome::ABORTED_MESSAGE);
    assert_painted(&harness, "ROLLED BACK · 2");
    // …and the third state is not said by a rollback that did finish.
    assert_not_painted(
        &harness,
        &painted_title(ApplyOutcome::ROLLBACK_FAILED_TITLE),
    );

    // The durable record, not the toast: the feed outlives the notification.
    let feed: Vec<&String> = harness
        .state()
        .ui
        .activity
        .entries
        .iter()
        .map(|e| &e.message)
        .collect();
    assert!(
        feed.iter().any(|m| m.contains(ApplyOutcome::ABORTED_TITLE)),
        "the aborted outcome is recorded: {feed:?}"
    );
}

/// The state that makes the promise above honest. Painted **instead** of the aborted
/// one: an outcome that could not be restored has no way to also claim that
/// everything was rolled back, and this test's second half is exactly that absence.
#[test]
fn a_rollback_that_cannot_finish_paints_its_own_state_and_never_the_aborted_one() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());

    let mut harness = window(&dir, tmp.path(), PatchSource::Clipboard);
    harness.state_mut().ui.dlg.apply_patch_outcome = Some(ApplyOutcome::RollbackFailed {
        reason: "git exited with code 1: error: patch failed: second.txt:1".to_string(),
        unrestored: vec![Unrestored {
            path: PathBuf::from("first.txt"),
            reason: "injected: the write was refused".to_string(),
            recorded: Some(b"one\ntwo\nthree\n".to_vec()),
        }],
        restored: vec![PathBuf::from("second.txt")],
    });
    settle(&mut harness);

    assert_painted(
        &harness,
        &painted_title(ApplyOutcome::ROLLBACK_FAILED_TITLE),
    );
    assert_painted(&harness, "COULD NOT BE RESTORED · 1");
    assert_painted(&harness, "first.txt");
    // Never both. The two titled states are one value's two variants, and a frame
    // that showed them together would be the UI contradicting its own guarantee.
    assert_not_painted(&harness, &painted_title(ApplyOutcome::ABORTED_TITLE));
    assert_not_painted(&harness, ApplyOutcome::ABORTED_MESSAGE);
    // The recovery is offered as a control, not as prose.
    assert_painted(&harness, "Restore recorded content");
}

/// The `Rollback failed` state's recovery action, executed: the recorded bytes are
/// written again, the tree really comes back, and the window then states the aborted
/// outcome — truthfully, because by that point every file the apply changed is back.
/// This is the resolution of the question the ticket leaves unresolved, and it is
/// asserted as behaviour rather than as a button.
#[test]
fn the_failed_rollback_offers_one_action_and_that_action_fixes_the_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    // The state the apply would have left: `first.txt` holding what the patch wrote,
    // with the pre-apply content held only in the outcome's record.
    std::fs::write(dir.join("first.txt"), b"one\nTWO\nthree\n").unwrap();

    let mut harness = window(&dir, tmp.path(), PatchSource::Clipboard);
    let recorded = b"one\ntwo\nthree\n".to_vec();
    harness.state_mut().ui.dlg.apply_patch_outcome = Some(ApplyOutcome::RollbackFailed {
        reason: "the patch did not apply".to_string(),
        unrestored: vec![Unrestored {
            path: PathBuf::from("first.txt"),
            reason: "injected: the write was refused".to_string(),
            recorded: Some(recorded.clone()),
        }],
        restored: Vec::new(),
    });
    settle(&mut harness);
    assert_not_painted(&harness, &painted_title(ApplyOutcome::ABORTED_TITLE));

    click(&mut harness, "Restore recorded content");
    settle(&mut harness);

    assert_eq!(
        std::fs::read(dir.join("first.txt")).unwrap(),
        recorded,
        "the retry wrote the recorded bytes back"
    );
    // The state moved with the tree: `Aborted` is now the sentence that is true.
    let state = harness.state();
    assert!(
        matches!(
            state.ui.dlg.apply_patch_outcome,
            Some(ApplyOutcome::Aborted { .. })
        ),
        "a finished rollback is reported as the state it reached"
    );
    assert_painted(&harness, &painted_title(ApplyOutcome::ABORTED_TITLE));
    assert_not_painted(
        &harness,
        &painted_title(ApplyOutcome::ROLLBACK_FAILED_TITLE),
    );
}

/// A retry that is refused again leaves the named state in place rather than
/// reporting a recovery that did not happen.
#[test]
fn a_second_refused_retry_keeps_the_state_named_and_records_it_again() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    // A path the filesystem cannot write back: the destination is replaced by a
    // directory, so the recorded content has nowhere to go and the retry
    // genuinely fails.
    std::fs::remove_file(dir.join("first.txt")).unwrap();
    std::fs::create_dir_all(dir.join("first.txt")).unwrap();

    let mut harness = window(&dir, tmp.path(), PatchSource::Clipboard);
    harness.state_mut().ui.dlg.apply_patch_outcome = Some(ApplyOutcome::RollbackFailed {
        reason: "the patch did not apply".to_string(),
        unrestored: vec![Unrestored {
            path: PathBuf::from("first.txt"),
            reason: "injected".to_string(),
            recorded: Some(b"one\ntwo\nthree\n".to_vec()),
        }],
        restored: Vec::new(),
    });
    settle(&mut harness);
    let entries = harness.state().ui.activity.entries.len();

    click(&mut harness, "Restore recorded content");
    settle(&mut harness);

    let state = harness.state();
    assert!(
        matches!(
            state.ui.dlg.apply_patch_outcome,
            Some(ApplyOutcome::RollbackFailed { .. })
        ),
        "a retry that failed is still the named state"
    );
    assert!(
        state.ui.activity.entries.len() > entries,
        "the second failure is recorded too — the feed is where the path list survives"
    );
    assert!(
        state
            .ui
            .activity
            .entries
            .iter()
            .any(|e| e.message.contains("first.txt") && e.message.contains("Rollback failed")),
        "the durable entry names the path it could not restore"
    );
}

// ------------------------------------------------ the recovery's lifetime --
//
// The promise `Apply patch` makes is about the WORKING TREE, and the record that
// pays it is the failed rollback's held bytes. A transition inside the window —
// typing in the patch field, changing route, closing and reopening — changes
// nothing on disk, so it cannot be allowed to be the thing that decides whether
// the developer can still get their file back. Each test below strands a real
// half-applied tree, runs one transition, and then asks the window to finish the
// restore it promised: the file's bytes are the assertion, not a field.

/// The pre-apply content, and the content the failed apply left behind.
const RECORDED: &[u8] = b"one\ntwo\nthree\n";
const LEFT_BY_APPLY: &[u8] = b"one\nTWO\nthree\n";

/// The window in the state a failed rollback leaves: `first.txt` on disk holding
/// what the apply wrote, and the bytes it held before living only in the
/// outcome's record.
fn stranded(dir: &Path, parent: &Path) -> Harness<'static, AppState> {
    std::fs::write(dir.join("first.txt"), LEFT_BY_APPLY).unwrap();
    let mut harness = window(dir, parent, PatchSource::Clipboard);
    harness.state_mut().ui.dlg.apply_patch_outcome = Some(ApplyOutcome::RollbackFailed {
        reason: "git exited with code 1: error: patch failed: second.txt:1".to_string(),
        unrestored: vec![Unrestored {
            path: PathBuf::from("first.txt"),
            reason: "injected: the write was refused".to_string(),
            recorded: Some(RECORDED.to_vec()),
        }],
        restored: Vec::new(),
    });
    settle(&mut harness);
    assert_painted(
        &harness,
        &painted_title(ApplyOutcome::ROLLBACK_FAILED_TITLE),
    );
    harness
}

/// Every transition has to leave the same two things true: the state is still
/// named on screen with its one action, and that action still has the bytes to
/// write. Shared because the three tests differ only in the transition.
#[track_caller]
fn assert_still_rescuable(harness: &mut Harness<'static, AppState>, dir: &Path) {
    // The state is still named on screen, with its one action attached: a frame
    // that painted the patch list instead would be the transition retiring the
    // failure it owes an answer to.
    assert_painted(harness, &painted_title(ApplyOutcome::ROLLBACK_FAILED_TITLE));
    assert_painted(harness, "Restore recorded content");
    click(harness, "Restore recorded content");
    settle(harness);
    assert_eq!(
        std::fs::read(dir.join("first.txt")).unwrap(),
        RECORDED,
        "the recorded content was still held after the transition"
    );
}

/// The stranding this feature cannot ship with: one keystroke in the patch field
/// erasing the only copy of the content the working tree lost.
#[test]
fn editing_the_patch_text_leaves_the_failed_rollback_resolvable() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    let mut harness = stranded(&dir, tmp.path());

    harness.state_mut().ui.dlg.apply_patch_text =
        "diff --git a/third.txt b/third.txt\n--- /dev/null\n+++ b/third.txt\n@@ -0,0 +1 @@\n+x\n"
            .to_string();
    settle(&mut harness);

    assert_still_rescuable(&mut harness, &dir);
}

/// A route switch drops the payload the other route read — it has no business
/// dropping the tree's recovery with it.
#[test]
fn switching_the_patch_route_leaves_the_failed_rollback_resolvable() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    let mut harness = stranded(&dir, tmp.path());

    click(&mut harness, "From file");
    settle(&mut harness);

    assert_still_rescuable(&mut harness, &dir);
}

/// Closing the window is not fixing the tree: reopening it has to find the same
/// unresolved rollback where it left it.
#[test]
fn reopening_the_window_leaves_the_failed_rollback_resolvable() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    let mut harness = stranded(&dir, tmp.path());

    click(&mut harness, "Close");
    settle(&mut harness);
    assert_eq!(harness.state().ui.dialog, None, "the window is closed");
    turbogit_app::apply_patch::open(harness.state_mut(), PatchSource::Clipboard);
    settle(&mut harness);

    assert_still_rescuable(&mut harness, &dir);
}
